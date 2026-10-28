//! Strata — a native, cross-platform database workbench.
//!
//! Usage:
//!   strata                         open the app
//!   strata postgres://u@host/db    open a connection from a URL (also mysql://, redis://, ...)
//!   strata path/to/file.sqlite     open a SQLite file
//!   strata <target> --run "SQL"   open a target and run a query in a new tab
//!   strata mcp                     run the stdio MCP server for coding agents

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod db;
mod export;
mod mcp;
#[cfg(target_os = "macos")]
mod macos_menu;
mod model;
mod query_transaction;
mod runtime;
mod store;
mod tableplus;
mod theme;
mod ui;
mod update;

use model::{ConnectionConfig, DriverKind};

fn launch_targets(args: &[String]) -> Vec<ConnectionConfig> {
    let run_value = args.iter().position(|a| a == "--run").map(|i| i + 1);
    args.iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != run_value)
        .map(|(_, a)| a)
        .filter_map(|a| {
            if a.contains("://") {
                ConnectionConfig::from_url(a).map_err(|e| eprintln!("strata: {a}: {e}")).ok()
            } else if std::path::Path::new(a).is_file() {
                let mut c = ConnectionConfig::new(DriverKind::Sqlite);
                c.path = std::fs::canonicalize(a).map(|p| p.display().to_string()).unwrap_or_else(|_| a.clone());
                Some(c)
            } else {
                None
            }
        })
        .collect()
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let store = store::Store::open()?;

    if args.first().map(|a| a == "mcp" || a == "--mcp").unwrap_or(false) {
        return mcp::serve(store);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("strata {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let launch = launch_targets(&args);
    let run_sql = args.iter().position(|a| a == "--run").and_then(|i| args.get(i + 1)).cloned();
    let translucent = store.load_settings().window_translucency > 0.0 && cfg!(any(target_os = "macos", target_os = "windows"));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Strata")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([960.0, 600.0])
        .with_app_id("dev.strata.app")
        .with_transparent(translucent);
    // Same three-layer mark as the header logo; sets the Dock, taskbar and window icon.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/strata.png")) {
        viewport = viewport.with_icon(icon);
    }
    if cfg!(target_os = "macos") {
        // Content runs under the title bar; the traffic lights float over the rail.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Strata", options, Box::new(move |cc| Ok(Box::new(app::StrataApp::new(cc, store, launch, run_sql)))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}
