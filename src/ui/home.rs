//! Connections home: searchable cards grouped into folders, plus the slide-in connection editor.

use super::*;
use crate::model::*;
use crate::runtime::{take, test_connection, Task};
use crate::store::ConnectionFile;
use egui::{vec2, RichText, Stroke};
use uuid::Uuid;

pub enum HomeAction {
    Open(Uuid),
    /// Connect straight from a pasted URL (saved as a new connection).
    ConnectUrl(ConnectionConfig),
    ImportTablePlus,
    Save { cfg: ConnectionConfig, open: bool },
    Delete(Uuid),
    Duplicate(Uuid),
    CreateGroup(String),
    RenameGroup(Uuid, String),
    DeleteGroup(Uuid),
    MoveToGroup(Uuid, Option<Uuid>),
    ExportConnections,
    ImportConnections,
    OpenSqliteFile,
    OpenSettings,
    ToggleTheme,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EditorTab {
    General,
    Ssh,
    Advanced,
}

pub struct ConnEditor {
    pub cfg: ConnectionConfig,
    pub is_new: bool,
    tab: EditorTab,
    test: Option<Task<anyhow::Result<String>>>,
    test_result: Option<(bool, String)>,
    /// Showing the engine picker instead of the form.
    picking: bool,
    pick_query: String,
    pick_filter: u8,
    /// Quick start: a pasted URL fills the form.
    url: String,
}

#[derive(Default)]
pub struct Home {
    search: String,
    connection_search: String,
    page: usize,
    group: Option<Option<Uuid>>,
    env: Option<Tag>,
    /// The launchpad row that is unfolded to show its details.
    open_row: Option<Uuid>,
    pub editor: Option<ConnEditor>,
    picking_driver: bool,
    import_url: Option<String>,
    new_group: Option<String>,
    renaming_group: Option<(Uuid, String)>,
}

const SWATCHES: [[u8; 3]; 8] = [[91, 140, 255], [54, 201, 138], [242, 177, 64], [242, 92, 100], [185, 140, 255], [43, 179, 163], [255, 138, 61], [120, 128, 140]];

impl Home {
    pub fn begin_connection(&mut self) { self.picking_driver = true; }
    /// The saved connection open in the editor (not a new one).
    pub fn editing(&mut self) -> Option<&mut ConnectionConfig> {
        self.editor.as_mut().filter(|e| !e.is_new).map(|e| &mut e.cfg)
    }
    pub fn edit(&mut self, cfg: ConnectionConfig) {
        self.editor = Some(ConnEditor { cfg, is_new: false, tab: EditorTab::General, test: None, test_result: None, picking: false, pick_query: String::new(), pick_filter: 0, url: String::new() });
    }

    fn new_connection(&mut self, driver: DriverKind, settings: &Settings) {
        let mut cfg = ConnectionConfig::new(driver);
        cfg.safe_mode = settings.default_safe_mode;
        cfg.group = self.group.flatten();
        self.editor = Some(ConnEditor { cfg, is_new: true, tab: EditorTab::General, test: None, test_result: None, picking: false, pick_query: String::new(), pick_filter: 0, url: String::new() });
        self.picking_driver = false;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, conns: &ConnectionFile, open: &[Uuid], settings: &Settings, workspaces: &[(Uuid, String, Color32)]) -> Vec<HomeAction> {
        let pal = p();
        let mut actions = Vec::new();

        self.top_bar(ui, workspaces, &mut actions);

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let full = ui.available_width();
                let narrow = full < 860.0;
                let side = if narrow { 16.0 } else { ((full - 1072.0) / 2.0).max(24.0) };
                let max_w = (full - 2.0 * side).max(0.0);
                ui.add_space(if narrow { 24.0 } else { 48.0 });
                ui.horizontal_top(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                    ui.set_width(max_w);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    self.hero(ui, narrow, &mut actions);
                    ui.add_space(24.0);
                    self.omni(ui, conns, &mut actions);
                    if conns.connections.is_empty() {
                        self.welcome_ui(ui);
                        return;
                    }
                    ui.add_space(32.0);
                    if narrow {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            self.filters_ui(ui, conns, false, &mut actions);
                        });
                        ui.add_space(16.0);
                        self.connection_list(ui, conns, open, true, &mut actions);
                    } else {
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(184.0);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                self.filters_ui(ui, conns, true, &mut actions);
                            });
                            ui.add_space(32.0);
                            ui.vertical(|ui| self.connection_list(ui, conns, open, false, &mut actions));
                        });
                    }
                    ui.add_space(64.0);
                    });
                });
            });
        });

        if self.editor.is_some() {
            self.editor_dialog(ui.ctx(), conns, &mut actions);
        }
        self.driver_picker(ui.ctx(), settings);
        self.small_dialogs(ui.ctx(), &mut actions);
        let _ = pal;
        actions
    }

    /// 48 px bar: the mark and name, open workspaces, then theme and settings on the right.
    fn top_bar(&mut self, ui: &mut egui::Ui, workspaces: &[(Uuid, String, Color32)], actions: &mut Vec<HomeAction>) {
        let pal = p();
        let left: i8 = if cfg!(target_os = "macos") { 84 } else { 16 };
        egui::Panel::top("home-top").exact_size(48.0).show_separator_line(false).resizable(false)
            .frame(egui::Frame::new().fill(pal.sidebar).inner_margin(egui::Margin { left, right: 16, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                let bar = ui.max_rect();
                // The title bar is hidden on macOS: the empty bar drags the window.
                let drag = ui.interact(bar, egui::Id::unique("home-top-drag"), egui::Sense::click_and_drag());
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                ui.painter().hline(bar.x_range().expand(200.0), bar.bottom() - 0.5, Stroke::new(1.0, line()));
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::hover());
                    paint_logo(ui.painter(), r.shrink(1.0));
                    ui.label(RichText::new("Strata").font(semibold(14.0)).color(pal.text));
                    if !workspaces.is_empty() {
                        ui.add_space(12.0);
                        for (id, name, color) in workspaces {
                            let g = ui.painter().layout_no_wrap(name.clone(), egui::FontId::proportional(13.0), pal.text_dim);
                            let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 30.0, 32.0), egui::Sense::click());
                            let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), super::motion::FAST as f32);
                            if t > 0.0 {
                                ui.painter().rect_filled(r, egui::CornerRadius::same(6), pal.surface_hover.gamma_multiply(t));
                            }
                            ui.painter().circle_filled(egui::pos2(r.left() + 12.0, r.center().y), 4.0, *color);
                            ui.painter().galley(egui::pos2(r.left() + 22.0, r.center().y - g.size().y / 2.0), g, mix(pal.text_dim, pal.text, t));
                            if resp.on_hover_text("Back to this workspace").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                actions.push(HomeAction::Open(*id));
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if tool_button(ui, Icon::Sliders, "Settings (⌘,)", false).clicked() {
                            actions.push(HomeAction::OpenSettings);
                        }
                        if tool_button(ui, Icon::Contrast, "Switch between the dark and light theme", false).clicked() {
                            actions.push(HomeAction::ToggleTheme);
                        }
                    });
                });
            });
    }

    fn hero(&mut self, ui: &mut egui::Ui, narrow: bool, actions: &mut Vec<HomeAction>) {
        let pal = p();
        let heading = |ui: &mut egui::Ui| {
            ui.label(RichText::new("Where to dig today?").font(super::display(if narrow { 32.0 } else { 40.0 })).color(pal.text));
        };
        if narrow {
            heading(ui);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                self.hero_buttons(ui, actions, false);
            });
        } else {
            ui.horizontal(|ui| {
                heading(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    self.hero_buttons(ui, actions, true);
                });
            });
        }
    }

    fn hero_buttons(&mut self, ui: &mut egui::Ui, actions: &mut Vec<HomeAction>, rtl: bool) {
        let new_conn = |ui: &mut egui::Ui, me: &mut Self| {
            if btn(ui, "New connection", BtnKind::Primary).clicked() {
                me.picking_driver = true;
            }
        };
        if rtl {
            new_conn(ui, self);
        }
        let import = btn(ui, "Import", BtnKind::Secondary);
        egui::Popup::menu(&import).show(|ui| {
            ui.set_min_width(220.0);
            if ui.button("Import connections…").clicked() {
                actions.push(HomeAction::ImportConnections);
                ui.close();
            }
            if ui.button("Import from TablePlus…").clicked() {
                actions.push(HomeAction::ImportTablePlus);
                ui.close();
            }
            if ui.button("Import from URL…").clicked() {
                self.import_url = Some(String::new());
                ui.close();
            }
            if ui.button("Open SQLite file…").clicked() {
                actions.push(HomeAction::OpenSqliteFile);
                ui.close();
            }
            ui.separator();
            if ui.button("New group…").clicked() {
                self.new_group = Some(String::new());
                ui.close();
            }
            if ui.button("Export connections…").clicked() {
                actions.push(HomeAction::ExportConnections);
                ui.close();
            }
        });
        if !rtl {
            new_conn(ui, self);
        }
    }

    /// One field for both: paste a URL to connect, or type to find a saved connection.
    fn omni(&mut self, ui: &mut egui::Ui, conns: &ConnectionFile, actions: &mut Vec<HomeAction>) {
        let pal = p();
        let parsed = self.search.contains("://").then(|| ConnectionConfig::from_url(&self.search));
        let detected = match &parsed {
            Some(Ok(c)) => Some(c.driver.label()),
            _ => None,
        };
        let mut connect = false;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), egui::Sense::hover());
        let focus_id = egui::Id::unique("home-omni");
        let focused = ui.ctx().memory(|m| m.has_focus(focus_id));
        let ft = ui.ctx().animate_bool_with_time(focus_id.with("ring"), focused, super::motion::FAST as f32);
        ui.painter().rect_filled(rect, egui::CornerRadius::same(255), pal.control());
        if ft > 0.0 {
            ui.painter().rect_stroke(rect, egui::CornerRadius::same(255), Stroke::new(1.0, pal.text_dim.gamma_multiply(ft)), egui::StrokeKind::Inside);
        }
        paint_icon(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), vec2(16.0, 16.0)), Icon::Search, pal.text_faint);
        // Detected engine label slides open beside the Connect button.
        let dt = ui.ctx().animate_bool_with_time(focus_id.with("detect"), detected.is_some(), super::motion::SETTLE as f32);
        let mut right = rect.right() - 8.0;
        if dt > 0.0 {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(egui::pos2(rect.right() - 300.0, rect.top()), egui::pos2(rect.right() - 8.0, rect.bottom()))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            child.spacing_mut().item_spacing.x = 12.0;
            child.set_opacity(dt);
            if btn(&mut child, "Connect", BtnKind::Primary).clicked() {
                connect = true;
            }
            if let Some(name) = detected {
                child.label(RichText::new(format!("{name} detected")).color(pal.text_dim));
            }
            right = child.min_rect().left() - 12.0;
        }
        let edit_rect = egui::Rect::from_min_max(egui::pos2(rect.left() + 40.0, rect.top()), egui::pos2(right, rect.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let resp = child.add(
            crate::ui::line_edit(&mut self.search)
                .id(focus_id)
                .hint_text(RichText::new("Paste a URL like postgres://… or type a connection name").font(egui::FontId::proportional(14.0)).color(pal.text_faint))
                .font(egui::FontId::monospace(14.0))
                .frame(egui::Frame::NONE)
                .desired_width(edit_rect.width()),
        );
        if resp.changed() {
            self.page = 0;
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            connect = true;
        }
        ui.add_space(8.0);
        match &parsed {
            Some(Err(e)) if !self.search.trim().is_empty() => {
                ui.label(RichText::new(e.to_string()).color(pal.danger));
            }
            Some(Ok(c)) => {
                ui.label(dim(format!("{} · {}. Press Enter to connect; it's saved as a new connection.", c.driver.label(), c.subtitle())));
            }
            _ => {
                ui.label(dim("Strata detects the engine from the URL. Drop a .sqlite or .db file on the window to open it."));
            }
        }
        if connect {
            match parsed {
                Some(Ok(c)) => {
                    actions.push(HomeAction::ConnectUrl(c));
                    self.search.clear();
                }
                Some(Err(_)) => {}
                None => {
                    let q = self.search.to_lowercase();
                    let best = conns.connections.iter().filter(|c| self.env.map(|e| c.tag == e).unwrap_or(true)).find(|c| q.is_empty() || c.display_name().to_lowercase().contains(&q) || c.host.to_lowercase().contains(&q));
                    if let Some(c) = best {
                        actions.push(HomeAction::Open(c.id));
                    }
                }
            }
        }
    }

    /// Environment filters: a column of flat rows (or a strip on narrow windows) with counts.
    fn filters_ui(&mut self, ui: &mut egui::Ui, conns: &ConnectionFile, column: bool, actions: &mut Vec<HomeAction>) {
        let row = |ui: &mut egui::Ui, selected: bool, label: &str, dot: Option<Color32>, n: usize| -> egui::Response {
            let pal = p();
            let font = if selected { medium(13.0) } else { egui::FontId::proportional(13.0) };
            let g = ui.painter().layout_no_wrap(label.to_string(), font, pal.text);
            let ng = ui.painter().layout_no_wrap(n.to_string(), egui::FontId::monospace(12.0), pal.text_faint);
            let dot_w = if dot.is_some() { 16.0 } else { 0.0 };
            let w = if column { ui.available_width() } else { g.size().x + ng.size().x + dot_w + 24.0 };
            let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), egui::Sense::click());
            let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), super::motion::FAST as f32);
            let fill = if selected { pal.control_on() } else { pal.control().gamma_multiply(t) };
            ui.painter().rect_filled(r, egui::CornerRadius::same(8), fill);
            let mut x = r.left() + 8.0;
            if let Some(c) = dot {
                ui.painter().circle_filled(egui::pos2(x + 4.0, r.center().y), 4.0, c);
                x += dot_w;
            }
            let text = if selected { pal.text } else { mix(pal.text_dim, pal.text, t) };
            let ngw = ng.size().x;
            ui.painter().galley(egui::pos2(x, r.center().y - g.size().y / 2.0), g, text);
            let nx = if column { r.right() - 8.0 - ngw } else { r.right() - 8.0 - ngw };
            ui.painter().galley(egui::pos2(nx, r.center().y - ng.size().y / 2.0), ng, pal.text_faint);
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        if row(ui, self.env.is_none() && self.group.is_none(), "All", None, conns.connections.len()).clicked() {
            self.page = 0;
            self.env = None;
            self.group = None;
        }
        for t in [Tag::Local, Tag::Development, Tag::Testing, Tag::Staging, Tag::Production] {
            let n = conns.connections.iter().filter(|c| c.tag == t).count();
            let label = t.label();
            let label = format!("{}{}", label[..1].to_uppercase(), &label[1..]);
            if row(ui, self.env == Some(t), &label, Some(env_color(t)), n).clicked() {
                self.page = 0;
                self.env = if self.env == Some(t) { None } else { Some(t) };
                self.group = None;
            }
        }
        if !conns.groups.is_empty() && column {
            ui.add_space(14.0);
        }
        for g in &conns.groups {
            let n = conns.connections.iter().filter(|c| c.group == Some(g.id)).count();
            let r = row(ui, self.group == Some(Some(g.id)), &g.name, Some(crate::theme::rgb(g.color)), n);
            if r.clicked() {
                self.page = 0;
                self.group = if self.group == Some(Some(g.id)) { None } else { Some(Some(g.id)) };
                self.env = None;
            }
            r.context_menu(|ui| {
                if ui.button("Rename…").clicked() {
                    self.renaming_group = Some((g.id, g.name.clone()));
                    ui.close();
                }
                if ui.button("Delete group").clicked() {
                    actions.push(HomeAction::DeleteGroup(g.id));
                    if self.group == Some(Some(g.id)) {
                        self.group = None;
                    }
                    ui.close();
                }
            });
        }
    }

    fn welcome_ui(&mut self, ui: &mut egui::Ui) {
        let pal = p();
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No connections yet").font(semibold(16.0)).color(pal.text));
            ui.add_space(4.0);
            ui.label(dim("A fast, native workbench for PostgreSQL, MySQL, SQLite, SQL Server, Redis, MongoDB and more."));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space((ui.available_width() - 290.0).max(0.0) / 2.0);
                if btn(ui, "Create a connection", BtnKind::Primary).clicked() {
                    self.picking_driver = true;
                }
                if btn(ui, "Import from URL", BtnKind::Secondary).clicked() {
                    self.import_url = Some(String::new());
                }
            });
            ui.add_space(32.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space((ui.available_width() - 7.0 * 40.0).max(0.0) / 2.0);
                for d in DriverKind::ALL.iter().take(7) {
                    driver_tile(ui, *d, 32.0).on_hover_text(d.label());
                }
            });
        });
    }

    /// Search and paginate saved connections after applying the environment/group filters.
    fn connection_list(&mut self, ui: &mut egui::Ui, conns: &ConnectionFile, open: &[Uuid], narrow: bool, actions: &mut Vec<HomeAction>) {
        ui.label(RichText::new("Saved connections").font(semibold(14.0)).color(p().text));
        ui.add_space(8.0);
        if super::form::search_field(ui, &mut self.connection_search,
            "Search name, host, database or engine", egui::Id::unique("home-connection-search"), ui.available_width()).changed() {
            self.page = 0;
        }
        ui.add_space(12.0);
        let queries = [self.search.trim().to_lowercase(), self.connection_search.trim().to_lowercase()];
        let mut visible: Vec<&ConnectionConfig> = conns.connections.iter()
            .filter(|c| self.group.map(|g| c.group == g).unwrap_or(true))
            .filter(|c| self.env.map(|e| c.tag == e).unwrap_or(true))
            .filter(|c| queries.iter().all(|q| q.is_empty() || q.contains("://")
                || c.display_name().to_lowercase().contains(q)
                || c.host.to_lowercase().contains(q)
                || c.database.to_lowercase().contains(q)
                || c.driver.label().to_lowercase().contains(q)))
            .collect();
        visible.sort_by(|a, b| b.last_used.cmp(&a.last_used).then_with(|| a.display_name().to_lowercase().cmp(&b.display_name().to_lowercase())));
        let pages = visible.len().div_ceil(10).max(1);
        self.page = self.page.min(pages - 1);
        let start = self.page * 10;
        let end = (start + 10).min(visible.len());
        self.table(ui, &visible[start..end], conns, open, narrow, actions);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let first = if visible.is_empty() { 0 } else { start + 1 };
            ui.label(dim(format!("{first}–{end} of {}", visible.len())));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(self.page + 1 < pages, egui::Button::new("Next")).clicked() {
                    self.page += 1;
                }
                ui.label(dim(format!("Page {} of {pages}", self.page + 1)));
                if ui.add_enabled(self.page > 0, egui::Button::new("Previous")).clicked() {
                    self.page -= 1;
                }
            });
        });
    }

    /// Swiss table: brand tile, name, address, environment, last used. A row unfolds to its details.
    fn table(&mut self, ui: &mut egui::Ui, items: &[&ConnectionConfig], conns: &ConnectionFile, open: &[Uuid], narrow: bool, actions: &mut Vec<HomeAction>) {
        let pal = p();
        let full = ui.available_width();
        // Column geometry: 32 | 1.1fr | 1.5fr | 112 | 84 | 16, 12 px gaps, 12 px sides.
        let inner = full - 24.0;
        let (name_w, addr_w) = if narrow { (inner - 32.0 - 16.0 - 24.0, 0.0) } else {
            let flex = (inner - 32.0 - 112.0 - 84.0 - 16.0 - 60.0).max(80.0);
            (flex * 1.1 / 2.6, flex * 1.5 / 2.6)
        };
        let x_tile = 12.0;
        let x_name = x_tile + 32.0 + 12.0;
        let x_addr = x_name + name_w + 12.0;
        let x_env = x_addr + addr_w + 12.0;
        let x_used = x_env + 112.0 + 12.0;
        // Flat list on the canvas: hairlines between rows, a rounded soft fill under the pointer.
        let frame = egui::Frame::new();
        frame.show(ui, |ui| {
            ui.set_width(full);
            ui.spacing_mut().item_spacing.y = 0.0;
            if !narrow {
                let (hr, _) = ui.allocate_exact_size(vec2(full, 32.0), egui::Sense::hover());
                for (x, t) in [(x_name, "Name"), (x_addr, "Address"), (x_env, "Environment"), (x_used, "Last used")] {
                    ui.painter().text(egui::pos2(hr.left() + x, hr.center().y), egui::Align2::LEFT_CENTER, t, egui::FontId::proportional(12.0), pal.text_faint);
                }
            }
            if items.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(full, 84.0), egui::Sense::hover());
                ui.painter().hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, line()));
                ui.painter().text(egui::pos2(r.left() + 12.0, r.center().y), egui::Align2::LEFT_CENTER, "No connections match. Try another name, host or environment.", egui::FontId::proportional(13.0), pal.text_dim);
                return;
            }
            for (i, c) in items.iter().enumerate() {
                let expanded = self.open_row == Some(c.id);
                let (rect, resp) = ui.allocate_exact_size(vec2(full, 52.0), egui::Sense::click());
                let painter = ui.painter();
                if i > 0 || !narrow {
                    painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, line()));
                }
                let t = ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), super::motion::FAST as f32);
                let et = ui.ctx().animate_bool_with_time(resp.id.with("open"), expanded, super::motion::SETTLE as f32);
                let radius = egui::CornerRadius::same(10);
                let fill = pal.control().gamma_multiply(t).lerp_to_gamma(pal.control_hover(), et);
                if t > 0.0 || et > 0.0 {
                    painter.rect_filled(rect.shrink2(vec2(0.5, 0.0)), radius, fill);
                }
                let y = rect.center().y;
                let tile = egui::Rect::from_center_size(egui::pos2(rect.left() + x_tile + 16.0, y), vec2(32.0, 32.0));
                paint_driver_badge(painter, tile, c.driver);
                let clip = |x: f32, w: f32| ui.painter_at(egui::Rect::from_min_max(egui::pos2(rect.left() + x, rect.top()), egui::pos2(rect.left() + x + w, rect.bottom())));
                let name = ellipsize_to(painter, &c.display_name(), medium(13.0), name_w);
                clip(x_name, name_w).text(egui::pos2(rect.left() + x_name, y), egui::Align2::LEFT_CENTER, name, medium(13.0), pal.text);
                if !narrow {
                    let addr = ellipsize_to(painter, &short_address(c), egui::FontId::monospace(12.0), addr_w);
                    clip(x_addr, addr_w).text(egui::pos2(rect.left() + x_addr, y), egui::Align2::LEFT_CENTER, addr, egui::FontId::monospace(12.0), pal.text_dim);
                    if c.tag != Tag::None {
                        painter.circle_filled(egui::pos2(rect.left() + x_env + 4.0, y), 4.0, env_color(c.tag));
                        clip(x_env + 16.0, 96.0).text(egui::pos2(rect.left() + x_env + 16.0, y), egui::Align2::LEFT_CENTER, c.tag.label(), egui::FontId::proportional(13.0), pal.text);
                    }
                    let used = if open.contains(&c.id) { "open now".to_string() } else { c.last_used.map(relative_time).unwrap_or_else(|| "never".into()) };
                    painter.text(egui::pos2(rect.left() + x_used, y), egui::Align2::LEFT_CENTER, used, egui::FontId::proportional(13.0), pal.text_dim);
                }
                // Chevron turns to point down as the row opens.
                let chev = egui::Rect::from_center_size(egui::pos2(rect.right() - 12.0 - 8.0, y), vec2(12.0, 12.0));
                paint_chevron(painter, chev, et, pal.text_faint);
                if resp.double_clicked() {
                    actions.push(HomeAction::Open(c.id));
                } else if resp.clicked() {
                    self.open_row = if expanded { None } else { Some(c.id) };
                }
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                resp.context_menu(|ui| self.row_menu(ui, c, conns, actions));
                let body = super::motion::unfold(ui, egui::Id::unique(("conn-detail", c.id)), expanded, |ui| {
                    egui::Frame::new().fill(pal.control()).inner_margin(egui::Margin { left: if narrow { 12 } else { 56 }, right: 12, top: 4, bottom: 12 })
                        .corner_radius(egui::CornerRadius::same(10))
                        .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(24.0, 8.0);
                            let safe = c.safe_mode.label().split(" — ").next().unwrap_or("").to_string();
                            let db = if c.driver == DriverKind::Sqlite { "main".to_string() } else if c.database.is_empty() { "default".to_string() } else { c.database.clone() };
                            for (k, v) in [("Engine", c.driver.label().to_string()), ("Database", db), ("Safe mode", safe)] {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.label(RichText::new(k).size(12.0).color(pal.text_faint));
                                    ui.label(RichText::new(v).font(egui::FontId::monospace(13.0)).color(pal.text));
                                });
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if btn(ui, "Connect", BtnKind::Primary).clicked() {
                                    actions.push(HomeAction::Open(c.id));
                                }
                                if btn(ui, "Duplicate", BtnKind::Secondary).clicked() {
                                    actions.push(HomeAction::Duplicate(c.id));
                                }
                                if btn(ui, "Edit", BtnKind::Ghost).clicked() {
                                    self.edit((*c).clone());
                                }
                            });
                        });
                    });
                });
                let _ = body;
            }
        });
    }

    fn row_menu(&mut self, ui: &mut egui::Ui, c: &ConnectionConfig, conns: &ConnectionFile, actions: &mut Vec<HomeAction>) {
        ui.set_min_width(200.0);
        if ui.button("Connect").clicked() {
            actions.push(HomeAction::Open(c.id));
            ui.close();
        }
        if ui.button("Edit…").clicked() {
            self.edit(c.clone());
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            actions.push(HomeAction::Duplicate(c.id));
            ui.close();
        }
        if ui.button("Copy connection URL").clicked() {
            ui.ctx().copy_text(c.to_url(false));
            ui.close();
        }
        if !conns.groups.is_empty() {
            ui.menu_button("Move to group", |ui| {
                if ui.button("Ungrouped").clicked() {
                    actions.push(HomeAction::MoveToGroup(c.id, None));
                    ui.close();
                }
                for g in &conns.groups {
                    if ui.button(&g.name).clicked() {
                        actions.push(HomeAction::MoveToGroup(c.id, Some(g.id)));
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        if ui.button(RichText::new("Delete").color(p().danger)).clicked() {
            actions.push(HomeAction::Delete(c.id));
            ui.close();
        }
    }

    fn small_dialogs(&mut self, ctx: &egui::Context, actions: &mut Vec<HomeAction>) {
        let pal = p();
        if let Some(url) = &mut self.import_url {
            let mut close = false;
            let mut parsed = None;
            let r = egui::Modal::new(egui::Id::unique("import-url")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(RichText::new("Import from URL").size(16.0).strong());
                ui.label(dim("postgresql://user:password@host:5432/db?sslmode=require&name=Production"));
                ui.add(crate::ui::line_edit(url).desired_width(f32::INFINITY).hint_text("Paste a connection URL")).request_focus();
                let res = ConnectionConfig::from_url(url);
                if let (Err(e), false) = (&res, url.trim().is_empty()) {
                    ui.label(RichText::new(e.to_string()).color(pal.danger).size(12.0));
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary_button(ui, "Import").clicked() {
                            parsed = res.ok();
                        }
                        if secondary_button(ui, "Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
            if let Some(cfg) = parsed {
                self.editor = Some(ConnEditor { cfg, is_new: true, tab: EditorTab::General, test: None, test_result: None, picking: false, pick_query: String::new(), pick_filter: 0, url: String::new() });
                close = true;
            }
            if close || r.should_close() {
                self.import_url = None;
            }
        }
        if let Some(name) = &mut self.new_group {
            let mut close = false;
            let r = egui::Modal::new(egui::Id::unique("new-group")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("New group").size(16.0).strong());
                ui.add(crate::ui::line_edit(name).hint_text("e.g. Client A, Staging").desired_width(f32::INFINITY)).request_focus();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary_button(ui, "Create").clicked() && !name.trim().is_empty() {
                            actions.push(HomeAction::CreateGroup(name.trim().to_string()));
                            close = true;
                        }
                        if secondary_button(ui, "Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
            if close || r.should_close() {
                self.new_group = None;
            }
        }
        if let Some((id, name)) = &mut self.renaming_group {
            let mut close = false;
            let r = egui::Modal::new(egui::Id::unique("rename-group")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Rename group").size(16.0).strong());
                ui.add(crate::ui::line_edit(name).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if primary_button(ui, "Save").clicked() && !name.trim().is_empty() {
                        actions.push(HomeAction::RenameGroup(*id, name.trim().to_string()));
                        close = true;
                    }
                    if secondary_button(ui, "Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if close || r.should_close() {
                self.renaming_group = None;
            }
        }
    }

    /// The connection dialog: header, underline tabs, flat field groups, a footer of actions.
    /// "Change database" swaps the body for the engine picker.
    fn editor_dialog(&mut self, ctx: &egui::Context, conns: &ConnectionFile, actions: &mut Vec<HomeAction>) {
        let pal = p();
        let mut close = false;
        let Some(ed) = &mut self.editor else { return };
        if let Some(r) = take(&mut ed.test) {
            ed.test_result = Some(match r {
                Ok(m) => (true, m),
                Err(e) => (false, format!("{e:#}")),
            });
        }
        let ((), _) = super::form::dialog(ctx, "conn-dialog", vec2(680.0, 780.0), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let size = ui.max_rect().size();
            // Header.
            ui.allocate_ui_with_layout(vec2(size.x, 64.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.add_space(16.0);
                driver_tile(ui, ed.cfg.driver, 40.0);
                ui.vertical(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(if ed.is_new { "New connection" } else { "Edit connection" }).font(semibold(16.0)).color(pal.text));
                    let work = ed.cfg.driver.workload().split(" / ").last().unwrap_or("");
                    let work = format!("{}{}", work[..1].to_uppercase(), &work[1..]);
                    ui.label(RichText::new(format!("{} · {work}", ed.cfg.driver.label())).size(12.0).color(pal.text_dim));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_space(12.0);
                    if tool_button(ui, Icon::Close, "Close", false).clicked() {
                        close = true;
                    }
                    if btn(ui, if ed.picking { "Back to the form" } else { "Change database" }, BtnKind::Ghost).clicked() {
                        ed.picking = !ed.picking;
                    }
                });
            });
            if ed.picking {
                if let Some(d) = engine_picker(ui, &mut ed.pick_query, &mut ed.pick_filter, Some(ed.cfg.driver)) {
                    if d != ed.cfg.driver {
                        ed.cfg.port = d.default_port();
                        ed.cfg.driver = d;
                    }
                    ed.picking = false;
                }
                return;
            }
            let mut tabs = vec![(EditorTab::General, "General")];
            if ed.cfg.driver.supports_ssh() {
                tabs.push((EditorTab::Ssh, "SSH"));
            }
            tabs.push((EditorTab::Advanced, "Advanced"));
            if !tabs.iter().any(|t| t.0 == ed.tab) {
                ed.tab = EditorTab::General;
            }
            super::form::underline_tabs(ui, &mut ed.tab, &tabs, 96.0);
            let footer_h = 57.0;
            let body_h = ui.available_height() - footer_h;
            // Panels cross-fade and rise 4 px when the tab changes.
            let tab_t = super::motion::ease_out(super::motion::entered(ui.ctx(), egui::Id::unique(("conn-tab", ed.tab as u8)), super::motion::SETTLE));
            ui.allocate_ui(vec2(size.x, body_h), |ui| {
                ui.set_min_height(body_h);
                ui.set_opacity(tab_t);
                egui::ScrollArea::vertical().id_salt(("conn-body", ed.tab as u8)).auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 16 + (4.0 * (1.0 - tab_t)) as i8, bottom: 0 }).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        editor_body(ui, ed, conns);
                    });
                });
            });
            // Footer.
            let (fr, _) = ui.allocate_exact_size(vec2(size.x, footer_h), egui::Sense::hover());
            ui.painter().hline(fr.x_range(), fr.top() + 0.5, Stroke::new(1.0, line()));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(fr.shrink2(vec2(16.0, 12.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            child.spacing_mut().item_spacing.x = 8.0;
            let can_save = ed.cfg.form_problem().is_none();
            if child.add_enabled_ui(can_save, |ui| btn(ui, "Connect", BtnKind::Primary)).inner.clicked() {
                actions.push(HomeAction::Save { cfg: ed.cfg.clone(), open: true });
                close = true;
            }
            if child.add_enabled_ui(can_save, |ui| btn(ui, "Save", BtnKind::Secondary)).inner.clicked() {
                actions.push(HomeAction::Save { cfg: ed.cfg.clone(), open: false });
                close = true;
            }
            if child.add_enabled_ui(can_save && ed.test.is_none(), |ui| btn(ui, "Test", BtnKind::Secondary)).inner.clicked() {
                ed.test_result = None;
                let cfg = ed.cfg.clone();
                ed.test = Some(Task::spawn(ui.ctx(), test_connection(cfg)));
            }
            child.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ed.test.is_some() {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(dim("Testing the connection…"));
                } else if let Some((ok, msg)) = &ed.test_result {
                    let first = msg.lines().next().unwrap_or("").to_string();
                    ui.label(RichText::new(first).color(if *ok { pal.text_dim } else { pal.danger })).on_hover_text(msg);
                } else if let Some(problem) = ed.cfg.form_problem() {
                    ui.label(dim(problem));
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if close {
            self.editor = None;
        }
    }

    /// New connection opens the dialog on the engine picker.
    fn driver_picker(&mut self, _ctx: &egui::Context, settings: &Settings) {
        if !self.picking_driver {
            return;
        }
        self.new_connection(DriverKind::Postgres, settings);
        if let Some(ed) = &mut self.editor {
            ed.picking = true;
        }
    }
}

/// Engine picker: a search box, a kind filter and a grid of engines. Returns the one chosen.
fn engine_picker(ui: &mut egui::Ui, query: &mut String, filter: &mut u8, current: Option<DriverKind>) -> Option<DriverKind> {
    let pal = p();
    let mut chosen = None;
    let full = ui.available_width();
    egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 12 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let w = (ui.available_width() - 328.0).max(180.0);
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), egui::Sense::hover());
            ui.painter().rect_filled(r, egui::CornerRadius::same(6), pal.window);
            ui.painter().rect_stroke(r, egui::CornerRadius::same(6), Stroke::new(1.0, line2()), egui::StrokeKind::Inside);
            paint_icon(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.left() + 18.0, r.center().y), vec2(16.0, 16.0)), Icon::Search, pal.text_faint);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(egui::pos2(r.left() + 34.0, r.top()), r.max - vec2(8.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
            child.add(crate::ui::line_edit(query).frame(egui::Frame::NONE).hint_text(faint("Search databases")).desired_width(r.width() - 42.0)).request_focus();
            super::form::seg_control(ui, filter, &[(0, "All"), (1, "SQL"), (2, "Warehouse"), (3, "NoSQL")], 320.0);
        });
    });
    let q = query.to_lowercase();
    let engines: Vec<DriverKind> = DriverKind::ALL
        .into_iter()
        .filter(|d| match *filter {
            1 => d.workload().starts_with("OLTP"),
            2 => d.workload().starts_with("OLAP"),
            3 => d.workload().starts_with("Operational"),
            _ => true,
        })
        .filter(|d| q.is_empty() || d.label().to_lowercase().contains(&q))
        .collect();
    egui::ScrollArea::vertical().id_salt("engine-grid").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 16 }).show(ui, |ui| {
            if engines.is_empty() {
                ui.add_space(12.0);
                ui.label(dim("No database matches that search."));
                return;
            }
            let inner = full - 24.0;
            let cols = ((inner + 4.0) / 204.0).floor().max(1.0) as usize;
            let cw = (inner - (cols - 1) as f32 * 4.0) / cols as f32;
            for row in engines.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for d in row {
                        let (r, resp) = ui.allocate_exact_size(vec2(cw, 48.0), egui::Sense::click());
                        let on = current == Some(*d);
                        let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), super::motion::FAST as f32);
                        if on {
                            ui.painter().rect_filled(r, egui::CornerRadius::same(6), pal.surface);
                            ui.painter().rect_stroke(r, egui::CornerRadius::same(6), Stroke::new(1.5, pal.grid_selection_border), egui::StrokeKind::Inside);
                        } else if t > 0.0 {
                            ui.painter().rect_filled(r, egui::CornerRadius::same(6), pal.surface_hover.gamma_multiply(t));
                        }
                        paint_driver_badge(ui.painter(), egui::Rect::from_min_size(r.min + vec2(8.0, 8.0), vec2(32.0, 32.0)), *d);
                        let x = r.left() + 52.0;
                        let name = ellipsize_to(ui.painter(), d.label(), medium(13.0), r.right() - x - 8.0);
                        ui.painter().text(egui::pos2(x, r.top() + 14.0), egui::Align2::LEFT_CENTER, name, medium(13.0), pal.text);
                        let work = if d.is_odbc() { "Vendor ODBC".to_string() } else { d.workload().to_string() };
                        ui.painter().text(egui::pos2(x, r.top() + 33.0), egui::Align2::LEFT_CENTER, work, egui::FontId::proportional(12.0), pal.text_faint);
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, d.label()));
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            chosen = Some(*d);
                        }
                    }
                });
                ui.add_space(4.0);
            }
        });
    });
    chosen
}

const LABEL_W: Option<f32> = Some(136.0);

/// The fields of the current tab, in flat groups.
fn editor_body(ui: &mut egui::Ui, ed: &mut ConnEditor, conns: &ConnectionFile) {
    use super::form::{group, group_with_note};
    match ed.tab {
        EditorTab::General => {
            if ed.is_new {
                group(ui, "Quick start", LABEL_W, |g| {
                    if g.text("Connection URL", &mut ed.url, "postgres://user:pass@host:5432/db", true).changed() {
                        if let Ok(parsed) = ConnectionConfig::from_url(&ed.url) {
                            let keep = (ed.cfg.id, ed.cfg.tag, ed.cfg.color, ed.cfg.group, ed.cfg.safe_mode);
                            ed.cfg = parsed;
                            (ed.cfg.id, ed.cfg.tag, ed.cfg.color, ed.cfg.group, ed.cfg.safe_mode) = keep;
                        }
                    }
                });
            }
            let c = &mut ed.cfg;
            group(ui, "Identity", LABEL_W, |g| {
                let hint = c.display_name();
                g.text("Name", &mut c.name, &hint, false);
                let tags: Vec<(Tag, String, Option<Color32>)> = Tag::ALL.iter().map(|t| (*t, t.label().to_string(), (*t != Tag::None).then(|| env_color(*t)))).collect();
                g.menu("Tag", &mut c.tag, &tags);
                g.swatches("Color", &mut c.color, &SWATCHES);
                if !conns.groups.is_empty() {
                    let mut opts: Vec<(Option<Uuid>, String, Option<Color32>)> = vec![(None, "Ungrouped".into(), None)];
                    opts.extend(conns.groups.iter().map(|x| (Some(x.id), x.name.clone(), Some(crate::theme::rgb(x.color)))));
                    g.menu("Group", &mut c.group, &opts);
                }
            });
            let mut browse_db = false;
            let note = match c.driver {
                d if d.is_odbc() => "Configure the DSN in your ODBC administrator with the server, TLS and authentication settings. Passwords are stored in the system keychain.",
                DriverKind::ClickHouse => "Use the HTTP port (8123) or the native port (9000); Strata detects which one it reached, also through a forwarded port. With SSL on, it encrypts on the same port.",
                DriverKind::Elasticsearch => "Connects over HTTP. With SSL on, Strata uses HTTPS on the same port.",
                DriverKind::MongoDb if c.mongo_uri() => "Port, user, password and TLS are read from the URI.",
                _ => "",
            };
            group_with_note(ui, "Connection", note, LABEL_W, |g| match c.driver {
                driver if driver.is_odbc() => {
                    g.text("Data source (DSN)", &mut c.odbc_dsn, "", true);
                    g.text("User", &mut c.user, "", true);
                    g.secret("Password", &mut c.password, "Stored in your system keychain");
                    g.text("Catalog filter", &mut c.database, "optional", true);
                }
                DriverKind::Sqlite => {
                    browse_db = g.path("Database file", &mut c.path, "/path/to/database.sqlite");
                }
                DriverKind::LibSql => {
                    g.text("Database URL", &mut c.host, "libsql://my-db-org.turso.io", true);
                    g.secret("Auth token", &mut c.password, "");
                }
                DriverKind::CloudflareD1 => {
                    g.text("Account ID", &mut c.host, "", true);
                    g.text("Database ID", &mut c.database, "", true);
                    g.secret("API token", &mut c.password, "");
                }
                _ => {
                    let uri = c.mongo_uri();
                    if uri || c.driver == DriverKind::MongoDb && c.host.contains("://") {
                        g.text("Host or URI", &mut c.host, "mongodb+srv://…", true);
                    } else {
                        let host_label = if c.driver == DriverKind::MongoDb { "Host or URI" } else { "Host" };
                        g.host_port(host_label, &mut c.host, "localhost", &mut c.port);
                        let user_label = match c.driver {
                            DriverKind::Redis => "User (ACL)",
                            DriverKind::MongoDb | DriverKind::Elasticsearch => "User, optional",
                            _ => "User",
                        };
                        g.text(user_label, &mut c.user, "", true);
                        g.secret("Password", &mut c.password, "Stored in your system keychain");
                    }
                    let (db_label, db_hint) = match c.driver {
                        DriverKind::Redis => ("Database index", "0"),
                        DriverKind::Elasticsearch => ("", ""),
                        DriverKind::ClickHouse => ("Database", "default"),
                        DriverKind::MongoDb => ("Database", "test"),
                        _ => ("Database", "optional"),
                    };
                    if !db_label.is_empty() {
                        g.text(db_label, &mut c.database, db_hint, true);
                    }
                    if !uri {
                        let opts: Vec<(SslMode, &str)> = SslMode::ALL.iter().map(|m| (*m, m.label())).collect();
                        g.seg("SSL", &mut c.ssl, &opts);
                    }
                }
            });
            if browse_db {
                if let Some(path) = rfd::FileDialog::new().add_filter("SQLite", &["sqlite", "sqlite3", "db", "db3"]).pick_file() {
                    c.path = path.display().to_string();
                }
            }
        }
        EditorTab::Ssh => {
            let c = &mut ed.cfg;
            group(ui, "", LABEL_W, |g| {
                g.switch("Connect over an SSH tunnel", "Reach a database that isn’t exposed directly.", &mut c.ssh.enabled);
            });
            let mut browse = false;
            super::motion::unfold(ui, egui::Id::unique("ssh-tunnel"), c.ssh.enabled, |ui| {
                group_with_note(ui, "Tunnel", "The database host and port are resolved from the SSH server.", LABEL_W, |g| {
                    g.host_port("SSH host", &mut c.ssh.host, "bastion.internal", &mut c.ssh.port);
                    g.text("SSH user", &mut c.ssh.user, "", true);
                    g.seg("Sign in with", &mut c.ssh.use_key, &[(false, "Password"), (true, "Private key")]);
                    if c.ssh.use_key {
                        browse = g.path("Private key", &mut c.ssh.key_path, "~/.ssh/id_ed25519");
                        g.secret("Key passphrase", &mut c.ssh.key_passphrase, "If the key is encrypted");
                    } else {
                        g.secret("SSH password", &mut c.ssh.password, "");
                    }
                });
            });
            if browse {
                let start = dirs::home_dir().map(|h| h.join(".ssh")).unwrap_or_default();
                if let Some(p) = rfd::FileDialog::new().set_directory(start).pick_file() {
                    c.ssh.key_path = p.display().to_string();
                }
            }
        }
        EditorTab::Advanced => {
            let c = &mut ed.cfg;
            group(ui, "Safe mode", LABEL_W, |g| {
                for m in SafeMode::ALL {
                    let full = m.label();
                    let mut parts = full.splitn(2, " — ");
                    let (name, desc) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                    let desc = format!("{}{}", desc[..1].to_uppercase(), &desc[1..]);
                    if g.radio(name, &desc, c.safe_mode == m) {
                        c.safe_mode = m;
                    }
                }
            });
            group(ui, "Behaviour", LABEL_W, |g| {
                g.switch("Read-only", "Block any statement that changes data.", &mut c.read_only);
                g.switch("Keep connection alive", "Ping every 30 s.", &mut c.keep_alive);
            });
            group(ui, "", LABEL_W, |g| {
                g.area("Startup query, runs after connecting", &mut c.startup_query, "SET search_path TO app, public;");
            });
        }
    }
}

/// Address column: the file name for SQLite, otherwise user@host:port/db without the SSH hop.
fn short_address(c: &ConnectionConfig) -> String {
    match c.driver {
        DriverKind::Sqlite => std::path::Path::new(&c.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| c.path.clone()),
        _ => c.subtitle().split("  ·  via").next().unwrap_or("").to_string(),
    }
}

/// Cuts `text` with an ellipsis so it fits `width` in `font`.
pub fn ellipsize_to(painter: &egui::Painter, text: &str, font: egui::FontId, width: f32) -> String {
    let fits = |t: &str| painter.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x <= width;
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let cand: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&cand) { lo = mid; } else { hi = mid - 1; }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// A right-pointing chevron that turns `t` of a quarter turn, to point down when open.
pub fn paint_chevron(painter: &egui::Painter, r: egui::Rect, t: f32, color: Color32) {
    let a = t * std::f32::consts::FRAC_PI_2;
    let rot = |x: f32, y: f32| { let (s, c) = a.sin_cos(); r.center() + vec2(x * c - y * s, x * s + y * c) * (r.width() / 12.0) };
    painter.add(egui::Shape::line(vec![rot(-1.5, -3.5), rot(2.0, 0.0), rot(-1.5, 3.5)], Stroke::new(1.5, color)));
}

/// "3 min ago", "yesterday", "12 Oct".
pub fn relative_time(t: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (chrono::Utc::now() - t).num_seconds().max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        86_400..=172_799 => "yesterday".into(),
        172_800..=604_799 => format!("{} days ago", secs / 86_400),
        _ => t.with_timezone(&chrono::Local).format("%-d %b").to_string(),
    }
}

/// Strata's mark, "Row S": five offset rows that trace an S, read as table rows and rock layers.
/// Drawn in the 496 × 472 design box of `assets/icon/strata.svg`, fitted inside `r`.
pub fn paint_logo(painter: &egui::Painter, r: egui::Rect) {
    let dark = p().dark;
    let c = |d: &str, l: &str| crate::theme::hex(if dark { d } else { l }).unwrap_or(Color32::GRAY);
    let (lime, sand, ochre, teal) = (c("#E7E1D6", "#2A2724"), c("#8E867B", "#A39B8F"), c("#F0AB43", "#D18A22"), c("#54D1B8", "#0E8F78"));
    let rows = [(96.0, 0.0, 400.0, lime), (0.0, 100.0, 256.0, sand), (96.0, 200.0, 304.0, ochre), (240.0, 300.0, 256.0, teal), (0.0, 400.0, 400.0, lime)];
    let u = (r.width() / 496.0).min(r.height() / 472.0);
    let origin = r.center() - egui::vec2(248.0 * u, 236.0 * u);
    for (x, y, w, color) in rows {
        let rect = egui::Rect::from_min_size(origin + egui::vec2(x * u, y * u), egui::vec2(w * u, 72.0 * u));
        painter.rect_filled(rect, 18.0 * u, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ConnectionFile {
        ConnectionFile {
            connections: (0..23).map(|i| {
                let mut c = ConnectionConfig::new(DriverKind::Postgres);
                c.name = format!("Database {i:02}");
                c.host = format!("host-{i:02}.test");
                c.database = format!("catalog_{i:02}");
                c.tag = if i < 12 { Tag::Staging } else { Tag::Production };
                c
            }).collect(),
            groups: Vec::new(),
        }
    }

    fn render(ctx: &egui::Context, home: &mut Home, conns: &ConnectionFile, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 1600.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut actions = Vec::new();
            home.filters_ui(ui, conns, true, &mut actions);
            home.connection_list(ui, conns, &[], false, &mut actions);
        });
        out.textures_delta.clear();
        fn collect(shape: &egui::Shape, texts: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => texts.push((t.galley.text().to_owned(), egui::Rect::from_min_size(t.pos, t.galley.size()))),
                egui::Shape::Vec(shapes) => for s in shapes { collect(s, texts); },
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for s in out.shapes { collect(&s.shape, &mut texts); }
        texts
    }

    fn click(ctx: &egui::Context, home: &mut Home, conns: &ConnectionFile, label: &str) -> Vec<(String, egui::Rect)> {
        let texts = render(ctx, home, conns, Vec::new());
        let pos = texts.iter().find(|(s, _)| s == label).unwrap().1.center();
        render(ctx, home, conns, vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton {
            pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE,
        }]);
        render(ctx, home, conns, vec![egui::Event::PointerButton {
            pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE,
        }]);
        render(ctx, home, conns, Vec::new())
    }

    fn names(texts: &[(String, egui::Rect)]) -> Vec<String> {
        texts.iter().filter(|(s, _)| s.starts_with("Database ")).map(|(s, _)| s.clone()).collect()
    }

    #[test]
    fn saved_connections_page_through_ten_rows_and_clamp_after_removal() {
        let ctx = egui::Context::default();
        let mut home = Home::default();
        let mut conns = fixture();
        let first = render(&ctx, &mut home, &conns, Vec::new());
        assert_eq!(names(&first), (0..10).map(|i| format!("Database {i:02}")).collect::<Vec<_>>());
        let second = click(&ctx, &mut home, &conns, "Next");
        assert_eq!(names(&second), (10..20).map(|i| format!("Database {i:02}")).collect::<Vec<_>>());
        let third = click(&ctx, &mut home, &conns, "Next");
        assert_eq!(names(&third), vec!["Database 20", "Database 21", "Database 22"]);
        let back = click(&ctx, &mut home, &conns, "Previous");
        assert_eq!(names(&back).len(), 10);
        assert_eq!(home.page, 1);
        conns.connections.truncate(2);
        assert_eq!(names(&render(&ctx, &mut home, &conns, Vec::new())), vec!["Database 00", "Database 01"]);
        assert_eq!(home.page, 0);
    }

    #[test]
    fn searching_and_environment_filters_reset_to_first_page() {
        let ctx = egui::Context::default();
        let mut home = Home::default();
        let conns = fixture();
        render(&ctx, &mut home, &conns, Vec::new());
        click(&ctx, &mut home, &conns, "Next");
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique("home-connection-search")));
        let result = render(&ctx, &mut home, &conns, vec![egui::Event::Text("host-22".into())]);
        assert_eq!(names(&result), vec!["Database 22"]);
        assert_eq!(home.page, 0);
        home.connection_search.clear();
        click(&ctx, &mut home, &conns, "Next");
        let staging = click(&ctx, &mut home, &conns, "Staging");
        assert_eq!(home.page, 0);
        assert_eq!(names(&staging), (0..10).map(|i| format!("Database {i:02}")).collect::<Vec<_>>());
        let last = click(&ctx, &mut home, &conns, "Next");
        assert_eq!(names(&last), vec!["Database 10", "Database 11"]);
        home.connection_search = "missing".into();
        let empty = render(&ctx, &mut home, &conns, Vec::new());
        assert_eq!(names(&empty), Vec::<String>::new());
        assert_eq!(home.page, 0);
        assert!(empty.iter().any(|(s, _)| s == "0–0 of 0"));
    }
}
