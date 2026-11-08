# Development

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

`STRATA_TEST_CH_NATIVE=clickhouse://user:pass@host:9000/db` runs a read-only check of the ClickHouse native protocol; it creates nothing, so it can point at a real server.

The ODBC transport integration test also needs `STRATA_TEST_ODBC_DSN` and the PostgreSQL fixture. It tests the adapter through psqlODBC; it does not certify the seven vendor services. These tests create and remove their own objects and databases; use disposable servers with create-database permission.

### CI and releases

GitHub Actions (`.github/workflows/release.yml`) builds the macOS, Linux and Windows packages only for `v*` release tags and attaches them, with a `SHA256SUMS.txt`, to the GitHub release. The macOS and Windows jobs run the test suite first. The `scripts/package-*` scripts do the packaging and also run locally.

To publish a release:

1. Bump `version` in `Cargo.toml` and commit.
2. Tag and push: `git tag v0.3.3 && git push origin v0.3.3`.

CircleCI (`.circleci/config.yml`) is a fallback, triggered by GitHub Actions if a release job fails.

The tag build checks that the tag matches `Cargo.toml`, builds all three platforms and creates the GitHub release with the files attached. The in-app update check compares against that version. GitHub Actions publishes with its repository token. The fallback needs the `CIRCLECI_TOKEN` repository secret and a `GH_TOKEN` environment variable in the CircleCI project.

### macOS signing and keychain prompts

macOS remembers "Always Allow" on a keychain item per code signature. Ad-hoc signed builds get a new signature every time, so each build asks again. Run `scripts/setup-signing.sh` once to create a self-signed "Strata Local Signing" certificate in your login keychain. After that, `cargo run` (through `scripts/cargo-runner.sh`) and `scripts/package-macos.sh` sign with it, and the keychain stays allowed across rebuilds. Without the certificate, both fall back to ad-hoc signing. Release builds from CI are still ad-hoc signed.

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
