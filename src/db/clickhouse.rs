//! ClickHouse over its HTTP interface (JSONCompact output), or over the native TCP protocol
//! when the port turns out to be the one `clickhouse-client` uses (9000, or a forward to it).

use super::*;
use crate::model::SslMode;
use klickhouse::{KlickhouseError, Type, Value};
use serde_json::Value as Json;
use tokio_stream::StreamExt;

pub struct ChDriver {
    transport: Transport,
    cfg: ConnectionConfig,
    user: String,
    password: String,
    database: String,
}

enum Transport {
    Http { http: reqwest::Client, base: String },
    Native(klickhouse::Client),
}

/// What ClickHouse answers when an HTTP request reaches its native port.
const NATIVE_PORT_REPLY: &str = "is for clickhouse-client program";

pub(crate) fn json_cell(v: &Json) -> Cell {
    match v {
        Json::Null => None,
        Json::String(s) => Some(s.clone()),
        Json::Bool(b) => Some(b.to_string()),
        Json::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

pub(crate) fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("Strata/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

impl ChDriver {
    pub async fn connect(cfg: &ConnectionConfig) -> Result<Self> {
        let database = if cfg.database.is_empty() { "default".to_string() } else { cfg.database.clone() };
        let user = if cfg.user.is_empty() { "default".to_string() } else { cfg.user.clone() };
        // 9000 and 9440 are ClickHouse's native ports; anything else is tried over HTTP first.
        if matches!(cfg.port, 9000 | 9440) {
            let client = native_connect(cfg, &user, &database).await?;
            return Ok(Self { transport: Transport::Native(client), cfg: cfg.clone(), user, password: cfg.password.clone(), database });
        }
        let scheme = if cfg.ssl == SslMode::Disable { "http" } else { "https" };
        let transport = Transport::Http { http: http_client()?, base: format!("{scheme}://{}:{}/", cfg.host, cfg.port) };
        let mut d = Self { transport, cfg: cfg.clone(), user, password: cfg.password.clone(), database };
        match d.ping().await {
            Ok(()) => Ok(d),
            // A forwarded port can lead to the native port; switch protocols instead of failing.
            Err(e) if e.to_string().contains(NATIVE_PORT_REPLY) => {
                d.transport = Transport::Native(native_connect(cfg, &d.user, &d.database).await?);
                Ok(d)
            }
            Err(e) => Err(e),
        }
    }

    async fn one(&mut self, sql: &str) -> Result<QueryResult> {
        let (http, base) = match &self.transport {
            Transport::Http { http, base } => (http, base),
            Transport::Native(_) => return self.one_native(sql).await,
        };
        let started = std::time::Instant::now();
        let resp = http
            .post(base)
            .query(&[("database", self.database.as_str()), ("default_format", "JSONCompact")])
            .header("X-ClickHouse-User", &self.user)
            .header("X-ClickHouse-Key", &self.password)
            .body(sql.to_string())
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("{}", body.trim());
        }
        let mut r = QueryResult { sql: sql.to_string(), elapsed_ms: started.elapsed().as_millis() as u64, ..Default::default() };
        if body.trim().is_empty() {
            r.message = Some("OK".into());
            return Ok(r);
        }
        let json: Json = serde_json::from_str(&body).map_err(|_| anyhow::anyhow!("{}", body.trim()))?;
        if let Some(meta) = json["meta"].as_array() {
            r.columns = meta.iter().map(|m| m["name"].as_str().unwrap_or("").to_string()).collect();
            r.types = meta.iter().map(|m| m["type"].as_str().unwrap_or("").to_string()).collect();
        }
        if let Some(rows) = json["data"].as_array() {
            r.rows = rows.iter().map(|row| row.as_array().map(|c| c.iter().map(json_cell).collect()).unwrap_or_default()).collect();
        }
        Ok(r)
    }

    async fn one_native(&mut self, sql: &str) -> Result<QueryResult> {
        let started = std::time::Instant::now();
        let mut r = match self.native_blocks(sql).await {
            Ok(r) => r,
            // The native client can't decode some newer types (Nothing, Date32, JSON, Variant);
            // for a read query, let the server turn every column into text instead.
            Err(NativeError::Type(_)) if is_select(sql) => {
                let mut r = self.native_blocks(&format!("SELECT * APPLY(x -> CAST(x AS Nullable(String))) FROM (\n{}\n)", sql.trim().trim_end_matches(';'))).await.map_err(NativeError::into_anyhow)?;
                r.types.iter_mut().for_each(|t| *t = "String".into());
                r
            }
            Err(e) => return Err(e.into_anyhow()),
        };
        r.sql = sql.to_string();
        r.elapsed_ms = started.elapsed().as_millis() as u64;
        if r.columns.is_empty() {
            r.message = Some("OK".into());
        }
        Ok(r)
    }

    async fn native_blocks(&mut self, sql: &str) -> std::result::Result<QueryResult, NativeError> {
        if matches!(&self.transport, Transport::Native(c) if c.is_closed()) {
            self.transport = Transport::Native(native_connect(&self.cfg, &self.user, &self.database).await.map_err(NativeError::Other)?);
        }
        let Transport::Native(client) = &self.transport else { unreachable!("native_blocks needs the native transport") };
        failure_log().take();
        let mut stream = client.query_raw(sql, None).await.map_err(NativeError::from)?;
        let mut r = QueryResult::default();
        let mut types: Vec<Type> = Vec::new();
        while let Some(block) = stream.next().await {
            let block = block.map_err(NativeError::from)?;
            if r.columns.is_empty() && !block.column_types.is_empty() {
                r.columns = block.column_types.keys().cloned().collect();
                r.types = block.column_types.values().map(|t| type_name(&t.to_string())).collect();
                types = block.column_types.values().cloned().collect();
            }
            let columns: Vec<&Vec<Value>> = block.column_data.values().collect();
            for i in 0..block.rows as usize {
                r.rows.push(columns.iter().zip(&types).map(|(c, t)| c.get(i).and_then(|v| native_cell(v, t))).collect());
            }
        }
        // When the client fails to decode a block, its task stops and the stream just ends;
        // the reason only reaches the log.
        tokio::task::yield_now().await;
        if client.is_closed() {
            let reason = failure_log().wait().await.unwrap_or_else(|| "The ClickHouse connection closed".into());
            if reason.contains("TypeParseError") {
                return Err(NativeError::Type(reason));
            }
            return Err(NativeError::Other(anyhow::anyhow!("{}", server_message(&reason).unwrap_or(reason))));
        }
        Ok(r)
    }
}

enum NativeError {
    Type(String),
    Other(anyhow::Error),
}

impl From<KlickhouseError> for NativeError {
    fn from(e: KlickhouseError) -> Self {
        match e {
            KlickhouseError::TypeParseError(m) => NativeError::Type(m),
            // The stack trace is for server logs; the message already names the error.
            KlickhouseError::ServerException { message, .. } => NativeError::Other(anyhow::anyhow!("{message}")),
            other => NativeError::Other(other.into()),
        }
    }
}

impl NativeError {
    fn into_anyhow(self) -> anyhow::Error {
        match self {
            NativeError::Type(m) => anyhow::anyhow!("Strata can't read this ClickHouse type over the native protocol yet: {m}"),
            NativeError::Other(e) => e,
        }
    }
}

/// klickhouse spells decimals by width (`Decimal64(2)`); ClickHouse itself says `Decimal(18, 2)`.
fn type_name(t: &str) -> String {
    let mut out = t.to_string();
    for (width, precision) in [("Decimal32(", 9), ("Decimal64(", 18), ("Decimal128(", 38), ("Decimal256(", 76)] {
        while let Some(at) = out.find(width) {
            let rest = &out[at + width.len()..];
            let Some(close) = rest.find(')') else { break };
            let scale = rest[..close].to_string();
            out.replace_range(at..at + width.len() + close + 1, &format!("Decimal({precision}, {scale})"));
        }
    }
    out
}

fn is_select(sql: &str) -> bool {
    let head = sql.trim_start().split(|c: char| c.is_whitespace() || c == '(').next().unwrap_or("").to_ascii_uppercase();
    head == "SELECT" || head == "WITH"
}

async fn native_connect(cfg: &ConnectionConfig, user: &str, database: &str) -> Result<klickhouse::Client> {
    let mut options = klickhouse::ClientOptions { username: user.to_string(), password: cfg.password.clone(), default_database: database.to_string(), ..Default::default() };
    // klickhouse asks for compressed blocks but reads only LZ4; newer servers default to ZSTD.
    options.settings.insert("network_compression_method".into(), "LZ4".into());
    let addr = (cfg.host.as_str(), cfg.port);
    let timeout = std::time::Duration::from_secs(15);
    let connect = async {
        if cfg.ssl == SslMode::Disable {
            return Ok(klickhouse::Client::connect(addr, options).await?);
        }
        let mut b = native_tls::TlsConnector::builder();
        // Same rule as Postgres: only "verify full" checks the certificate chain and host name.
        if cfg.ssl != SslMode::VerifyFull {
            b.danger_accept_invalid_certs(true).danger_accept_invalid_hostnames(true);
        }
        let tls = tokio_native_tls::TlsConnector::from(b.build()?);
        let tcp = tokio::net::TcpStream::connect(addr).await?;
        tcp.set_nodelay(true)?;
        match tls.connect(&cfg.host, tcp).await {
            Ok(stream) => {
                let (read, write) = tokio::io::split(stream);
                Ok(klickhouse::Client::connect_stream(read, write, options).await?)
            }
            Err(_) if cfg.ssl == SslMode::Prefer => Ok(klickhouse::Client::connect(addr, options).await?),
            Err(e) => Err(anyhow::Error::from(e)),
        }
    };
    let failure = failure_log();
    failure.take();
    match tokio::time::timeout(timeout, connect).await {
        Err(_) => Err(anyhow::anyhow!("Timed out connecting to ClickHouse at {}:{}", cfg.host, cfg.port)),
        Ok(Ok(client)) => Ok(client),
        // klickhouse logs the server's reason (wrong password, unknown database) and returns only
        // "channel closed"; show the logged reason instead.
        // The background task logs its reason just after our call sees the closed channel.
        Ok(Err(e)) => Err(failure.wait().await.map(|m| anyhow::anyhow!("{}", server_message(&m).unwrap_or(m))).unwrap_or(e)),
    }
}

/// Keeps the last connection failure klickhouse logs, which its connect error leaves out.
struct FailureLog(std::sync::Mutex<Option<String>>);

impl FailureLog {
    fn take(&self) -> Option<String> {
        self.0.lock().ok()?.take()
    }

    /// Waits briefly for the reason the client's background task logs as it stops.
    async fn wait(&self) -> Option<String> {
        for _ in 0..20 {
            if let Some(m) = self.take() {
                return Some(m);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        None
    }
}

impl log::Log for FailureLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() == log::Level::Error && m.target().starts_with("klickhouse")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            if let Ok(mut slot) = self.0.lock() {
                *slot = Some(record.args().to_string());
            }
        }
    }

    fn flush(&self) {}
}

fn failure_log() -> &'static FailureLog {
    static LOG: FailureLog = FailureLog(std::sync::Mutex::new(None));
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        // Strata installs no other logger; if something else did, errors stay generic.
        if log::set_logger(&LOG).is_ok() {
            log::set_max_level(log::LevelFilter::Error);
        }
    });
    &LOG
}

/// The first line of `message` in a logged `ServerException { .. message: "..", .. }`.
fn server_message(logged: &str) -> Option<String> {
    let start = logged.find("message: \"")? + "message: \"".len();
    let rest = &logged[start..];
    let end = rest.find("\", stack_trace").unwrap_or(rest.len());
    let first = rest[..end].split("\\n").next().unwrap_or("").replace("\\\"", "\"");
    let first = first.trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// A native value as the grid shows it: the text ClickHouse itself prints, NULL as `None`.
fn native_cell(v: &Value, t: &Type) -> Cell {
    match v {
        Value::Null => None,
        _ => Some(native_text(v, t, false)),
    }
}

fn base_type(t: &Type) -> &Type {
    match t {
        Type::Nullable(inner) | Type::LowCardinality(inner) => base_type(inner),
        other => other,
    }
}

fn decimal_text(digits: String, scale: usize) -> String {
    let (sign, digits) = match digits.strip_prefix('-') { Some(d) => ("-", d.to_string()), None => ("", digits) };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = padded.split_at(padded.len() - scale);
    format!("{sign}{int}.{frac}")
}

/// `nested` formats like ClickHouse's text output inside arrays, tuples and maps: strings and
/// dates are quoted there, bare at the top level.
fn native_text(v: &Value, t: &Type, nested: bool) -> String {
    let t = base_type(t);
    let quote = |s: String| if nested { format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")) } else { s };
    match v {
        Value::Null => "NULL".into(),
        Value::Int8(x) => x.to_string(),
        Value::Int16(x) => x.to_string(),
        Value::Int32(x) => x.to_string(),
        Value::Int64(x) => x.to_string(),
        Value::Int128(x) => x.to_string(),
        Value::Int256(x) => x.to_string(),
        Value::UInt8(x) => x.to_string(),
        Value::UInt16(x) => x.to_string(),
        Value::UInt32(x) => x.to_string(),
        Value::UInt64(x) => x.to_string(),
        Value::UInt128(x) => x.to_string(),
        Value::UInt256(x) => x.to_string(),
        Value::Float32(x) => x.to_string(),
        Value::Float64(x) => x.to_string(),
        Value::Decimal32(s, x) => decimal_text(x.to_string(), *s),
        Value::Decimal64(s, x) => decimal_text(x.to_string(), *s),
        Value::Decimal128(s, x) => decimal_text(x.to_string(), *s),
        Value::Decimal256(s, x) => decimal_text(x.to_string(), *s),
        Value::String(bytes) => {
            let s = String::from_utf8_lossy(bytes);
            let s = if matches!(t, Type::FixedString(_)) { s.trim_end_matches('\0').to_string() } else { s.into_owned() };
            quote(s)
        }
        Value::Uuid(u) => quote(u.to_string()),
        Value::Date(d) => quote((chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default() + chrono::Duration::days(d.0 as i64)).format("%Y-%m-%d").to_string()),
        Value::DateTime(dt) => quote(chrono::DateTime::<chrono_tz::Tz>::try_from(*dt).map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_else(|_| dt.1.to_string())),
        Value::DateTime64(dt) => {
            let precision = dt.2.min(9);
            let text = chrono::DateTime::<chrono_tz::Tz>::try_from(dt.clone())
                .map(|d| {
                    let base = d.format("%Y-%m-%d %H:%M:%S").to_string();
                    if precision == 0 { base } else { format!("{base}.{}", &format!("{:09}", d.timestamp_subsec_nanos())[..precision]) }
                })
                .unwrap_or_else(|_| dt.1.to_string());
            quote(text)
        }
        Value::Enum8(x) => match t {
            Type::Enum8(names) => names.iter().find(|(_, v)| v == x).map(|(n, _)| quote(n.clone())).unwrap_or_else(|| x.to_string()),
            _ => x.to_string(),
        },
        Value::Enum16(x) => match t {
            Type::Enum16(names) => names.iter().find(|(_, v)| v == x).map(|(n, _)| quote(n.clone())).unwrap_or_else(|| x.to_string()),
            _ => x.to_string(),
        },
        Value::Array(items) => {
            let inner = match t { Type::Array(inner) => inner.as_ref(), other => other };
            format!("[{}]", items.iter().map(|i| native_text(i, inner, true)).collect::<Vec<_>>().join(","))
        }
        Value::Tuple(items) => {
            let types: &[Type] = match t { Type::Tuple(ts) => ts, _ => &[] };
            format!("({})", items.iter().enumerate().map(|(i, v)| native_text(v, types.get(i).unwrap_or(&Type::String), true)).collect::<Vec<_>>().join(","))
        }
        Value::Map(keys, values) => {
            let (kt, vt) = match t { Type::Map(k, v) => (k.as_ref(), v.as_ref()), _ => (&Type::String, &Type::String) };
            format!("{{{}}}", keys.iter().zip(values).map(|(k, v)| format!("{}:{}", native_text(k, kt, true), native_text(v, vt, true))).collect::<Vec<_>>().join(","))
        }
        Value::Ipv4(ip) => quote(ip.0.to_string()),
        Value::Ipv6(ip) => quote(ip.0.to_string()),
        Value::Point(p) => format!("({},{})", p.0[0], p.0[1]),
        other => format!("{other:?}"),
    }
}

#[async_trait]
impl Driver for ChDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::ClickHouse
    }

    fn current_database(&self) -> String {
        self.database.clone()
    }

    async fn execute(&mut self, sql: &str) -> Result<Vec<QueryResult>> {
        let mut out = Vec::new();
        for stmt in statements(sql) {
            if let Some(db) = stmt.strip_prefix("USE ").or_else(|| stmt.strip_prefix("use ")) {
                let db = db.trim().trim_matches(|c| c == '`' || c == '"').to_string();
                self.switch_database(&db).await?;
                out.push(QueryResult::message(format!("Database changed to {}", self.database)));
                continue;
            }
            out.push(self.one(&stmt).await?);
        }
        Ok(out)
    }

    async fn switch_database(&mut self, db: &str) -> Result<()> {
        self.database = db.to_string();
        // A native session keeps its database, so start one in the new database.
        if matches!(self.transport, Transport::Native(_)) {
            self.transport = Transport::Native(native_connect(&self.cfg, &self.user, &self.database).await?);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_keep_their_sign_and_scale() {
        assert_eq!(decimal_text("-150".into(), 2), "-1.50");
        assert_eq!(decimal_text("5".into(), 3), "0.005");
        assert_eq!(decimal_text("-5".into(), 3), "-0.005");
        assert_eq!(decimal_text("42".into(), 0), "42");
    }

    #[test]
    fn values_print_like_clickhouse_text() {
        let s = |x: &str| Value::String(x.as_bytes().to_vec());
        assert_eq!(native_cell(&Value::Null, &Type::Nullable(Box::new(Type::String))), None);
        assert_eq!(native_cell(&s("it's"), &Type::String).as_deref(), Some("it's"));
        assert_eq!(native_text(&Value::Array(vec![s("a"), s("b'c")]), &Type::Array(Box::new(Type::String)), false), "['a','b\\'c']");
        let e = Type::Enum8(vec![("on".into(), 1), ("off".into(), 2)]);
        assert_eq!(native_text(&Value::Enum8(2), &e, false), "off");
        let m = Type::Map(Box::new(Type::String), Box::new(Type::UInt8));
        assert_eq!(native_text(&Value::Map(vec![s("k")], vec![Value::UInt8(1)]), &m, false), "{'k':1}");
        assert_eq!(native_text(&Value::Date(klickhouse::Date(20398)), &Type::Date, false), "2025-11-06");
        assert_eq!(native_text(&s("ab\0\0"), &Type::FixedString(4), false), "ab");
    }

    #[test]
    fn reads_the_server_reason_from_a_logged_failure() {
        let logged = r#"clickhouse client failed: ServerException { code: 516, name: "DB::Exception", message: "DB::Exception: default: Authentication failed: password is incorrect.\n\nIf you have installed", stack_trace: "0. x" }"#;
        assert_eq!(server_message(logged).as_deref(), Some("DB::Exception: default: Authentication failed: password is incorrect."));
        assert_eq!(server_message("channel closed"), None);
    }

    #[test]
    fn decimal_types_use_clickhouse_spelling() {
        assert_eq!(type_name("Nullable(Decimal64(2))"), "Nullable(Decimal(18, 2))");
        assert_eq!(type_name("Array(Decimal128(4))"), "Array(Decimal(38, 4))");
        assert_eq!(type_name("String"), "String");
    }

    #[test]
    fn only_selects_get_the_text_fallback() {
        assert!(is_select("  select 1"));
        assert!(is_select("WITH x AS (SELECT 1) SELECT * FROM x"));
        assert!(!is_select("SHOW TABLES"));
        assert!(!is_select("INSERT INTO t SELECT 1"));
    }
}
