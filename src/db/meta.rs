//! Catalog queries per dialect and the parsers that turn their rows into typed metadata.

use super::sqlgen::{lit, qualified};
use super::*;
use crate::model::Dialect;

pub fn supports(d: Dialect, f: StructureFeature) -> bool {
    match f {
        StructureFeature::Databases => matches!(d, Dialect::Postgres | Dialect::MySql | Dialect::SqlServer | Dialect::ClickHouse | Dialect::Mongo | Dialect::Redis),
        StructureFeature::EditStructure => matches!(d, Dialect::Postgres | Dialect::MySql | Dialect::SqlServer | Dialect::ClickHouse | Dialect::Sqlite | Dialect::Mongo),
    }
}

/// How far the structure editor may change one column attribute.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attr {
    Hidden,
    /// Shown for existing columns, settable only on columns being added.
    NewOnly,
    Editable,
}

impl Attr {
    pub fn shown(self) -> bool {
        self != Attr::Hidden
    }

    pub fn editable(self, is_new: bool) -> bool {
        self == Attr::Editable || (self == Attr::NewOnly && is_new)
    }
}

/// What the structure editor shows for one engine: column attributes, the type catalog and index options.
/// Every `Editable` attribute has a matching statement in `sqlgen::structure_sql`.
pub struct StructureForm {
    pub title: &'static str,
    pub item: &'static str,
    pub columns_editable: bool,
    pub data_type: Attr,
    pub nullable: Attr,
    pub default: Attr,
    pub comment: Attr,
    pub default_type: &'static str,
    pub types: &'static [(&'static str, &'static [&'static str])],
    pub index_methods: &'static [&'static str],
    pub unique_index: bool,
    pub index_hint: &'static str,
    /// Primary and foreign keys can be changed with ALTER TABLE.
    pub keys_editable: bool,
    /// Referential actions offered for ON DELETE / ON UPDATE; the first is the engine default.
    pub fk_actions: &'static [&'static str],
}

const PG_TYPES: &[(&str, &[&str])] = &[
    ("Numeric", &["smallint", "integer", "bigint", "serial", "bigserial", "numeric(10,2)", "real", "double precision"]),
    ("Text", &["text", "varchar(255)", "char(1)"]),
    ("Date & time", &["date", "time", "timestamp", "timestamptz", "interval"]),
    ("Other", &["boolean", "uuid", "json", "jsonb", "bytea", "inet", "cidr", "macaddr", "tsvector", "point"]),
    ("Arrays", &["text[]", "integer[]", "bigint[]", "uuid[]"]),
];

const MYSQL_TYPES: &[(&str, &[&str])] = &[
    ("Numeric", &["tinyint", "smallint", "mediumint", "int", "bigint", "int unsigned", "bigint unsigned", "decimal(10,2)", "float", "double", "bit(1)", "boolean"]),
    ("Text", &["varchar(255)", "char(36)", "tinytext", "text", "mediumtext", "longtext", "enum('a','b')", "set('a','b')"]),
    ("Date & time", &["date", "datetime", "timestamp", "time", "year"]),
    ("Binary", &["binary(16)", "varbinary(255)", "blob", "mediumblob", "longblob"]),
    ("Other", &["json", "geometry", "point"]),
];

const SQLITE_TYPES: &[(&str, &[&str])] = &[
    ("Storage classes", &["INTEGER", "REAL", "TEXT", "BLOB", "NUMERIC"]),
    ("Common aliases", &["BOOLEAN", "VARCHAR(255)", "DATE", "DATETIME", "DECIMAL(10,2)"]),
];

const MSSQL_TYPES: &[(&str, &[&str])] = &[
    ("Numeric", &["tinyint", "smallint", "int", "bigint", "bit", "decimal(18,2)", "numeric(18,2)", "money", "float", "real"]),
    ("Text", &["nvarchar(255)", "nvarchar(max)", "varchar(255)", "varchar(max)", "nchar(10)", "char(10)"]),
    ("Date & time", &["date", "time", "datetime2", "datetimeoffset", "datetime", "smalldatetime"]),
    ("Other", &["uniqueidentifier", "varbinary(max)", "xml"]),
];

const CLICKHOUSE_TYPES: &[(&str, &[&str])] = &[
    ("Integers", &["UInt8", "UInt16", "UInt32", "UInt64", "Int8", "Int16", "Int32", "Int64"]),
    ("Decimals", &["Float32", "Float64", "Decimal(18,4)"]),
    ("Text", &["String", "FixedString(16)", "LowCardinality(String)", "Enum8('a' = 1, 'b' = 2)"]),
    ("Date & time", &["Date", "Date32", "DateTime", "DateTime64(3)"]),
    ("Nullable", &["Nullable(String)", "Nullable(Int64)", "Nullable(Float64)", "Nullable(DateTime)"]),
    ("Other", &["Bool", "UUID", "IPv4", "IPv6", "JSON", "Array(String)", "Map(String, String)"]),
];

pub fn structure_form(d: Dialect) -> StructureForm {
    use Attr::*;
    let sql = StructureForm {
        title: "Table structure",
        item: "Column",
        columns_editable: true,
        data_type: Editable,
        nullable: Editable,
        default: Editable,
        comment: Editable,
        default_type: "text",
        types: PG_TYPES,
        index_methods: &[],
        unique_index: true,
        index_hint: "col_a, col_b",
        keys_editable: false,
        fk_actions: &[],
    };
    match d {
        Dialect::Postgres => StructureForm {
            index_methods: &["btree", "hash", "gin", "gist", "brin", "spgist"],
            keys_editable: true,
            fk_actions: &["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
            ..sql
        },
        Dialect::MySql => StructureForm {
            default_type: "varchar(255)",
            types: MYSQL_TYPES,
            index_methods: &["BTREE", "HASH", "FULLTEXT", "SPATIAL"],
            keys_editable: true,
            fk_actions: &["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
            ..sql
        },
        // SQLite has no column comments and rewrites a table to change type, nullability or default.
        Dialect::Sqlite => StructureForm { data_type: NewOnly, nullable: NewOnly, default: NewOnly, comment: Hidden, default_type: "TEXT", types: SQLITE_TYPES, ..sql },
        // Changing a default means finding its named constraint; comments live in extended properties.
        Dialect::SqlServer => StructureForm {
            default: NewOnly,
            comment: Hidden,
            default_type: "nvarchar(255)",
            types: MSSQL_TYPES,
            index_methods: &["NONCLUSTERED", "CLUSTERED"],
            keys_editable: true,
            fk_actions: &["NO ACTION", "CASCADE", "SET NULL", "SET DEFAULT"],
            ..sql
        },
        // Nullability is part of the type: Nullable(String).
        Dialect::ClickHouse => StructureForm {
            nullable: Hidden,
            default_type: "String",
            types: CLICKHOUSE_TYPES,
            index_methods: &["minmax", "set(100)", "bloom_filter", "ngrambf_v1(3, 256, 2, 0)", "tokenbf_v1(256, 2, 0)"],
            unique_index: false,
            index_hint: "col_a, col_b",
            ..sql
        },
        // Collections have no declared columns: fields and their BSON types are sampled from documents.
        Dialect::Mongo => StructureForm {
            title: "Collection schema",
            item: "Field",
            columns_editable: false,
            nullable: NewOnly,
            default: Hidden,
            comment: Hidden,
            default_type: "string",
            types: &[],
            index_methods: &["1", "-1", "text", "hashed", "2dsphere"],
            index_hint: "email, created_at:-1",
            ..sql
        },
        _ => StructureForm { columns_editable: false, data_type: NewOnly, nullable: NewOnly, default: NewOnly, comment: NewOnly, types: &[], ..sql },
    }
}

pub fn default_schema(d: Dialect, db: &str) -> String {
    match d {
        Dialect::Postgres => "public".into(),
        Dialect::SqlServer => "dbo".into(),
        Dialect::Sqlite => "main".into(),
        _ => db.to_string(),
    }
}

pub fn databases_sql(d: Dialect) -> Option<&'static str> {
    match d {
        Dialect::Postgres => Some("SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname"),
        Dialect::MySql => Some("SHOW DATABASES"),
        Dialect::SqlServer => Some("SELECT name FROM sys.databases ORDER BY name"),
        Dialect::ClickHouse => Some("SELECT name FROM system.databases ORDER BY name"),
        _ => None,
    }
}

pub fn schemas_sql(d: Dialect) -> Option<&'static str> {
    match d {
        Dialect::Postgres => Some(
            "SELECT nspname FROM pg_namespace WHERE nspname NOT LIKE 'pg\\_toast%' AND nspname NOT LIKE 'pg\\_temp%' ORDER BY nspname = 'public' DESC, nspname",
        ),
        Dialect::SqlServer => Some(
            "SELECT name FROM sys.schemas WHERE name NOT IN ('guest','INFORMATION_SCHEMA','sys') AND name NOT LIKE 'db\\_%' ESCAPE '\\' ORDER BY CASE WHEN name='dbo' THEN 0 ELSE 1 END, name",
        ),
        Dialect::Sqlite => Some("SELECT name FROM pragma_database_list ORDER BY seq"),
        _ => None,
    }
}

/// Read one database's namespaces without issuing USE or changing the session context.
pub fn sqlserver_completion_sql(database: &str) -> String {
    let database = sqlgen::quote_ident(Dialect::SqlServer, database);
    // unslop-ignore NO_SQL_INTERPOLATION -- The database identifier is escaped by quote_ident.
    format!("SELECT s.name, o.name, c.name FROM {database}.sys.schemas AS s \
        LEFT JOIN {database}.sys.objects AS o ON o.schema_id = s.schema_id AND o.type IN ('U','V') AND o.is_ms_shipped = 0 \
        LEFT JOIN {database}.sys.columns AS c ON c.object_id = o.object_id \
        WHERE s.name NOT IN ('guest','INFORMATION_SCHEMA','sys') AND s.name NOT LIKE 'db\\_%' ESCAPE '\\' \
        ORDER BY CASE WHEN s.name = 'dbo' THEN 0 ELSE 1 END, s.name, o.name, c.column_id")
}

/// Each query returns (name, type?) rows. A fixed kind means the second column is ignored.
pub fn objects_sql(d: Dialect, schema: &str, db: &str) -> Vec<(String, Option<ObjectKind>)> {
    let s = lit(d, schema);
    match d {
        Dialect::Postgres => vec![
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = {s}"), None),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT matviewname FROM pg_matviews WHERE schemaname = {s}"), Some(ObjectKind::MaterializedView)),
            (
                // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
                format!("SELECT p.proname, CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END, NULL, pg_get_function_identity_arguments(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname={s} AND p.prokind IN ('f','p')"),
                None,
            ),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT sequence_name FROM information_schema.sequences WHERE sequence_schema = {s}"), Some(ObjectKind::Sequence)),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT t.tgname, 'TRIGGER', c.relname FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={s} AND NOT t.tgisinternal"), Some(ObjectKind::Trigger)),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT t.typname, CASE t.typtype WHEN 'e' THEN 'ENUM' ELSE 'TYPE' END, NULL, CASE t.typtype WHEN 'd' THEN 'DOMAIN' ELSE 'TYPE' END FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace LEFT JOIN pg_class c ON c.oid=t.typrelid WHERE n.nspname={s} AND (t.typtype IN ('e','d') OR (t.typtype='c' AND c.relkind='c'))"), None),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT indexname, 'INDEX', tablename FROM pg_indexes WHERE schemaname={s}"), Some(ObjectKind::Index)),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT e.extname FROM pg_extension e JOIN pg_namespace n ON n.oid=e.extnamespace WHERE n.nspname={s}"), Some(ObjectKind::Extension)),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT constraint_name, 'CONSTRAINT', table_name, constraint_type FROM information_schema.table_constraints WHERE constraint_schema={s}"), Some(ObjectKind::Constraint)),
        ],
        Dialect::MySql => {
            let db = lit(d, db);
            vec![
                // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
                (format!("SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_SCHEMA = {db}"), None),
                // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
                (format!("SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = {db}"), None),
                // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
                (format!("SELECT TRIGGER_NAME, 'TRIGGER', EVENT_OBJECT_TABLE FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA={db}"), Some(ObjectKind::Trigger)),
                // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
                (format!("SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA={db}"), Some(ObjectKind::Event)),
                // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
                (format!("SELECT DISTINCT INDEX_NAME, 'INDEX', TABLE_NAME FROM information_schema.STATISTICS WHERE TABLE_SCHEMA={db}"), Some(ObjectKind::Index)),
                // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
                (format!("SELECT CONSTRAINT_NAME, 'CONSTRAINT', TABLE_NAME, CONSTRAINT_TYPE FROM information_schema.TABLE_CONSTRAINTS WHERE CONSTRAINT_SCHEMA={db}"), Some(ObjectKind::Constraint)),
            ]
        }
        Dialect::SqlServer => vec![
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT TABLE_NAME, TABLE_TYPE FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = {s}"), None),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT ROUTINE_NAME, ROUTINE_TYPE FROM INFORMATION_SCHEMA.ROUTINES WHERE ROUTINE_SCHEMA = {s}"), None),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT q.name FROM sys.sequences q JOIN sys.schemas s ON s.schema_id=q.schema_id WHERE s.name={s}"), Some(ObjectKind::Sequence)),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT tr.name, 'TRIGGER', t.name FROM sys.triggers tr JOIN sys.tables t ON t.object_id=tr.parent_id JOIN sys.schemas s ON s.schema_id=t.schema_id WHERE s.name={s}"), Some(ObjectKind::Trigger)),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT i.name, 'INDEX', t.name FROM sys.indexes i JOIN sys.tables t ON t.object_id=i.object_id JOIN sys.schemas s ON s.schema_id=t.schema_id WHERE s.name={s} AND i.name IS NOT NULL"), Some(ObjectKind::Index)),
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            (format!("SELECT CONSTRAINT_NAME, 'CONSTRAINT', TABLE_NAME, CONSTRAINT_TYPE FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS WHERE CONSTRAINT_SCHEMA={s}"), Some(ObjectKind::Constraint)),
        ],
        Dialect::Sqlite => {
            let master = if schema == "main" || schema.is_empty() { "sqlite_master".to_string() } else { format!("\"{}\".sqlite_master", schema.replace('"', "\"\"")) };
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            vec![(format!("SELECT name, type, tbl_name FROM {master} WHERE type IN ('table','view','trigger','index') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name"), None)]
        }
        Dialect::ClickHouse => {
            let db = lit(d, db);
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            vec![(format!("SELECT name, engine FROM system.tables WHERE database = {db} ORDER BY name"), None)]
        }
        _ => vec![],
    }
}

pub fn all_columns_sql(d: Dialect, schema: &str, db: &str) -> Option<String> {
    let s = lit(d, schema);
    Some(match d {
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Postgres | Dialect::SqlServer => format!("SELECT table_name, column_name FROM information_schema.columns WHERE table_schema = {s} ORDER BY table_name, ordinal_position"),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::MySql => format!("SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME, ORDINAL_POSITION", lit(d, db)),
        Dialect::Sqlite => "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p WHERE m.type IN ('table','view') AND m.name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY m.name, p.cid".to_string(),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::ClickHouse => format!("SELECT table, name FROM system.columns WHERE database = {} ORDER BY table, position", lit(d, db)),
        _ => return None,
    })
}

/// (table, estimated rows, total bytes) for a schema, from catalog statistics (no table scans).
pub fn table_stats_sql(d: Dialect, schema: &str, db: &str) -> Option<String> {
    let s = lit(d, schema);
    Some(match d {
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Postgres => format!(
            "SELECT c.relname, GREATEST(c.reltuples, 0)::bigint, pg_total_relation_size(c.oid) FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = {s} AND c.relkind IN ('r', 'p', 'm')"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::MySql => format!("SELECT TABLE_NAME, TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH FROM information_schema.TABLES WHERE TABLE_SCHEMA = {}", lit(d, db)),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::SqlServer => format!(
            "SELECT t.name, SUM(CASE WHEN p.index_id IN (0, 1) THEN p.rows ELSE 0 END), SUM(a.total_pages) * 8192 FROM sys.tables t \
             JOIN sys.schemas s ON s.schema_id = t.schema_id JOIN sys.partitions p ON p.object_id = t.object_id \
             JOIN sys.allocation_units a ON a.container_id = p.partition_id WHERE s.name = {s} GROUP BY t.name"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::ClickHouse => format!("SELECT name, total_rows, total_bytes FROM system.tables WHERE database = {}", lit(d, db)),
        _ => return None,
    })
}

pub fn kind_from_type(t: Option<&str>) -> ObjectKind {
    let t = t.unwrap_or("").to_ascii_uppercase();
    if t == "TRIGGER" { ObjectKind::Trigger
    } else if t == "ENUM" { ObjectKind::Enum
    } else if t == "TYPE" { ObjectKind::Type
    } else if t == "INDEX" { ObjectKind::Index
    } else if t == "EVENT" { ObjectKind::Event
    } else if t == "CONSTRAINT" { ObjectKind::Constraint
    } else if t.contains("MATERIALIZED") {
        ObjectKind::MaterializedView
    } else if t.contains("VIEW") {
        ObjectKind::View
    } else if t.contains("FUNCTION") {
        ObjectKind::Function
    } else if t.contains("PROCEDURE") {
        ObjectKind::Procedure
    } else {
        ObjectKind::Table
    }
}

pub fn columns_sql(d: Dialect, t: &TableRef, db: &str) -> String {
    let s = lit(d, &t.schema);
    let n = lit(d, &t.name);
    match d {
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Postgres => format!(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, pg_get_expr(ad.adbin, ad.adrelid), \
             EXISTS (SELECT 1 FROM pg_index i WHERE i.indrelid = a.attrelid AND i.indisprimary AND a.attnum = ANY(i.indkey)), \
             col_description(a.attrelid, a.attnum), CASE WHEN a.attidentity <> '' THEN 'identity' ELSE '' END \
             FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace ns ON ns.oid = c.relnamespace \
             LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             WHERE ns.nspname = {s} AND c.relname = {n} AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::MySql => format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE = 'YES', COLUMN_DEFAULT, COLUMN_KEY = 'PRI', COLUMN_COMMENT, EXTRA \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {n} ORDER BY ORDINAL_POSITION",
            lit(d, db)
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::SqlServer => format!(
            "SELECT c.COLUMN_NAME, c.DATA_TYPE + COALESCE('(' + CASE WHEN c.CHARACTER_MAXIMUM_LENGTH = -1 THEN 'max' ELSE CAST(c.CHARACTER_MAXIMUM_LENGTH AS varchar(10)) END + ')', ''), \
             CASE WHEN c.IS_NULLABLE = 'YES' THEN 1 ELSE 0 END, c.COLUMN_DEFAULT, \
             CASE WHEN EXISTS (SELECT 1 FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE k ON k.CONSTRAINT_NAME = tc.CONSTRAINT_NAME AND k.TABLE_SCHEMA = tc.TABLE_SCHEMA \
               WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' AND tc.TABLE_SCHEMA = c.TABLE_SCHEMA AND tc.TABLE_NAME = c.TABLE_NAME AND k.COLUMN_NAME = c.COLUMN_NAME) THEN 1 ELSE 0 END, \
             NULL, CASE WHEN COLUMNPROPERTY(OBJECT_ID(QUOTENAME(c.TABLE_SCHEMA) + '.' + QUOTENAME(c.TABLE_NAME)), c.COLUMN_NAME, 'IsIdentity') = 1 THEN 'identity' ELSE '' END \
             FROM INFORMATION_SCHEMA.COLUMNS c WHERE c.TABLE_SCHEMA = {s} AND c.TABLE_NAME = {n} ORDER BY c.ORDINAL_POSITION"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Sqlite => format!("SELECT name, type, \"notnull\" = 0, dflt_value, pk > 0, NULL, '' FROM pragma_table_info({n}, {s})"),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::ClickHouse => format!(
            "SELECT name, type, startsWith(type, 'Nullable'), default_expression, is_in_primary_key, comment, default_kind \
             FROM system.columns WHERE database = {} AND table = {n} ORDER BY position",
            lit(d, db)
        ),
        Dialect::Elastic => format!("DESCRIBE \"{}\"", t.name.replace('"', "\"\"")),
        _ => String::new(),
    }
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v.map(|s| s.trim().to_ascii_lowercase()).as_deref(), Some("t" | "true" | "1" | "yes"))
}

pub fn parse_columns(d: Dialect, r: &QueryResult) -> Vec<ColumnInfo> {
    if d == Dialect::Elastic {
        return r
            .rows
            .iter()
            .map(|row| ColumnInfo {
                name: row.first().cloned().flatten().unwrap_or_default(),
                data_type: row.get(2).or(row.get(1)).cloned().flatten().unwrap_or_default(),
                nullable: true,
                ..Default::default()
            })
            .collect();
    }
    r.rows
        .iter()
        .map(|row| {
            let get = |i: usize| row.get(i).cloned().flatten();
            ColumnInfo {
                name: get(0).unwrap_or_default(),
                data_type: get(1).unwrap_or_default(),
                nullable: truthy(get(2).as_deref()),
                default: get(3).filter(|s| !s.is_empty()),
                primary_key: truthy(get(4).as_deref()),
                comment: get(5).filter(|s| !s.is_empty()),
                extra: get(6).unwrap_or_default(),
            }
        })
        .collect()
}

pub fn indexes_sql(d: Dialect, t: &TableRef, db: &str) -> Option<String> {
    let s = lit(d, &t.schema);
    let n = lit(d, &t.name);
    Some(match d {
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Postgres => format!(
            "SELECT i.relname, (SELECT string_agg(pg_get_indexdef(ix.indexrelid, k.n::int + 1, true), ', ' ORDER BY k.n) FROM generate_subscripts(ix.indkey, 1) AS k(n)), \
             ix.indisunique, ix.indisprimary, am.amname \
             FROM pg_index ix JOIN pg_class i ON i.oid = ix.indexrelid JOIN pg_class tb ON tb.oid = ix.indrelid \
             JOIN pg_namespace ns ON ns.oid = tb.relnamespace JOIN pg_am am ON am.oid = i.relam \
             WHERE ns.nspname = {s} AND tb.relname = {n} ORDER BY ix.indisprimary DESC, i.relname"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::MySql => format!(
            "SELECT INDEX_NAME, GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX SEPARATOR ', '), MIN(NON_UNIQUE) = 0, INDEX_NAME = 'PRIMARY', MIN(INDEX_TYPE) \
             FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {n} GROUP BY INDEX_NAME ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME",
            lit(d, db)
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::SqlServer => format!(
            "SELECT i.name, STUFF((SELECT ', ' + c.name FROM sys.index_columns ic JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
               WHERE ic.object_id = i.object_id AND ic.index_id = i.index_id ORDER BY ic.key_ordinal FOR XML PATH('')), 1, 2, ''), \
             CAST(i.is_unique AS int), CAST(i.is_primary_key AS int), i.type_desc FROM sys.indexes i \
             WHERE i.object_id = OBJECT_ID(QUOTENAME({s}) + '.' + QUOTENAME({n})) AND i.name IS NOT NULL ORDER BY i.is_primary_key DESC, i.name"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Sqlite => format!(
            "SELECT il.name, (SELECT group_concat(ii.name, ', ') FROM pragma_index_info(il.name, {s}) ii), il.\"unique\", il.origin = 'pk', il.origin FROM pragma_index_list({n}, {s}) il"
        ),
        _ => return None,
    })
}

pub fn parse_indexes(_d: Dialect, r: &QueryResult) -> Vec<IndexInfo> {
    r.rows
        .iter()
        .map(|row| {
            let get = |i: usize| row.get(i).cloned().flatten();
            IndexInfo {
                name: get(0).unwrap_or_default(),
                columns: get(1).unwrap_or_default(),
                unique: truthy(get(2).as_deref()),
                primary: truthy(get(3).as_deref()),
                method: get(4).unwrap_or_default(),
            }
        })
        .collect()
}

pub fn foreign_keys_sql(d: Dialect, t: &TableRef, db: &str) -> Option<String> {
    let s = lit(d, &t.schema);
    let n = lit(d, &t.name);
    Some(match d {
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Postgres => format!(
            "SELECT con.conname, a.attname, fns.nspname, ft.relname, fa.attname \
             FROM pg_constraint con JOIN pg_class tb ON tb.oid = con.conrelid JOIN pg_namespace ns ON ns.oid = tb.relnamespace \
             JOIN pg_class ft ON ft.oid = con.confrelid JOIN pg_namespace fns ON fns.oid = ft.relnamespace \
             CROSS JOIN LATERAL unnest(con.conkey, con.confkey) AS k(src, dst) \
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.src \
             JOIN pg_attribute fa ON fa.attrelid = con.confrelid AND fa.attnum = k.dst \
             WHERE con.contype = 'f' AND ns.nspname = {s} AND tb.relname = {n}"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::MySql => format!(
            "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
             FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {n} AND REFERENCED_TABLE_NAME IS NOT NULL",
            lit(d, db)
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::SqlServer => format!(
            "SELECT fk.name, pc.name, SCHEMA_NAME(rt.schema_id), rt.name, rc.name FROM sys.foreign_key_columns fkc \
             JOIN sys.foreign_keys fk ON fk.object_id = fkc.constraint_object_id \
             JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id \
             JOIN sys.tables rt ON rt.object_id = fkc.referenced_object_id \
             JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id \
             WHERE fkc.parent_object_id = OBJECT_ID(QUOTENAME({s}) + '.' + QUOTENAME({n}))"
        ),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Sqlite => format!("SELECT 'fk_' || id, \"from\", {s}, \"table\", \"to\" FROM pragma_foreign_key_list({n}, {s})"),
        _ => return None,
    })
}

pub fn parse_foreign_keys(_d: Dialect, r: &QueryResult, default_schema: &str) -> Vec<ForeignKey> {
    r.rows
        .iter()
        .map(|row| {
            let get = |i: usize| row.get(i).cloned().flatten().unwrap_or_default();
            let ref_schema = get(2);
            ForeignKey {
                name: get(0),
                column: get(1),
                ref_schema: if ref_schema.is_empty() { default_schema.to_string() } else { ref_schema },
                ref_table: get(3),
                ref_column: get(4),
            }
        })
        .collect()
}

/// Foreign-key relations in a schema, optionally only those pointing at one table. Rows:
/// (constraint, schema, table, column, ref_schema, ref_table, ref_column, on_delete).
pub fn relations_sql(d: Dialect, schema: &str, db: &str, referenced: Option<&str>) -> Option<String> {
    let s = lit(d, schema);
    Some(match d {
        Dialect::Postgres => {
            let filter = match referenced {
                Some(t) => format!("fns.nspname = {s} AND ft.relname = {}", lit(d, t)),
                None => format!("ns.nspname = {s}"),
            };
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            format!(
                "SELECT con.conname, ns.nspname, tb.relname, a.attname, fns.nspname, ft.relname, fa.attname, \
                 CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END \
                 FROM pg_constraint con JOIN pg_class tb ON tb.oid = con.conrelid JOIN pg_namespace ns ON ns.oid = tb.relnamespace \
                 JOIN pg_class ft ON ft.oid = con.confrelid JOIN pg_namespace fns ON fns.oid = ft.relnamespace \
                 CROSS JOIN LATERAL unnest(con.conkey, con.confkey) AS k(src, dst) \
                 JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.src \
                 JOIN pg_attribute fa ON fa.attrelid = con.confrelid AND fa.attnum = k.dst \
                 WHERE con.contype = 'f' AND {filter} ORDER BY tb.relname, con.conname"
            )
        }
        Dialect::MySql => {
            let db = lit(d, db);
            let filter = match referenced {
                Some(t) => format!("k.REFERENCED_TABLE_SCHEMA = {db} AND k.REFERENCED_TABLE_NAME = {}", lit(d, t)),
                None => format!("k.TABLE_SCHEMA = {db} AND k.REFERENCED_TABLE_NAME IS NOT NULL"),
            };
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            format!(
                "SELECT k.CONSTRAINT_NAME, k.TABLE_SCHEMA, k.TABLE_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME, r.DELETE_RULE \
                 FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
                 ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME WHERE {filter} ORDER BY k.TABLE_NAME"
            )
        }
        Dialect::SqlServer => {
            let filter = match referenced {
                Some(t) => format!("fkc.referenced_object_id = OBJECT_ID(QUOTENAME({s}) + '.' + QUOTENAME({}))", lit(d, t)),
                None => format!("SCHEMA_NAME(pt.schema_id) = {s}"),
            };
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            format!(
                "SELECT fk.name, SCHEMA_NAME(pt.schema_id), pt.name, pc.name, SCHEMA_NAME(rt.schema_id), rt.name, rc.name, REPLACE(fk.delete_referential_action_desc, '_', ' ') \
                 FROM sys.foreign_key_columns fkc JOIN sys.foreign_keys fk ON fk.object_id = fkc.constraint_object_id \
                 JOIN sys.tables pt ON pt.object_id = fkc.parent_object_id JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id \
                 JOIN sys.tables rt ON rt.object_id = fkc.referenced_object_id JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id \
                 WHERE {filter} ORDER BY pt.name"
            )
        }
        Dialect::Sqlite => {
            let filter = match referenced {
                Some(t) => format!(" AND f.\"table\" = {}", lit(d, t)),
                None => String::new(),
            };
            // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
            format!(
                "SELECT 'fk_' || m.name || '_' || f.id, 'main', m.name, f.\"from\", 'main', f.\"table\", f.\"to\", f.on_delete \
                 FROM sqlite_master m JOIN pragma_foreign_key_list(m.name) f WHERE m.type = 'table'{filter} ORDER BY m.name"
            )
        }
        _ => return None,
    })
}

pub fn parse_relations(r: &QueryResult) -> Vec<Relation> {
    r.rows
        .iter()
        .map(|row| {
            let get = |i: usize| row.get(i).cloned().flatten().unwrap_or_default();
            Relation {
                name: get(0),
                schema: get(1),
                table: get(2),
                column: get(3),
                ref_schema: get(4),
                ref_table: get(5),
                ref_column: get(6),
                on_delete: get(7).to_uppercase(),
            }
        })
        .collect()
}

/// Server-provided DDL where the engine has it. `None` means: build it from the catalog.
pub fn ddl_sql(d: Dialect, obj: &DbObject) -> Option<String> {
    let t = TableRef { schema: obj.schema.clone(), name: obj.name.clone() };
    let q = qualified(d, &t);
    let s = lit(d, &obj.schema);
    let n = lit(d, &obj.name);
    if let Some(sql) = super::objects::definition_sql(d, obj) { return Some(sql); }
    match (d, obj.kind) {
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        (Dialect::Postgres, ObjectKind::View | ObjectKind::MaterializedView) => Some(format!(
            "SELECT 'CREATE {} ' || {} || E' AS\\n' || pg_get_viewdef({}::regclass, true)",
            if obj.kind == ObjectKind::View { "VIEW" } else { "MATERIALIZED VIEW" },
            lit(d, &q),
            lit(d, &q)
        )),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        (Dialect::Postgres, ObjectKind::Function | ObjectKind::Procedure) => Some(format!(
            "SELECT string_agg(pg_get_functiondef(p.oid), E';\\n\\n') FROM pg_proc p JOIN pg_namespace ns ON ns.oid = p.pronamespace WHERE ns.nspname = {s} AND p.proname = {n} AND pg_get_function_identity_arguments(p.oid) = {}", lit(d, obj.arguments.as_deref().unwrap_or(""))
        )),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        (Dialect::Postgres, ObjectKind::Sequence) => Some(format!(
            "SELECT 'CREATE SEQUENCE ' || {} || ' START ' || start_value || ' INCREMENT ' || increment_by || ' MINVALUE ' || min_value || ' MAXVALUE ' || max_value || ';' FROM pg_sequences WHERE schemaname = {s} AND sequencename = {n}",
            lit(d, &q)
        )),
        (Dialect::MySql, ObjectKind::Table) => Some(format!("SHOW CREATE TABLE {q}")),
        (Dialect::MySql, ObjectKind::View) => Some(format!("SHOW CREATE VIEW {q}")),
        (Dialect::MySql, ObjectKind::Function) => Some(format!("SHOW CREATE FUNCTION {q}")),
        (Dialect::MySql, ObjectKind::Procedure) => Some(format!("SHOW CREATE PROCEDURE {q}")),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        (Dialect::SqlServer, k) if k != ObjectKind::Table => Some(format!("SELECT OBJECT_DEFINITION(OBJECT_ID({}))", lit(d, &q))),
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        (Dialect::Sqlite, _) => Some(format!("SELECT sql FROM sqlite_master WHERE name = {n}")),
        (Dialect::ClickHouse, _) => Some(format!("SHOW CREATE TABLE {q}")),
        _ => None,
    }
}

pub fn pick_ddl(d: Dialect, r: &QueryResult) -> Option<String> {
    let row = r.rows.first()?;
    let text = match d {
        // SHOW CREATE returns (name, ddl, ...); for functions the DDL is the third column.
        Dialect::MySql => {
            let idx = r.columns.iter().position(|c| c.to_ascii_lowercase().starts_with("create ") || c.eq_ignore_ascii_case("SQL Original Statement")).unwrap_or(1);
            row.get(idx).cloned().flatten()
        }
        _ => row.first().cloned().flatten(),
    }?;
    if text.trim().is_empty() { None } else { Some(text.trim_end_matches(';').to_string() + ";") }
}

pub fn processes_sql(d: Dialect) -> Option<&'static str> {
    match d {
        Dialect::Postgres => Some(
            "SELECT pid, usename AS user, datname AS database, client_addr::text AS client, state, now() - query_start AS duration, query FROM pg_stat_activity WHERE pid <> pg_backend_pid() ORDER BY query_start DESC NULLS LAST",
        ),
        Dialect::MySql => Some("SHOW FULL PROCESSLIST"),
        Dialect::SqlServer => Some(
            "SELECT r.session_id AS id, s.login_name AS [user], DB_NAME(r.database_id) AS [database], s.host_name AS client, r.status, r.total_elapsed_time AS elapsed_ms, t.text AS query \
             FROM sys.dm_exec_requests r JOIN sys.dm_exec_sessions s ON s.session_id = r.session_id CROSS APPLY sys.dm_exec_sql_text(r.sql_handle) t WHERE r.session_id <> @@SPID",
        ),
        Dialect::ClickHouse => Some("SELECT query_id, user, elapsed, read_rows, memory_usage, query FROM system.processes"),
        _ => None,
    }
}

pub fn kill_sql(d: Dialect, id: &str) -> Option<String> {
    let numeric = id.chars().all(|c| c.is_ascii_digit());
    match d {
        // unslop-ignore NO_SQL_INTERPOLATION -- Catalog values are escaped by lit; identifiers by quote_ident/qualified; process IDs are checked as numeric before formatting.
        Dialect::Postgres if numeric => Some(format!("SELECT pg_terminate_backend({id})")),
        Dialect::MySql if numeric => Some(format!("KILL {id}")),
        Dialect::SqlServer if numeric => Some(format!("KILL {id}")),
        Dialect::ClickHouse => Some(format!("KILL QUERY WHERE query_id = {}", lit(d, id))),
        _ => None,
    }
}

pub fn users_sql(d: Dialect) -> Option<&'static str> {
    match d {
        Dialect::Postgres => Some(
            "SELECT rolname AS name, rolsuper AS superuser, rolcanlogin AS can_login, rolcreatedb AS create_db, rolcreaterole AS create_role, rolconnlimit AS conn_limit, rolvaliduntil AS valid_until FROM pg_roles WHERE rolname NOT LIKE 'pg\\_%' ORDER BY rolname",
        ),
        Dialect::MySql => Some("SELECT user AS name, host, account_locked, password_expired FROM mysql.user ORDER BY user"),
        Dialect::SqlServer => Some("SELECT name, type_desc, create_date, is_disabled FROM sys.server_principals WHERE type IN ('S','U','G') ORDER BY name"),
        Dialect::ClickHouse => Some("SELECT name, storage, auth_type FROM system.users ORDER BY name"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlserver_completion_reads_quoted_database_metadata_without_switching() {
        let sql = sqlserver_completion_sql("sales] archive");
        assert!(sql.starts_with("SELECT s.name, o.name, c.name FROM [sales]] archive].sys.schemas"));
        assert!(sql.contains("LEFT JOIN [sales]] archive].sys.objects"));
        assert!(sql.contains("LEFT JOIN [sales]] archive].sys.columns"));
        assert!(!sql.contains("USE "));
        assert!(!sql.contains("SELECT *"));
    }

    #[test]
    fn parses_column_flags_across_boolean_spellings() {
        let r = QueryResult {
            columns: vec!["n".into(); 7],
            rows: vec![
                vec![Some("id".into()), Some("integer".into()), Some("f".into()), None, Some("t".into()), None, Some("".into())],
                vec![Some("name".into()), Some("text".into()), Some("1".into()), Some("".into()), Some("0".into()), Some("hi".into()), None],
            ],
            ..Default::default()
        };
        let cols = parse_columns(Dialect::Postgres, &r);
        assert!(!cols[0].nullable && cols[0].primary_key);
        assert!(cols[1].nullable && !cols[1].primary_key);
        assert_eq!(cols[1].default, None, "empty default is no default");
        assert_eq!(cols[1].comment.as_deref(), Some("hi"));
    }

    #[test]
    fn kill_refuses_non_numeric_ids_for_sql_engines() {
        assert_eq!(kill_sql(Dialect::Postgres, "1; drop table x"), None);
        assert_eq!(kill_sql(Dialect::MySql, "42").as_deref(), Some("KILL 42"));
    }

    #[test]
    fn object_kind_from_catalog_type() {
        assert_eq!(kind_from_type(Some("BASE TABLE")), ObjectKind::Table);
        assert_eq!(kind_from_type(Some("VIEW")), ObjectKind::View);
        assert_eq!(kind_from_type(Some("MaterializedView")), ObjectKind::MaterializedView);
        assert_eq!(kind_from_type(Some("FUNCTION")), ObjectKind::Function);
    }
}
