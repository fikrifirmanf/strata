//! Tests against real servers. Ignored by default; run with
//! `STRATA_TEST_PG=postgres://... STRATA_TEST_MYSQL=mysql://... STRATA_TEST_REDIS=redis://... cargo test -- --ignored`.

use super::*;
use crate::model::ConnectionConfig;

fn cfg(var: &str) -> Option<ConnectionConfig> {
    let url = std::env::var(var).ok()?;
    Some(ConnectionConfig::from_url(&url).expect("valid test URL"))
}

/// The full browse/edit cycle the table tab performs, on a scratch table.
async fn sql_round_trip(cfg: ConnectionConfig, create: &str) {
    let (mut d, _t) = connect(&cfg).await.expect("connect");
    let dialect = d.dialect();
    let dbs = d.databases().await.unwrap();
    assert!(!dbs.is_empty());
    let schema = meta::default_schema(dialect, &d.current_database());
    let t = TableRef { schema: schema.clone(), name: "strata_live".into() };
    let _ = d.execute(&format!("DROP TABLE IF EXISTS {}", sqlgen::qualified(dialect, &t))).await;
    d.execute(create).await.expect("create");
    d.execute(&format!("CREATE INDEX strata_live_name ON {} (name)", sqlgen::qualified(dialect, &t))).await.expect("index");

    let objects = d.objects(&schema).await.unwrap();
    assert!(objects.iter().any(|o| o.name == "strata_live" && o.kind == ObjectKind::Table), "{objects:?}");
    let cols = d.columns(&t).await.unwrap();
    assert_eq!(cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["id", "name", "note"]);
    assert!(cols[0].primary_key && !cols[1].primary_key);
    assert!(cols[2].nullable);
    let idx = d.indexes(&t).await.unwrap();
    assert!(idx.iter().any(|i| i.name == "strata_live_name" && i.columns.contains("name")), "{idx:?}");
    assert!(idx.iter().any(|i| i.primary), "{idx:?}");
    let all = d.all_columns(&schema).await.unwrap();
    assert!(all.contains(&("strata_live".to_string(), "note".to_string())));

    let changes = vec![
        RowChange::Insert { values: vec![("id".into(), EditValue::Text("1".into())), ("name".into(), EditValue::Text("O'Hara".into())), ("note".into(), EditValue::Null)] },
        RowChange::Insert { values: vec![("id".into(), EditValue::Text("2".into())), ("name".into(), EditValue::Text("Bo".into())), ("note".into(), EditValue::Text("x".into()))] },
    ];
    d.apply_changes(&t, &changes).await.expect("insert");
    d.apply_changes(&t, &[RowChange::Update { key: vec![("id".into(), Some("2".into()))], set: vec![("note".into(), EditValue::Text("edited".into()))] }]).await.expect("update");

    let req = BrowseRequest { filters: vec![Filter { column: Some("name".into()), op: FilterOp::Contains, value: "o'h".into(), ..Default::default() }], limit: 10, ..Default::default() };
    let r = d.browse(&t, &req, &cols).await.unwrap();
    assert_eq!(r.rows.len(), 1, "case-insensitive contains with a quote: {}", r.sql);
    assert_eq!(r.rows[0][2], None, "NULL survives as None");
    assert_eq!(d.count(&t, &BrowseRequest::default(), &cols).await.unwrap(), 2);

    let sorted = d.browse(&t, &BrowseRequest { sort: vec![("id".into(), false)], limit: 10, ..Default::default() }, &cols).await.unwrap();
    assert_eq!(sorted.rows[0][2].as_deref(), Some("edited"));

    // A failing statement inside a commit rolls back the earlier ones.
    let bad = vec![
        RowChange::Delete { key: vec![("id".into(), Some("1".into()))] },
        RowChange::Insert { values: vec![("id".into(), EditValue::Text("2".into())), ("name".into(), EditValue::Text("dup".into()))] },
    ];
    assert!(d.apply_changes(&t, &bad).await.is_err());
    assert_eq!(d.count(&t, &BrowseRequest::default(), &cols).await.unwrap(), 2, "rolled back");

    let ddl = d.ddl(&DbObject { parent: None, arguments: None, schema: schema.clone(), name: "strata_live".into(), kind: ObjectKind::Table }).await.unwrap();
    assert!(ddl.to_uppercase().contains("CREATE TABLE"), "{ddl}");
    let multi = d.execute("SELECT 1 AS a; SELECT 2 AS b").await.unwrap();
    assert_eq!(multi.len(), 2);
    assert_eq!(multi[1].cell(0, 0), Some("2"));
    assert!(d.processes().await.unwrap().has_rows());
    assert!(d.users().await.unwrap().has_rows());

    for c in [
        sqlgen::StructureChange::AddColumn(ColumnInfo { name: "extra".into(), data_type: "int".into(), nullable: true, ..Default::default() }),
        sqlgen::StructureChange::DropColumn("extra".into()),
    ] {
        for s in sqlgen::structure_sql(dialect, &t, &c) {
            d.execute(&s).await.unwrap_or_else(|e| panic!("{s}: {e}"));
        }
    }
    d.execute(&format!("DROP TABLE {}", sqlgen::qualified(dialect, &t))).await.unwrap();
}

// unslop-ignore TEST_WITHOUT_ASSERTION -- sql_round_trip asserts schema metadata, values, counts, rollback and DDL for this driver.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires STRATA_TEST_PG; see README.md#building"]
async fn postgres_live() {
    let c = cfg("STRATA_TEST_PG").expect("STRATA_TEST_PG must be set");
    sql_round_trip(c, "CREATE TABLE public.strata_live (id int PRIMARY KEY, name text NOT NULL, note text)").await;
}

// unslop-ignore TEST_WITHOUT_ASSERTION -- sql_round_trip asserts schema metadata, values, counts, rollback and DDL for this driver.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires STRATA_TEST_MYSQL; see README.md#building"]
async fn mysql_live() {
    let c = cfg("STRATA_TEST_MYSQL").expect("STRATA_TEST_MYSQL must be set");
    sql_round_trip(c, "CREATE TABLE strata_live (id int PRIMARY KEY, name varchar(50) NOT NULL, note text)").await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires STRATA_TEST_REDIS; see README.md#building"]
async fn redis_live() {
    let c = cfg("STRATA_TEST_REDIS").expect("STRATA_TEST_REDIS must be set");
    let (mut d, _t) = connect(&c).await.expect("connect");
    d.execute("DEL strata:s strata:h\nSET strata:s hello\nHSET strata:h a 1 b 2").await.unwrap();
    let t = TableRef { schema: "db0".into(), name: "keys".into() };
    let cols = d.columns(&t).await.unwrap();
    let req = BrowseRequest { filters: vec![Filter { column: Some("key".into()), op: FilterOp::HasPrefix, value: "strata:".into(), ..Default::default() }], limit: 50, ..Default::default() };
    let r = d.browse(&t, &req, &cols).await.unwrap();
    assert_eq!(r.rows.len(), 2);
    let h = r.rows.iter().find(|row| row[0].as_deref() == Some("strata:h")).unwrap();
    assert_eq!(h[1].as_deref(), Some("hash"));
    d.apply_changes(&t, &[RowChange::Update { key: vec![("key".into(), Some("strata:h".into())), ("type".into(), Some("hash".into()))], set: vec![("value".into(), EditValue::Text(r#"{"z":"9"}"#.into())), ("ttl".into(), EditValue::Text("120".into()))] }])
        .await
        .unwrap();
    let after = d.execute("HGETALL strata:h\nTTL strata:h").await.unwrap();
    assert_eq!(after[0].rows, vec![vec![Some("z".into()), Some("9".into())]]);
    let ttl: i64 = after[1].cell(0, 0).unwrap().parse().unwrap();
    assert!(ttl > 0 && ttl <= 120);
    d.apply_changes(&t, &[RowChange::Delete { key: vec![("key".into(), Some("strata:s".into())), ("type".into(), Some("string".into()))] }]).await.unwrap();
    assert_eq!(d.count(&t, &req, &cols).await.unwrap(), 1);
    d.execute("DEL strata:h").await.unwrap();
}

/// Exercises the same structured forms the UI sends, in an isolated schema.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STRATA_TEST_PG pointing at a disposable PostgreSQL database"]
async fn postgres_object_forms_live() {
    use super::objects::ObjectDraft;
    let c = cfg("STRATA_TEST_PG").expect("STRATA_TEST_PG must be set");
    let (mut d, _) = connect(&c).await.unwrap();
    assert!(d.server_version().await.unwrap().contains("PostgreSQL"));
    d.execute("SET standard_conforming_strings=off").await.unwrap();
    let value = "O'Brien\\path";
    let result = d.query_one(&format!("SELECT {}", sqlgen::lit(Dialect::Postgres, value))).await.unwrap();
    assert_eq!(result.cell(0,0), Some(value), "literal escaping must not depend on server string settings");
    d.execute("SET standard_conforming_strings=on").await.unwrap();
    let schema = format!("strata_objects_{}", std::process::id());
    let qs = sqlgen::quote_ident(Dialect::Postgres, &schema);
    d.execute(&format!("CREATE SCHEMA {qs}")).await.unwrap();
    d.execute(&format!("CREATE TABLE {qs}.parent (id integer PRIMARY KEY); CREATE TABLE {qs}.child (id integer PRIMARY KEY, parent_id integer, note text)")).await.unwrap();
    let mut drafts = Vec::new();
    let mut draft = ObjectDraft::new(ObjectKind::Enum, schema.clone()); draft.name = "state".into(); draft.body = "ready\ncan't".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Type, schema.clone()); draft.name = "positive_number".into(); draft.base_type = "integer".into(); draft.body = "VALUE > 0".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Type, schema.clone()); draft.name = "point_pair".into(); draft.type_kind = "COMPOSITE".into(); draft.body = "x: integer\ny: integer".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Sequence, schema.clone()); draft.name = "number_seq".into(); draft.start = 41; drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::View, schema.clone()); draft.name = "child_view".into(); draft.body = format!("SELECT id,note FROM {qs}.child"); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::MaterializedView, schema.clone()); draft.name = "child_snapshot".into(); draft.body = format!("SELECT id,note FROM {qs}.child"); drafts.push(draft);
    for ty in ["integer", "text"] {
        let mut draft = ObjectDraft::new(ObjectKind::Function, schema.clone()); draft.name = "as_text".into(); draft.parameters = format!("value {ty}"); draft.body = "SELECT value::text".into(); drafts.push(draft);
    }
    let mut draft = ObjectDraft::new(ObjectKind::Procedure, schema.clone()); draft.name = "do_nothing".into(); draft.language = "plpgsql".into(); draft.body = "BEGIN NULL; END".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Function, schema.clone()); draft.name = "uppercase_note".into(); draft.language = "plpgsql".into(); draft.returns = "trigger".into(); draft.body = "BEGIN NEW.note := upper(NEW.note); RETURN NEW; END".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Trigger, schema.clone()); draft.name = "child_audit".into(); draft.table = "child".into(); draft.function = "uppercase_note".into(); draft.timing = "BEFORE".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Index, schema.clone()); draft.name = "child_note_idx".into(); draft.table = "child".into(); draft.columns = "note".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Constraint, schema.clone()); draft.name = "child_parent_fk".into(); draft.table = "child".into(); draft.columns = "parent_id".into(); draft.ref_table = "parent".into(); draft.ref_columns = "id".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Extension, schema.clone()); draft.name = "hstore".into(); drafts.push(draft);
    for draft in &drafts {
        let sql = draft.sql(Dialect::Postgres).unwrap();
        d.execute(&sql).await.unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
    d.execute(&format!("INSERT INTO {qs}.parent(id) VALUES(1); INSERT INTO {qs}.child(id,parent_id,note) VALUES(1,1,'hello')")).await.unwrap();
    assert_eq!(d.query_one(&format!("SELECT note FROM {qs}.child WHERE id=1")).await.unwrap().cell(0,0), Some("HELLO"));
    assert!(d.execute(&format!("INSERT INTO {qs}.child(id,parent_id,note) VALUES(2,99,'invalid')")).await.is_err(), "the created foreign key must reject absent parents");
    let objects = d.objects(&schema).await.unwrap();
    assert_eq!(objects.iter().filter(|o| o.name == "as_text").count(), 2, "overloads must remain distinct");
    for draft in &drafts {
        let object = objects.iter().find(|o| o.name == draft.name && o.kind == draft.kind).unwrap_or_else(|| panic!("missing {}", draft.name));
        let ddl = d.ddl(object).await.unwrap_or_else(|error| panic!("{}: {error}", object.name));
        assert!(ddl.contains(&draft.name), "{ddl}");
    }
    let table = DbObject { schema: schema.clone(), name: "child".into(), kind: ObjectKind::Table, parent: None, arguments: None };
    for (_, sql) in objects::maintenance(Dialect::Postgres, &table) { d.run_in_transaction(&[sql.clone()]).await.unwrap_or_else(|error| panic!("{sql}: {error}")); }
    let constraint = objects.iter().find(|o| o.name == "child_parent_fk").unwrap();
    d.execute(&sqlgen::drop_sql(Dialect::Postgres, constraint, false)).await.unwrap();
    let overload = objects.iter().find(|o| o.name == "as_text" && o.arguments.as_deref().unwrap_or_default().contains("integer")).unwrap();
    d.execute(&sqlgen::drop_sql(Dialect::Postgres, overload, false)).await.unwrap();
    assert_eq!(d.objects(&schema).await.unwrap().iter().filter(|o| o.name == "as_text").count(), 1);
    d.execute(&format!("DROP SCHEMA {qs} CASCADE")).await.unwrap();
    let database = format!("strata_create_db_{}", std::process::id());
    d.run_in_transaction(&[sqlgen::create_database_sql(Dialect::Postgres, &database, "")]).await.unwrap();
    assert!(d.databases().await.unwrap().contains(&database));
    d.run_in_transaction(&[format!("DROP DATABASE {}", sqlgen::quote_ident(Dialect::Postgres, &database))]).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STRATA_TEST_MYSQL pointing at a disposable MySQL database"]
async fn mysql_object_forms_live() {
    use super::objects::ObjectDraft;
    let c = cfg("STRATA_TEST_MYSQL").expect("STRATA_TEST_MYSQL must be set");
    let (mut d, _) = connect(&c).await.unwrap();
    assert!(!d.server_version().await.unwrap().is_empty());
    let schema = format!("strata_objects_{}", std::process::id()); let qs = sqlgen::quote_ident(Dialect::MySql, &schema);
    d.execute(&format!("CREATE DATABASE {qs}")).await.unwrap();
    d.switch_database(&schema).await.unwrap();
    d.execute("CREATE TABLE parent (id int PRIMARY KEY); CREATE TABLE child (id int PRIMARY KEY, parent_id int, note varchar(100)); CREATE TABLE audit (note varchar(100))").await.unwrap();
    let mut drafts = Vec::new();
    let mut draft = ObjectDraft::new(ObjectKind::View, schema.clone()); draft.name = "child_view".into(); draft.body = format!("SELECT id,note FROM {qs}.child"); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Function, schema.clone()); draft.name = "double_value".into(); draft.parameters = "value int".into(); draft.returns = "int".into(); draft.body = "RETURN value * 2".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Procedure, schema.clone()); draft.name = "do_nothing".into(); draft.body = "BEGIN SELECT 1; END".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Trigger, schema.clone()); draft.name = "child_audit".into(); draft.table = "child".into(); draft.body = format!("BEGIN INSERT INTO {qs}.audit(note) VALUES (NEW.note); END"); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Event, schema.clone()); draft.name = "scheduled_audit".into(); draft.body = format!("INSERT INTO {qs}.audit(note) VALUES ('scheduled')"); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Index, schema.clone()); draft.name = "child_note_idx".into(); draft.table = "child".into(); draft.columns = "note".into(); drafts.push(draft);
    let mut draft = ObjectDraft::new(ObjectKind::Constraint, schema.clone()); draft.name = "child_parent_fk".into(); draft.table = "child".into(); draft.columns = "parent_id".into(); draft.ref_table = "parent".into(); draft.ref_columns = "id".into(); drafts.push(draft);
    for draft in &drafts { let sql = draft.sql(Dialect::MySql).unwrap(); d.execute(&sql).await.unwrap_or_else(|error| panic!("{sql}: {error}")); }
    d.execute("INSERT INTO parent(id) VALUES (1); INSERT INTO child(id,parent_id,note) VALUES (1,1,'saved')").await.unwrap();
    assert_eq!(d.query_one("SELECT note FROM audit").await.unwrap().cell(0,0), Some("saved"));
    assert_eq!(d.query_one(&format!("SELECT {qs}.double_value(4)")).await.unwrap().cell(0,0), Some("8"));
    let objects = d.objects(&schema).await.unwrap();
    for draft in &drafts {
        let object = objects.iter().find(|o| o.name == draft.name && o.kind == draft.kind).unwrap_or_else(|| panic!("missing {}", draft.name));
        assert!(d.ddl(object).await.unwrap().contains(&draft.name), "definition of {}", draft.name);
    }
    let event = objects.iter().find(|o| o.name == "scheduled_audit").unwrap();
    let status = d.query_one(&format!("SELECT STATUS FROM information_schema.EVENTS WHERE EVENT_SCHEMA={} AND EVENT_NAME='scheduled_audit'", sqlgen::lit(Dialect::MySql, &schema))).await.unwrap();
    assert_eq!(status.cell(0,0), Some("DISABLED"));
    for (_, sql) in objects::maintenance(Dialect::MySql, event) { d.execute(&sql).await.unwrap(); }
    let constraint = objects.iter().find(|o| o.name == "child_parent_fk" && o.kind == ObjectKind::Constraint).unwrap();
    d.execute(&sqlgen::drop_sql(Dialect::MySql, constraint, false)).await.unwrap();
    d.switch_database(&c.database).await.unwrap();
    d.execute(&format!("DROP DATABASE {qs}")).await.unwrap();
}

/// Tests the generic ODBC transport against PostgreSQL's installed vendor driver,
/// not a live Vertica service. The Vertica dialect uses the same ANSI browse path.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires STRATA_TEST_ODBC_DSN and STRATA_TEST_PG with an installed psqlODBC driver"]
async fn odbc_transport_live() {
    let mut c = cfg("STRATA_TEST_PG").expect("STRATA_TEST_PG must be set");
    c.driver = crate::model::DriverKind::Vertica;
    c.odbc_dsn = std::env::var("STRATA_TEST_ODBC_DSN").expect("STRATA_TEST_ODBC_DSN must be set");
    let mut d = super::odbc::OdbcDriver::connect(&c).await.unwrap();
    assert!(d.server_version().await.unwrap().contains("PostgreSQL"));
    assert!(d.schemas().await.unwrap().contains(&"public".into()));
    let name = format!("strata_odbc_{}", std::process::id());
    let table = TableRef { schema: "public".into(), name: name.clone() };
    let q = sqlgen::qualified(d.dialect(), &table);
    d.execute(&format!("CREATE TABLE {q} (id int PRIMARY KEY, note text)")).await.unwrap();
    assert!(d.objects("public").await.unwrap().iter().any(|o| o.name == name));
    let columns = d.columns(&table).await.unwrap();
    assert_eq!(columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["id", "note"]);
    assert!(columns[0].primary_key, "ODBC primary-key metadata must drive safe row matching");
    d.apply_changes(&table, &[RowChange::Insert { values: vec![("id".into(), EditValue::Text("1".into())), ("note".into(), EditValue::Text("saved".into()))] }]).await.unwrap();
    let result = d.browse(&table, &BrowseRequest { limit: 10, ..Default::default() }, &columns).await.unwrap();
    assert_eq!(result.cell(0,1), Some("saved"));
    let result = d.execute("SELECT 1 AS first; SELECT 2 AS second").await.unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[1].cell(0,0), Some("2"));
    let failure = d.run_in_transaction(&[
        format!("INSERT INTO {q} (id, note) VALUES (2, 'rollback')"),
        format!("INSERT INTO {q} (id, note) VALUES (1, 'duplicate key')"),
    ]).await.unwrap_err();
    assert!(failure.to_string().contains("rolled back"));
    assert!(d.execute("SELECT 1").await.is_err(), "A failed transaction must close the worker before reuse");
    let mut fresh = super::odbc::OdbcDriver::connect(&c).await.unwrap();
    assert_eq!(fresh.query_one(&format!("SELECT COUNT(*) FROM {q}")).await.unwrap().cell(0,0), Some("1"));
    fresh.execute(&format!("DROP TABLE {q}")).await.unwrap();
}

/// Read-only: a ClickHouse native port (9000, or a forward to it) is found and read over the
/// native protocol. Nothing is created, so it can point at a real warehouse.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires STRATA_TEST_CH_NATIVE (clickhouse://user:pass@host:native-port/db)"]
async fn clickhouse_native_live() {
    let Some(cfg) = cfg("STRATA_TEST_CH_NATIVE") else { return };
    let (mut d, _) = connect(&cfg).await.expect("connect over the native port");
    let r = d
        .query_one("SELECT toInt64(-7) AS i, toDecimal64(-1.5, 2) AS dec, 'it''s' AS s, CAST('b' AS Enum8('a' = 1, 'b' = 2)) AS e, [1, 2] AS arr, ['x'] AS sarr, map('k', 1) AS m, toDate('2026-11-06') AS dt, toDateTime64('2026-11-06 01:02:03.456', 3, 'UTC') AS ts, CAST(NULL AS Nullable(String)) AS n")
        .await
        .expect("typed select");
    assert_eq!(r.columns, ["i", "dec", "s", "e", "arr", "sarr", "m", "dt", "ts", "n"]);
    let row: Vec<Option<&str>> = r.rows[0].iter().map(|c| c.as_deref()).collect();
    assert_eq!(row, [Some("-7"), Some("-1.50"), Some("it's"), Some("b"), Some("[1,2]"), Some("['x']"), Some("{'k':1}"), Some("2026-11-06"), Some("2026-11-06 01:02:03.456"), None]);
    assert_eq!(r.types[1], "Decimal(18, 2)", "real types reach the grid");
    // Types the native client can't decode fall back to text from the server.
    let r = d.query_one("SELECT NULL AS nothing, toDate32('1900-01-02') AS d32").await.expect("fallback select");
    assert_eq!(r.rows[0], vec![None, Some("1900-01-02".to_string())]);
    let err = d.query_one("SELECT * FROM table_that_does_not_exist_strata").await.unwrap_err().to_string();
    assert!(err.contains("table_that_does_not_exist_strata") && !err.contains("stack"), "the server message reaches the user: {err}");
    // The session survives an error.
    assert_eq!(d.query_one("SELECT 1 AS one").await.unwrap().rows[0][0].as_deref(), Some("1"));
    let db = d.current_database();
    let objects = d.objects(&db).await.expect("list tables");
    eprintln!("{} objects in {db}", objects.len());
    d.switch_database("system").await.expect("switch database");
    assert_eq!(d.query_one("SELECT currentDatabase()").await.unwrap().rows[0][0].as_deref(), Some("system"));
}
