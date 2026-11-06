//! `strata mcp`: a stdio MCP server so coding agents can explore saved connections.
//! Only read-only statements run; anything that changes data is refused.

use crate::db::{self, is_read_only, TableRef};
use crate::model::ConnectionConfig;
use crate::store::Store;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";
const MAX_ROWS: usize = 500;

fn tools() -> Value {
    json!([
        {
            "name": "list_connections",
            "description": "List the database connections saved in Strata (name, driver, host, database, tag).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "list_tables",
            "description": "List tables and views of a saved connection. Use the connection name from list_connections.",
            "inputSchema": { "type": "object", "properties": {
                "connection": { "type": "string" },
                "schema": { "type": "string", "description": "Optional schema; defaults to the connection's default schema." }
            }, "required": ["connection"], "additionalProperties": false }
        },
        {
            "name": "describe_table",
            "description": "Columns (name, type, nullable, default, primary key), indexes and foreign keys of a table.",
            "inputSchema": { "type": "object", "properties": {
                "connection": { "type": "string" },
                "table": { "type": "string" },
                "schema": { "type": "string" }
            }, "required": ["connection", "table"], "additionalProperties": false }
        },
        {
            "name": "run_query",
            "description": "Run a read-only query (SELECT / EXPLAIN / SHOW / WITH) and return up to 500 rows as JSON. Statements that change data are refused.",
            "inputSchema": { "type": "object", "properties": {
                "connection": { "type": "string" },
                "sql": { "type": "string" }
            }, "required": ["connection", "sql"], "additionalProperties": false }
        }
    ])
}

fn find_connection(store: &Store, name: &str) -> Result<ConnectionConfig> {
    let file = store.load_connections();
    file.connections
        .iter()
        .find(|c| c.id.to_string() == name || c.display_name().eq_ignore_ascii_case(name))
        .cloned()
        .map(|mut c| { store.fill_secrets(&mut c); c })
        .ok_or_else(|| anyhow!("No saved connection named '{name}'. Call list_connections first."))
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key].as_str().ok_or_else(|| anyhow!("missing argument '{key}'"))
}

async fn call(store: &Store, name: &str, args: &Value) -> Result<Value> {
    match name {
        "list_connections" => {
            let file = store.load_connections();
            Ok(json!(file
                .connections
                .iter()
                .map(|c| json!({ "name": c.display_name(), "id": c.id, "driver": c.driver.label(), "host": c.host, "database": c.database, "tag": c.tag.label(), "read_only": c.read_only }))
                .collect::<Vec<_>>()))
        }
        "list_tables" => {
            let cfg = find_connection(store, arg(args, "connection")?)?;
            let (mut d, _t) = db::connect(&cfg).await?;
            let schema = match args["schema"].as_str() {
                Some(s) => s.to_string(),
                None => db::meta::default_schema(d.dialect(), &d.current_database()),
            };
            let objs = d.objects(&schema).await?;
            Ok(json!(objs.iter().map(|o| json!({ "name": o.name, "kind": o.kind.section(), "schema": o.schema })).collect::<Vec<_>>()))
        }
        "describe_table" => {
            let cfg = find_connection(store, arg(args, "connection")?)?;
            let (mut d, _t) = db::connect(&cfg).await?;
            let schema = args["schema"].as_str().map(str::to_string).unwrap_or_else(|| db::meta::default_schema(d.dialect(), &d.current_database()));
            let t = TableRef { schema, name: arg(args, "table")?.to_string() };
            let cols = d.columns(&t).await?;
            let idx = d.indexes(&t).await.unwrap_or_default();
            let fks = d.foreign_keys(&t).await.unwrap_or_default();
            Ok(json!({
                "columns": cols.iter().map(|c| json!({ "name": c.name, "type": c.data_type, "nullable": c.nullable, "default": c.default, "primary_key": c.primary_key, "comment": c.comment })).collect::<Vec<_>>(),
                "indexes": idx.iter().map(|i| json!({ "name": i.name, "columns": i.columns, "unique": i.unique, "primary": i.primary })).collect::<Vec<_>>(),
                "foreign_keys": fks.iter().map(|f| json!({ "column": f.column, "references": format!("{}.{}({})", f.ref_schema, f.ref_table, f.ref_column) })).collect::<Vec<_>>(),
            }))
        }
        "run_query" => {
            let cfg = find_connection(store, arg(args, "connection")?)?;
            let sql = arg(args, "sql")?;
            if !is_read_only(sql) {
                return Err(anyhow!("Refused: Strata's MCP server only runs read-only statements. Ask the user to run changes from the Strata app, where safe mode applies."));
            }
            let (mut d, _t) = db::connect(&cfg).await?;
            let results = d.execute(sql).await?;
            Ok(json!(results
                .iter()
                .map(|r| {
                    let rows: Vec<Value> = r.rows.iter().take(MAX_ROWS).map(|row| Value::Object(r.columns.iter().cloned().zip(row.iter().map(|v| v.clone().map(Value::String).unwrap_or(Value::Null))).collect())).collect();
                    json!({ "columns": r.columns, "rows": rows, "row_count": r.rows.len(), "truncated": r.rows.len() > MAX_ROWS })
                })
                .collect::<Vec<_>>()))
        }
        other => Err(anyhow!("Unknown tool '{other}'")),
    }
}

fn handle(store: &Store, rt: &tokio::runtime::Runtime, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "strata", "version": env!("CARGO_PKG_VERSION") }
        })),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let args = req["params"].get("arguments").cloned().unwrap_or(json!({}));
            Ok(match rt.block_on(call(store, name, &args)) {
                Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
                Err(e) => json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true }),
            })
        }
        "ping" => Ok(json!({})),
        m if m.starts_with("notifications/") => return None,
        _ => Err(json!({ "code": -32601, "message": format!("Method not found: {method}") })),
    };
    let id = id?;
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

pub fn serve(store: Store) -> Result<()> {
    let rt = crate::runtime::rt();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&store, rt, &req),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        // A test store: the real one would read the user's keychain.
        let dir = std::env::temp_dir().join(format!("strata-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Store::for_test(dir)
    }

    #[test]
    fn lists_tools_and_ignores_notifications() {
        let s = store();
        let rt = crate::runtime::rt();
        let r = handle(&s, rt, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).unwrap();
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), 4);
        assert!(handle(&s, rt, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
    }

    #[test]
    fn refuses_writes_and_queries_sqlite() {
        let s = store();
        let db_path = s.root().join("t.sqlite");
        rusqlite::Connection::open(&db_path).unwrap().execute_batch("CREATE TABLE t(a); INSERT INTO t VALUES (1),(2);").unwrap();
        let mut c = ConnectionConfig::new(crate::model::DriverKind::Sqlite);
        c.name = "Local".into();
        c.path = db_path.display().to_string();
        s.save_connections(&crate::store::ConnectionFile { connections: vec![c], groups: vec![] }).unwrap();
        let rt = crate::runtime::rt();
        let call_tool = |sql: &str| handle(&s, rt, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "run_query", "arguments": {"connection": "local", "sql": sql}}})).unwrap();
        let denied = call_tool("DELETE FROM t");
        assert_eq!(denied["result"]["isError"], true);
        let ok = call_tool("SELECT a FROM t ORDER BY a");
        let text = ok["result"]["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(v[0]["row_count"], 2);
        assert_eq!(v[0]["rows"][1]["a"], "2");
    }
}
