<p align="center">
  <img src="assets/icon/strata.png" width="96" alt="Strata app icon">
</p>

<h1 align="center">Strata</h1>

<p align="center">A native database workbench for macOS, Windows, and Linux. Built with Rust and egui.</p>

<p align="center">
  <a href="https://github.com/fikrifirmanf/strata/releases/latest">Download</a> ·
  <a href="docs/USER_GUIDE.md">User guide</a> ·
  <a href="docs/DEVELOPMENT.md">Build from source</a> ·
  <a href="CONTRIBUTING.md">Contribute</a>
</p>

<p align="center">
  <a href="https://github.com/fikrifirmanf/strata/releases"><img src="https://img.shields.io/github/v/release/fikrifirmanf/strata" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
</p>

![Strata workspace with SQL queries and a result grid](docs/screenshots/workspace.png)

## Why Strata exists

Strata started with the free-plan limits in another database app. Building a personal workbench made it possible to add features for daily work without another subscription. It is open source so others can use it, suggest improvements, and contribute.

The app is under active development. See the [current limitations](docs/USER_GUIDE.md#not-yet-implemented) before using it with important data.

## What you can do

| Area | Features |
|---|---|
| Connections | Saved connections, environment labels, search, 10 connections per page, URL import, and SSH tunnels |
| Data | Compact table grid, filters, inline edits, row inspector, and staged changes with SQL preview |
| Queries | SQL autocomplete, parameters, query history, saved queries, and result charts |
| Structure | Columns, indexes, foreign keys, object definitions, and relationship diagrams |
| Tools | Import and export, backup and restore, process lists, and query-backed metrics boards |
| Integration | Optional AI sidebar and a read-only stdio MCP server for coding agents |

Features vary by database. The [user guide](docs/USER_GUIDE.md) covers transactions, driver capabilities, shortcuts, and settings.

![Strata Home with saved database connections](docs/screenshots/home.png)

## Supported databases

| Connection type | Databases |
|---|---|
| Native protocols | PostgreSQL, MySQL, MariaDB, SQLite, SQL Server, ClickHouse, Redis, MongoDB |
| PostgreSQL protocol | CockroachDB, Amazon Redshift, Greenplum |
| HTTP APIs | LibSQL / Turso, Cloudflare D1, Elasticsearch / OpenSearch |
| Vendor ODBC driver and DSN required | Oracle, Snowflake, BigQuery, Cassandra, DynamoDB, DuckDB, Vertica |

ODBC support uses your installed vendor driver. Strata does not bundle native SDKs for those engines. See [driver details](docs/USER_GUIDE.md#drivers) for setup and limits.

## Download and install

Get the latest packages from [GitHub Releases](https://github.com/fikrifirmanf/strata/releases/latest).

| Platform | Package |
|---|---|
| macOS, Apple silicon and Intel | Universal `.dmg` |
| Windows x86_64 | Setup `.exe` or portable `.zip` |
| Linux x86_64 | `.tar.gz` |

### macOS

Install from the terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/fikrifirmanf/strata/main/install.sh | sh
```

The installer checks the download against `SHA256SUMS.txt` and installs into `/Applications`. Set `STRATA_INSTALL_DIR` to use another directory.

Builds are ad-hoc signed and are not notarized. For a browser-downloaded `.dmg`, macOS may require **System Settings → Privacy & Security → Open Anyway** after the first launch attempt.

### Windows

Run the setup `.exe` to install for your user, or unpack the portable `.zip`. Builds are not code-signed yet, so Windows SmartScreen may show a warning.

### Linux

Unpack the archive and put `strata` on your `PATH`. Copy the included desktop entry and icon into your local applications and icons directories for a launcher entry.

Use **Help → Check for Updates** to check for a newer release. The portable Windows build opens the download page for updates.

<a id="building"></a>

## Build from source

Requires Rust 1.95 or newer. Linux also needs GTK 3, OpenSSL, pkg-config, and libxkbcommon development packages.

```sh
git clone https://github.com/fikrifirmanf/strata.git
cd strata
cargo run --locked --release
```

See [development instructions](docs/DEVELOPMENT.md) for platform packages, tests, signing, and release packaging.

## Contribute

Bug reports, feature ideas, documentation fixes, and pull requests are welcome. Start with the [contribution guide](CONTRIBUTING.md), or [open an issue](https://github.com/fikrifirmanf/strata/issues).

All contributors are expected to follow the [code of conduct](CODE_OF_CONDUCT.md). Report security concerns privately to [dev@fikrifirmanf.com](mailto:dev@fikrifirmanf.com).

## License and credits

Strata's source code is licensed under the [MIT License](LICENSE). You may use, modify, and redistribute it, including commercially, under its terms.

Bundled fonts use the SIL Open Font License 1.1. Database logos come from Simple Icons under CC0; trademarks belong to their owners. Rust dependencies retain their own licenses. See [third-party notices](THIRD_PARTY_NOTICES.md) for details.
