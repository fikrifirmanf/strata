//! Microsoft SQL Server via TDS (tiberius).

use super::*;
use crate::model::SslMode;
use anyhow::Context;
use futures::TryStreamExt;
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

struct CursorEntry {
    metadata: QueryResult,
    owner: std::sync::Weak<()>,
    name: String,
}

fn cursor_compatible(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut words = Vec::new();
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'[' => {
                let end = if bytes[i] == b'[' { b']' } else { bytes[i] };
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == end {
                        i += 1;
                        if bytes.get(i) == Some(&end) { i += 1; } else { break; }
                    } else { i += 1; }
                }
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut comments = 1;
                i += 2;
                while i < bytes.len() && comments > 0 {
                    if bytes.get(i..i + 2) == Some(b"/*") { comments += 1; i += 2; }
                    else if bytes.get(i..i + 2) == Some(b"*/") { comments -= 1; i += 2; }
                    else { i += 1; }
                }
            }
            b'(' => { depth += 1; i += 1; }
            b')' => { depth = depth.saturating_sub(1); i += 1; }
            c if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80) { i += 1; }
                if depth == 0 { words.push(sql[start..i].to_ascii_uppercase()); }
            }
            _ => i += 1,
        }
    }
    let selects = words.iter().filter(|word| *word == "SELECT").count();
    let unions = words.iter().filter(|word| matches!(word.as_str(), "UNION" | "INTERSECT" | "EXCEPT")).count();
    selects == unions + 1
        && !words.iter().any(|word| matches!(word.as_str(), "RETURN" | "GOTO" | "IF" | "WHILE"))
        && !words.windows(2).any(|pair| pair[0] == "FOR" && matches!(pair[1].as_str(), "XML" | "JSON" | "BROWSE"))
}

pub struct MsDriver {
    cfg: ConnectionConfig,
    client: Client<Compat<TcpStream>>,
    database: String,
    cursors: std::collections::HashMap<i32, CursorEntry>,
    pending_cursors: std::collections::HashSet<String>,
}

impl MsDriver {
    pub async fn connect(cfg: &ConnectionConfig) -> Result<Self> {
        let client = Self::open(cfg, &cfg.database).await?;
        let mut d = Self { cfg: cfg.clone(), client, database: cfg.database.clone(), cursors: Default::default(), pending_cursors: Default::default() };
        if let Ok(r) = d.query_one("SELECT DB_NAME()").await {
            d.database = r.cell(0, 0).unwrap_or("master").to_string();
        }
        Ok(d)
    }

    async fn open(cfg: &ConnectionConfig, db: &str) -> Result<Client<Compat<TcpStream>>> {
        let mut c = Config::new();
        c.host(&cfg.host);
        c.port(cfg.port);
        c.application_name("Strata");
        if !db.is_empty() {
            c.database(db);
        }
        c.authentication(AuthMethod::sql_server(&cfg.user, &cfg.password));
        c.encryption(match cfg.ssl {
            SslMode::Disable => EncryptionLevel::Off,
            SslMode::Prefer => EncryptionLevel::On,
            SslMode::Require | SslMode::VerifyFull => EncryptionLevel::Required,
        });
        if cfg.ssl != SslMode::VerifyFull {
            c.trust_cert();
        }
        let tcp = TcpStream::connect(c.get_addr()).await.context("Can't reach SQL Server")?;
        tcp.set_nodelay(true)?;
        Ok(Client::connect(c, tcp.compat_write()).await.context("Can't log in to SQL Server")?)
    }
}

fn cell_text(row: &tiberius::Row, i: usize, data: &ColumnData<'static>) -> Cell {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
    match data {
        ColumnData::U8(v) => v.map(|x| x.to_string()),
        ColumnData::I16(v) => v.map(|x| x.to_string()),
        ColumnData::I32(v) => v.map(|x| x.to_string()),
        ColumnData::I64(v) => v.map(|x| x.to_string()),
        ColumnData::F32(v) => v.map(|x| x.to_string()),
        ColumnData::F64(v) => v.map(|x| x.to_string()),
        ColumnData::Bit(v) => v.map(|x| if x { "1".into() } else { "0".into() }),
        ColumnData::String(v) => v.as_ref().map(|s| s.to_string()),
        ColumnData::Guid(v) => v.map(|g| g.to_string().to_uppercase()),
        ColumnData::Binary(v) => v.as_ref().map(|b| format!("0x{}", hex::encode_upper(b))),
        ColumnData::Numeric(v) => v.map(|n| n.to_string()),
        ColumnData::Xml(v) => v.as_ref().map(|x| x.to_string()),
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            row.try_get::<NaiveDateTime, _>(i).ok().flatten().map(|d| d.to_string())
        }
        ColumnData::Date(_) => row.try_get::<NaiveDate, _>(i).ok().flatten().map(|d| d.to_string()),
        ColumnData::Time(_) => row.try_get::<NaiveTime, _>(i).ok().flatten().map(|d| d.to_string()),
        ColumnData::DateTimeOffset(_) => row.try_get::<DateTime<FixedOffset>, _>(i).ok().flatten().map(|d| d.to_rfc3339()),
        #[allow(unreachable_patterns)]
        other => Some(format!("{other:?}")),
    }
}

#[async_trait]
impl Driver for MsDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::SqlServer
    }

    fn current_database(&self) -> String {
        self.database.clone()
    }

    async fn execute(&mut self, sql: &str) -> Result<Vec<QueryResult>> {
        self.recover_cursor_open().await?;
        self.execute_inner(sql).await
    }

    async fn execute_paged(&mut self, sql: &str, page_size: usize) -> Result<Vec<QueryResult>> {
        if page_size == 0 || !crate::query_transaction::is_managed_read_only(sql, Dialect::SqlServer) {
            return self.execute(sql).await;
        }
        let statements = super::statements(sql);
        if statements.iter().any(|statement| !cursor_compatible(statement)) { return self.execute(sql).await; }
        let mut out = Vec::new();
        for statement in statements {
            self.recover_cursor_open().await?;
            let started = std::time::Instant::now();
            let stmt = sqlgen::lit(Dialect::SqlServer, &statement);
            let name = format!("strata_page_{}", uuid::Uuid::new_v4().simple());
            // The query is a quoted literal, never executable text in the outer batch.
            // FAST_FORWARD + READ_ONLY preserves the query's own ordering and TOP clauses.
            let open = format!("DECLARE @handle int, @status int, @scroll int = 16, @concurrency int = 1; \
                EXEC @status = sys.sp_cursoropen @handle OUTPUT, {stmt}, @scroll OUTPUT, @concurrency OUTPUT; \
                IF @status = 0 EXEC sys.sp_cursoroption @handle, 2, N'{name}'; \
                SELECT @handle AS __strata_cursor_handle, @status AS __strata_cursor_status;");
            self.pending_cursors.insert(name.clone());
            let results = self.execute_inner(&open).await?;
            let control = results.iter().find(|result| result.columns == ["__strata_cursor_handle", "__strata_cursor_status"])
                .context("SQL Server did not return a result cursor")?;
            anyhow::ensure!(control.cell(0, 1) == Some("0"), "SQL Server could not open this query for incremental loading");
            let cursor = control.cell(0, 0).and_then(|value| value.parse::<i32>().ok()).context("SQL Server returned an invalid result cursor")?;
            let mut metadata = results.iter().find(|result| result.has_rows() && result.columns != control.columns).cloned().unwrap_or_default();
            metadata.rows.clear();
            metadata.sql = statement;
            let owner = std::sync::Arc::new(());
            self.cursors.insert(cursor, CursorEntry { metadata, owner: std::sync::Arc::downgrade(&owner), name: name.clone() });
            self.pending_cursors.remove(&name);
            let mut page = self.fetch_page(cursor, page_size).await?;
            if page.cursor.is_some() { page.cursor_owner = Some(owner); }
            page.elapsed_ms = started.elapsed().as_millis() as u64;
            out.push(page);
        }
        Ok(out)
    }

    async fn fetch_page(&mut self, cursor: i32, page_size: usize) -> Result<QueryResult> {
        self.recover_cursor_open().await?;
        let entry = self.cursors.get(&cursor).context("This result is closed; run the query again")?;
        let owner = entry.owner.upgrade().context("This result is closed; run the query again")?;
        let metadata = entry.metadata.clone();
        let count = page_size.clamp(1, i32::MAX as usize);
        let fetched = self.execute_inner(&format!("EXEC sys.sp_cursorfetch {cursor}, 2, 0, {count};")).await?;
        let mut page = fetched.into_iter().find(QueryResult::has_rows).unwrap_or_else(|| metadata.clone());
        let fetched_count = page.rows.len();
        // API cursors append a hidden row-status column. Missing rows contain dummy data.
        if page.columns.last().is_some_and(|name| name.eq_ignore_ascii_case("rowstat")) {
            page.columns.pop();
            page.types.pop();
            page.rows.retain_mut(|row| row.pop().flatten().as_deref() != Some("2"));
        }
        page.sql = metadata.sql;
        if fetched_count < count {
            self.close_cursor(cursor).await?;
        } else {
            page.cursor = Some(cursor);
            page.cursor_owner = Some(owner);
        }
        Ok(page)
    }

    async fn close_cursor(&mut self, cursor: i32) -> Result<()> {
        if cursor == -1 {
            self.recover_cursor_open().await?;
            let handles: Vec<_> = self.cursors.keys().copied().collect();
            for handle in handles { self.close_owned_cursor(handle).await?; }
        } else if self.cursors.contains_key(&cursor) {
            self.close_owned_cursor(cursor).await?;
        }
        Ok(())
    }

    async fn close(&mut self) {
        let _ = self.close_cursor(-1).await;
    }

    async fn switch_database(&mut self, db: &str) -> Result<()> {
        self.close_cursor(-1).await?;
        self.client.simple_query(format!("USE {}", sqlgen::quote_ident(Dialect::SqlServer, db))).await?.into_results().await?;
        self.database = db.to_string();
        let _ = &self.cfg;
        Ok(())
    }
}

impl MsDriver {
    async fn close_owned_cursor(&mut self, cursor: i32) -> Result<()> {
        if let Some(entry) = self.cursors.get(&cursor) {
            // Commit/rollback or end-of-results may already have closed the cursor.
            let cleanup = format!("IF CURSOR_STATUS('global', N'{}') >= -1 EXEC sys.sp_cursorclose {cursor};", entry.name);
            self.execute_inner(&cleanup).await?;
            self.cursors.remove(&cursor);
        }
        Ok(())
    }

    async fn recover_cursor_open(&mut self) -> Result<()> {
        // A canceled open is named before its handle is returned. Clean up that name
        // only; another query tab can own other live cursors on this same connection.
        for name in self.pending_cursors.clone() {
            let ident = sqlgen::quote_ident(Dialect::SqlServer, &name);
            let cleanup = format!("IF CURSOR_STATUS('global', N'{name}') >= 0 CLOSE GLOBAL {ident}; \
                IF CURSOR_STATUS('global', N'{name}') >= -1 DEALLOCATE GLOBAL {ident};");
            self.execute_inner(&cleanup).await?;
            self.pending_cursors.remove(&name);
        }
        let abandoned: Vec<_> = self.cursors.iter().filter(|(_, entry)| entry.owner.strong_count() == 0).map(|(&handle, _)| handle).collect();
        for handle in abandoned { self.close_owned_cursor(handle).await?; }
        Ok(())
    }

    async fn execute_inner(&mut self, sql: &str) -> Result<Vec<QueryResult>> {
        let started = std::time::Instant::now();
        let mut out: Vec<QueryResult> = Vec::new();
        {
            let mut stream = self.client.simple_query(sql).await?;
            while let Some(item) = stream.try_next().await? {
                match item {
                    QueryItem::Metadata(meta) => out.push(QueryResult {
                        columns: meta.columns().iter().map(|c| c.name().to_string()).collect(),
                        types: meta.columns().iter().map(|c| format!("{:?}", c.column_type()).to_lowercase()).collect(),
                        ..Default::default()
                    }),
                    QueryItem::Row(row) => {
                        let cells = row.cells().enumerate().map(|(i, (_, d))| cell_text(&row, i, d)).collect();
                        if let Some(r) = out.last_mut() {
                            r.rows.push(cells);
                        }
                    }
                }
            }
        }
        if out.is_empty() {
            out.push(QueryResult { message: Some("Query executed".into()), ..Default::default() });
        }
        let elapsed = started.elapsed().as_millis() as u64;
        for r in &mut out {
            r.elapsed_ms = elapsed;
            r.sql = sql.to_string();
        }
        if sql.trim_start().to_ascii_uppercase().starts_with("USE ") {
            let rows = self.client.simple_query("SELECT DB_NAME()").await?.into_first_result().await?;
            if let Some(name) = rows.first().and_then(|r| r.get::<&str, _>(0)) {
                self.database = name.to_string();
            }
        }
        Ok(out)
    }

}
