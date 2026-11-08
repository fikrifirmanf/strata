# User guide

[Project overview](../README.md) · [Build from source](DEVELOPMENT.md)

## Command line

```
cargo run --release                                   # open the app
cargo run --release -- postgres://u:p@host/db         # open a connection from a URL
cargo run --release -- ./shop.sqlite --run "select 1" # open a file and run a query
cargo run --release -- mcp                            # stdio MCP server for coding agents
```

## Interface

The workspace has one explorer panel with connection buttons above the table filter and saved-query and history controls below the object list. The header groups the connection, database, schema and search controls. On smaller windows, the header splits into two rows and the inspector opens over the workspace. Panels use solid fills. Floating menus and dialogs use frosted glass when the renderer supports it. macOS Reduce Transparency keeps these surfaces opaque. The minimum window size is 960 × 600. Wide tables scroll horizontally.

The File, View, Connections, Database, Query and Help menus expose workspace commands. macOS uses the native application menu; Windows and Linux use the in-window menus. Use **+ Create** above the explorer for new tables, databases, schemas and supported database objects. SQLite uses **New database file**. Right-click an explorer object or its open tab for actions; the object’s ellipsis button opens the same menu. The top bar shows the server provider and version when available.

The launchpad filters saved connections by name, host, database and environment. The saved connection list has a dedicated search field and shows up to 10 connections per page. Changing search or environment filters returns to the first page. Recent queries can be reopened from the launchpad.

## Drivers

| Engine | Protocol | Notes |
|---|---|---|
| PostgreSQL, CockroachDB, Redshift, Greenplum | native (tokio-postgres, simple-query) | cancel running query, SSL modes |
| MySQL, MariaDB | native (mysql_async, text protocol) | multi-statement scripts |
| SQLite | bundled libsqlite3 | interrupt, `VACUUM INTO` backup |
| SQL Server | TDS (tiberius) | |
| ClickHouse | HTTP, or native TCP when the port is 9000 or forwards to it | edits become `ALTER TABLE … UPDATE/DELETE` mutations |
| Redis | RESP (redis-rs) | keyspace as a table (key, type, ttl, value); raw commands in the editor |
| MongoDB | official driver | collections as tables; `db.coll.find({...}).sort().limit()` shell syntax |
| LibSQL / Turso | Hrana over HTTP | |
| Cloudflare D1 | REST API | |
| Elasticsearch / OpenSearch | `_sql` endpoint | read-only |
| Oracle, Snowflake, BigQuery, Cassandra, DynamoDB, DuckDB, Vertica | vendor ODBC | requires the vendor driver and DSN; no native SDK is bundled |

Native TCP drivers can run through an SSH tunnel (password or private key). ODBC networking, TLS and authentication are configured in the vendor DSN. Enter its DSN name and user/password or token in Strata; credentials use the same secret storage as other connections. An optional catalog filters metadata; changing a fixed DSN catalog requires a separate connection.

The driver picker filters OLTP databases, OLAP/warehouses and operational stores. Vendor capabilities determine query syntax, transactions and editable results. ODBC uses catalog metadata for tables, columns, primary keys and foreign keys. It limits results to 50,000 rows or 128 MiB and requests a 30-second statement timeout. A failed transaction closes the connection after rollback so it cannot be reused in an uncertain state. BigQuery and DynamoDB report that a managed-service server version is not published. Object DDL is retrieved for Oracle, Snowflake, DuckDB and Vertica; other vendor connections use catalog SQL in the editor.

## Features

**Connections**
- Saved connections with groups, tags (local / development / testing / staging / production), colors, and search.
- Import from a URL (`postgres://`, `mysql+ssh://`, `redis://`, …) and copy a connection URL.
- Export and import connection bundles (passwords are never exported).
- Passwords stored in the OS keychain, in one item read on first connect, never at launch (macOS Keychain, Windows Credential Manager, Linux Secret Service via GNOME Keyring or KWallet). When no keychain is reachable they go to `secrets.json` in the data folder, readable only by your user.
- Test connection, keep-alive ping every 30 s, startup query, reconnect (⌘⇧R).
- Safe modes: Silent, Alert 1 and 2 (confirm before sending), Safe 1 and 2 (password before sending); plus a per-connection read-only mode.
- Multiple open workspaces, reopened at launch.

**Data browser**
- Compact paged grid (default 300 rows) with row numbers, alternating rows, cell borders, and NULL styling.
- Click to sort, shift-click for multi-column sort.
- Filters: =, <>, <, >, <=, >=, IN, NOT IN, IS (NOT) NULL, (NOT) BETWEEN, (NOT) LIKE, ILIKE, contains, not contains, has prefix, has suffix, raw SQL. Each filter can target one column or any column, and filters combine with match all or match any.
- Inline editing, insert / duplicate / delete rows, and set NULL / EMPTY / DEFAULT. Paste a TSV block into cells.
- Pending changes are tinted (modified, inserted, deleted). Commit with ⌘S, discard, or preview the SQL first. Commits run in a transaction.
- Tables without a primary key show a warning; rows are then matched by all their values.
- Double-click a cell value to edit it. Only the arrow button opens a foreign-key reference.
- Filter by a cell's value, quick look (Space) with JSON formatting.
- Row inspector with linked rows and JSON trees; Text, Tree and geometry Map modes in Quick Look.
- Data, Structure, Relations and Profile views, column value charts and filter labels.
- Review staged changes as rows, SQL and cascade impact before committing; production commits require the database name.
- Copy rows as TSV / CSV / JSON / SQL INSERT / Markdown, hide columns.

**Structure**
- Edit columns (name, type, nullable, default, comment) with a per-engine type list, add or drop columns, add or drop indexes with engine index methods. All changes generate dialect-specific `ALTER` statements for review.
- Set primary keys and add or drop foreign keys (referenced table and column, ON DELETE / ON UPDATE) on PostgreSQL, MySQL/MariaDB and SQL Server.
- MongoDB collections show a field schema sampled from documents, and their indexes can be created and dropped.
- Create and inspect database objects through engine-specific forms, with SQL review before execution.
- PostgreSQL: views, materialized views, functions, procedures, sequences, triggers, enums, domains, composite types, indexes, extensions and constraints.
- MySQL/MariaDB: views, functions, procedures, triggers, scheduled events, indexes and constraints. New events start disabled; their maintenance menu can enable them.
- SQLite: views, triggers and indexes. SQL Server: views, routines, sequences, triggers, indexes and constraints. ClickHouse: views and materialized views.
- Foreign-key forms support multiple columns and delete rules; object identity includes routine signatures and parent tables.
- Open definitions in a query tab to edit and run SQL. PostgreSQL and MySQL catalog definitions cover the new object types; some SQL Server and vendor ODBC objects require their engine’s catalog SQL.
- Table maintenance menus provide engine-specific analyze, vacuum, optimize, reindex, statistics and consistency checks; materialized views, triggers, extensions and events have their relevant maintenance actions.

**SQL editor**
- Syntax highlighting, line numbers, current-statement tint.
- Autocomplete for keywords, functions, tables and columns, with alias resolution (`u.` after `FROM users u`).
- Suggestions appear while typing; use arrows and Tab/Enter, or click a suggestion. Ctrl+Space opens suggestions on demand.
- Native PostgreSQL, MySQL/MariaDB, file-backed SQLite and SQL Server query tabs start transactions automatically. Changes stay pending until **Commit** or **Rollback**; **Auto-commit** restores immediate execution. Each manual query tab has its own connection, separate from grid edits and other tabs. Closing, reconnecting or changing database/schema asks how to resolve pending work. DDL and transaction/session-control scripts require Auto-commit mode; MySQL rollback requires transactional tables. Other drivers and in-memory SQLite retain immediate execution.
- Run current statement or selection (⌘↩), run all (⌘⇧↩), stop (⌘.), explain, format (⌘⇧F), toggle comment (⌘/).
- Auto-closing brackets and quotes, optional keyword upper-casing.
- `:name` query parameters with a prompt.
- One result card per statement, with table and chart views; results sort locally and can be exported.
- Saved queries (per connection or global) and searchable query history with retention.

**Database tools**
- Search and scroll databases/schemas from the header dropdowns, or switch database with ⌘K (Ctrl+K on Windows and Linux); create database or schema.
- Create, rename, clone, truncate and drop tables.
- Import CSV / JSON (into an existing table or a new one) and SQL dumps.
- Export a table or a result to CSV / JSON / SQL / Markdown / Excel; export many tables at once.
- Backup and restore through `pg_dump` / `pg_restore` / `psql` and `mysqldump` / `mysql`, through the SSH tunnel when one is set; `VACUUM INTO` for SQLite; `BACKUP DATABASE` for SQL Server.
- Process list with auto-refresh and kill; users & roles with create and drop.
- Relationship graph and ER diagram, three layouts, search, table details and SVG export.
- Metrics boards: scoreboard, table, bar, line and pie widgets backed by read-only queries, with auto refresh.
- Console log of every statement sent (⌘⇧C), Open Anything (⌘P).

**App**
- Themes: Strata Night / Day, Jade, Graphite, Dusk and Paper, with Auto following the system setting. Theme JSON files (`AppContentBackground`, `EditorKeywordText`, …) can be imported.
- Passcode lock with auto-lock.
- AI assistant sidebar that writes SQL from your schema (table and column names only; row data is never sent). It uses the Anthropic API (`claude-opus-5-5`) or any OpenAI-compatible endpoint.
- `strata mcp`: an MCP server for coding agents (list connections, list tables, describe table, run read-only queries).

## Not yet implemented

- Native SDK drivers for Oracle, Snowflake, BigQuery, Cassandra, DynamoDB, DuckDB and Vertica (vendor ODBC connections are available).
- Plugin system, Git-hosted theme repositories, shared connection folders (Dropbox / iCloud sync).
- SSL client certificates.
- Touch ID unlock.
- Remote (HTTP/TLS) MCP server with token management. The stdio server here only allows read-only queries.
- **SSH host-key verification:** tunnels currently accept any host key, like `StrictHostKeyChecking=no`. Don't use SSH tunnels over untrusted networks until pinning is added.
- Separate OS windows per workspace (workspaces live in the left rail instead).

## Data location

Connections, settings, saved queries, metrics boards and history live in the platform config directory (`~/Library/Application Support/Strata` on macOS, `%APPDATA%\Strata` on Windows, `~/.config/Strata` on Linux). Set `STRATA_HOME` to override it.

