<p align="center">
  <img src="assets/icon/strata.png" width="112" alt="Strata app icon">
</p>

<h1 align="center">Strata</h1>

<p align="center">
  A native, cross-platform database workbench written in Rust.<br>
  PostgreSQL, MySQL, SQLite, SQL Server, ClickHouse, Redis, MongoDB and more, in one fast window.
</p>

<p align="center">
  <a href="https://github.com/fikrifirmanf/strata/releases/latest">Download</a> ·
  <a href="#building">Build from source</a> ·
  <a href="#drivers">Drivers</a> ·
  <a href="#features">Features</a>
</p>

![Strata workspace: SQL editor with a result grid](docs/screenshots/workspace.png)

The name comes from geological *strata*, the layers that tables, schemas and databases stack into. Strata is built with egui, so it starts in a moment and stays responsive on large result sets. Connection cards, a SQL editor with autocomplete, staged data edits and a structure editor are in place.

![Strata launchpad with saved connections grouped by environment](docs/screenshots/home.png)

## Download

Builds for macOS (universal, Apple silicon and Intel), Windows (x86_64) and Linux (x86_64) are attached to each [GitHub release](https://github.com/fikrifirmanf/strata/releases).

- **macOS:** install from the terminal with `curl -fsSL https://raw.githubusercontent.com/fikrifirmanf/strata/main/install.sh | sh`. It downloads the latest universal build, checks it against `SHA256SUMS.txt` and copies Strata into `/Applications` (set `STRATA_INSTALL_DIR` to change that). The build is ad-hoc signed, not notarized; files fetched with `curl` carry no quarantine flag, so it opens normally. If you download the `.dmg` in a browser instead, macOS says “Apple could not verify Strata”: dismiss it, open **System Settings → Privacy & Security** and choose **Open Anyway**.
- **Windows:** run `Strata-<version>-windows-x86_64-setup.exe`. It installs for your user without an admin prompt, adds a Start menu entry and an uninstaller, and upgrades in place. The `.zip` is a portable copy that runs from any folder. Neither is code-signed yet, so SmartScreen may ask you to confirm with **More info → Run anyway**.
- **Linux:** unpack the `.tar.gz`, put `strata` on your `PATH`, and copy `strata.desktop` and `strata.png` into `~/.local/share/applications` and `~/.local/share/icons` if you want a launcher entry.

Strata checks GitHub for a newer release when it starts and shows a popup when one is out. **Help → Check for Updates** checks on demand, and the About window can turn the startup check off.

## Command line

```
cargo run --release                                   # open the app
cargo run --release -- postgres://u:p@host/db         # open a connection from a URL
cargo run --release -- ./shop.sqlite --run "select 1" # open a file and run a query
cargo run --release -- mcp                            # stdio MCP server for coding agents
```

## Building

Requires Rust 1.95 or newer.

| Platform | Extra packages |
|---|---|
| macOS | none |
| Windows | none (MSVC toolchain); [Inno Setup 6](https://jrsoftware.org/isinfo.php) to build the installer |
| Linux | `libgtk-3-dev libssl-dev pkg-config libxkbcommon-dev` |

The Unix ODBC manager is bundled at build time. Vendor ODBC connections also need an installed vendor driver and a configured DSN. Windows uses the system ODBC manager.

`cargo test` runs the unit suite, which includes SQLite round-trips. The live driver tests need running servers:

```
STRATA_TEST_PG=postgres://postgres:secret@127.0.0.1:5432/postgres \
STRATA_TEST_MYSQL=mysql://root:secret@127.0.0.1:3306/app \
STRATA_TEST_REDIS=redis://127.0.0.1:6379/0 \
cargo test live -- --ignored
```

The ODBC transport integration test also needs `STRATA_TEST_ODBC_DSN` and the PostgreSQL fixture. It tests the adapter through psqlODBC; it does not certify the seven vendor services. These tests create and remove their own objects and databases; use disposable servers with create-database permission.

### CI and releases

CircleCI (`.circleci/config.yml`) builds the macOS, Linux and Windows packages only for `v*` release tags and attaches them, with a `SHA256SUMS.txt`, to the GitHub release. The macOS and Windows jobs run the test suite first. The `scripts/package-*` scripts do the packaging and also run locally.

To publish a release:

1. Bump `version` in `Cargo.toml` and commit.
2. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.

The tag build checks that the tag matches `Cargo.toml`, builds all three platforms and creates the GitHub release with the files attached. The in-app update check compares against that version. The CircleCI project needs a `GH_TOKEN` environment variable holding a GitHub token that can create releases on this repository.

## Interface

The workspace has one explorer panel with connection buttons above the table filter and saved-query and history controls below the object list. The header groups the connection, database, schema and search controls. On smaller windows, the header splits into two rows and the inspector opens over the workspace. Panels have solid fills and the background has no decorative lines. The minimum window size is 960 × 600. Wide tables scroll horizontally.

The File, View, Connections, Database, Query and Help menus expose workspace commands. macOS uses the native application menu; Windows and Linux use the in-window menus. Use **+ Create** above the explorer for new tables, databases, schemas and supported database objects. SQLite uses **New database file**. Right-click an explorer object or its open tab for actions; the object’s ellipsis button opens the same menu. The top bar shows the server provider and version when available.

The launchpad filters saved connections by name, host, database and environment. Cards show activity from local query history, and recent queries can be reopened from the launchpad.

## Drivers

| Engine | Protocol | Notes |
|---|---|---|
| PostgreSQL, CockroachDB, Redshift, Greenplum | native (tokio-postgres, simple-query) | cancel running query, SSL modes |
| MySQL, MariaDB | native (mysql_async, text protocol) | multi-statement scripts |
| SQLite | bundled libsqlite3 | interrupt, `VACUUM INTO` backup |
| SQL Server | TDS (tiberius) | |
| ClickHouse | HTTP | edits become `ALTER TABLE … UPDATE/DELETE` mutations |
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
- Saved connections with groups, tags (local / development / testing / staging / production), colors, and search
- Import from a URL (`postgres://`, `mysql+ssh://`, `redis://`, …) and copy a connection URL
- Export and import connection bundles (passwords are never exported)
- Passwords stored in the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service via GNOME Keyring or KWallet). When no keychain is reachable they go to `secrets.json` in the data folder, readable only by your user
- Test connection, keep-alive ping every 30 s, startup query, reconnect (⌘⇧R)
- Safe modes: Silent, Alert 1 and 2 (confirm before sending), Safe 1 and 2 (password before sending); plus a per-connection read-only mode
- Multiple open workspaces, reopened at launch

**Data browser**
- Paged grid (default 300 rows) with row numbers, alternating rows, and NULL styling
- Click to sort, shift-click for multi-column sort
- Filters: =, <>, <, >, <=, >=, IN, NOT IN, IS (NOT) NULL, (NOT) BETWEEN, (NOT) LIKE, ILIKE, contains, not contains, has prefix, has suffix, raw SQL. Each filter can target one column or any column, and filters combine with match all or match any
- Inline editing, insert / duplicate / delete rows, and set NULL / EMPTY / DEFAULT. Paste a TSV block into cells
- Pending changes are tinted (modified, inserted, deleted). Commit with ⌘S, discard, or preview the SQL first. Commits run in a transaction
- Tables without a primary key show a warning; rows are then matched by all their values
- Double-click a cell value to edit it. Only the arrow button opens a foreign-key reference
- Filter by a cell's value, quick look (Space) with JSON formatting
- Row inspector with linked rows and JSON trees; Text, Tree and geometry Map modes in Quick Look
- Data, Structure, Relations and Profile views, column value charts and filter labels
- Review staged changes as rows, SQL and cascade impact before committing; production commits require the database name
- Copy rows as TSV / CSV / JSON / SQL INSERT / Markdown, hide columns

**Structure**
- Edit columns (name, type, nullable, default, comment) with a per-engine type list, add or drop columns, add or drop indexes with engine index methods. All changes generate dialect-specific `ALTER` statements for review
- Set primary keys and add or drop foreign keys (referenced table and column, ON DELETE / ON UPDATE) on PostgreSQL, MySQL/MariaDB and SQL Server
- MongoDB collections show a field schema sampled from documents, and their indexes can be created and dropped
- Create and inspect database objects through engine-specific forms, with SQL review before execution
- PostgreSQL: views, materialized views, functions, procedures, sequences, triggers, enums, domains, composite types, indexes, extensions and constraints
- MySQL/MariaDB: views, functions, procedures, triggers, scheduled events, indexes and constraints. New events start disabled; their maintenance menu can enable them
- SQLite: views, triggers and indexes. SQL Server: views, routines, sequences, triggers, indexes and constraints. ClickHouse: views and materialized views
- Foreign-key forms support multiple columns and delete rules; object identity includes routine signatures and parent tables
- Open definitions in a query tab to edit and run SQL. PostgreSQL and MySQL catalog definitions cover the new object types; some SQL Server and vendor ODBC objects require their engine’s catalog SQL
- Table maintenance menus provide engine-specific analyze, vacuum, optimize, reindex, statistics and consistency checks; materialized views, triggers, extensions and events have their relevant maintenance actions

**SQL editor**
- Syntax highlighting, line numbers, current-statement tint
- Autocomplete for keywords, functions, tables and columns, with alias resolution (`u.` after `FROM users u`)
- Suggestions appear while typing; use arrows and Tab/Enter, or click a suggestion. Ctrl+Space opens suggestions on demand.
- Native PostgreSQL, MySQL/MariaDB, file-backed SQLite and SQL Server query tabs start transactions automatically. Changes stay pending until **Commit** or **Rollback**; **Auto-commit** restores immediate execution. Each manual query tab has its own connection, separate from grid edits and other tabs. Closing, reconnecting or changing database/schema asks how to resolve pending work. DDL and transaction/session-control scripts require Auto-commit mode; MySQL rollback requires transactional tables. Other drivers and in-memory SQLite retain immediate execution.
- Run current statement or selection (⌘↩), run all (⌘⇧↩), stop (⌘.), explain, format (⌘⇧F), toggle comment (⌘/)
- Auto-closing brackets and quotes, optional keyword upper-casing
- `:name` query parameters with a prompt
- One result card per statement, with table and chart views; results sort locally and can be exported
- Saved queries (per connection or global) and searchable query history with retention

**Database tools**
- Search and scroll databases/schemas from the header dropdowns, or switch database with ⌘K; create database or schema
- Create, rename, clone, truncate and drop tables
- Import CSV / JSON (into an existing table or a new one) and SQL dumps
- Export a table or a result to CSV / JSON / SQL / Markdown / Excel; export many tables at once
- Backup and restore through `pg_dump` / `pg_restore` / `psql` and `mysqldump` / `mysql`, through the SSH tunnel when one is set; `VACUUM INTO` for SQLite; `BACKUP DATABASE` for SQL Server
- Process list with auto-refresh and kill; users & roles with create and drop
- Relationship graph and ER diagram, three layouts, search, table details and SVG export
- Metrics boards: scoreboard, table, bar, line and pie widgets backed by read-only queries, with auto refresh
- Console log of every statement sent (⌘⇧C), Open Anything (⌘P)

**App**
- Themes: Strata Night / Day, Jade, Graphite, Dusk and Paper, with Auto following the system setting. Theme JSON files (`AppContentBackground`, `EditorKeywordText`, …) can be imported
- Passcode lock with auto-lock
- AI assistant sidebar that writes SQL from your schema (table and column names only; row data is never sent). It uses the Anthropic API (`claude-opus-5-5`) or any OpenAI-compatible endpoint
- `strata mcp`: an MCP server for coding agents (list connections, list tables, describe table, run read-only queries)

## Not yet implemented

- Native SDK drivers for Oracle, Snowflake, BigQuery, Cassandra, DynamoDB, DuckDB and Vertica (vendor ODBC connections are available)
- Plugin system, Git-hosted theme repositories, shared connection folders (Dropbox / iCloud sync)
- SSL client certificates
- Touch ID unlock and in-place auto-update (Strata finds new releases and links to the download, but doesn't install them itself)
- Remote (HTTP/TLS) MCP server with token management. The stdio server here only allows read-only queries
- **SSH host-key verification:** tunnels currently accept any host key, like `StrictHostKeyChecking=no`. Don't use SSH tunnels over untrusted networks until pinning is added
- Separate OS windows per workspace (workspaces live in the left rail instead)

## Data location

Connections, settings, saved queries, metrics boards and history live in the platform config directory (`~/Library/Application Support/Strata` on macOS, `%APPDATA%\Strata` on Windows, `~/.config/Strata` on Linux). Set `STRATA_HOME` to override it.

## Repository layout

```
src/
  main.rs        CLI entry, launches the app or the MCP server
  app.rs         rail, home, settings, lock screen, persistence
  model.rs       connections, settings, saved queries, boards
  store.rs       JSON files + keychain secrets
  update.rs      GitHub release check
  theme.rs       palettes and theme JSON import
  runtime.rs     tokio runtime, background tasks, sessions
  export.rs      CSV / JSON / SQL / Markdown / XLSX
  mcp.rs         stdio MCP server
  db/            Driver trait, per-engine drivers, catalog SQL, SQL generation, SSH tunnel
  ui/            home, workspace, grid, table tab, query tab, editor, dialogs,
                 metrics, diagram, assistant, settings
assets/          fonts, driver logos (Simple Icons, CC0), app icon
scripts/         release packaging for macOS, Linux and Windows
```

## License

[MIT](LICENSE). Geist fonts are under the SIL Open Font License (`assets/fonts/OFL.txt`); driver logos come from Simple Icons (CC0, `assets/db/README.md`).
