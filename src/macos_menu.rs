//! Native macOS menu bar for the workspace commands. Windows and Linux keep the in-window menus.
use muda::{Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

pub struct NativeMenu {
    _menu: Menu,
}

impl NativeMenu {
    pub fn install() -> anyhow::Result<Self> {
        let menu = Menu::new();
        let mut items = Vec::new();
        let application = Submenu::new("Strata", true);
        append_action(&application, &mut items, "about", "About Strata")?;
        append_action(&application, &mut items, "check_updates", "Check for Updates…")?;
        application.append(&PredefinedMenuItem::separator())?;
        append_action(&application, &mut items, "settings", "Settings…")?;
        application.append(&PredefinedMenuItem::separator())?;
        let quit = MenuItem::with_id("quit", "Quit Strata", true, Some(muda::accelerator::Accelerator::new(muda::accelerator::Modifiers::SUPER, muda::accelerator::Code::KeyQ)));
        items.push((quit.id().clone(), "quit".to_owned()));
        application.append(&quit)?;
        menu.append(&application)?;

        let file = Submenu::new("File", true);
        append_action(&file, &mut items, "new_connection", "New Connection…")?;
        append_action(&file, &mut items, "new_query", "New Query")?;
        append_action(&file, &mut items, "open_sqlite", "Open SQLite Database…")?;
        append_action(&file, &mut items, "new_sqlite", "New SQLite Database…")?;
        append_action(&file, &mut items, "close_connection", "Close Connection")?;
        menu.append(&file)?;

        let view = Submenu::new("View", true);
        for (id, label) in [("explorer", "Explorer"), ("saved_queries", "Saved Queries"), ("history", "Query History"), ("graph", "Relationship Graph"), ("console", "Console"), ("inspector", "Inspector")] {
            append_action(&view, &mut items, id, label)?;
        }
        menu.append(&view)?;

        let connections = Submenu::new("Connections", true);
        append_action(&connections, &mut items, "connections", "All Connections")?;
        append_action(&connections, &mut items, "reconnect", "Reconnect")?;
        append_action(&connections, &mut items, "import_connections", "Import Connections…")?;
        append_action(&connections, &mut items, "export_connections", "Export Connections…")?;
        menu.append(&connections)?;

        let database = Submenu::new("Database", true);
        for (id, label) in [("new_database", "New Database…"), ("new_table", "New Table…"), ("refresh_objects", "Refresh Objects")] {
            append_action(&database, &mut items, id, label)?;
        }
        menu.append(&database)?;

        let query = Submenu::new("Query", true);
        append_action(&query, &mut items, "run_query", "Run All Statements")?;
        menu.append(&query)?;

        let help = Submenu::new("Help", true);
        append_action(&help, &mut items, "connection_info", "Connection Information")?;
        append_action(&help, &mut items, "shortcuts", "Keyboard Shortcuts")?;
        append_action(&help, &mut items, "check_updates", "Check for Updates…")?;
        help.set_as_help_menu_for_nsapp();
        menu.append(&help)?;

        menu.init_for_nsapp();
        NATIVE_MENU_ITEMS.with(|slot| *slot.borrow_mut() = items);
        Ok(Self { _menu: menu })
    }

    pub fn take_actions() -> Vec<String> {
        let items = NATIVE_MENU_ITEMS.with(|slot| slot.borrow().clone());
        muda::MenuEvent::receiver().try_iter().filter_map(|event| {
            items.iter().find(|(id, _)| id == &event.id).map(|(_, action)| action.clone())
        }).collect()
    }
}

thread_local! {
    static NATIVE_MENU_ITEMS: std::cell::RefCell<Vec<(MenuId, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn append_action(menu: &Submenu, items: &mut Vec<(MenuId, String)>, id: &str, label: &str) -> anyhow::Result<()> {
    let item = MenuItem::with_id(id, label, true, None);
    items.push((item.id().clone(), id.to_owned()));
    menu.append(&item)?;
    Ok(())
}
