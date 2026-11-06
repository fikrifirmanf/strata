//! Driver abstraction. Every engine implements `execute`; metadata, browsing and edits default to
//! dialect-specific SQL (see `meta` and `sqlgen`) and are overridden by non-SQL engines.

pub mod clickhouse;
pub mod d1;
pub mod elastic;
pub mod libsql;
#[cfg(test)]
mod live_tests;
pub mod meta;
pub mod objects;
pub mod odbc;
pub mod mongo;
pub mod mssql;
pub mod mysql;
pub mod postgres;
pub mod redis;
pub mod sqlgen;
pub mod sqlite;
pub mod ssh;

use crate::model::{ConnectionConfig, Dialect, DriverKind};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub type Cell = Option<String>;

#[derive(Debug, Clone, Default)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub types: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub affected: Option<u64>,
    pub message: Option<String>,
    pub elapsed_ms: u64,
    pub sql: String,
    /// A server-side read cursor with another batch available on this connection.
    pub cursor: Option<i32>,
    /// Keeps an incremental result owned while it is queued or displayed.
    pub cursor_owner: Option<Arc<()>>,
}

impl QueryResult {
    pub fn has_rows(&self) -> bool {
        !self.columns.is_empty()
    }

    pub fn message(msg: impl Into<String>) -> Self {
        Self { message: Some(msg.into()), ..Default::default() }
    }

    pub fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.eq_ignore_ascii_case(name))
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&str> {
        self.rows.get(row)?.get(col)?.as_deref()
    }

    /// First column of every row; handy for metadata listings.
    pub fn first_column(&self) -> Vec<String> {
        self.rows.iter().filter_map(|r| r.first().cloned().flatten()).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectKind {
    Table,
    View,
    MaterializedView,
    Function,
    Procedure,
    Sequence,
    Trigger,
    Enum,
    Type,
    Event,
    Index,
    Extension,
    Constraint,
    Collection,
    Keys,
}

impl ObjectKind {
    pub fn section(self) -> &'static str {
        match self {
            ObjectKind::Table => "Tables",
            ObjectKind::View => "Views",
            ObjectKind::MaterializedView => "Materialized Views",
            ObjectKind::Function => "Functions",
            ObjectKind::Procedure => "Procedures",
            ObjectKind::Sequence => "Sequences",
            ObjectKind::Trigger => "Triggers",
            ObjectKind::Enum => "Enums",
            ObjectKind::Type => "Types",
            ObjectKind::Event => "Events",
            ObjectKind::Index => "Indexes",
            ObjectKind::Extension => "Extensions",
            ObjectKind::Constraint => "Constraints",
            ObjectKind::Collection => "Collections",
            ObjectKind::Keys => "Keys",
        }
    }

    pub fn has_rows(self) -> bool {
        matches!(self, ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView | ObjectKind::Collection | ObjectKind::Keys)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DbObject {
    pub parent: Option<String>,
    pub arguments: Option<String>,
    pub schema: String,
    pub name: String,
    pub kind: ObjectKind,
}

impl DbObject {
    pub fn display_name(&self) -> String {
        if matches!(self.kind, ObjectKind::Function | ObjectKind::Procedure) {
            if let Some(arguments) = &self.arguments { return format!("{}({arguments})", self.name); }
        }
        if matches!(self.kind, ObjectKind::Trigger | ObjectKind::Index | ObjectKind::Constraint) {
            if let Some(parent) = &self.parent { return format!("{} · {parent}", self.name); }
        }
        self.name.clone()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub primary_key: bool,
    pub comment: Option<String>,
    pub extra: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexInfo {
    pub name: String,
    pub columns: String,
    pub unique: bool,
    pub primary: bool,
    pub method: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForeignKey {
    pub name: String,
    pub column: String,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_column: String,
}

/// One column of a foreign key, seen from the schema (both ends named).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Relation {
    pub name: String,
    pub schema: String,
    pub table: String,
    pub column: String,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_column: String,
    /// CASCADE, SET NULL, RESTRICT, NO ACTION…
    pub on_delete: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableStat {
    pub name: String,
    pub rows: Option<u64>,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TableRef {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    In,
    NotIn,
    IsNull,
    IsNotNull,
    Between,
    NotBetween,
    Like,
    NotLike,
    ILike,
    Contains,
    NotContains,
    HasPrefix,
    HasSuffix,
    Raw,
}

impl FilterOp {
    pub const ALL: [FilterOp; 20] = [
        FilterOp::Eq,
        FilterOp::Ne,
        FilterOp::Lt,
        FilterOp::Gt,
        FilterOp::Le,
        FilterOp::Ge,
        FilterOp::In,
        FilterOp::NotIn,
        FilterOp::IsNull,
        FilterOp::IsNotNull,
        FilterOp::Between,
        FilterOp::NotBetween,
        FilterOp::Like,
        FilterOp::NotLike,
        FilterOp::ILike,
        FilterOp::Contains,
        FilterOp::NotContains,
        FilterOp::HasPrefix,
        FilterOp::HasSuffix,
        FilterOp::Raw,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterOp::Eq => "=",
            FilterOp::Ne => "<>",
            FilterOp::Lt => "<",
            FilterOp::Gt => ">",
            FilterOp::Le => "<=",
            FilterOp::Ge => ">=",
            FilterOp::In => "IN",
            FilterOp::NotIn => "NOT IN",
            FilterOp::IsNull => "IS NULL",
            FilterOp::IsNotNull => "IS NOT NULL",
            FilterOp::Between => "BETWEEN",
            FilterOp::NotBetween => "NOT BETWEEN",
            FilterOp::Like => "LIKE",
            FilterOp::NotLike => "NOT LIKE",
            FilterOp::ILike => "ILIKE",
            FilterOp::Contains => "Contains",
            FilterOp::NotContains => "Not contains",
            FilterOp::HasPrefix => "Has prefix",
            FilterOp::HasSuffix => "Has suffix",
            FilterOp::Raw => "Raw SQL",
        }
    }

    pub fn needs_value(self) -> bool {
        !matches!(self, FilterOp::IsNull | FilterOp::IsNotNull)
    }

    pub fn needs_second_value(self) -> bool {
        matches!(self, FilterOp::Between | FilterOp::NotBetween)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub enabled: bool,
    /// `None` = match any column.
    pub column: Option<String>,
    pub op: FilterOp,
    pub value: String,
    pub value2: String,
}

impl Default for Filter {
    fn default() -> Self {
        Self { enabled: true, column: None, op: FilterOp::Contains, value: String::new(), value2: String::new() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BrowseRequest {
    pub filters: Vec<Filter>,
    pub match_any: bool,
    pub sort: Vec<(String, bool)>,
    pub limit: usize,
    pub offset: usize,
}

/// A pending edit to one cell.
#[derive(Debug, Clone, PartialEq)]
pub enum EditValue {
    Text(String),
    Null,
    Default,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowChange {
    Update { key: Vec<(String, Cell)>, set: Vec<(String, EditValue)> },
    Insert { values: Vec<(String, EditValue)> },
    Delete { key: Vec<(String, Cell)> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureFeature {
    Databases,
    EditStructure,
}

pub type Canceller = Arc<dyn Fn() + Send + Sync>;

#[async_trait]
pub trait Driver: Send {
    fn kind(&self) -> DriverKind;

    fn dialect(&self) -> Dialect {
        self.kind().dialect()
    }

    fn current_database(&self) -> String;
    fn default_schema(&self) -> String { meta::default_schema(self.dialect(), &self.current_database()) }

    /// Runs one or more statements and returns one result per statement.
    async fn execute(&mut self, sql: &str) -> Result<Vec<QueryResult>>;

    /// Return the initial editor batch, retaining a cursor when more rows are available.
    async fn execute_paged(&mut self, sql: &str, _page_size: usize) -> Result<Vec<QueryResult>> {
        self.execute(sql).await
    }

    async fn fetch_page(&mut self, _cursor: i32, _page_size: usize) -> Result<QueryResult> {
        anyhow::bail!("This driver has no open result cursor")
    }

    async fn close_cursor(&mut self, _cursor: i32) -> Result<()> { Ok(()) }

    async fn switch_database(&mut self, db: &str) -> Result<()>;

    async fn server_version(&mut self) -> Result<String> {
        let sql = match self.dialect() {
            Dialect::Postgres => "SELECT version()",
            Dialect::MySql => "SELECT VERSION()",
            Dialect::Sqlite => "SELECT sqlite_version()",
            Dialect::SqlServer => "SELECT CAST(SERVERPROPERTY('ProductVersion') AS varchar(128))",
            Dialect::ClickHouse => "SELECT version()",
            _ => anyhow::bail!("Server version is unavailable for this driver"),
        };
        let result = self.query_one(sql).await?;
        result.cell(0, 0).map(str::to_owned).ok_or_else(|| anyhow::anyhow!("The server returned no version"))
    }

    async fn ping(&mut self) -> Result<()> {
        self.execute("SELECT 1").await.map(|_| ())
    }

    fn canceller(&self) -> Option<Canceller> {
        None
    }

    async fn query_one(&mut self, sql: &str) -> Result<QueryResult> {
        let mut all = self.execute(sql).await?;
        Ok(all.pop().unwrap_or_default())
    }

    async fn databases(&mut self) -> Result<Vec<String>> {
        match meta::databases_sql(self.dialect()) {
            Some(sql) => Ok(self.query_one(sql).await?.first_column()),
            None => Ok(vec![self.current_database()]),
        }
    }

    async fn schemas(&mut self) -> Result<Vec<String>> {
        match meta::schemas_sql(self.dialect()) {
            Some(sql) => Ok(self.query_one(sql).await?.first_column()),
            None => Ok(vec![meta::default_schema(self.dialect(), &self.current_database())]),
        }
    }

    async fn objects(&mut self, schema: &str) -> Result<Vec<DbObject>> {
        let dialect = self.dialect();
        let db = self.current_database();
        let mut out = Vec::new();
        for (sql, fixed_kind) in meta::objects_sql(dialect, schema, &db) {
            let res = match self.query_one(&sql).await {
                Ok(r) => r,
                // Routines listings need privileges some users lack; tables are what matters.
                Err(e) if fixed_kind.is_some() => {
                    eprintln!("strata: listing skipped: {e}");
                    continue;
                }
                Err(e) => return Err(e),
            };
            for row in &res.rows {
                let Some(name) = row.first().cloned().flatten() else { continue };
                let kind = fixed_kind.unwrap_or_else(|| meta::kind_from_type(row.get(1).cloned().flatten().as_deref()));
                out.push(DbObject { parent: row.get(2).cloned().flatten(), arguments: row.get(3).cloned().flatten(), schema: schema.to_string(), name, kind });
            }
        }
        out.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        out.dedup();
        Ok(out)
    }

    async fn columns(&mut self, t: &TableRef) -> Result<Vec<ColumnInfo>> {
        let sql = meta::columns_sql(self.dialect(), t, &self.current_database());
        let res = self.query_one(&sql).await?;
        Ok(meta::parse_columns(self.dialect(), &res))
    }

    /// (table, column) pairs for a whole schema, used for autocomplete and diagrams.
    async fn all_columns(&mut self, schema: &str) -> Result<Vec<(String, String)>> {
        let Some(sql) = meta::all_columns_sql(self.dialect(), schema, &self.current_database()) else { return Ok(vec![]) };
        let res = self.query_one(&sql).await?;
        Ok(res.rows.iter().filter_map(|r| Some((r.first()?.clone()?, r.get(1)?.clone()?))).collect())
    }

    /// Every foreign key declared in a schema.
    async fn relations(&mut self, schema: &str) -> Result<Vec<Relation>> {
        let Some(sql) = meta::relations_sql(self.dialect(), schema, &self.current_database(), None) else { return Ok(vec![]) };
        Ok(meta::parse_relations(&self.query_one(&sql).await?))
    }

    /// Foreign keys in other tables that point at `t`.
    async fn referencing(&mut self, t: &TableRef) -> Result<Vec<Relation>> {
        let Some(sql) = meta::relations_sql(self.dialect(), &t.schema, &self.current_database(), Some(&t.name)) else { return Ok(vec![]) };
        Ok(meta::parse_relations(&self.query_one(&sql).await?))
    }

    /// Estimated row counts and on-disk sizes per table, where the catalog keeps them.
    async fn table_stats(&mut self, schema: &str) -> Result<Vec<TableStat>> {
        let Some(sql) = meta::table_stats_sql(self.dialect(), schema, &self.current_database()) else { return Ok(vec![]) };
        let res = self.query_one(&sql).await?;
        let num = |v: Option<&Cell>| v.cloned().flatten().and_then(|s| s.trim().parse::<f64>().ok()).map(|f| f.max(0.0) as u64);
        Ok(res.rows.iter().filter_map(|r| Some(TableStat { name: r.first()?.clone()?, rows: num(r.get(1)), bytes: num(r.get(2)) })).collect())
    }

    async fn indexes(&mut self, t: &TableRef) -> Result<Vec<IndexInfo>> {
        let Some(sql) = meta::indexes_sql(self.dialect(), t, &self.current_database()) else { return Ok(vec![]) };
        let res = self.query_one(&sql).await?;
        Ok(meta::parse_indexes(self.dialect(), &res))
    }

    async fn foreign_keys(&mut self, t: &TableRef) -> Result<Vec<ForeignKey>> {
        let Some(sql) = meta::foreign_keys_sql(self.dialect(), t, &self.current_database()) else { return Ok(vec![]) };
        let res = self.query_one(&sql).await?;
        Ok(meta::parse_foreign_keys(self.dialect(), &res, &t.schema))
    }

    async fn ddl(&mut self, obj: &DbObject) -> Result<String> {
        if let Some(sql) = meta::ddl_sql(self.dialect(), obj) {
            let res = self.query_one(&sql).await?;
            if let Some(text) = meta::pick_ddl(self.dialect(), &res) {
                return Ok(text);
            }
        }
        if !obj.kind.has_rows() { anyhow::bail!("The server did not return a definition for this object"); }
        let t = TableRef { schema: obj.schema.clone(), name: obj.name.clone() };
        let cols = self.columns(&t).await?;
        let idx = self.indexes(&t).await.unwrap_or_default();
        Ok(sqlgen::create_table_ddl(self.dialect(), &t, &cols, &idx))
    }

    async fn browse(&mut self, t: &TableRef, req: &BrowseRequest, columns: &[ColumnInfo]) -> Result<QueryResult> {
        let sql = sqlgen::browse_sql(self.dialect(), t, req, columns);
        let mut r = self.query_one(&sql).await?;
        r.sql = sql;
        Ok(r)
    }

    async fn count(&mut self, t: &TableRef, req: &BrowseRequest, columns: &[ColumnInfo]) -> Result<u64> {
        let sql = sqlgen::count_sql(self.dialect(), t, req, columns);
        let r = self.query_one(&sql).await?;
        Ok(r.cell(0, 0).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0) as u64)
    }

    fn preview_changes(&self, t: &TableRef, changes: &[RowChange]) -> Vec<String> {
        changes.iter().map(|c| sqlgen::change_sql(self.dialect(), t, c)).collect()
    }

    async fn apply_changes(&mut self, t: &TableRef, changes: &[RowChange]) -> Result<()> {
        let stmts = self.preview_changes(t, changes);
        self.run_in_transaction(&stmts).await
    }

    async fn run_in_transaction(&mut self, stmts: &[String]) -> Result<()> {
        if stmts.iter().any(|sql| objects::requires_standalone(self.dialect(), sql)) {
            anyhow::ensure!(stmts.len() == 1, "This operation must run on its own, outside a transaction");
            self.execute(&stmts[0]).await?;
            return Ok(());
        }
        let (begin, commit, rollback) = sqlgen::transaction_words(self.dialect());
        if let Some(b) = begin {
            self.execute(b).await?;
        }
        for s in stmts {
            if let Err(e) = self.execute(s).await {
                if let Some(r) = rollback {
                    let _ = self.execute(r).await;
                }
                return Err(e.context(format!("while running: {}", sqlgen::ellipsize(s, 200))));
            }
        }
        if let Some(c) = commit {
            self.execute(c).await?;
        }
        Ok(())
    }

    async fn processes(&mut self) -> Result<QueryResult> {
        match meta::processes_sql(self.dialect()) {
            Some(sql) => self.query_one(sql).await,
            None => anyhow::bail!("This feature is not available for this driver."),
        }
    }

    async fn kill_process(&mut self, id: &str) -> Result<()> {
        match meta::kill_sql(self.dialect(), id) {
            Some(sql) => self.execute(&sql).await.map(|_| ()),
            None => anyhow::bail!("This feature is not available for this driver."),
        }
    }

    async fn users(&mut self) -> Result<QueryResult> {
        match meta::users_sql(self.dialect()) {
            Some(sql) => self.query_one(sql).await,
            None => anyhow::bail!("This feature is not available for this driver."),
        }
    }

    async fn close(&mut self) {}
}

pub type DriverBox = Box<dyn Driver>;

/// Statements a commit would send, without touching the connection.
pub fn preview_changes(kind: DriverKind, t: &TableRef, changes: &[RowChange]) -> Vec<String> {
    match kind.dialect() {
        Dialect::Redis => redis::preview(changes),
        Dialect::Mongo => mongo::preview(t, changes),
        d => changes.iter().map(|c| sqlgen::change_sql(d, t, c)).collect(),
    }
}

/// Opens a connection, including any SSH tunnel and the startup query.
pub async fn connect(cfg: &ConnectionConfig) -> Result<(DriverBox, Option<ssh::Tunnel>)> {
    let mut cfg = cfg.clone();
    let mut tunnel = None;
    if cfg.ssh.enabled && cfg.driver.supports_ssh() {
        let t = ssh::Tunnel::open(&cfg.ssh, &cfg.host, cfg.port).await?;
        cfg.host = "127.0.0.1".into();
        cfg.port = t.local_port;
        tunnel = Some(t);
    }
    let mut drv: DriverBox = match cfg.driver {
        kind if kind.is_odbc() => Box::new(odbc::OdbcDriver::connect(&cfg).await?),
        DriverKind::Postgres | DriverKind::CockroachDb | DriverKind::Redshift | DriverKind::Greenplum => {
            Box::new(postgres::PgDriver::connect(&cfg).await?)
        }
        DriverKind::MySql | DriverKind::MariaDb => Box::new(mysql::MyDriver::connect(&cfg).await?),
        DriverKind::Sqlite => Box::new(sqlite::SqliteDriver::open(&cfg).await?),
        DriverKind::LibSql => Box::new(libsql::LibSqlDriver::connect(&cfg).await?),
        DriverKind::CloudflareD1 => Box::new(d1::D1Driver::connect(&cfg).await?),
        DriverKind::SqlServer => Box::new(mssql::MsDriver::connect(&cfg).await?),
        DriverKind::ClickHouse => Box::new(clickhouse::ChDriver::connect(&cfg).await?),
        DriverKind::Redis => Box::new(redis::RedisDriver::connect(&cfg).await?),
        DriverKind::MongoDb => Box::new(mongo::MongoDriver::connect(&cfg).await?),
        DriverKind::Elasticsearch => Box::new(elastic::EsDriver::connect(&cfg).await?),
        _ => anyhow::bail!("Unsupported driver"),
    };
    if !cfg.startup_query.trim().is_empty() {
        drv.execute(&cfg.startup_query).await.map_err(|e| e.context("startup query failed"))?;
    }
    Ok((drv, tunnel))
}

/// Splits a script into statements on `;`, honouring quotes, comments and dollar-quoting.
pub fn split_statements(sql: &str) -> Vec<(usize, usize)> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\'' | b'"' | b'`' | b'[' => {
                let q = if b[i] == b'[' { b']' } else { b[i] };
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' && q == b'\'' {
                        i += 2;
                        continue;
                    }
                    if b[i] == q {
                        if i + 1 < b.len() && b[i + 1] == q {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'#' if i == 0 || b[i - 1] == b'\n' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'$' => {
                // $tag$ ... $tag$
                let mut j = i + 1;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                if j < b.len() && b[j] == b'$' {
                    let tag = &sql[i..=j];
                    if let Some(end) = sql[j + 1..].find(tag) {
                        i = j + 1 + end + tag.len() - 1;
                    } else {
                        i = b.len();
                    }
                }
            }
            b';' => {
                out.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < sql.len() {
        out.push((start, sql.len()));
    }
    out.into_iter().filter(|(s, e)| !strip_comments(&sql[*s..(*e).min(sql.len())]).trim().is_empty()).collect()
}

pub fn statements(sql: &str) -> Vec<String> {
    split_statements(sql).into_iter().map(|(s, e)| sql[s..e].trim().to_string()).collect()
}

fn strip_comments(s: &str) -> String {
    s.lines().filter(|l| !l.trim_start().starts_with("--")).collect::<Vec<_>>().join("\n")
}

/// True when every statement only reads (the "except SELECT/EXPLAIN/SHOW" rule).
pub fn is_read_only(sql: &str) -> bool {
    statements(sql).iter().all(|s| {
        let first = strip_comments(s).trim_start().split(|c: char| c.is_whitespace() || c == '(').find(|w| !w.is_empty()).unwrap_or("").to_ascii_uppercase();
        matches!(
            first.as_str(),
            "SELECT" | "EXPLAIN" | "SHOW" | "DESCRIBE" | "DESC" | "WITH" | "PRAGMA" | "VALUES" | "TABLE"
                | "GET" | "HGET" | "HGETALL" | "LRANGE" | "SMEMBERS" | "ZRANGE" | "KEYS" | "SCAN" | "TTL" | "TYPE" | "INFO" | "EXISTS"
        ) && !(first == "WITH" && contains_write_keyword(s))
            && !(first == "SELECT" && s.to_ascii_uppercase().contains(" INTO "))
    })
}

fn contains_write_keyword(s: &str) -> bool {
    let up = s.to_ascii_uppercase();
    ["INSERT ", "UPDATE ", "DELETE ", "MERGE "].iter().any(|k| up.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_respecting_quotes_comments_and_dollar_bodies() {
        let sql = "select ';' as a; -- c;\nselect 2;\ncreate function f() returns int as $$ begin return 1; end $$ language plpgsql; select 'it''s;'";
        let parts = statements(sql);
        assert_eq!(parts.len(), 4, "{parts:?}");
        assert_eq!(parts[0], "select ';' as a");
        assert!(parts[2].contains("return 1; end $$"));
        assert_eq!(parts[3], "select 'it''s;'");
    }

    #[test]
    fn drops_comment_only_segments() {
        assert_eq!(statements("-- just a note\n;select 1;").len(), 1);
    }

    #[test]
    fn classifies_reads_and_writes() {
        // unslop-ignore NO_SELECT_STAR -- This is non-executed SQL input for safety-classifier regression tests.
        assert!(is_read_only("SELECT * FROM t; explain select 1"));
        assert!(is_read_only("  show tables"));
        // unslop-ignore WRITE_WITHOUT_WHERE -- Regression-test SQL is only passed to the safety classifier, not executed.
        assert!(!is_read_only("select 1; delete from t"));
        // unslop-ignore NO_SELECT_STAR -- This is non-executed SQL input for safety-classifier regression tests.
        // unslop-ignore WRITE_WITHOUT_WHERE -- Regression-test SQL is only passed to the safety classifier, not executed.
        assert!(!is_read_only("WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x")); // unslop-ignore NO_SELECT_STAR -- Safety-classifier input only; this SQL is never executed.
        assert!(!is_read_only("select * into backup from t"));
        // unslop-ignore WRITE_WITHOUT_WHERE -- Regression-test SQL is only passed to the safety classifier, not executed.
        assert!(!is_read_only("update t set a=1"));
    }
}
