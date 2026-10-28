//! Top-level app: workspace rail, home screen, settings, passcode lock and persistence.

use crate::model::*;
use crate::store::{ConnectionExport, ConnectionFile, Store};
use crate::theme::{mix, rgb, Palette};
use crate::ui::home::{Home, HomeAction};
use crate::ui::settings::{SettingsEvent, SettingsUi};
use crate::ui::workspace::{Shared, Workspace};
use crate::ui::*;
use egui::{vec2, RichText, Stroke};
use std::time::Instant;
use uuid::Uuid;

pub struct StrataApp {
    store: Store,
    settings: Settings,
    conns: ConnectionFile,
    queries: Vec<SavedQuery>,
    boards: Vec<MetricsBoard>,
    workspaces: Vec<Workspace>,
    /// `None` shows the connections home.
    active: Option<usize>,
    home: Home,
    toasts: Toasts,
    settings_open: bool,
    settings_ui: SettingsUi,
    updates: crate::ui::about::Updates,
    themes: Vec<Palette>,
    applied_theme: Option<(String, f32)>,
    locked: bool,
    passcode_input: String,
    passcode_error: bool,
    last_activity: Instant,
    pending_open: Vec<ConnectionConfig>,
    run_on_open: Option<String>,
    /// Decided at launch: the window was created transparent and the OS blurs behind it.
    translucent: bool,
    /// `STRATA_SCREENSHOT=out.png`: saves the window once it has settled, then quits. Used for the
    /// README images; reads the framebuffer, so it needs no screen-recording permission.
    screenshot_to: Option<String>,
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::macos_menu::NativeMenu>,
}

impl StrataApp {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Store, launch: Vec<ConnectionConfig>, run_on_open: Option<String>) -> Self {
        install_fonts(&cc.egui_ctx);
        crate::ui::install_driver_logos(&cc.egui_ctx);
        let settings = store.load_settings();
        let conns = store.load_connections();
        let mut themes = Palette::builtins();
        if let Ok(dir) = std::fs::read_dir(store.themes_dir()) {
            for e in dir.flatten() {
                if let Ok(t) = Palette::load_file(&e.path()) {
                    themes.retain(|x| x.name != t.name);
                    themes.push(t);
                }
            }
        }
        let translucent = settings.window_translucency > 0.0 && cfg!(any(target_os = "macos", target_os = "windows"));
        if translucent {
            apply_window_blur(cc);
        }
        #[cfg(target_os = "macos")]
        let native_menu = crate::macos_menu::NativeMenu::install().ok();
        let mut updates = crate::ui::about::Updates::default();
        if settings.check_updates {
            updates.check(&cc.egui_ctx, false);
        }
        let mut pending_open = launch;
        if settings.reopen_workspaces && pending_open.is_empty() {
            pending_open = settings.open_workspaces.iter().filter_map(|id| conns.connections.iter().find(|c| c.id == *id).cloned()).collect();
        }
        Self {
            locked: settings.passcode_hash.is_some(),
            queries: store.load_queries(),
            boards: store.load_boards(),
            store,
            settings,
            conns,
            workspaces: Vec::new(),
            active: None,
            home: Home::default(),
            toasts: Toasts::default(),
            settings_open: false,
            settings_ui: SettingsUi::default(),
            updates,
            themes,
            applied_theme: None,
            passcode_input: String::new(),
            passcode_error: false,
            last_activity: Instant::now(),
            pending_open,
            run_on_open,
            translucent,
            screenshot_to: std::env::var("STRATA_SCREENSHOT").ok().filter(|p| !p.is_empty()),
            #[cfg(target_os = "macos")]
            native_menu,
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::Auto => ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(true),
        };
        let name = if dark { &self.settings.dark_theme } else { &self.settings.light_theme };
        let key = (name.clone(), self.settings.ui_scale);
        if self.applied_theme.as_ref() == Some(&key) {
            return;
        }
        let pal = self.themes.iter().find(|t| &t.name == name && t.dark == dark).cloned().unwrap_or_else(|| if dark { Palette::night() } else { Palette::day() });
        pal.apply(ctx, self.settings.ui_scale);
        set_palette(pal);
        self.applied_theme = Some(key);
    }

    fn screenshot_when_settled(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.screenshot_to else { return };
        let delay = std::env::var("STRATA_SCREENSHOT_DELAY").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(5.0);
        if ctx.input(|i| i.time) < delay {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            return;
        }
        let path = path.clone();
        self.screenshot_to = None;
        let closer = ctx.clone();
        ctx.request_screenshot(move |shot| {
            let rgba: Vec<u8> = shot.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            let [w, h] = shot.size;
            if let Err(e) = image::save_buffer(&path, &rgba, w as u32, h as u32, image::ColorType::Rgba8) {
                eprintln!("strata: couldn't save screenshot {path}: {e}");
            }
            closer.send_viewport_cmd(egui::ViewportCommand::Close);
            closer.request_repaint();
        });
        ctx.request_repaint();
    }

    fn handle_app_request(&mut self, request: Option<workspace::AppRequest>, ctx: &egui::Context) {
        match request {
            Some(workspace::AppRequest::About) => self.updates.about_open = true,
            Some(workspace::AppRequest::CheckForUpdates) => {
                self.updates.about_open = true;
                self.updates.check(ctx, true);
            }
            None => {}
        }
    }

    fn save_conns(&mut self) {
        if let Err(e) = self.store.save_connections(&self.conns) {
            self.toasts.error(format!("Couldn't save connections: {e:#}"));
        }
        if let Some(e) = self.store.take_keychain_error() {
            self.toasts.error(format!("The system keychain refused the password ({e}); it was saved to secrets.json in the Strata folder instead"));
        }
    }

    fn open_connection(&mut self, cfg: ConnectionConfig, ctx: &egui::Context) {
        if let Some(i) = self.workspaces.iter().position(|w| w.id() == cfg.id) {
            self.active = Some(i);
            return;
        }
        if let Some(c) = self.conns.connections.iter_mut().find(|c| c.id == cfg.id) {
            c.last_used = Some(chrono::Utc::now());
            let _ = self.store.save_connections(&self.conns);
        }
        self.workspaces.push(Workspace::open(cfg, ctx));
        self.active = Some(self.workspaces.len() - 1);
        self.remember_open();
    }

    fn close_workspace(&mut self, i: usize) {
        if i >= self.workspaces.len() {
            return;
        }
        if self.workspaces[i].has_pending_transactions() {
            self.workspaces[i].request_close(false);
            self.active = Some(i);
            return;
        }
        let ws = self.workspaces.remove(i);
        if let Some(s) = ws.session {
            let d = s.driver.clone();
            crate::runtime::rt().spawn(async move { d.lock().await.close().await });
        }
        self.active = match self.active {
            Some(a) if a == i => if self.workspaces.is_empty() { None } else { Some(i.saturating_sub(1).min(self.workspaces.len() - 1)) },
            Some(a) if a > i => Some(a - 1),
            other => other,
        };
        self.remember_open();
    }

    fn request_quit(&mut self, ctx: &egui::Context) {
        if let Some(i) = self.workspaces.iter().position(|workspace| workspace.has_pending_transactions()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.workspaces[i].request_close(true);
            self.active = Some(i);
        } else { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
    }

    fn remember_open(&mut self) {
        // Only saved connections are reopened at launch.
        self.settings.open_workspaces = self.workspaces.iter().map(|w| w.id()).filter(|id| self.conns.connections.iter().any(|c| c.id == *id)).collect();
        let _ = self.store.save_settings(&self.settings);
    }

    fn handle_home(&mut self, a: HomeAction, ctx: &egui::Context) {
        match a {
            HomeAction::Open(id) => {
                if let Some(cfg) = self.conns.connections.iter().find(|c| c.id == id).cloned() {
                    self.open_connection(cfg, ctx);
                }
            }
            HomeAction::OpenWithQuery(id, sql) => {
                if let Some(cfg) = self.conns.connections.iter().find(|c| c.id == id).cloned() {
                    self.open_connection(cfg, ctx);
                    if let Some(i) = self.active {
                        self.workspaces[i].open_after_connect(sql, ctx);
                    }
                }
            }
            HomeAction::ConnectUrl(cfg) => {
                self.conns.connections.push(cfg.clone());
                self.save_conns();
                self.open_connection(cfg, ctx);
            }
            HomeAction::ImportTablePlus => {
                let default = crate::tableplus::default_dir().map(|d| d.join("Connections.plist")).filter(|p| p.is_file());
                let path = default.or_else(|| rfd::FileDialog::new().set_title("Choose TablePlus' Connections.plist").add_filter("TablePlus connections", &["plist"]).pick_file());
                if let Some(path) = path {
                    match crate::tableplus::import(&path) {
                        Ok(imp) => {
                            let n = imp.connections.len();
                            for g in imp.groups {
                                if !self.conns.groups.iter().any(|x| x.name == g.name) {
                                    self.conns.groups.push(g);
                                }
                            }
                            for c in imp.connections {
                                let dup = self.conns.connections.iter().any(|x| x.name == c.name && x.host == c.host && x.port == c.port && x.driver == c.driver);
                                if !dup {
                                    self.conns.connections.push(c);
                                }
                            }
                            self.save_conns();
                            let skipped = if imp.skipped.is_empty() { String::new() } else { format!(" Skipped (driver not supported yet): {}.", imp.skipped.join(", ")) };
                            self.toasts.success(format!("Imported {n} connections from TablePlus. Passwords stay in TablePlus; you'll be asked on first connect.{skipped}"));
                        }
                        Err(e) => self.toasts.error(format!("{e:#}")),
                    }
                }
            }
            HomeAction::Save { cfg, open } => {
                match self.conns.connections.iter_mut().find(|c| c.id == cfg.id) {
                    Some(c) => *c = cfg.clone(),
                    None => self.conns.connections.push(cfg.clone()),
                }
                self.save_conns();
                // Keep an open workspace in sync with its edited settings.
                if let Some(i) = self.workspaces.iter().position(|w| w.id() == cfg.id) {
                    let reconnect = {
                        let w = &self.workspaces[i];
                        w.cfg.host != cfg.host || w.cfg.port != cfg.port || w.cfg.user != cfg.user || w.cfg.password != cfg.password || w.cfg.ssh != cfg.ssh || w.cfg.database != cfg.database || w.cfg.path != cfg.path || w.cfg.ssl != cfg.ssl
                    };
                    self.workspaces[i].cfg = cfg.clone();
                    if reconnect {
                        self.workspaces[i].reconnect(ctx);
                    }
                }
                if open {
                    self.open_connection(cfg, ctx);
                } else {
                    self.toasts.success("Connection saved");
                }
            }
            HomeAction::Delete(id) => {
                self.conns.connections.retain(|c| c.id != id);
                self.store.forget_secrets(id);
                self.save_conns();
                if let Some(i) = self.workspaces.iter().position(|w| w.id() == id) {
                    self.close_workspace(i);
                }
            }
            HomeAction::Duplicate(id) => {
                if let Some(mut c) = self.conns.connections.iter().find(|c| c.id == id).cloned() {
                    c.id = Uuid::new_v4();
                    c.name = format!("{} copy", c.display_name());
                    c.last_used = None;
                    self.conns.connections.push(c);
                    self.save_conns();
                }
            }
            HomeAction::CreateGroup(name) => {
                let palette = [[91, 140, 255], [54, 201, 138], [242, 177, 64], [185, 140, 255], [43, 179, 163]];
                let color = palette[self.conns.groups.len() % palette.len()];
                self.conns.groups.push(Group { id: Uuid::new_v4(), name, color, collapsed: false });
                self.save_conns();
            }
            HomeAction::RenameGroup(id, name) => {
                if let Some(g) = self.conns.groups.iter_mut().find(|g| g.id == id) {
                    g.name = name;
                }
                self.save_conns();
            }
            HomeAction::DeleteGroup(id) => {
                self.conns.groups.retain(|g| g.id != id);
                for c in self.conns.connections.iter_mut().filter(|c| c.group == Some(id)) {
                    c.group = None;
                }
                self.save_conns();
            }
            HomeAction::MoveToGroup(id, g) => {
                if let Some(c) = self.conns.connections.iter_mut().find(|c| c.id == id) {
                    c.group = g;
                }
                self.save_conns();
            }
            HomeAction::ExportConnections => {
                if let Some(path) = rfd::FileDialog::new().set_file_name("connections.strataconnection").save_file() {
                    let export = ConnectionExport { format: "strata/1".into(), connections: self.conns.connections.clone() };
                    match serde_json::to_vec_pretty(&export).map_err(anyhow::Error::from).and_then(|b| std::fs::write(&path, b).map_err(Into::into)) {
                        Ok(()) => self.toasts.success(format!("Exported {} connections (without passwords)", export.connections.len())),
                        Err(e) => self.toasts.error(format!("{e:#}")),
                    }
                }
            }
            HomeAction::ImportConnections => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Strata connections", &["strataconnection", "json"]).pick_file() {
                    let parsed: anyhow::Result<ConnectionExport> = std::fs::read(&path).map_err(Into::into).and_then(|b| serde_json::from_slice(&b).map_err(Into::into));
                    match parsed {
                        Ok(ex) => {
                            let mut n = 0;
                            for mut c in ex.connections {
                                if self.conns.connections.iter().any(|x| x.id == c.id) {
                                    c.id = Uuid::new_v4();
                                }
                                if !self.conns.groups.iter().any(|g| Some(g.id) == c.group) {
                                    c.group = None;
                                }
                                self.conns.connections.push(c);
                                n += 1;
                            }
                            self.save_conns();
                            self.toasts.success(format!("Imported {n} connections"));
                        }
                        Err(e) => self.toasts.error(format!("Strata couldn’t read your connections: {e:#}")),
                    }
                }
            }
            HomeAction::OpenSqliteFile => {
                if let Some(path) = rfd::FileDialog::new().add_filter("SQLite", &["sqlite", "sqlite3", "db", "db3"]).pick_file() {
                    let mut c = ConnectionConfig::new(DriverKind::Sqlite);
                    c.path = path.display().to_string();
                    self.conns.connections.push(c.clone());
                    self.save_conns();
                    self.open_connection(c, ctx);
                }
            }
        }
    }

    fn home_menu_bar(&mut self, ui: &mut egui::Ui) {
        let mut actions = Vec::new();
        egui::Panel::top("home-menubar").exact_size(32.0).show_separator_line(false)
            .frame(egui::Frame::new().fill(p().window).inner_margin(egui::Margin { left: if cfg!(target_os = "macos") { 88 } else { 12 }, right: 12, top: 2, bottom: 2 }))
            .show(ui, |ui| { egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New connection…").clicked() { self.home.begin_connection(); ui.close(); }
                    if ui.button("Open SQLite database…").clicked() { actions.push(HomeAction::OpenSqliteFile); ui.close(); }
                    if ui.button("New SQLite database…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("SQLite database", &["sqlite", "db"]).set_file_name("database.sqlite").save_file() {
                            let mut cfg = ConnectionConfig::new(DriverKind::Sqlite); cfg.path = path.display().to_string(); actions.push(HomeAction::Save { cfg, open: true });
                        }
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| { if ui.button("Settings…").clicked() { self.settings_open = true; ui.close(); } });
                ui.menu_button("Connections", |ui| {
                    if ui.button("Import connections…").clicked() { actions.push(HomeAction::ImportConnections); ui.close(); }
                    if ui.button("Import from TablePlus…").clicked() { actions.push(HomeAction::ImportTablePlus); ui.close(); }
                    if ui.button("Export connections…").clicked() { actions.push(HomeAction::ExportConnections); ui.close(); }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Check for updates…").clicked() { self.updates.about_open = true; self.updates.check(ui.ctx(), true); ui.close(); }
                    if ui.button("About Strata").clicked() { self.updates.about_open = true; ui.close(); }
                    ui.separator();
                    ui.label(crate::ui::faint("ODBC engines require an installed vendor driver and configured DSN."));
                });
            }); });
        let ctx = ui.ctx().clone();
        for action in actions { self.handle_home(action, &ctx); }
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        let pal = p();
        let ctx = ui.ctx().clone();
        let top_inset = if cfg!(target_os = "macos") { 30 } else { 0 };
        egui::Panel::left("rail").exact_size(76.0).resizable(false).show_separator_line(false).frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 0, top: 12 + top_inset, bottom: 12 })).show(ui, |ui| {
            glass_frame().inner_margin(egui::Margin::symmetric(0, 12)).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.vertical_centered(|ui| {
                    let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::click());
                    home::paint_logo(ui.painter(), r);
                    if resp.on_hover_text("Launchpad").clicked() {
                        self.home.refresh_activity();
                        self.active = None;
                    }
                    ui.add_space(10.0);
                    let home_active = self.active.is_none();
                    if rail_button(ui, Icon::Home, "Launchpad", home_active).clicked() {
                        self.home.refresh_activity();
                        self.active = None;
                    }
                    ui.add_space(6.0);
                    let sep = ui.cursor().top();
                    ui.painter().hline(ui.max_rect().center().x - 14.0..=ui.max_rect().center().x + 14.0, sep, Stroke::new(1.0, pal.glass_stroke));
                    ui.add_space(8.0);
                    let mut close = None;
                    for (i, ws) in self.workspaces.iter().enumerate() {
                        let active = self.active == Some(i);
                        let (r, resp) = ui.allocate_exact_size(vec2(64.0, 48.0), egui::Sense::click());
                        let tile = egui::Rect::from_center_size(r.center(), vec2(40.0, 40.0));
                        let env = rgb(ws.cfg.accent());
                        let initials: String = ws.cfg.display_name().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).take(2).filter_map(|w| w.chars().next()).collect::<String>().to_uppercase();
                        let initials = if initials.is_empty() { ws.cfg.driver.badge().to_string() } else { initials };
                        let fill = if active { mix(pal.surface, env, 0.16) } else if resp.hovered() { pal.surface_hover } else { pal.surface };
                        ui.painter().rect_filled(tile, egui::CornerRadius::same(13), fill);
                        ui.painter().rect_stroke(tile, egui::CornerRadius::same(13), Stroke::new(if active { 2.0 } else { 1.0 }, if active { env } else { pal.glass_stroke }), egui::StrokeKind::Inside);
                        ui.painter().text(tile.center(), egui::Align2::CENTER_CENTER, initials, semibold(12.5), readable_on_tint(env));
                        let dot = match ws.status {
                            workspace::ConnState::Connected => pal.success,
                            workspace::ConnState::Connecting => pal.warning,
                            _ => pal.danger,
                        };
                        ui.painter().circle(tile.right_bottom() - vec2(2.0, 2.0), 4.5, dot, Stroke::new(2.0, pal.glass_opaque()));
                        let resp = resp.on_hover_text(format!("{}\n{}", ws.cfg.display_name(), ws.cfg.subtitle()));
                        if resp.clicked() {
                            self.active = Some(i);
                        }
                        if resp.middle_clicked() {
                            close = Some(i);
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Reconnect").clicked() {
                                ui.close();
                                close = Some(usize::MAX - i);
                            }
                            if ui.button("Close workspace").clicked() {
                                close = Some(i);
                                ui.close();
                            }
                        });
                    }
                    if let Some(i) = close {
                        if i > self.workspaces.len() {
                            let idx = usize::MAX - i;
                            self.workspaces[idx].reconnect(&ctx);
                            self.active = Some(idx);
                        } else {
                            self.close_workspace(i);
                        }
                    }
                    let (r, resp) = ui.allocate_exact_size(vec2(64.0, 46.0), egui::Sense::click());
                    let tile = egui::Rect::from_center_size(r.center(), vec2(40.0, 40.0));
                    let dash = if resp.hovered() { pal.text_dim } else { pal.border };
                    ui.painter().add(egui::Shape::dashed_line(&[tile.left_top() + vec2(12.0, 0.0), tile.right_top() - vec2(12.0, 0.0)], Stroke::new(1.0, dash), 3.0, 3.0));
                    ui.painter().rect_stroke(tile, egui::CornerRadius::same(13), Stroke::new(1.0, dash), egui::StrokeKind::Inside);
                    paint_icon(ui.painter(), egui::Rect::from_center_size(tile.center(), vec2(15.0, 15.0)), Icon::Plus, if resp.hovered() { pal.text } else { pal.text_dim });
                    if resp.on_hover_text("Open another connection").clicked() {
                        self.active = None;
                    }
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        if rail_button(ui, Icon::Gear, "Settings (⌘,)", self.settings_open).clicked() {
                            self.settings_open = !self.settings_open;
                        }
                        if self.settings.passcode_hash.is_some() && rail_button(ui, Icon::Lock, "Lock (⌘L)", false).clicked() {
                            self.locked = true;
                        }
                    });
                });
            });
        });
    }

    fn lock_screen(&mut self, ui: &mut egui::Ui) {
        let pal = p();
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), egui::Sense::hover());
                home::paint_logo(ui.painter(), r);
                ui.add_space(12.0);
                ui.label(title("Strata is locked", 20.0));
                ui.label(dim("Enter your passcode"));
                ui.add_space(10.0);
                let resp = ui.add(crate::ui::line_edit(&mut self.passcode_input).password(true).desired_width(240.0).align(egui::Align2::CENTER_CENTER));
                resp.request_focus();
                if self.passcode_error {
                    ui.label(RichText::new("Incorrect passcode").color(pal.danger));
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if Some(crate::store::hash_passcode(&self.passcode_input)) == self.settings.passcode_hash {
                        self.locked = false;
                        self.passcode_error = false;
                        self.last_activity = Instant::now();
                    } else {
                        self.passcode_error = true;
                    }
                    self.passcode_input.clear();
                }
                ui.add_space(20.0);
                ui.label(faint("If you forget your passcode, delete settings.json in the data folder (connections stay).").size(11.0));
            });
        });
    }
}

fn rail_button(ui: &mut egui::Ui, icon: Icon, tip: &str, active: bool) -> egui::Response {
    let pal = p();
    let (r, resp) = ui.allocate_exact_size(vec2(64.0, 44.0), egui::Sense::click());
    let tile = egui::Rect::from_center_size(r.center(), vec2(40.0, 40.0));
    if active || resp.hovered() {
        ui.painter().rect_filled(tile, egui::CornerRadius::same(12), if active { mix(pal.surface, pal.accent, 0.16) } else { pal.surface_hover });
    }
    if active {
        ui.painter().rect_stroke(tile, egui::CornerRadius::same(12), Stroke::new(1.0, pal.accent.gamma_multiply(0.45)), egui::StrokeKind::Inside);
    }
    paint_icon(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(19.0, 19.0)), icon, if active { pal.accent } else { pal.text_dim });
    resp.on_hover_text(tip)
}

/// Turns on the platform's behind-window blur: vibrancy on macOS, acrylic on Windows.
fn apply_window_blur(cc: &eframe::CreationContext<'_>) {
    #[cfg(target_os = "macos")]
    if let Err(e) = mac_blur(cc) {
        eprintln!("strata: window blur unavailable: {e}");
    }
    #[cfg(target_os = "windows")]
    {
        if window_vibrancy::apply_acrylic(cc, Some((11, 14, 19, 120))).is_err() {
            let _ = window_vibrancy::apply_mica(cc, Some(true));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = cc;
}

/// Puts an NSVisualEffectView behind winit's content view, inside the window frame view.
/// (A subview of the content view would draw over the GL surface and hide the app.)
#[cfg(target_os = "macos")]
fn mac_blur(cc: &eframe::CreationContext<'_>) -> anyhow::Result<()> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = cc.window_handle().map_err(|e| anyhow::anyhow!("{e}"))?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { anyhow::bail!("not an AppKit window") };
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow::anyhow!("not on the main thread"))?;
    // SAFETY: winit hands out a valid NSView pointer for the lifetime of the window.
    let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
    let frame_view = unsafe { view.superview() }.ok_or_else(|| anyhow::anyhow!("content view has no frame view"))?;
    let effect = NSVisualEffectView::initWithFrame(mtm.alloc(), frame_view.bounds());
    effect.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);
    effect.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    frame_view.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, Some(view));
    Ok(())
}

impl eframe::App for StrataApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.translucent { [0.0, 0.0, 0.0, 0.0] } else { egui::Rgba::from(p().window).to_array() }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for workspace in &mut self.workspaces { workspace.poll_transactions(); }
        if ctx.input(|i| i.viewport().close_requested()) && self.workspaces.iter().any(|workspace| workspace.has_pending_transactions()) {
            self.request_quit(&ctx);
        }
        self.apply_theme(&ctx);
        set_ground_opacity(if self.translucent { 1.0 - self.settings.window_translucency.clamp(0.0, 0.9) } else { 1.0 });
        paint_ground(&ctx);
        for cfg in std::mem::take(&mut self.pending_open) {
            self.open_connection(cfg, &ctx);
            if let (Some(sql), Some(i)) = (self.run_on_open.take(), self.active) {
                self.workspaces[i].run_after_connect = Some(sql);
            }
        }

        if ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.last_activity = Instant::now();
        }
        if self.settings.passcode_hash.is_some() {
            if self.settings.auto_lock_minutes > 0 && self.last_activity.elapsed().as_secs() >= self.settings.auto_lock_minutes as u64 * 60 {
                self.locked = true;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::L)) {
                self.locked = true;
            }
            if self.settings.auto_lock_minutes > 0 {
                ctx.request_repaint_after(std::time::Duration::from_secs(15));
            }
        }
        if self.locked {
            self.lock_screen(ui);
            return;
        }
        #[cfg(target_os = "macos")]
        if self.native_menu.is_some() { self.dispatch_native_menu(&ctx); }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.settings_open = !self.settings_open;
        }

        if self.active.is_none() {
            #[cfg(not(target_os = "macos"))]
            self.home_menu_bar(ui);
            #[cfg(target_os = "macos")]
            if self.native_menu.is_none() { self.home_menu_bar(ui); }
            self.rail(ui);
        }

        let mut edit_request: Option<Uuid> = None;
        let mut switch_workspace = None;
        let mut workspace_command = None;
        let mut create_connection = None;
        let mut app_request = None;
        let mut show_home = false;
        let workspace_choices: Vec<workspace::WorkspaceChoice> = self.workspaces.iter().map(|ws| workspace::WorkspaceChoice {
            id: ws.id(), name: ws.cfg.display_name(), color: rgb(ws.cfg.accent()),
        }).collect();
        match self.active {
            Some(i) if i < self.workspaces.len() => {
                let mut sh = Shared { store: &self.store, settings: &self.settings, queries: &mut self.queries, boards: &mut self.boards, toasts: &mut self.toasts, edit_connection: &mut edit_request, workspace_choices: &workspace_choices, switch_workspace: &mut switch_workspace, show_home: &mut show_home, open_settings: &mut self.settings_open, workspace_command: &mut workspace_command, create_connection: &mut create_connection, app_request: &mut app_request };
                let ws = &mut self.workspaces[i];
                egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| ws.ui(ui, &mut sh));
                // Safe-mode changes from the workspace toolbar persist to the saved connection.
                let (id, mode) = (ws.cfg.id, ws.cfg.safe_mode);
                if let Some(c) = self.conns.connections.iter_mut().find(|c| c.id == id) {
                    if c.safe_mode != mode {
                        c.safe_mode = mode;
                        let _ = self.store.save_connections(&self.conns);
                    }
                }
            }
            _ => {
                self.active = None;
                let open: Vec<Uuid> = self.workspaces.iter().map(|w| w.id()).collect();
                let actions = self.home.ui(ui, &self.conns, &open, &self.settings, &self.store);
                for a in actions {
                    self.handle_home(a, &ctx);
                }
            }
        }
        if let Some(cfg) = create_connection { self.handle_home(HomeAction::Save { cfg, open: true }, &ctx); }
        self.handle_app_request(app_request, &ctx);
        if let Some(id) = switch_workspace {
            self.active = self.workspaces.iter().position(|ws| ws.id() == id);
        }
        if show_home {
            self.home.refresh_activity();
            self.active = None;
        }
        if let Some(command) = workspace_command {
            match command {
                workspace::WorkspaceCommand::Close(id) => {
                    if let Some(i) = self.workspaces.iter().position(|ws| ws.id() == id) { self.close_workspace(i); }
                }
                workspace::WorkspaceCommand::Reconnect(id) => {
                    if let Some(i) = self.workspaces.iter().position(|ws| ws.id() == id) { self.workspaces[i].reconnect(&ctx); self.active = Some(i); }
                }
                workspace::WorkspaceCommand::Quit => self.request_quit(&ctx),
            }
        }
        if let Some(id) = edit_request {
            let cfg = self.conns.connections.iter().find(|c| c.id == id).cloned().or_else(|| self.workspaces.iter().find(|w| w.id() == id).map(|w| w.cfg.clone()));
            if let Some(cfg) = cfg {
                self.active = None;
                self.home.edit(cfg);
            }
        }

        if self.settings_open {
            let mut open = true;
            let events = self.settings_ui.show(&ctx, &mut open, &mut self.settings, &self.store, &self.themes);
            self.settings_open = open;
            for e in events {
                match e {
                    SettingsEvent::ImportTheme(t) => {
                        self.themes.retain(|x| x.name != t.name);
                        if t.dark {
                            self.settings.dark_theme = t.name.clone();
                        } else {
                            self.settings.light_theme = t.name.clone();
                        }
                        self.themes.push(t);
                        let _ = self.store.save_settings(&self.settings);
                    }
                }
            }
            self.applied_theme = self.applied_theme.take().filter(|(n, s)| *s == self.settings.ui_scale && (n == &self.settings.dark_theme || n == &self.settings.light_theme));
            if self.settings.theme != ThemeChoice::Auto {
                self.applied_theme = None;
            }
        }
        self.screenshot_when_settled(&ctx);
        if self.updates.show(&ctx, &mut self.settings, &mut self.toasts) {
            let _ = self.store.save_settings(&self.settings);
        }
        self.toasts.show(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.remember_open();
    }
}

#[cfg(target_os = "macos")]
impl StrataApp {
    fn dispatch_native_menu(&mut self, ctx: &egui::Context) {
        for action in crate::macos_menu::NativeMenu::take_actions() {
            match action.as_str() {
                "quit" => self.request_quit(ctx),
                "new_connection" => { self.active = None; self.home.begin_connection(); }
                "connections" => self.active = None,
                "settings" => self.settings_open = !self.settings_open,
                "about" => self.updates.about_open = true,
                "check_updates" => { self.updates.about_open = true; self.updates.check(ctx, true); }
                "shortcuts" => self.toasts.info("⌘T New query · ⌘K Find · ⌘↩ Run · ⌘⇧↩ Run all"),
                "open_sqlite" => { self.active = None; self.handle_home(HomeAction::OpenSqliteFile, ctx); }
                "import_connections" => { self.active = None; self.handle_home(HomeAction::ImportConnections, ctx); }
                "export_connections" => { self.active = None; self.handle_home(HomeAction::ExportConnections, ctx); }
                "new_sqlite" => {
                    if let Some(path) = rfd::FileDialog::new().add_filter("SQLite database", &["sqlite", "db"]).set_file_name("database.sqlite").save_file() {
                        let mut cfg = ConnectionConfig::new(DriverKind::Sqlite);
                        cfg.path = path.display().to_string();
                        self.handle_home(HomeAction::Save { cfg, open: true }, ctx);
                    }
                }
                _ => self.dispatch_workspace_menu(&action, ctx),
            }
        }
    }

    fn dispatch_workspace_menu(&mut self, action: &str, ctx: &egui::Context) {
        let Some(i) = self.active.filter(|i| *i < self.workspaces.len()) else { return };
        let mut edit_request = None;
        let mut switch_workspace = None;
        let mut workspace_command = None;
        let mut create_connection = None;
        let mut app_request = None;
        let mut show_home = false;
        let choices: Vec<workspace::WorkspaceChoice> = self.workspaces.iter().map(|ws| workspace::WorkspaceChoice {
            id: ws.id(), name: ws.cfg.display_name(), color: rgb(ws.cfg.accent()),
        }).collect();
        let mut sh = workspace::Shared {
            store: &self.store, settings: &self.settings, queries: &mut self.queries, boards: &mut self.boards,
            toasts: &mut self.toasts, edit_connection: &mut edit_request, workspace_choices: &choices,
            switch_workspace: &mut switch_workspace, show_home: &mut show_home, open_settings: &mut self.settings_open,
            workspace_command: &mut workspace_command, create_connection: &mut create_connection, app_request: &mut app_request,
        };
        self.workspaces[i].handle_native_menu(action, &mut sh, ctx);
        if show_home { self.active = None; }
        if let Some(cfg) = create_connection { self.handle_home(HomeAction::Save { cfg, open: true }, ctx); }
        self.handle_app_request(app_request, ctx);
        if let Some(workspace_command) = workspace_command {
            match workspace_command {
                workspace::WorkspaceCommand::Close(id) => {
                    if let Some(i) = self.workspaces.iter().position(|workspace| workspace.id() == id) { self.close_workspace(i); }
                }
                workspace::WorkspaceCommand::Reconnect(id) => {
                    if let Some(i) = self.workspaces.iter().position(|workspace| workspace.id() == id) { self.workspaces[i].reconnect(ctx); self.active = Some(i); }
                }
                workspace::WorkspaceCommand::Quit => self.request_quit(ctx),
            }
        }
        if let Some(id) = edit_request {
            if let Some(cfg) = self.conns.connections.iter().find(|cfg| cfg.id == id).cloned().or_else(|| self.workspaces.iter().find(|workspace| workspace.id() == id).map(|workspace| workspace.cfg.clone())) {
                self.active = None;
                self.home.edit(cfg);
            }
        }
        if let Some(id) = switch_workspace { self.active = self.workspaces.iter().position(|workspace| workspace.id() == id); }
    }
}
