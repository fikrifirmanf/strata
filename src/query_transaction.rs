//! A query tab's private connection and managed transaction lifecycle.

use crate::db::{self, Canceller, QueryResult};
use crate::model::{ConnectionConfig, Dialect, DriverKind};
use crate::runtime::Session;
use anyhow::{Context, Result};
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransactionState {
    Idle,
    Active,
    Failed,
    Unknown,
}

pub fn supported(cfg: &ConnectionConfig) -> bool {
    match cfg.driver {
        DriverKind::Postgres
        | DriverKind::CockroachDb
        | DriverKind::Redshift
        | DriverKind::Greenplum
        | DriverKind::MySql
        | DriverKind::MariaDb
        | DriverKind::SqlServer => true,
        DriverKind::Sqlite => {
            let path = cfg.path.trim().to_ascii_lowercase();
            !path.is_empty() && !path.contains(":memory:") && !path.contains("mode=memory")
        }
        _ => false,
    }
}

/// Conservatively allow data/query statements, never session controls or implicit-commit DDL.
/// Lex before splitting so a control word hidden in an executable MySQL comment is rejected too.
#[cfg(test)]
fn validate(sql: &str) -> Result<()> {
    validate_for(sql, Dialect::Postgres)
}

fn validate_for(sql: &str, dialect: Dialect) -> Result<()> {
    validate_access(sql, dialect, false)
}

pub fn is_managed_read_only(sql: &str, dialect: Dialect) -> bool {
    validate_access(sql, dialect, true).is_ok()
}

fn validate_access(sql: &str, dialect: Dialect, read_only: bool) -> Result<()> {
    validate_lex(sql, dialect, false, read_only)?;
    // These engines permit session settings / string prefixes that change backslash escaping.
    // Both interpretations must be safe; never assume an unobserved server setting.
    if matches!(dialect, Dialect::Postgres | Dialect::MySql) {
        validate_lex(sql, dialect, true, read_only)?;
    }
    Ok(())
}

fn validate_lex(
    sql: &str,
    dialect: Dialect,
    backslash_escapes: bool,
    read_only: bool,
) -> Result<()> {
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut words = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-')
                && (dialect != Dialect::MySql || bytes.get(i + 2).is_none_or(|c| *c <= b' ')) =>
            {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'#' if dialect == Dialect::MySql => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                anyhow::ensure!(
                    bytes.get(i + 2) != Some(&b'!')
                        && !(bytes
                            .get(i + 2)
                            .is_some_and(|c| c.eq_ignore_ascii_case(&b'm'))
                            && bytes.get(i + 3) == Some(&b'!')),
                    "Executable comments require Auto-commit mode"
                );
                i += 2;
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    if bytes.get(i..i + 2) == Some(b"/*")
                        && matches!(dialect, Dialect::Postgres | Dialect::SqlServer)
                    {
                        depth += 1;
                        i += 2;
                    } else if bytes.get(i..i + 2) == Some(b"*/") {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                anyhow::ensure!(depth == 0, "Unclosed SQL comment");
            }
            quote @ (b'\'' | b'"' | b'`' | b'[') => {
                let end = if quote == b'[' { b']' } else { quote };
                i += 1;
                let mut closed = false;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && backslash_escapes && quote != b'[' {
                        i += 2;
                    } else if bytes[i] == end {
                        i += 1;
                        if bytes.get(i) == Some(&end) {
                            i += 1;
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                anyhow::ensure!(closed, "Unclosed SQL quote");
                words.push("<quoted>".to_string());
            }
            b'$' if dialect == Dialect::Postgres => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'$') {
                    let tag = &sql[start..=i];
                    let tail = &sql[i + 1..];
                    let end = tail.find(tag).context("Unclosed dollar-quoted SQL")?;
                    i += 1 + end + tag.len();
                    words.push("<quoted>".into());
                }
            }
            b';' => {
                words.push(";".into());
                i += 1;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                words.push(sql[start..i].to_ascii_uppercase());
            }
            _ => {
                i += 1;
            }
        }
    }
    anyhow::ensure!(
        words.iter().any(|word| word != ";"),
        "No executable SQL statement"
    );
    for statement in words
        .split(|word| word == ";")
        .filter(|words| !words.is_empty())
    {
        anyhow::ensure!(matches!(statement[0].as_str(), "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "WITH" | "VALUES" | "EXPLAIN"),
            "This statement requires Auto-commit mode. Use Commit/Rollback in the toolbar for managed transactions");
        let mut saw_select = false;
        let mut set_allowed = false;
        for (i, word) in statement.iter().enumerate() {
            anyhow::ensure!(
                !read_only
                    || !matches!(
                        word.as_str(),
                        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE"
                    ),
                "This connection is read-only"
            );
            anyhow::ensure!(
                !matches!(
                    word.as_str(),
                    "COMMIT"
                        | "ROLLBACK"
                        | "SAVEPOINT"
                        | "RELEASE"
                        | "BEGIN"
                        | "START"
                        | "CREATE"
                        | "ALTER"
                        | "DROP"
                        | "TRUNCATE"
                        | "CALL"
                        | "EXEC"
                        | "EXECUTE"
                        | "OUTFILE"
                        | "DUMPFILE"
                        | "OPENROWSET"
                        | "OPENDATASOURCE"
                        | "USE"
                        | "PRAGMA"
                        | "ATTACH"
                        | "DETACH"
                        | "VACUUM"
                        | "GRANT"
                        | "REVOKE"
                        | "LOCK"
                        | "UNLOCK"
                        | "DECLARE"
                        | "BACKUP"
                        | "RESTORE"
                        | "DBCC"
                        | "KILL"
                        | "PREPARE"
                        | "DEALLOCATE"
                        | "COPY"
                        | "LOAD"
                        | "CHECKPOINT"
                        | "REINDEX"
                ),
                "Transaction controls, session changes and DDL require Auto-commit mode"
            );
            if word == "UPDATE" || (word == "INSERT" && dialect == Dialect::MySql) {
                set_allowed = true;
            }
            if word == "SET" {
                anyhow::ensure!(set_allowed, "Session settings require Auto-commit mode");
                set_allowed = false;
            }
            if word == "SELECT" {
                saw_select = true;
            }
            if word == "INTO" && saw_select {
                anyhow::ensure!(
                    i > 0 && matches!(statement[i - 1].as_str(), "INSERT" | "MERGE"),
                    "SELECT INTO requires Auto-commit mode"
                );
            }
        }
    }
    Ok(())
}

struct Connection {
    session: Option<Session>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            crate::runtime::rt().spawn(async move {
                let mut driver = session.driver.lock().await;
                if let Some(rollback) = db::sqlgen::transaction_words(session.dialect()).2 {
                    let _ = tokio::time::timeout(Duration::from_secs(5), driver.execute(rollback))
                        .await;
                }
                let _ = tokio::time::timeout(Duration::from_secs(5), driver.close()).await;
            });
        }
    }
}

#[derive(Clone)]
pub struct ManagedQuery {
    cfg: ConnectionConfig,
    schema: String,
    connection: Arc<tokio::sync::Mutex<Connection>>,
    state: Arc<AtomicU8>,
    canceller: Arc<Mutex<Option<Canceller>>>,
    cancelled: Arc<AtomicBool>,
}

impl ManagedQuery {
    pub fn new(cfg: ConnectionConfig, schema: String) -> Self {
        Self {
            cfg,
            schema,
            connection: Arc::new(tokio::sync::Mutex::new(Connection { session: None })),
            state: Arc::new(AtomicU8::new(0)),
            canceller: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn state(&self) -> TransactionState {
        match self.state.load(Ordering::SeqCst) {
            0 => TransactionState::Idle,
            1 => TransactionState::Active,
            2 => TransactionState::Failed,
            _ => TransactionState::Unknown,
        }
    }
    fn set_state(&self, state: TransactionState) {
        self.state.store(state as u8, Ordering::SeqCst);
    }
    pub fn pending(&self) -> bool {
        self.state() != TransactionState::Idle
    }
    pub fn prepare_run(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(cancel) = self.canceller.lock() {
            if let Some(cancel) = cancel.as_ref() {
                cancel();
            }
        }
    }

    pub async fn execute(&self, sql: &str, timeout_secs: u64) -> Result<Vec<QueryResult>> {
        validate_for(sql, self.cfg.driver.dialect())?;
        anyhow::ensure!(
            supported(&self.cfg),
            "Persistent manual transactions are unavailable for this connection"
        );
        anyhow::ensure!(
            !self.cfg.read_only || is_managed_read_only(sql, self.cfg.driver.dialect()),
            "This connection is read-only"
        );
        let mut connection = self.connection.lock().await;
        anyhow::ensure!(
            matches!(
                self.state(),
                TransactionState::Idle | TransactionState::Active
            ),
            "Roll back the failed transaction before running another statement"
        );
        if connection.session.is_none() {
            let session =
                tokio::time::timeout(Duration::from_secs(30), Session::open(self.cfg.clone()))
                    .await
                    .context("Opening the query connection timed out")??;
            if self.cfg.driver.dialect() == Dialect::Postgres && !self.schema.is_empty() {
                let sql = format!(
                    "SET search_path TO {}",
                    db::sqlgen::quote_ident(Dialect::Postgres, &self.schema)
                );
                tokio::time::timeout(
                    Duration::from_secs(10),
                    session.driver.lock().await.execute(&sql),
                )
                .await
                .context("Selecting the query schema timed out")??;
            }
            *self.canceller.lock().expect("query canceller lock") = session.canceller.clone();
            connection.session = Some(session);
        }
        anyhow::ensure!(
            !self.cancelled.load(Ordering::SeqCst),
            "Query cancelled before execution"
        );
        let session = connection.session.as_ref().expect("query connection open");
        let mut driver = session.driver.lock().await;
        if !self.pending() {
            self.set_state(TransactionState::Failed);
            let begin = db::sqlgen::transaction_words(session.dialect())
                .0
                .context("Transactions unavailable")?;
            tokio::time::timeout(Duration::from_secs(10), driver.execute(begin))
                .await
                .context("Starting transaction timed out; use Rollback")??;
            self.set_state(TransactionState::Active);
        }
        let result = if timeout_secs > 0 {
            match tokio::time::timeout(Duration::from_secs(timeout_secs), driver.execute(sql)).await
            {
                Ok(result) => result,
                Err(_) => {
                    self.cancel();
                    Err(anyhow::anyhow!("Query timed out after {timeout_secs} seconds; use Rollback before continuing"))
                }
            }
        } else {
            driver.execute(sql).await
        };
        if result.is_err() || self.cancelled.load(Ordering::SeqCst) {
            self.set_state(TransactionState::Failed);
        }
        if self.cancelled.load(Ordering::SeqCst) && result.is_ok() {
            anyhow::bail!("Query cancelled; use Rollback before continuing");
        }
        result
    }

    pub async fn finish(&self, commit: bool) -> Result<()> {
        let mut connection = self.connection.lock().await;
        if !self.pending() {
            return Ok(());
        }
        anyhow::ensure!(
            !commit || self.state() == TransactionState::Active,
            "A failed or uncertain transaction must be rolled back"
        );
        let session = connection
            .session
            .as_ref()
            .context("The connection was lost; the transaction outcome is unknown")?;
        let words = db::sqlgen::transaction_words(session.dialect());
        let sql = if commit { words.1 } else { words.2 }.context("Transactions unavailable")?;
        self.set_state(TransactionState::Unknown);
        let result = {
            let mut driver = session.driver.lock().await;
            match tokio::time::timeout(Duration::from_secs(10), driver.execute(sql)).await {
                Ok(result) => result.map(|_| ()),
                Err(_) => Err(anyhow::anyhow!(
                    "Transaction operation timed out; its outcome is unknown"
                )),
            }
        };
        match result {
            Ok(()) => {
                self.set_state(TransactionState::Idle);
                self.cancelled.store(false, Ordering::SeqCst);
                Ok(())
            }
            Err(error) => {
                if !commit {
                    if let Some(session) = connection.session.take() {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(5),
                            session.driver.lock().await.close(),
                        )
                        .await;
                    }
                    *self.canceller.lock().expect("query canceller lock") = None;
                }
                Err(error.context(
                    "The transaction outcome is uncertain. Check the database before retrying",
                ))
            }
        }
    }

    /// Only exposed after an explicit user decision to close an uncertain connection.
    pub async fn abandon(&self) {
        let mut connection = self.connection.lock().await;
        if let Some(session) = connection.session.take() {
            let _ =
                tokio::time::timeout(Duration::from_secs(5), session.driver.lock().await.close())
                    .await;
        }
        *self.canceller.lock().expect("query canceller lock") = None;
        self.set_state(TransactionState::Idle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Driver;

    #[test]
    fn validates_scripts_before_sending_any_statement() {
        for sql in [
            "SELECT 1; COMMIT",
            "/* comment */ SET autocommit=1",
            "WITH t AS (SELECT 1) SELECT 1; DROP TABLE t",
            "SELECT 1 INTO t",
            "SELECT 1 INTO OUTFILE 'x'",
            "/*! COMMIT */ SELECT 1",
            "USE other",
            "BEGIN",
            "PRAGMA journal_mode=WAL",
            "CALL commit_work()",
        ] {
            assert!(validate(sql).is_err(), "must reject {sql}");
        }
        for sql in [
            "-- hi\nSELECT 'COMMIT; DROP TABLE t'",
            "/* hi */ UPDATE t SET name='BEGIN' WHERE id=1; SELECT name FROM t",
            "WITH t AS (SELECT 1 AS id) SELECT id FROM t",
            "WITH t AS (SELECT 1 AS id) INSERT INTO dest SELECT id FROM t",
            "SELECT CASE WHEN 1=1 THEN 'yes' END",
            "SELECT $$COMMIT;$$",
        ] {
            assert!(validate(sql).is_ok(), "must permit {sql}");
        }
    }

    #[test]
    fn dialect_quoting_and_comments_cannot_hide_transaction_controls() {
        assert!(!is_managed_read_only(
            "WITH t AS (SELECT 1) DELETE\nFROM dest",
            Dialect::Postgres
        ));
        assert!(is_managed_read_only(
            "SELECT 'DELETE FROM dest'",
            Dialect::Postgres
        ));
        for dialect in [
            Dialect::Sqlite,
            Dialect::Postgres,
            Dialect::SqlServer,
            Dialect::MySql,
        ] {
            assert!(validate_for("SELECT '\\'; COMMIT; -- '", dialect).is_err());
            assert!(validate_for("SELECT 1\nUSE otherdb\nUPDATE dbo.t SET x=2", dialect).is_err());
            assert!(validate_for("SELECT 1\nSET IMPLICIT_TRANSACTIONS OFF", dialect).is_err());
        }
        assert!(validate_for("SELECT 1--1; COMMIT", Dialect::MySql).is_err());
        assert!(validate_for("SELECT 1 # 1; COMMIT", Dialect::Postgres).is_err());
        assert!(validate_for("/*M! COMMIT */ SELECT 1", Dialect::MySql).is_err());
        assert!(validate_for("UPDATE t SET x='C:\\files' WHERE id=1", Dialect::Sqlite).is_ok());
        assert!(validate_for("SELECT 'C:\\files'", Dialect::MySql).is_ok());
        assert!(validate_for(
            "SELECT 1; /* outer /* nested */ COMMIT; -- */",
            Dialect::Sqlite
        )
        .is_err());
        assert!(validate_for(
            "SELECT 1; /* outer /* nested */ COMMIT; -- */",
            Dialect::MySql
        )
        .is_err());
    }

    #[test]
    fn only_persistent_native_connections_support_managed_transactions() {
        for kind in [
            DriverKind::Postgres,
            DriverKind::MySql,
            DriverKind::MariaDb,
            DriverKind::SqlServer,
        ] {
            assert!(supported(&ConnectionConfig::new(kind)));
        }
        for kind in [
            DriverKind::LibSql,
            DriverKind::CloudflareD1,
            DriverKind::Oracle,
            DriverKind::Redis,
        ] {
            assert!(!supported(&ConnectionConfig::new(kind)));
        }
        let mut cfg = ConnectionConfig::new(DriverKind::Sqlite);
        for path in [
            ":memory:",
            "file::memory:?cache=shared",
            "file:test?mode=memory",
            "",
        ] {
            cfg.path = path.into();
            assert!(!supported(&cfg), "{path}");
        }
        cfg.path = "example.sqlite".into();
        assert!(supported(&cfg));
    }

    #[tokio::test]
    async fn pending_changes_are_isolated_committed_and_rolled_back() {
        let path = std::env::temp_dir().join(format!(
            "strata-transactions-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let mut cfg = ConnectionConfig::new(DriverKind::Sqlite);
        cfg.path = path.display().to_string();
        let mut observer = crate::db::sqlite::SqliteDriver::open(&cfg).await.unwrap();
        observer.execute("PRAGMA journal_mode=WAL; CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t VALUES(1, 'original')").await.unwrap();
        let query = ManagedQuery::new(cfg.clone(), "main".into());
        query
            .execute("UPDATE t SET name='pending' WHERE id=1", 5)
            .await
            .unwrap();
        assert_eq!(query.state(), TransactionState::Active);
        assert_eq!(
            query.execute("SELECT name FROM t", 5).await.unwrap()[0].cell(0, 0),
            Some("pending")
        );
        assert_eq!(
            observer
                .query_one("SELECT name FROM t")
                .await
                .unwrap()
                .cell(0, 0),
            Some("original")
        );
        // An independent grid-like commit must not finalize this tab's transaction.
        let other_tab = ManagedQuery::new(cfg, "main".into());
        other_tab.execute("SELECT name FROM t", 5).await.unwrap();
        other_tab.finish(true).await.unwrap();
        assert!(query.pending());
        query.finish(false).await.unwrap();
        assert_eq!(
            observer
                .query_one("SELECT name FROM t")
                .await
                .unwrap()
                .cell(0, 0),
            Some("original")
        );
        query
            .execute("UPDATE t SET name='committed' WHERE id=1", 5)
            .await
            .unwrap();
        query.finish(true).await.unwrap();
        assert_eq!(
            observer
                .query_one("SELECT name FROM t")
                .await
                .unwrap()
                .cell(0, 0),
            Some("committed")
        );
        assert!(!query.pending());
        query
            .execute(
                "UPDATE t SET name='still pending' WHERE id=1; SELECT missing FROM t",
                5,
            )
            .await
            .unwrap_err();
        assert_eq!(query.state(), TransactionState::Failed);
        assert!(query.execute("SELECT 1", 5).await.is_err());
        assert!(query.finish(true).await.is_err());
        query.finish(false).await.unwrap();
        assert_eq!(
            observer
                .query_one("SELECT name FROM t")
                .await
                .unwrap()
                .cell(0, 0),
            Some("committed")
        );
        drop(query);
        drop(other_tab);
        drop(observer);
        // Handles close asynchronously on drop; cleanup is best effort on Windows.
        let _ = std::fs::remove_file(path);
    }

    async fn file_connection() -> (ConnectionConfig, crate::db::sqlite::SqliteDriver) {
        let mut cfg = ConnectionConfig::new(DriverKind::Sqlite);
        cfg.path = std::env::temp_dir()
            .join(format!(
                "strata-transaction-errors-{}.sqlite",
                uuid::Uuid::new_v4()
            ))
            .display()
            .to_string();
        let driver = crate::db::sqlite::SqliteDriver::open(&cfg).await.unwrap();
        (cfg, driver)
    }

    #[tokio::test]
    async fn failed_commit_does_not_claim_success_and_allows_rollback() {
        let (cfg, mut observer) = file_connection().await;
        observer.execute("CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(parent_id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED)").await.unwrap();
        let query = ManagedQuery::new(cfg, "main".into());
        query
            .execute("INSERT INTO child VALUES(42)", 5)
            .await
            .unwrap();
        assert!(query.finish(true).await.is_err());
        assert_eq!(query.state(), TransactionState::Unknown);
        assert!(query.pending());
        assert!(query.execute("SELECT 1", 5).await.is_err());
        query.finish(false).await.unwrap();
        assert_eq!(
            observer
                .query_one("SELECT COUNT(*) FROM child")
                .await
                .unwrap()
                .cell(0, 0),
            Some("0")
        );
    }

    #[tokio::test]
    async fn failed_rollback_invalidates_connection_until_explicit_abandon() {
        let (cfg, _observer) = file_connection().await;
        let query = ManagedQuery::new(cfg, "main".into());
        query.execute("SELECT 1", 5).await.unwrap();
        // Simulate a server that has ended the transaction independently.
        query
            .connection
            .lock()
            .await
            .session
            .as_ref()
            .unwrap()
            .driver
            .lock()
            .await
            .execute("ROLLBACK")
            .await
            .unwrap();
        assert!(query.finish(false).await.is_err());
        assert_eq!(query.state(), TransactionState::Unknown);
        assert!(query.connection.lock().await.session.is_none());
        assert!(query.execute("SELECT 1", 5).await.is_err());
        query.abandon().await;
        assert!(!query.pending());
        query.execute("SELECT 1", 5).await.unwrap();
        query.finish(false).await.unwrap();
    }

    #[tokio::test]
    async fn timeout_requires_rollback_before_next_run() {
        let (cfg, _observer) = file_connection().await;
        let query = ManagedQuery::new(cfg, "main".into());
        let error = query.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n", 1).await.unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error:#}");
        assert_eq!(query.state(), TransactionState::Failed);
        assert!(query.execute("SELECT 1", 5).await.is_err());
        query.finish(false).await.unwrap();
        query.execute("SELECT 1", 5).await.unwrap();
        query.finish(false).await.unwrap();
    }
}
