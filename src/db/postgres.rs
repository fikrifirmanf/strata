//! PostgreSQL family (Postgres, CockroachDB, Redshift, Greenplum) over the simple-query protocol,
//! which returns every value as text and allows multi-statement scripts.

use super::*;
use crate::model::SslMode;
use anyhow::Context;
use postgres_native_tls::MakeTlsConnector;
use tokio_postgres::{Client, SimpleQueryMessage};

pub struct PgDriver {
    cfg: ConnectionConfig,
    client: Client,
    tls: MakeTlsConnector,
}

fn tls_connector(mode: SslMode) -> Result<MakeTlsConnector> {
    let mut b = native_tls::TlsConnector::builder();
    // libpq's "require" encrypts without verifying; only verify-full checks the chain and host.
    if mode != SslMode::VerifyFull {
        b.danger_accept_invalid_certs(true).danger_accept_invalid_hostnames(true);
    }
    Ok(MakeTlsConnector::new(b.build()?))
}

impl PgDriver {
    pub async fn connect(cfg: &ConnectionConfig) -> Result<Self> {
        let mut cfg = cfg.clone();
        let tls = tls_connector(cfg.ssl)?;
        let client = match Self::open(&cfg, &cfg.database, tls.clone()).await {
            // pg_hba refuses unencrypted logins on hosts such as Azure; libpq would have tried TLS first.
            Err(e) if cfg.ssl == SslMode::Disable && format!("{e:#}").contains("no encryption") => {
                cfg.ssl = SslMode::Require;
                Self::open(&cfg, &cfg.database, tls.clone()).await?
            }
            other => other?,
        };
        Ok(Self { cfg, client, tls })
    }

    async fn open(cfg: &ConnectionConfig, db: &str, tls: MakeTlsConnector) -> Result<Client> {
        let mut pc = tokio_postgres::Config::new();
        pc.host(&cfg.host)
            .port(cfg.port)
            .user(&cfg.user)
            .application_name("Strata")
            .connect_timeout(std::time::Duration::from_secs(15));
        if !cfg.password.is_empty() {
            pc.password(&cfg.password);
        }
        let db = if db.is_empty() { if cfg.driver == DriverKind::CockroachDb { "defaultdb" } else { "postgres" } } else { db };
        pc.dbname(db);
        pc.ssl_mode(match cfg.ssl {
            SslMode::Disable => tokio_postgres::config::SslMode::Disable,
            SslMode::Prefer => tokio_postgres::config::SslMode::Prefer,
            SslMode::Require | SslMode::VerifyFull => tokio_postgres::config::SslMode::Require,
        });
        let (client, conn) = pc.connect(tls).await.context("Can't connect to PostgreSQL")?;
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                eprintln!("strata: postgres connection closed: {e}");
            }
        });
        Ok(client)
    }
}

#[async_trait]
impl Driver for PgDriver {
    fn kind(&self) -> DriverKind {
        self.cfg.driver
    }

    fn current_database(&self) -> String {
        if self.cfg.database.is_empty() { "postgres".into() } else { self.cfg.database.clone() }
    }

    async fn execute(&mut self, sql: &str) -> Result<Vec<QueryResult>> {
        let started = std::time::Instant::now();
        let messages = self.client.simple_query(sql).await.map_err(pg_error)?;
        let elapsed = started.elapsed().as_millis() as u64;
        let mut out = Vec::new();
        let mut current: Option<QueryResult> = None;
        for m in messages {
            match m {
                SimpleQueryMessage::RowDescription(cols) => {
                    current = Some(QueryResult { columns: cols.iter().map(|c| c.name().to_string()).collect(), ..Default::default() });
                }
                SimpleQueryMessage::Row(row) => {
                    let r = current.get_or_insert_with(|| QueryResult {
                        columns: row.columns().iter().map(|c| c.name().to_string()).collect(),
                        ..Default::default()
                    });
                    r.rows.push((0..row.len()).map(|i| row.get(i).map(str::to_string)).collect());
                }
                SimpleQueryMessage::CommandComplete(n) => {
                    let mut r = current.take().unwrap_or_default();
                    if !r.has_rows() {
                        r.affected = Some(n);
                    }
                    out.push(r);
                }
                _ => {}
            }
        }
        let stmts = statements(sql);
        for (i, r) in out.iter_mut().enumerate() {
            r.elapsed_ms = elapsed;
            r.sql = stmts.get(i).cloned().unwrap_or_else(|| sql.to_string());
        }
        Ok(out)
    }

    async fn switch_database(&mut self, db: &str) -> Result<()> {
        let client = Self::open(&self.cfg, db, self.tls.clone()).await?;
        self.client = client;
        self.cfg.database = db.to_string();
        Ok(())
    }

    fn canceller(&self) -> Option<Canceller> {
        let token = self.client.cancel_token();
        let tls = self.tls.clone();
        let handle = tokio::runtime::Handle::current();
        Some(Arc::new(move || {
            let token = token.clone();
            let tls = tls.clone();
            handle.spawn(async move {
                let _ = token.cancel_query(tls).await;
            });
        }))
    }
}

fn pg_error(e: tokio_postgres::Error) -> anyhow::Error {
    if let Some(db) = e.as_db_error() {
        let mut msg = format!("{}: {}", db.severity(), db.message());
        if let Some(d) = db.detail() {
            msg.push_str(&format!("\nDETAIL: {d}"));
        }
        if let Some(h) = db.hint() {
            msg.push_str(&format!("\nHINT: {h}"));
        }
        if let Some(tokio_postgres::error::ErrorPosition::Original(p)) = db.position() {
            msg.push_str(&format!("\nPOSITION: {p}"));
        }
        anyhow::anyhow!(msg)
    } else {
        anyhow::anyhow!(e.to_string())
    }
}
