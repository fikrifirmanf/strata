//! Persistent domain types: connections, groups, settings, saved queries, history, boards.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DriverKind {
    Postgres,
    CockroachDb,
    Redshift,
    Greenplum,
    MySql,
    MariaDb,
    Sqlite,
    LibSql,
    CloudflareD1,
    SqlServer,
    ClickHouse,
    Redis,
    MongoDb,
    Elasticsearch,
    Oracle,
    Snowflake,
    BigQuery,
    Cassandra,
    DynamoDb,
    DuckDb,
    Vertica,
}

/// How a driver family speaks SQL. Several products share one dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    SqlServer,
    ClickHouse,
    Redis,
    Mongo,
    Elastic,
    Oracle,
    Ansi,
}

impl DriverKind {
    pub const ALL: [DriverKind; 21] = [
        DriverKind::Postgres,
        DriverKind::MySql,
        DriverKind::MariaDb,
        DriverKind::Sqlite,
        DriverKind::SqlServer,
        DriverKind::Redis,
        DriverKind::MongoDb,
        DriverKind::ClickHouse,
        DriverKind::CockroachDb,
        DriverKind::Redshift,
        DriverKind::Greenplum,
        DriverKind::LibSql,
        DriverKind::CloudflareD1,
        DriverKind::Elasticsearch,
        DriverKind::Oracle,
        DriverKind::Snowflake,
        DriverKind::BigQuery,
        DriverKind::Cassandra,
        DriverKind::DynamoDb,
        DriverKind::DuckDb,
        DriverKind::Vertica,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DriverKind::Postgres => "PostgreSQL",
            DriverKind::CockroachDb => "CockroachDB",
            DriverKind::Redshift => "Amazon Redshift",
            DriverKind::Greenplum => "Greenplum",
            DriverKind::MySql => "MySQL",
            DriverKind::MariaDb => "MariaDB",
            DriverKind::Sqlite => "SQLite",
            DriverKind::LibSql => "LibSQL / Turso",
            DriverKind::CloudflareD1 => "Cloudflare D1",
            DriverKind::SqlServer => "SQL Server",
            DriverKind::ClickHouse => "ClickHouse",
            DriverKind::Redis => "Redis",
            DriverKind::MongoDb => "MongoDB",
            DriverKind::Elasticsearch => "Elasticsearch",
            DriverKind::Oracle => "Oracle",
            DriverKind::Snowflake => "Snowflake",
            DriverKind::BigQuery => "Google BigQuery",
            DriverKind::Cassandra => "Apache Cassandra",
            DriverKind::DynamoDb => "Amazon DynamoDB",
            DriverKind::DuckDb => "DuckDB",
            DriverKind::Vertica => "Vertica",
        }
    }

    /// Two-letter badge drawn on connection tiles.
    pub fn badge(self) -> &'static str {
        match self {
            DriverKind::Postgres => "Pg",
            DriverKind::CockroachDb => "Cr",
            DriverKind::Redshift => "Rs",
            DriverKind::Greenplum => "Gp",
            DriverKind::MySql => "My",
            DriverKind::MariaDb => "Ma",
            DriverKind::Sqlite => "Sl",
            DriverKind::LibSql => "Ls",
            DriverKind::CloudflareD1 => "D1",
            DriverKind::SqlServer => "Ms",
            DriverKind::ClickHouse => "Ch",
            DriverKind::Redis => "Rd",
            DriverKind::MongoDb => "Mg",
            DriverKind::Elasticsearch => "Es",
            DriverKind::Oracle => "Or",
            DriverKind::Snowflake => "Sf",
            DriverKind::BigQuery => "Bq",
            DriverKind::Cassandra => "Ca",
            DriverKind::DynamoDb => "Dy",
            DriverKind::DuckDb => "Dk",
            DriverKind::Vertica => "Ve",
        }
    }

    pub fn brand_color(self) -> [u8; 3] {
        match self {
            DriverKind::Postgres => [51, 103, 145],
            DriverKind::CockroachDb => [99, 82, 204],
            DriverKind::Redshift => [140, 79, 255],
            DriverKind::Greenplum => [0, 140, 110],
            DriverKind::MySql => [0, 117, 143],
            DriverKind::MariaDb => [192, 118, 94],
            DriverKind::Sqlite => [44, 130, 201],
            DriverKind::LibSql => [77, 200, 150],
            DriverKind::CloudflareD1 => [243, 128, 32],
            DriverKind::SqlServer => [204, 41, 39],
            DriverKind::ClickHouse => [214, 180, 0],
            DriverKind::Redis => [220, 56, 45],
            DriverKind::MongoDb => [0, 150, 80],
            DriverKind::Elasticsearch => [0, 160, 170],
            DriverKind::Oracle => [210, 65, 55],
            DriverKind::Snowflake => [65, 160, 210],
            DriverKind::BigQuery => [75, 125, 210],
            DriverKind::Cassandra => [90, 160, 180],
            DriverKind::DynamoDb => [60, 120, 185],
            DriverKind::DuckDb => [200, 170, 45],
            DriverKind::Vertica => [90, 150, 100],
        }
    }

    pub fn dialect(self) -> Dialect {
        match self {
            DriverKind::Postgres | DriverKind::CockroachDb | DriverKind::Redshift | DriverKind::Greenplum => Dialect::Postgres,
            DriverKind::MySql | DriverKind::MariaDb => Dialect::MySql,
            DriverKind::Sqlite | DriverKind::LibSql | DriverKind::CloudflareD1 => Dialect::Sqlite,
            DriverKind::SqlServer => Dialect::SqlServer,
            DriverKind::ClickHouse => Dialect::ClickHouse,
            DriverKind::Redis => Dialect::Redis,
            DriverKind::MongoDb => Dialect::Mongo,
            DriverKind::Elasticsearch => Dialect::Elastic,
            DriverKind::Oracle => Dialect::Oracle,
            DriverKind::Snowflake => Dialect::Ansi,
            DriverKind::BigQuery => Dialect::Ansi,
            DriverKind::Cassandra => Dialect::Ansi,
            DriverKind::DynamoDb => Dialect::Ansi,
            DriverKind::DuckDb => Dialect::Ansi,
            DriverKind::Vertica => Dialect::Ansi,
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            DriverKind::Postgres | DriverKind::Greenplum => 5432,
            DriverKind::CockroachDb => 26257,
            DriverKind::Redshift => 5439,
            DriverKind::MySql | DriverKind::MariaDb => 3306,
            DriverKind::SqlServer => 1433,
            DriverKind::ClickHouse => 8123,
            DriverKind::Redis => 6379,
            DriverKind::MongoDb => 27017,
            DriverKind::Elasticsearch => 9200,
            DriverKind::Oracle => 1521,
            DriverKind::Snowflake => 443,
            DriverKind::BigQuery => 443,
            DriverKind::Cassandra => 9042,
            DriverKind::DynamoDb => 443,
            DriverKind::DuckDb => 0,
            DriverKind::Vertica => 5433,
            DriverKind::Sqlite | DriverKind::LibSql | DriverKind::CloudflareD1 => 0,
        }
    }

    pub fn default_user(self) -> &'static str {
        match self {
            DriverKind::Postgres | DriverKind::Greenplum => "postgres",
            DriverKind::CockroachDb => "root",
            DriverKind::MySql | DriverKind::MariaDb => "root",
            DriverKind::SqlServer => "sa",
            DriverKind::ClickHouse => "default",
            DriverKind::Elasticsearch => "elastic",
            _ => "",
        }
    }

    pub fn is_file_based(self) -> bool {
        matches!(self, DriverKind::Sqlite)
    }

    /// HTTP API drivers use `host` as a base URL / account id instead of host:port.
    pub fn is_http_api(self) -> bool {
        matches!(self, DriverKind::LibSql | DriverKind::CloudflareD1)
    }

    pub fn supports_ssh(self) -> bool {
        !self.is_odbc() && !self.is_file_based() && !self.is_http_api()
    }

    /// Vendor ODBC drivers handle cloud authentication, warehouses and native CQL/PartiQL translation.
    pub fn is_odbc(self) -> bool { matches!(self, DriverKind::Oracle | DriverKind::Snowflake | DriverKind::BigQuery | DriverKind::Cassandra | DriverKind::DynamoDb | DriverKind::DuckDb | DriverKind::Vertica) }
    pub fn workload(self) -> &'static str {
        match self {
            DriverKind::Snowflake | DriverKind::BigQuery | DriverKind::Redshift | DriverKind::ClickHouse | DriverKind::DuckDb | DriverKind::Vertica | DriverKind::Greenplum => "OLAP / warehouse",
            DriverKind::Cassandra | DriverKind::DynamoDb | DriverKind::Redis | DriverKind::MongoDb => "Operational / NoSQL",
            DriverKind::Postgres | DriverKind::Oracle | DriverKind::SqlServer | DriverKind::MySql | DriverKind::MariaDb | DriverKind::CockroachDb => "OLTP / analytical SQL",
            _ => "OLTP / embedded SQL",
        }
    }

    pub fn url_schemes(self) -> &'static [&'static str] {
        match self {
            DriverKind::Postgres => &["postgres", "postgresql"],
            DriverKind::CockroachDb => &["cockroach", "cockroachdb"],
            DriverKind::Redshift => &["redshift"],
            DriverKind::Greenplum => &["greenplum"],
            DriverKind::MySql => &["mysql"],
            DriverKind::MariaDb => &["mariadb"],
            DriverKind::Sqlite => &["sqlite", "file"],
            DriverKind::LibSql => &["libsql"],
            DriverKind::CloudflareD1 => &["d1"],
            DriverKind::SqlServer => &["sqlserver", "mssql", "microsoftsqlserver"],
            DriverKind::ClickHouse => &["clickhouse"],
            DriverKind::Redis => &["redis", "rediss"],
            DriverKind::MongoDb => &["mongodb"],
            DriverKind::Elasticsearch => &["elasticsearch", "elastic", "elastics"],
            DriverKind::Oracle => &["oracle"],
            DriverKind::Snowflake => &["snowflake"],
            DriverKind::BigQuery => &["bigquery"],
            DriverKind::Cassandra => &["cassandra"],
            DriverKind::DynamoDb => &["dynamodb"],
            DriverKind::DuckDb => &["duckdb"],
            DriverKind::Vertica => &["vertica"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Tag {
    #[default]
    None,
    Local,
    Development,
    Testing,
    Staging,
    Production,
}

impl Tag {
    pub const ALL: [Tag; 6] = [Tag::None, Tag::Local, Tag::Development, Tag::Testing, Tag::Staging, Tag::Production];

    pub fn label(self) -> &'static str {
        match self {
            Tag::None => "No tag",
            Tag::Local => "local",
            Tag::Development => "development",
            Tag::Testing => "testing",
            Tag::Staging => "staging",
            Tag::Production => "production",
        }
    }

    pub fn color(self) -> [u8; 3] {
        match self {
            Tag::None => [125, 135, 148],
            Tag::Local => [91, 214, 154],
            Tag::Development => [92, 200, 224],
            Tag::Testing => [143, 180, 216],
            Tag::Staging => [242, 178, 82],
            Tag::Production => [255, 107, 91],
        }
    }
}

/// Safety levels for sending statements to a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SafeMode {
    #[default]
    Silent,
    AlertAll,
    AlertWrites,
    PasswordAll,
    PasswordWrites,
}

impl SafeMode {
    pub const ALL: [SafeMode; 5] = [
        SafeMode::Silent,
        SafeMode::AlertAll,
        SafeMode::AlertWrites,
        SafeMode::PasswordAll,
        SafeMode::PasswordWrites,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SafeMode::Silent => "Silent — send queries without warnings",
            SafeMode::AlertAll => "Alert 1 — warn before sending any query",
            SafeMode::AlertWrites => "Alert 2 — warn except SELECT / EXPLAIN / SHOW",
            SafeMode::PasswordAll => "Safe 1 — ask for password before any query",
            SafeMode::PasswordWrites => "Safe 2 — ask for password except reads",
        }
    }


}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SslMode {
    #[default]
    Disable,
    Prefer,
    Require,
    VerifyFull,
}

impl SslMode {
    pub const ALL: [SslMode; 4] = [SslMode::Disable, SslMode::Prefer, SslMode::Require, SslMode::VerifyFull];
    pub fn label(self) -> &'static str {
        match self {
            SslMode::Disable => "Disabled",
            SslMode::Prefer => "Preferred",
            SslMode::Require => "Required",
            SslMode::VerifyFull => "Verify full",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SshConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub use_key: bool,
    pub key_path: String,
    /// Not serialized; lives in the OS keychain.
    #[serde(skip)]
    pub password: String,
    #[serde(skip)]
    pub key_passphrase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionConfig {
    pub id: Uuid,
    pub name: String,
    pub driver: DriverKind,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(skip)]
    pub password: String,
    pub database: String,
    /// SQLite file path; Cloudflare account id; LibSQL URL.
    pub path: String,
    #[serde(default)]
    pub odbc_dsn: String,
    pub ssl: SslMode,
    pub ssh: SshConfig,
    pub tag: Tag,
    pub color: Option<[u8; 3]>,
    pub group: Option<Uuid>,
    pub safe_mode: SafeMode,
    pub read_only: bool,
    pub keep_alive: bool,
    pub startup_query: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    /// Tables pinned to the top of the explorer.
    #[serde(default)]
    pub pinned: Vec<String>,
}

impl ConnectionConfig {
    pub fn new(driver: DriverKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: String::new(),
            driver,
            host: if driver.is_file_based() || driver.is_http_api() { String::new() } else { "127.0.0.1".into() },
            port: driver.default_port(),
            user: driver.default_user().into(),
            password: String::new(),
            database: String::new(),
            path: String::new(),
            odbc_dsn: String::new(),
            ssl: default_ssl(driver),
            ssh: SshConfig { port: 22, ..Default::default() },
            tag: Tag::None,
            color: None,
            group: None,
            safe_mode: SafeMode::Silent,
            read_only: false,
            keep_alive: true,
            startup_query: String::new(),
            created_at: chrono::Utc::now(),
            last_used: None,
            pinned: Vec::new(),
        }
    }

    /// A Mongo host field holding a full connection string, which carries its own port, credentials and TLS.
    pub fn mongo_uri(&self) -> bool {
        self.driver == DriverKind::MongoDb && (self.host.starts_with("mongodb://") || self.host.starts_with("mongodb+srv://"))
    }

    /// The first thing the connection form is missing or has wrong, in the terms the driver reads it.
    pub fn form_problem(&self) -> Option<&'static str> {
        let blank = |s: &str| s.trim().is_empty();
        match self.driver {
            d if d.is_odbc() => return blank(&self.odbc_dsn).then_some("Enter the ODBC data source name"),
            DriverKind::Sqlite => return blank(&self.path).then_some("Choose a database file"),
            DriverKind::LibSql => return blank(&self.host).then_some("Enter the database URL"),
            DriverKind::CloudflareD1 => {
                return if blank(&self.host) {
                    Some("Enter the Cloudflare account ID")
                } else if blank(&self.database) {
                    Some("Enter the D1 database ID")
                } else if blank(&self.password) {
                    Some("Enter an API token with D1 access")
                } else {
                    None
                };
            }
            _ => {}
        }
        if blank(&self.host) {
            return Some("Enter a host");
        }
        if !self.mongo_uri() {
            if self.host.contains("://") {
                return Some(if self.driver == DriverKind::MongoDb { "Use a mongodb:// or mongodb+srv:// URI, or a bare host name" } else { "Enter the host name only, without http:// or another scheme" });
            }
            if self.host.trim().contains(char::is_whitespace) {
                return Some("The host can't contain spaces");
            }
            if self.port == 0 {
                return Some("Enter a port");
            }
        }
        if self.driver == DriverKind::Redis && !blank(&self.database) && self.database.trim().trim_start_matches("db").parse::<u32>().is_err() {
            return Some("The Redis database index must be a number, such as 0");
        }
        if self.ssh.enabled && self.driver.supports_ssh() {
            if blank(&self.ssh.host) || blank(&self.ssh.user) {
                return Some("Enter the SSH host and user");
            }
            if self.ssh.use_key && blank(&self.ssh.key_path) {
                return Some("Choose the SSH private key");
            }
        }
        None
    }

    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.clone();
        }
        if self.driver.is_file_based() {
            return std::path::Path::new(&self.path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".into());
        }
        if self.host.is_empty() { "Untitled".into() } else { self.host.clone() }
    }

    pub fn subtitle(&self) -> String {
        if self.driver.is_odbc() { return format!("ODBC · {} · {}", self.odbc_dsn, self.driver.workload()); }
        match self.driver {
            DriverKind::Sqlite => self.path.clone(),
            DriverKind::CloudflareD1 => format!("account {}", self.host),
            DriverKind::LibSql => self.host.clone(),
            _ => {
                let user = if self.user.is_empty() { String::new() } else { format!("{}@", self.user) };
                let db = if self.database.is_empty() { String::new() } else { format!("/{}", self.database) };
                let via = if self.ssh.enabled { format!("  ·  via {}", self.ssh.host) } else { String::new() };
                format!("{user}{}:{}{db}{via}", self.host, self.port)
            }
        }
    }

    pub fn accent(&self) -> [u8; 3] {
        self.color.unwrap_or_else(|| if self.tag == Tag::None { self.driver.brand_color() } else { self.tag.color() })
    }

    /// Connection URL, with or without the password, for "Copy connection URL".
    pub fn to_url(&self, with_password: bool) -> String {
        use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
        let scheme = self.driver.url_schemes()[0];
        if self.driver.is_file_based() {
            return format!("sqlite://{}", self.path);
        }
        let mut auth = String::new();
        if !self.user.is_empty() {
            auth.push_str(&utf8_percent_encode(&self.user, NON_ALPHANUMERIC).to_string());
            if with_password && !self.password.is_empty() {
                auth.push(':');
                auth.push_str(&utf8_percent_encode(&self.password, NON_ALPHANUMERIC).to_string());
            }
            auth.push('@');
        }
        let ssh = if self.ssh.enabled { "+ssh" } else { "" };
        let mut url = format!("{scheme}{ssh}://{auth}{}:{}/{}", self.host, self.port, self.database);
        let mut params = vec![format!("name={}", utf8_percent_encode(&self.display_name(), NON_ALPHANUMERIC))];
        if self.driver.is_odbc() { params.push(format!("dsn={}", utf8_percent_encode(&self.odbc_dsn, NON_ALPHANUMERIC))); }
        if self.ssl != default_ssl(self.driver) {
            params.push(format!("sslmode={}", match self.ssl {
                SslMode::Prefer => "prefer",
                SslMode::Require => "require",
                SslMode::VerifyFull => "verify-full",
                SslMode::Disable => "disable",
            }));
        }
        if self.tag != Tag::None {
            params.push(format!("env={}", self.tag.label()));
        }
        url.push('?');
        url.push_str(&params.join("&"));
        url
    }

    /// Parse `postgres://user:pass@host:5432/db?sslmode=require&name=Prod`.
    pub fn from_url(input: &str) -> anyhow::Result<Self> {
        let input = input.trim();
        if let Some(rest) = input.strip_prefix("sqlite://").or_else(|| input.strip_prefix("file://")) {
            let mut c = ConnectionConfig::new(DriverKind::Sqlite);
            c.path = rest.to_string();
            return Ok(c);
        }
        let escaped = escape_userinfo(input);
        let parsed = url::Url::parse(&escaped)?;
        let raw_scheme = parsed.scheme().to_lowercase();
        let (scheme, via_ssh) = match raw_scheme.strip_suffix("+ssh") {
            Some(s) => (s.to_string(), true),
            None => (raw_scheme.clone(), false),
        };
        let driver = DriverKind::ALL
            .into_iter()
            .find(|d| d.url_schemes().contains(&scheme.as_str()))
            .ok_or_else(|| anyhow::anyhow!("Unsupported scheme '{scheme}'"))?;
        let decode = |s: &str| percent_encoding::percent_decode_str(s).decode_utf8_lossy().to_string();
        let mut c = ConnectionConfig::new(driver);
        c.host = parsed.host_str().unwrap_or("127.0.0.1").to_string();
        c.port = parsed.port().unwrap_or(driver.default_port());
        if !parsed.username().is_empty() {
            c.user = decode(parsed.username());
        }
        c.password = parsed.password().map(decode).unwrap_or_default();
        c.database = decode(parsed.path().trim_start_matches('/'));
        if scheme == "rediss" || scheme == "elastics" {
            c.ssl = SslMode::Require;
        }
        c.ssh.enabled = via_ssh;
        for (k, v) in parsed.query_pairs() {
            match k.as_ref() {
                "name" => c.name = v.to_string(),
                "dsn" => c.odbc_dsn = v.to_string(),
                "sslmode" | "ssl" => {
                    c.ssl = match v.as_ref() {
                        "require" | "true" | "1" => SslMode::Require,
                        "prefer" => SslMode::Prefer,
                        "verify-full" | "verify-ca" => SslMode::VerifyFull,
                        _ => SslMode::Disable,
                    }
                }
                "env" | "tag" => {
                    c.tag = Tag::ALL.into_iter().find(|t| t.label() == v.as_ref()).unwrap_or(Tag::None)
                }
                _ => {}
            }
        }
        Ok(c)
    }
}

/// Postgres-protocol servers start at libpq's own default, "prefer": TLS when the server offers it.
/// Hosted servers such as Azure refuse unencrypted logins outright.
fn default_ssl(driver: DriverKind) -> SslMode {
    if matches!(driver, DriverKind::Postgres | DriverKind::CockroachDb | DriverKind::Redshift | DriverKind::Greenplum) {
        SslMode::Prefer
    } else {
        SslMode::Disable
    }
}

/// Percent-encodes characters in the user:password part that would otherwise end the authority
/// early (`#`, `/`, `?`, `@`, ...), so a password like `abc#1` pasted raw still parses. Existing
/// `%XX` escapes are left alone.
fn escape_userinfo(input: &str) -> String {
    let Some((scheme, rest)) = input.split_once("://") else { return input.to_string() };
    let head_end = rest.find('?').unwrap_or(rest.len());
    let Some(at) = rest[..head_end].rfind('@') else { return input.to_string() };
    let (userinfo, tail) = (&rest[..at], &rest[at..]);
    const SET: &percent_encoding::AsciiSet =
        &percent_encoding::CONTROLS.add(b' ').add(b'#').add(b'/').add(b'?').add(b'@').add(b'[').add(b']').add(b'\\');
    let enc = |s: &str| percent_encoding::utf8_percent_encode(s, SET).to_string();
    let userinfo = match userinfo.split_once(':') {
        Some((u, p)) => format!("{}:{}", enc(u), enc(p)),
        None => enc(userinfo),
    };
    format!("{scheme}://{userinfo}{tail}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub color: [u8; 3],
    pub collapsed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    Auto,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AiProvider {
    #[default]
    Anthropic,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub dark_theme: String,
    pub light_theme: String,
    pub ui_scale: f32,
    pub editor_font_size: f32,
    pub grid_font_size: f32,
    pub page_size: usize,
    pub alternating_rows: bool,
    pub show_row_numbers: bool,
    pub estimate_count_over: u64,
    pub null_display: String,
    pub uppercase_keywords: bool,
    pub auto_close_brackets: bool,
    pub tab_width: usize,
    pub autosave_queries: bool,
    pub query_timeout_secs: u64,
    pub history_days: u32,
    pub passcode_hash: Option<String>,
    pub auto_lock_minutes: u32,
    pub default_safe_mode: SafeMode,
    pub ai_enabled: bool,
    pub ai_provider: AiProvider,
    pub ai_base_url: String,
    pub ai_model: String,
    pub csv_delimiter: char,
    pub reopen_workspaces: bool,
    pub open_workspaces: Vec<Uuid>,
    pub enable_query_params: bool,
    /// 0 = opaque window; higher lets the OS blur (macOS vibrancy, Windows acrylic) show through.
    pub window_translucency: f32,
    /// Value-shape bars under grid headers.
    pub column_profiles: bool,
    pub check_updates: bool,
    /// A release the user chose to skip; the launch check stays quiet about it.
    pub skipped_update: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::Auto,
            dark_theme: "Strata Night".into(),
            light_theme: "Strata Day".into(),
            ui_scale: 1.0,
            editor_font_size: 13.5,
            grid_font_size: 12.5,
            page_size: 300,
            alternating_rows: true,
            show_row_numbers: true,
            estimate_count_over: 500_000,
            null_display: "NULL".into(),
            uppercase_keywords: false,
            auto_close_brackets: true,
            tab_width: 4,
            autosave_queries: true,
            query_timeout_secs: 0,
            history_days: 365,
            passcode_hash: None,
            auto_lock_minutes: 0,
            default_safe_mode: SafeMode::Silent,
            ai_enabled: false,
            ai_provider: AiProvider::Anthropic,
            ai_base_url: "https://api.anthropic.com".into(),
            ai_model: "claude-opus-5-5".into(),
            csv_delimiter: ',',
            reopen_workspaces: true,
            open_workspaces: Vec::new(),
            enable_query_params: true,
            column_profiles: true,
            check_updates: true,
            skipped_update: String::new(),
            window_translucency: if cfg!(any(target_os = "macos", target_os = "windows")) { 0.35 } else { 0.0 },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedQuery {
    pub id: Uuid,
    pub name: String,
    pub sql: String,
    /// `None` = available in every connection.
    pub connection: Option<Uuid>,
    pub favorite: bool,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: chrono::DateTime<chrono::Utc>,
    pub database: String,
    pub sql: String,
    pub elapsed_ms: u64,
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetKind {
    Scoreboard,
    Table,
    Bar,
    Line,
    Pie,
}

impl WidgetKind {
    pub const ALL: [WidgetKind; 5] = [WidgetKind::Scoreboard, WidgetKind::Table, WidgetKind::Bar, WidgetKind::Line, WidgetKind::Pie];
    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::Scoreboard => "Scoreboard",
            WidgetKind::Table => "Table",
            WidgetKind::Bar => "Bar chart",
            WidgetKind::Line => "Line chart",
            WidgetKind::Pie => "Pie chart",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricWidget {
    pub id: Uuid,
    pub title: String,
    pub kind: WidgetKind,
    pub sql: String,
    /// Grid span: 1 or 2 columns wide.
    pub wide: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsBoard {
    pub id: Uuid,
    pub connection: Uuid,
    pub name: String,
    pub refresh_secs: u32,
    pub widgets: Vec<MetricWidget>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_requires_what_each_driver_reads() {
        // A fresh ODBC config gets host 127.0.0.1, which used to let it save without a DSN.
        let mut odbc = ConnectionConfig::new(DriverKind::Snowflake);
        assert!(odbc.form_problem().is_some());
        odbc.odbc_dsn = "SF".into();
        assert_eq!(odbc.form_problem(), None);

        let mut d1 = ConnectionConfig::new(DriverKind::CloudflareD1);
        d1.host = "acct".into();
        assert_eq!(d1.form_problem(), Some("Enter the D1 database ID"));
        d1.database = "db".into();
        assert_eq!(d1.form_problem(), Some("Enter an API token with D1 access"));
        d1.password = "tok".into();
        assert_eq!(d1.form_problem(), None);

        assert_eq!(ConnectionConfig::new(DriverKind::Sqlite).form_problem(), Some("Choose a database file"));
        assert_eq!(ConnectionConfig::new(DriverKind::Postgres).form_problem(), None);
    }

    #[test]
    fn form_rejects_hosts_the_driver_would_mangle() {
        let mut es = ConnectionConfig::new(DriverKind::Elasticsearch);
        es.host = "https://es.example.com".into();
        assert!(es.form_problem().unwrap().contains("without http://"));

        let mut mongo = ConnectionConfig::new(DriverKind::MongoDb);
        mongo.host = "mongodb+srv://u:p@cluster0.example.net".into();
        mongo.port = 0;
        assert!(mongo.mongo_uri());
        assert_eq!(mongo.form_problem(), None, "a URI carries its own port");

        let mut redis = ConnectionConfig::new(DriverKind::Redis);
        redis.database = "cache".into();
        assert!(redis.form_problem().is_some());
        redis.database = "db3".into();
        assert_eq!(redis.form_problem(), None);

        let mut pg = ConnectionConfig::new(DriverKind::Postgres);
        pg.ssh.enabled = true;
        pg.ssh.host = "bastion".into();
        pg.ssh.user = "ops".into();
        pg.ssh.use_key = true;
        assert_eq!(pg.form_problem(), Some("Choose the SSH private key"));
    }

    #[test]
    fn vendor_odbc_urls_preserve_dsn_and_omit_password() {
        for kind in DriverKind::ALL.into_iter().filter(|kind| kind.is_odbc()) {
            let mut cfg = ConnectionConfig::new(kind);
            cfg.odbc_dsn = "Warehouse / staging & reports".into();
            // unslop-ignore HARDCODED_SECRET -- Invented test sentinel verifies that URL export excludes passwords.
            cfg.password = "private-token".into();
            cfg.database = "analytics".into();
            let url = cfg.to_url(false);
            assert!(!url.contains("private-token"));
            let parsed = ConnectionConfig::from_url(&url).unwrap();
            assert_eq!(parsed.driver, kind);
            assert_eq!(parsed.odbc_dsn, cfg.odbc_dsn);
            assert_eq!(parsed.database, "analytics");
        }
    }

    #[test]
    fn parses_postgres_url_with_options() {
        let c = ConnectionConfig::from_url("postgresql://alice:p%40ss@db.example.com:6543/shop?sslmode=require&name=Shop&env=production").unwrap();
        assert_eq!(c.driver, DriverKind::Postgres);
        assert_eq!(c.user, "alice");
        assert_eq!(c.password, "p@ss");
        assert_eq!(c.host, "db.example.com");
        assert_eq!(c.port, 6543);
        assert_eq!(c.database, "shop");
        assert_eq!(c.ssl, SslMode::Require);
        assert_eq!(c.name, "Shop");
        assert_eq!(c.tag, Tag::Production);
    }

    #[test]
    fn raw_special_chars_in_password_parse() {
        let c = ConnectionConfig::from_url("postgresql://Adm:pa#ss/w@rd@db.example.com:5432/postgres").unwrap();
        assert_eq!(c.user, "Adm");
        assert_eq!(c.password, "pa#ss/w@rd");
        assert_eq!(c.host, "db.example.com");
        assert_eq!(c.port, 5432);
        assert_eq!(c.database, "postgres");
    }

    #[test]
    fn form_password_with_special_chars_round_trips_through_url() {
        let mut c = ConnectionConfig::new(DriverKind::Postgres);
        c.user = "Admin01".into();
        c.password = "p#ss@w/rd?x%41".into();
        let back = ConnectionConfig::from_url(&c.to_url(true)).unwrap();
        assert_eq!(back.password, "p#ss@w/rd?x%41");
        assert_eq!(back.user, "Admin01");
    }

    #[test]
    fn postgres_defaults_to_prefer_ssl_and_keeps_explicit_disable() {
        assert_eq!(ConnectionConfig::from_url("postgresql://u@db.example.com/app").unwrap().ssl, SslMode::Prefer);
        assert_eq!(ConnectionConfig::from_url("mysql://u@db.example.com/app").unwrap().ssl, SslMode::Disable);
        let mut c = ConnectionConfig::new(DriverKind::Postgres);
        c.ssl = SslMode::Disable;
        assert_eq!(ConnectionConfig::from_url(&c.to_url(false)).unwrap().ssl, SslMode::Disable);
    }

    #[test]
    fn ssh_suffix_enables_tunnel_and_defaults_port() {
        let c = ConnectionConfig::from_url("mysql+ssh://root@10.0.0.4/app").unwrap();
        assert_eq!(c.driver, DriverKind::MySql);
        assert!(c.ssh.enabled);
        assert_eq!(c.port, 3306);
    }

    #[test]
    fn url_round_trips_without_password() {
        let mut c = ConnectionConfig::new(DriverKind::Postgres);
        c.user = "bob".into();
        c.password = "secret".into();
        c.database = "x".into();
        let url = c.to_url(false);
        assert!(!url.contains("secret"));
        let back = ConnectionConfig::from_url(&url).unwrap();
        assert_eq!(back.user, "bob");
        assert_eq!(back.database, "x");
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert!(ConnectionConfig::from_url("ftp://x").is_err());
    }
}
