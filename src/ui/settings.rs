//! Settings: a centred dialog with a section list on the left and flat field groups on the right.

use super::form::{group, group_with_note};
use super::*;
use crate::model::*;
use crate::store::{hash_passcode, Store};
use crate::theme::Palette;
use egui::{vec2, RichText};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Section {
    #[default]
    General,
    Data,
    Editor,
    Security,
    Ai,
    Shortcuts,
    About,
}

const SECTIONS: [(Section, &str); 7] = [
    (Section::General, "General"),
    (Section::Data, "Data table"),
    (Section::Editor, "SQL editor"),
    (Section::Security, "Security"),
    (Section::Ai, "AI"),
    (Section::Shortcuts, "Shortcuts"),
    (Section::About, "About"),
];

#[derive(Default)]
pub struct SettingsUi {
    section: Section,
    setting_passcode: bool,
    new_passcode: String,
    confirm_passcode: String,
    api_key: Option<String>,
    message: Option<(bool, String)>,
}

pub enum SettingsEvent {
    ImportTheme(Palette),
    CheckForUpdates,
}

impl SettingsUi {
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, s: &mut Settings, store: &Store, themes: &[Palette]) -> Vec<SettingsEvent> {
        let pal = p();
        let mut events = Vec::new();
        let before = s.clone();
        let mut close = false;
        let (_, backdrop) = super::form::dialog(ctx, "settings-dialog", vec2(880.0, 680.0), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let full = ui.max_rect();
            let narrow = full.width() < 680.0;
            let nav_w = if narrow { 0.0 } else { 192.0 };
            // Section list.
            if !narrow {
                let nav = egui::Rect::from_min_size(full.min, vec2(nav_w, full.height()));
                ui.painter().vline(nav.right() - 0.5, nav.y_range(), egui::Stroke::new(1.0, line()));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(nav.shrink(12.0)).layout(egui::Layout::top_down(egui::Align::Min)));
                child.spacing_mut().item_spacing.y = 2.0;
                child.add_space(4.0);
                child.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Settings").font(semibold(16.0)).color(pal.text));
                });
                child.add_space(12.0);
                for (sec, label) in SECTIONS {
                    let current = self.section == sec;
                    let (r, resp) = child.allocate_exact_size(vec2(nav_w - 24.0, 32.0), egui::Sense::click());
                    let t = child.ctx().animate_bool_with_time(resp.id, current, super::motion::FAST as f32);
                    let h = child.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered(), super::motion::FAST as f32);
                    if t > 0.0 {
                        child.painter().rect_filled(r, egui::CornerRadius::same(6), pal.surface_hover.gamma_multiply(t));
                    }
                    let font = if current { medium(13.0) } else { egui::FontId::proportional(13.0) };
                    child.painter().text(r.left_center() + vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, label, font, mix(pal.text_dim, pal.text, t.max(h)));
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.section = sec;
                        self.message = None;
                    }
                }
            }
            // Content.
            let main = egui::Rect::from_min_max(full.min + vec2(nav_w, 0.0), full.max);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(main).layout(egui::Layout::top_down(egui::Align::Min)));
            let close_r = egui::Rect::from_min_size(egui::pos2(main.right() - 40.0, main.top() + 8.0), vec2(32.0, 32.0));
            if child.put(close_r, |ui: &mut egui::Ui| tool_button(ui, Icon::Close, "Close settings", false)).clicked() {
                close = true;
            }
            if narrow {
                child.horizontal(|ui| {
                    ui.add_space(12.0);
                    let mut sec = self.section;
                    let opts: Vec<(Section, &str)> = SECTIONS.to_vec();
                    egui::ScrollArea::horizontal().show(ui, |ui| super::form::seg_control(ui, &mut sec, &opts, 0.0));
                    self.section = sec;
                });
            }
            let t = super::motion::ease_out(super::motion::entered(child.ctx(), egui::Id::unique(("settings-sec", self.section as u8)), super::motion::SETTLE));
            child.set_opacity(t);
            egui::ScrollArea::vertical().id_salt(("settings-scroll", self.section as u8)).auto_shrink([false, false]).show(&mut child, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 12 + (4.0 * (1.0 - t)) as i8, bottom: 24 }).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let title = SECTIONS.iter().find(|x| x.0 == self.section).map(|x| x.1).unwrap_or("");
                    ui.allocate_ui_with_layout(vec2(ui.available_width() - 40.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new(title).font(semibold(16.0)).color(pal.text));
                    });
                    ui.add_space(16.0);
                    self.section_ui(ui, s, store, themes, &mut events);
                    if let Some((ok, m)) = &self.message {
                        ui.label(RichText::new(m).color(if *ok { pal.text_dim } else { pal.danger }));
                    }
                });
            });
        });
        let _ = backdrop;
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            *open = false;
        }
        if *s != before {
            let _ = store.save_settings(s);
        }
        events
    }

    fn section_ui(&mut self, ui: &mut egui::Ui, s: &mut Settings, store: &Store, themes: &[Palette], events: &mut Vec<SettingsEvent>) {
        let pal = p();
        match self.section {
            Section::General => {
                group(ui, "Appearance", None, |g| {
                    g.seg("Theme", &mut s.theme, &[(ThemeChoice::Auto, "Auto"), (ThemeChoice::Dark, "Dark"), (ThemeChoice::Light, "Light")]);
                    for dark in [true, false] {
                        let names: Vec<&Palette> = themes.iter().filter(|t| t.dark == dark).collect();
                        let current = if dark { &s.dark_theme } else { &s.light_theme };
                        let mut idx = names.iter().position(|t| &t.name == current).unwrap_or(0);
                        let labels: Vec<String> = names.iter().map(|t| short_theme_name(&t.name)).collect();
                        let opts: Vec<(usize, &str)> = labels.iter().enumerate().map(|(i, l)| (i, l.as_str())).collect();
                        if g.seg(if dark { "Dark theme" } else { "Light theme" }, &mut idx, &opts) {
                            let name = names[idx].name.clone();
                            if dark { s.dark_theme = name } else { s.light_theme = name }
                        }
                    }
                    if let Some(v) = g.step("Interface size", (s.ui_scale * 100.0).round() as f64, 10.0, 80.0, 150.0, "%") {
                        s.ui_scale = v as f32 / 100.0;
                    }
                    if g.button("Custom theme", "Load a theme from a JSON file.", "Import…") {
                        if let Some(path) = rfd::FileDialog::new().add_filter("Theme", &["json"]).pick_file() {
                            match Palette::load_file(&path) {
                                Ok(t) => {
                                    let _ = std::fs::copy(&path, store.themes_dir().join(path.file_name().unwrap_or_default()));
                                    self.message = Some((true, format!("Imported “{}”", t.name)));
                                    events.push(SettingsEvent::ImportTheme(t));
                                }
                                Err(e) => self.message = Some((false, format!("{e:#}"))),
                            }
                        }
                    }
                });
                group(ui, "Workspaces", None, |g| {
                    g.switch("Reopen workspaces at startup", "", &mut s.reopen_workspaces);
                });
            }
            Section::Data => {
                group(ui, "", None, |g| {
                    if let Some(v) = g.step("Rows per page", s.page_size as f64, 100.0, 100.0, 5000.0, "") {
                        s.page_size = v as usize;
                    }
                    g.row("NULL is shown as", |ui| {
                        ui.add_space(8.0);
                        let mut v = s.null_display.clone();
                        if right_field(ui, &mut v, "", 200.0).changed() {
                            s.null_display = v;
                        }
                    });
                    if let Some(v) = g.step("Grid font size", s.grid_font_size as f64, 1.0, 10.0, 18.0, "") {
                        s.grid_font_size = v as f32;
                    }
                    g.seg("CSV default delimiter", &mut s.csv_delimiter, &[(',', "Comma"), ('\t', "Tab"), (';', "Semicolon")]);
                });
            }
            Section::Editor => {
                group(ui, "", None, |g| {
                    if let Some(v) = g.step("Font size", s.editor_font_size as f64, 1.0, 10.0, 22.0, "") {
                        s.editor_font_size = v as f32;
                    }
                    g.switch("Automatically uppercase keywords", "", &mut s.uppercase_keywords);
                    g.switch("Automatically insert closing braces and quotes", "", &mut s.auto_close_brackets);
                    g.switch("Enable query params", "Use :name in a query to fill it in when you run.", &mut s.enable_query_params);
                    g.seg("Indent with", &mut s.tab_width, &[(2, "2 spaces"), (4, "4 spaces"), (8, "8 spaces")]);
                    if let Some(v) = g.step("Query timeout, 0 for none", s.query_timeout_secs as f64, 5.0, 0.0, 86_400.0, " s") {
                        s.query_timeout_secs = v as u64;
                    }
                    g.seg("Keep history", &mut s.history_days, &[(30, "30 days"), (365, "1 year"), (730, "2 years"), (36_500, "Forever")]);
                });
            }
            Section::Security => {
                group(ui, "Passcode", None, |g| {
                    if s.passcode_hash.is_some() {
                        if g.button("Passcode is on", "Strata asks for it at launch and after being idle.", "Turn off") {
                            s.passcode_hash = None;
                        }
                    } else {
                        if g.button("Passcode is off", "With a passcode, Strata asks for it at launch and after being idle.", if self.setting_passcode { "Cancel" } else { "Set passcode" }) {
                            self.setting_passcode = !self.setting_passcode;
                            self.new_passcode.clear();
                            self.confirm_passcode.clear();
                        }
                        if self.setting_passcode {
                            g.secret("New passcode", &mut self.new_passcode, "");
                            g.secret("Enter it again", &mut self.confirm_passcode, "");
                            if g.button("", "", "Turn on passcode") {
                                if self.new_passcode.is_empty() {
                                    self.message = Some((false, "Enter a passcode first.".into()));
                                } else if self.new_passcode != self.confirm_passcode {
                                    self.message = Some((false, "The two passcodes don’t match.".into()));
                                } else {
                                    s.passcode_hash = Some(hash_passcode(&self.new_passcode));
                                    self.new_passcode.clear();
                                    self.confirm_passcode.clear();
                                    self.setting_passcode = false;
                                    self.message = Some((true, "Passcode is on.".into()));
                                }
                            }
                        }
                    }
                    g.seg("Auto lock if away for", &mut s.auto_lock_minutes, &[(0, "Never"), (1, "1 min"), (5, "5 min"), (60, "1 hour")]);
                });
                group(ui, "Default safe mode for new connections", None, |g| {
                    for m in SafeMode::ALL {
                        let mut parts = m.label().splitn(2, " — ");
                        let (name, desc) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                        let desc = format!("{}{}", desc[..1].to_uppercase(), &desc[1..]);
                        if g.radio(name, &desc, s.default_safe_mode == m) {
                            s.default_safe_mode = m;
                        }
                    }
                });
            }
            Section::Ai => {
                let key = self.api_key.get_or_insert_with(|| store.load_secret_value(super::assistant::API_KEY_SECRET));
                let mut save_key = false;
                group_with_note(ui, "", "Your question, plus table and column names, is sent to the provider. Row data is never sent.", None, |g| {
                    g.switch("Enable the assistant in the right sidebar", "", &mut s.ai_enabled);
                    let before = s.ai_provider;
                    g.seg("Provider", &mut s.ai_provider, &[(AiProvider::Anthropic, "Anthropic"), (AiProvider::OpenAiCompatible, "Custom LLM")]);
                    if s.ai_provider != before {
                        if s.ai_provider == AiProvider::Anthropic {
                            s.ai_base_url = "https://api.anthropic.com".into();
                            s.ai_model = "claude-opus-5-5".into();
                        } else {
                            s.ai_base_url = "http://localhost:11434/v1".into();
                            s.ai_model = String::new();
                        }
                    }
                    g.row("Base URL", |ui| { ui.add_space(8.0); right_field(ui, &mut s.ai_base_url, "Only for a custom LLM", 260.0); });
                    g.row("Model", |ui| { ui.add_space(8.0); right_field(ui, &mut s.ai_model, "Model name", 260.0); });
                    g.row("API key", |ui| {
                        if super::form::mini(ui, "Save").clicked() {
                            save_key = true;
                        }
                        ui.add_sized(vec2(220.0, 39.0), line_edit(key).frame(egui::Frame::NONE).password(true).font(egui::FontId::monospace(13.0)).align(egui::Align2::RIGHT_CENTER).hint_text(faint("Stored in your system keychain")));
                    });
                });
                if save_key {
                    store.save_secret_value(super::assistant::API_KEY_SECRET, key.trim());
                    self.message = Some((true, "API key saved.".into()));
                }
                if s.ai_provider == AiProvider::Anthropic {
                    ui.label(RichText::new("ANTHROPIC_API_KEY in the environment is used when no key is saved.").size(12.0).color(pal.text_dim));
                }
            }
            Section::Shortcuts => {
                group_with_note(ui, "", "⌘ is Ctrl on Windows and Linux.", None, |g| {
                    for (k, v) in [
                        ("⌘ T", "New SQL query tab"),
                        ("⌘ ↩", "Run current statement or selection"),
                        ("⌘ ⇧ ↩", "Run all statements"),
                        ("⌘ .", "Stop the running query"),
                        ("⌘ S", "Commit changes or save query"),
                        ("⌘ R", "Reload table"),
                        ("⌘ F", "Toggle filters"),
                        ("⌘ P", "Open anything"),
                        ("⌘ K", "Switch database"),
                        ("⌘ I", "Insert row"),
                        ("⌘ D", "Duplicate row"),
                        ("⌫", "Delete selected rows"),
                        ("Space", "Quick look cell"),
                        ("⌘ 1", "Data view"),
                        ("⌘ 2", "Structure view"),
                        ("⌘ /", "Toggle line comment"),
                        ("⌘ ⇧ F", "Format SQL"),
                        ("⌘ W", "Close tab"),
                        ("⌘ ⇧ C", "Toggle console log"),
                        ("⌘ ⇧ R", "Reconnect"),
                        ("⌘ ,", "Settings"),
                        ("⌘ L", "Lock Strata (with a passcode)"),
                    ] {
                        let keys: Vec<&str> = k.split_whitespace().collect();
                        g.keys(v, &keys);
                    }
                });
            }
            Section::About => {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, egui::CornerRadius::same(10), pal.surface);
                    super::home::paint_logo(ui.painter(), r.shrink(12.0));
                    ui.vertical(|ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new(format!("Strata {}", env!("CARGO_PKG_VERSION"))).font(semibold(16.0)).color(pal.text));
                        ui.label(dim("A native database workbench written in Rust."));
                    });
                });
                ui.add_space(24.0);
                group(ui, "Updates", None, |g| {
                    g.switch("Check for updates at startup", "", &mut s.check_updates);
                    if g.button("Latest version", &format!("You have Strata {}.", env!("CARGO_PKG_VERSION")), "Check now") {
                        events.push(SettingsEvent::CheckForUpdates);
                    }
                });
                group_with_note(ui, "Data folder", "Connections, queries, history and boards live here. Passwords live in the system keychain.", None, |g| {
                    let path = store.root().display().to_string();
                    g.row("", |ui| {
                        if super::form::mini(ui, "Copy").clicked() {
                            ui.ctx().copy_text(path.clone());
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.label(RichText::new(&path).font(egui::FontId::monospace(12.0)).color(pal.text_dim));
                        });
                    });
                });
            }
        }
    }
}

/// "Strata Night" → "Basalt"-style short names for the segmented control.
fn short_theme_name(name: &str) -> String {
    match name {
        "Strata Night" => "Basalt".into(),
        "Strata Day" => "Limestone".into(),
        other => other.strip_prefix("Strata ").unwrap_or(other).to_string(),
    }
}

/// Right-aligned mono field for a settings row.
fn right_field(ui: &mut egui::Ui, value: &mut String, hint: &str, width: f32) -> egui::Response {
    ui.add_sized(vec2(width, 39.0), line_edit(value).frame(egui::Frame::NONE).font(egui::FontId::monospace(13.0)).align(egui::Align2::RIGHT_CENTER).hint_text(faint(hint)))
}
