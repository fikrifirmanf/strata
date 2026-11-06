//! Modal dialogs: safe-mode prompts, previews, import/export, backup/restore and DDL helpers.

use super::workspace::{Gated, WsAction};
use super::*;
use crate::db::*;
use crate::export::{ExportFormat, ExportOptions};
use crate::model::*;
use crate::runtime::{take, Session, Task};
use egui::{vec2, RichText};
use std::path::PathBuf;
use std::sync::Arc;

pub struct ModalInfo<'a> {
    pub session: &'a Arc<Session>,
    pub cfg: &'a ConnectionConfig,
    pub current_db: &'a str,
    pub objects: &'a [DbObject],
    pub databases: &'a [String],
    pub queries: &'a [SavedQuery],
}

pub enum ModalOutcome {
    Keep,
    Close,
    Approved(Gated),
    Actions(Vec<WsAction>),
    SaveQuery { tab: u64, name: String, shared: bool },
    Toast(ToastKind, String),
}

pub enum ExportSource {
    Table { table: TableRef, req: BrowseRequest },
    Rows { columns: Vec<String>, rows: Vec<Vec<Cell>> },
}

pub struct ExportState {
    source: ExportSource,
    opts: ExportOptions,
    all_rows: bool,
    task: Option<Task<anyhow::Result<String>>>,
}

pub struct ExportTablesState {
    tables: Vec<(DbObject, bool)>,
    format: ExportFormat,
    structure: bool,
    data: bool,
    task: Option<Task<anyhow::Result<String>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Csv,
    Json,
}

pub struct ImportState {
    kind: ImportKind,
    schema: String,
    path: Option<PathBuf>,
    delimiter: char,
    header: bool,
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
    target: String,
    create_new: bool,
    empty_as_null: bool,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DumpFormat {
    Custom,
    Plain,
}

pub struct BackupState {
    restore: bool,
    database: String,
    path: Option<PathBuf>,
    format: DumpFormat,
    schema_only: bool,
    binary: String,
    task: Option<Task<anyhow::Result<String>>>,
    output: Option<(bool, String)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReviewView {
    Rows,
    Sql,
    Impact,
}

/// The "Review & commit" sheet for a table's staged changes.
pub struct ReviewState {
    pub tab: u64,
    pub table: TableRef,
    pub items: Vec<super::table_tab::ReviewItem>,
    pub stmts: Vec<String>,
    pub view: ReviewView,
    /// (is a warning, text)
    pub impact: Vec<(bool, String)>,
    pub impact_task: Option<Task<anyhow::Result<Vec<(bool, String)>>>>,
    pub confirm: String,
    pub password: String,
    pub error: Option<String>,
}

pub enum Modal {
    CreateObject(Box<crate::db::objects::ObjectDraft>),
    Review(Box<ReviewState>),
    Confirm { sql: String, gated: Gated, production: bool },
    Password { sql: String, gated: Gated, input: String, error: Option<String> },
    Preview { title: String, stmts: Vec<String>, then: Option<Gated> },
    ConfirmAction { title: String, body: String, action: WsAction, danger: bool },
    Params { tab: u64, sql: String, values: Vec<(String, String)> },
    SaveQuery { tab: u64, name: String, shared: bool },
    QuickLook { title: String, viewer: Box<super::viewer::ValueViewer> },
    OpenAnything { query: String, selected: usize },
    SwitchDatabase { query: String, selected: usize },
    Export(Box<ExportState>),
    ExportTables(Box<ExportTablesState>),
    Import(Box<ImportState>),
    ImportSql { path: Option<PathBuf>, text: String, error: Option<String> },
    Backup(Box<BackupState>),
    NewDatabase { name: String, encoding: String },
    NewSchema { name: String },
    CreateTable { schema: String, name: String, columns: Vec<ColumnInfo>, dialect: Dialect },
    Truncate { obj: DbObject, cascade: bool, restart: bool },
    Drop { obj: DbObject, cascade: bool },
    Rename { obj: DbObject, name: String },
    Clone { obj: DbObject, name: String, with_data: bool },
    CreateUser { name: String, password: String, dialect: Dialect },
}

impl Modal {
    pub fn confirm_send(sql: String, gated: Gated, production: bool) -> Self {
        Modal::Confirm { sql, gated, production }
    }
    pub fn password(sql: String, gated: Gated) -> Self {
        Modal::Password { sql, gated, input: String::new(), error: None }
    }
    pub fn open_anything() -> Self {
        Modal::OpenAnything { query: String::new(), selected: 0 }
    }
    pub fn export_table(table: TableRef, req: BrowseRequest, delimiter: char) -> Self {
        Modal::Export(Box::new(ExportState { source: ExportSource::Table { table, req }, opts: ExportOptions { delimiter, ..Default::default() }, all_rows: true, task: None }))
    }
    pub fn export_rows(columns: Vec<String>, rows: Vec<Vec<Cell>>, delimiter: char) -> Self {
        Modal::Export(Box::new(ExportState { source: ExportSource::Rows { columns, rows }, opts: ExportOptions { delimiter, ..Default::default() }, all_rows: true, task: None }))
    }
    pub fn export_tables(tables: Vec<DbObject>) -> Self {
        Modal::ExportTables(Box::new(ExportTablesState { tables: tables.into_iter().map(|t| (t, true)).collect(), format: ExportFormat::Sql, structure: true, data: true, task: None }))
    }
    pub fn import_csv(schema: String, delimiter: char) -> Self {
        Modal::Import(Box::new(ImportState { kind: ImportKind::Csv, schema, path: None, delimiter, header: true, columns: vec![], rows: vec![], target: String::new(), create_new: false, empty_as_null: true, error: None }))
    }
    pub fn import_json(schema: String) -> Self {
        Modal::Import(Box::new(ImportState { kind: ImportKind::Json, schema, path: None, delimiter: ',', header: true, columns: vec![], rows: vec![], target: String::new(), create_new: false, empty_as_null: false, error: None }))
    }
    pub fn import_sql() -> Self {
        Modal::ImportSql { path: None, text: String::new(), error: None }
    }
    pub fn backup(db: String) -> Self {
        Modal::Backup(Box::new(BackupState { restore: false, database: db, path: None, format: DumpFormat::Custom, schema_only: false, binary: String::new(), task: None, output: None }))
    }
    pub fn restore(db: String) -> Self {
        Modal::Backup(Box::new(BackupState { restore: true, database: db, path: None, format: DumpFormat::Custom, schema_only: false, binary: String::new(), task: None, output: None }))
    }
    pub fn create_table(schema: String, dialect: Dialect) -> Self {
        let id_type = match dialect {
            Dialect::Postgres => "bigserial",
            Dialect::MySql => "bigint unsigned",
            Dialect::SqlServer => "bigint",
            Dialect::ClickHouse => "UInt64",
            _ => "integer",
        };
        let extra = if dialect == Dialect::MySql { "auto_increment" } else { "" };
        Modal::CreateTable {
            schema,
            name: String::new(),
            columns: vec![ColumnInfo { name: "id".into(), data_type: id_type.into(), primary_key: true, extra: extra.into(), ..Default::default() }],
            dialect,
        }
    }
}

pub(super) fn modal_window(ctx: &egui::Context, title: &str, width: f32, add: impl FnOnce(&mut egui::Ui)) -> bool {
    let mut open = true;
    let id = egui::Id::unique(("modal", title));
    // Dialogs rise 8 px into place over `ENTER` while egui fades them in.
    let rise = 8.0 * (1.0 - super::motion::ease_out(super::motion::entered(ctx, id, super::motion::ENTER)));
    let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, rise));
    let resp = egui::Modal::new(id).area(area).frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16))).show(ctx, |ui| {
        ui.set_width(width.min((ctx.content_rect().width() - 64.0).max(280.0)));
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(16.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Close, "Close (Esc)").clicked() {
                    open = false;
                }
            });
        });
        ui.add_space(8.0);
        add(ui);
    });
    if resp.should_close() {
        open = false;
    }
    open
}

fn sql_box(ui: &mut egui::Ui, sql: &str, height: f32) {
    let pal = p();
    egui::Frame::new().fill(pal.editor_bg).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(egui::CornerRadius::same(8)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
            let mut t = sql;
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _w: f32| ui.fonts_mut(|f| f.layout_job(super::editor::highlight(buf.as_str(), egui::FontId::monospace(12.5), None)));
            ui.add(egui::TextEdit::multiline(&mut t).code_editor().desired_width(f32::INFINITY).frame(egui::Frame::NONE).layouter(&mut layouter));
        });
    });
}

fn buttons(ui: &mut egui::Ui, ok: &str, danger: bool) -> (bool, bool) {
    let mut res = (false, false);
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            res.0 = if danger { danger_button(ui, ok).clicked() } else { primary_button(ui, ok).clicked() };
            res.1 = secondary_button(ui, "Cancel").clicked();
        });
    });
    res
}

pub fn show(m: &mut Modal, ctx: &egui::Context, info: &ModalInfo) -> ModalOutcome {
    let pal = p();
    let d = info.cfg.driver.dialect();
    let mut outcome = ModalOutcome::Keep;
    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    let open = match m {
        Modal::Review(st) => review_ui(ctx, st, info, &mut outcome),
        Modal::Confirm { sql, gated, production } => modal_window(ctx, "Send queries to the server?", 560.0, |ui| {
            if *production {
                pill(ui, "production", pal.danger);
                ui.add_space(4.0);
            }
            ui.label(dim(format!("Alert mode is on for “{}”. Review before sending:", info.cfg.display_name())));
            ui.add_space(6.0);
            sql_box(ui, sql, 260.0);
            let (ok, cancel) = buttons(ui, "Send", *production);
            if ok || enter {
                outcome = ModalOutcome::Approved(gated.clone());
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::Password { sql, gated, input, error } => modal_window(ctx, "Safe mode", 520.0, |ui| {
            ui.label(dim(format!("Enter the database password for “{}” to confirm.", info.cfg.display_name())));
            ui.add_space(6.0);
            sql_box(ui, sql, 180.0);
            ui.add_space(8.0);
            let r = ui.add(crate::ui::line_edit(input).password(true).hint_text("Database password").desired_width(f32::INFINITY));
            r.request_focus();
            if let Some(e) = error {
                ui.label(RichText::new(e.as_str()).color(pal.danger));
            }
            let (ok, cancel) = buttons(ui, "Unlock & send", false);
            if ok || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                if *input == info.cfg.password {
                    outcome = ModalOutcome::Approved(gated.clone());
                } else {
                    *error = Some("Incorrect password".into());
                    input.clear();
                }
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::Preview { title, stmts, then } => {
            let t = title.clone();
            modal_window(ctx, &t, 640.0, |ui| {
                ui.label(dim(format!("{} statement{}", stmts.len(), if stmts.len() == 1 { "" } else { "s" })));
                ui.add_space(6.0);
                sql_box(ui, &(stmts.join(";\n") + ";"), 360.0);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Copy SQL").clicked() {
                        ui.ctx().copy_text(stmts.join(";\n") + ";");
                    }
                });
                if let Some(g) = then.clone() {
                    let (ok, cancel) = buttons(ui, "Commit", false);
                    if ok {
                        outcome = ModalOutcome::Actions(vec![WsAction::Gate(g)]);
                    } else if cancel {
                        outcome = ModalOutcome::Close;
                    }
                }
            })
        }
        Modal::ConfirmAction { title, body, danger, .. } => {
            let t = title.clone();
            let mut go = false;
            let open = modal_window(ctx, &t, 460.0, |ui| {
                ui.add(egui::Label::new(dim(body.as_str())).wrap());
                let (ok, cancel) = buttons(ui, "Confirm", *danger);
                go = ok;
                if cancel {
                    outcome = ModalOutcome::Close;
                }
            });
            if go {
                if let Modal::ConfirmAction { action, .. } = std::mem::replace(m, Modal::NewSchema { name: String::new() }) {
                    return ModalOutcome::Actions(vec![action]);
                }
            }
            open
        }
        Modal::Params { tab, sql, values } => modal_window(ctx, "Query parameters", 460.0, |ui| {
            ui.label(dim("Values are quoted unless they are numbers, NULL, TRUE or FALSE."));
            ui.add_space(6.0);
            egui::Grid::new("params").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                for (i, (n, v)) in values.iter_mut().enumerate() {
                    ui.label(RichText::new(format!(":{n}")).monospace().color(pal.warning));
                    let r = ui.add(crate::ui::line_edit(v).desired_width(300.0));
                    if i == 0 && !r.has_focus() && v.is_empty() {
                        r.request_focus();
                    }
                    ui.end_row();
                }
            });
            let (ok, cancel) = buttons(ui, "Run", false);
            if ok || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let bound = super::query_tab::bind_params(sql, values, d);
                outcome = ModalOutcome::Actions(vec![WsAction::Gate(Gated::Query { tab: *tab, sql: bound })]);
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::SaveQuery { tab, name, shared } => modal_window(ctx, "Save query", 420.0, |ui| {
            let r = ui.add(crate::ui::line_edit(name).hint_text("Name").desired_width(f32::INFINITY));
            r.request_focus();
            ui.checkbox(shared, "Available in every connection");
            let (ok, cancel) = buttons(ui, "Save", false);
            if (ok || ui.input(|i| i.key_pressed(egui::Key::Enter))) && !name.trim().is_empty() {
                outcome = ModalOutcome::SaveQuery { tab: *tab, name: name.trim().to_string(), shared: *shared };
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::QuickLook { title, viewer } => {
            let t = format!("Quick look · {title}");
            modal_window(ctx, &t, 640.0, |ui| viewer.ui(ui, false, egui::Id::unique(("quick-look", t.as_str()))))
        }
        Modal::OpenAnything { query, selected } => open_anything(ctx, query, selected, info, &mut outcome),
        Modal::SwitchDatabase { query, selected } => modal_window(ctx, "Open database", 420.0, |ui| {
            let r = ui.add(crate::ui::line_edit(query).hint_text("Search for database…").desired_width(f32::INFINITY));
            r.request_focus();
            let list: Vec<&String> = info.databases.iter().filter(|db| super::workspace::fuzzy(&db.to_lowercase(), &query.to_lowercase())).collect();
            nav_keys(ui, selected, list.len());
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for (i, db) in list.iter().enumerate() {
                    let resp = ui.add_sized([ui.available_width(), 24.0], egui::Button::selectable(i == *selected, db.as_str()));
                    if resp.clicked() || (i == *selected && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        outcome = ModalOutcome::Actions(vec![WsAction::SwitchDatabase((*db).clone())]);
                    }
                }
            });
        }),
        Modal::Export(st) => export_ui(ctx, st, info, &mut outcome),
        Modal::ExportTables(st) => export_tables_ui(ctx, st, info, &mut outcome),
        Modal::Import(st) => import_ui(ctx, st, info, &mut outcome),
        Modal::ImportSql { path, text, error } => modal_window(ctx, "Import SQL dump", 560.0, |ui| {
            ui.horizontal(|ui| {
                if secondary_button(ui, "Choose file…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("SQL", &["sql", "psql", "txt"]).pick_file() {
                        match std::fs::read_to_string(&p) {
                            Ok(t) => {
                                *text = t;
                                *path = Some(p);
                                *error = None;
                            }
                            Err(e) => *error = Some(format!("The file couldn’t be opened as UTF-8 text ({e}). For pg_dump/mysqldump archives use Restore instead.")),
                        }
                    }
                }
                if let Some(p) = path {
                    ui.label(faint(p.display().to_string()));
                }
            });
            if let Some(e) = error {
                ui.label(RichText::new(e.as_str()).color(pal.danger));
            }
            if !text.is_empty() {
                let n = statements(text).len();
                ui.label(dim(format!("{n} statements · {} KB", text.len() / 1024)));
                let preview: String = text.chars().take(100_000).collect();
                sql_box(ui, &preview, 260.0);
            }
            let (ok, cancel) = buttons(ui, "Import", false);
            if ok && !text.is_empty() {
                let stmts = if matches!(d, Dialect::Postgres | Dialect::MySql | Dialect::Sqlite) { vec![text.clone()] } else { statements(text) };
                outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts, label: "SQL import finished".into(), refresh: true }]);
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::Backup(st) => backup_ui(ctx, st, info, &mut outcome),
        Modal::NewDatabase { name, encoding } => modal_window(ctx, "New database", 420.0, |ui| {
            let r = ui.add(crate::ui::line_edit(name).hint_text("Database name").desired_width(f32::INFINITY));
            r.request_focus();
            if matches!(d, Dialect::Postgres | Dialect::MySql) {
                ui.add(crate::ui::line_edit(encoding).hint_text(if d == Dialect::Postgres { "Encoding (e.g. UTF8)" } else { "Charset (e.g. utf8mb4)" }).desired_width(f32::INFINITY));
            }
            if d == Dialect::Mongo {
                ui.add(crate::ui::line_edit(encoding).hint_text("Initial collection (default: documents)").desired_width(f32::INFINITY));
                ui.label(dim("MongoDB creates a database when its first collection is created."));
            }
            let statements = if d == Dialect::Mongo {
                sqlgen::mongo_create_database(name.trim(), encoding.trim())
            } else { Ok(vec![sqlgen::create_database_sql(d, name.trim(), encoding.trim())]) };
            if let Err(error) = &statements { ui.label(RichText::new(error.to_string()).color(pal.danger)); }
            let (ok, cancel) = buttons(ui, "Create", false);
            if ok && !name.trim().is_empty() {
                if let Ok(stmts) = statements {
                    outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts, label: format!("Created {}", name.trim()), refresh: true }]);
                }
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::NewSchema { name } => modal_window(ctx, "New schema", 400.0, |ui| {
            ui.add(crate::ui::line_edit(name).hint_text("Schema name").desired_width(f32::INFINITY)).request_focus();
            let (ok, cancel) = buttons(ui, "Create", false);
            if ok && !name.trim().is_empty() {
                outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts: vec![format!("CREATE SCHEMA {}", sqlgen::quote_ident(d, name.trim()))], label: "Schema created".into(), refresh: true }]);
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::CreateObject(draft) => modal_window(ctx, &super::object_editor::title(draft), 620.0, |ui| {
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 180.0).clamp(160.0, 520.0)).show(ui, |ui| super::object_editor::fields(ui, draft, d, info.objects));
            let sql = draft.sql(d);
            let mut review = false;
            ui.add_enabled_ui(sql.is_ok() && !info.cfg.read_only, |ui| { review = ui.button("Review SQL").clicked(); });
            if review {
                if let Ok(sql) = sql {
                    let title = super::object_editor::title(draft);
                    outcome = ModalOutcome::Actions(vec![WsAction::ShowModal(Box::new(Modal::Preview { title: title.clone(), stmts: vec![sql.clone()], then: Some(Gated::Script { stmts: vec![sql], label: format!("{title} completed"), refresh: true }) }))]);
                }
            }
            if ui.button("Cancel").clicked() { outcome = ModalOutcome::Close; }
        }),
        Modal::CreateTable { schema, name, columns, dialect } => create_table_ui(ctx, schema, name, columns, *dialect, &mut outcome),
        Modal::Truncate { obj, cascade, restart } => {
            let t = format!("Truncate {}?", obj.name);
            modal_window(ctx, &t, 440.0, |ui| {
                ui.label(dim("Every row is deleted. This can't be undone."));
                if d == Dialect::Postgres {
                    ui.checkbox(cascade, "Cascade to referencing tables");
                    ui.checkbox(restart, "Restart identity");
                }
                let (ok, cancel) = buttons(ui, "Truncate", true);
                if ok {
                    let t = super::table_tab::table_ref(obj);
                    let stmt = if d == Dialect::Mongo { format!("db.getCollection({:?}).deleteMany({{}})", obj.name) } else { sqlgen::truncate_sql(d, &t, *cascade, *restart) };
                    outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts: vec![stmt], label: format!("Truncated {}", obj.name), refresh: false }]);
                } else if cancel {
                    outcome = ModalOutcome::Close;
                }
            })
        }
        Modal::Drop { obj, cascade } => {
            let t = format!("Drop {}?", obj.name);
            modal_window(ctx, &t, 440.0, |ui| {
                ui.label(dim(format!("The {} and its data are removed permanently.", obj.kind.section().trim_end_matches('s').to_lowercase())));
                if d == Dialect::Postgres {
                    ui.checkbox(cascade, "Cascade (drop dependent objects)");
                }
                let (ok, cancel) = buttons(ui, "Drop", true);
                if ok {
                    let stmt = if d == Dialect::Mongo { format!("db.getCollection({:?}).drop()", obj.name) } else { sqlgen::drop_sql(d, obj, *cascade) };
                    outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts: vec![stmt], label: format!("Dropped {}", obj.name), refresh: true }]);
                } else if cancel {
                    outcome = ModalOutcome::Close;
                }
            })
        }
        Modal::Rename { obj, name } => modal_window(ctx, "Rename table", 400.0, |ui| {
            ui.add(crate::ui::line_edit(name).desired_width(f32::INFINITY)).request_focus();
            let (ok, cancel) = buttons(ui, "Rename", false);
            if ok && !name.trim().is_empty() && name.trim() != obj.name {
                let stmt = sqlgen::rename_sql(d, &super::table_tab::table_ref(obj), name.trim());
                outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts: vec![stmt], label: "Renamed".into(), refresh: true }]);
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::Clone { obj, name, with_data } => modal_window(ctx, "Clone table", 420.0, |ui| {
            ui.add(crate::ui::line_edit(name).hint_text("New table name").desired_width(f32::INFINITY));
            ui.checkbox(with_data, "Copy data");
            let (ok, cancel) = buttons(ui, "Clone", false);
            if ok && !name.trim().is_empty() {
                let stmts = sqlgen::clone_table_sql(d, &super::table_tab::table_ref(obj), name.trim(), *with_data);
                outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts, label: format!("Created {}", name.trim()), refresh: true }]);
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
        Modal::CreateUser { name, password, dialect } => modal_window(ctx, "New user", 420.0, |ui| {
            ui.add(crate::ui::line_edit(name).hint_text("User name").desired_width(f32::INFINITY));
            ui.add(crate::ui::line_edit(password).hint_text("Password").password(true).desired_width(f32::INFINITY));
            let (ok, cancel) = buttons(ui, "Create", false);
            if ok && !name.trim().is_empty() {
                let stmt = create_user_sql(*dialect, name.trim(), password);
                outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts: vec![stmt], label: format!("Created user {}", name.trim()), refresh: false }]);
            } else if cancel {
                outcome = ModalOutcome::Close;
            }
        }),
    };
    if !open && matches!(outcome, ModalOutcome::Keep) {
        return ModalOutcome::Close;
    }
    outcome
}

fn review_ui(ctx: &egui::Context, st: &mut ReviewState, info: &ModalInfo, outcome: &mut ModalOutcome) -> bool {
    let pal = p();
    if let Some(r) = take(&mut st.impact_task) {
        match r {
            Ok(v) => st.impact.extend(v),
            Err(e) => st.impact.push((true, format!("Couldn't check cascades: {e:#}"))),
        }
    }
    let production = info.cfg.tag == Tag::Production;
    let word = if info.current_db.is_empty() { info.cfg.display_name() } else { info.current_db.to_string() };
    let needs_password = matches!(info.cfg.safe_mode, SafeMode::PasswordAll | SafeMode::PasswordWrites);
    let n = st.items.len().max(st.stmts.len());
    let mut open = true;
    let resp = egui::Modal::new(egui::Id::unique(("review", st.tab))).frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(0))).show(ctx, |ui| {
        ui.set_width(780.0);
        egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 22, bottom: 14 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
                let tone = if production { pal.danger } else { pal.accent };
                ui.painter().rect_filled(r, egui::CornerRadius::same(10), egui::Color32::from_rgba_unmultiplied(tone.r(), tone.g(), tone.b(), 40));
                paint_icon(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(20.0, 20.0)), Icon::Shield, readable_on_tint(tone));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(title(format!("Review {n} change{} to {}", if n == 1 { "" } else { "s" }, st.table.name), 19.0));
                    let env = if info.cfg.tag == Tag::None { String::new() } else { format!(" on {}", info.cfg.tag.label()) };
                    ui.label(dim(format!("{}{env} · runs as one transaction · nothing is sent until you commit", info.cfg.display_name())));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if icon_button(ui, Icon::Close, "Close (Esc)").clicked() {
                        open = false;
                    }
                });
            });
            ui.add_space(12.0);
            let warnings = st.impact.iter().filter(|(w, _)| *w).count();
            let impact_label = if st.impact_task.is_some() { "Impact …".to_string() } else if warnings > 0 { format!("Impact · {warnings}") } else { "Impact".to_string() };
            let mut v = st.view;
            segmented(ui, &mut v, &[(ReviewView::Rows, "Rows"), (ReviewView::Sql, "SQL"), (ReviewView::Impact, impact_label.as_str())]);
            st.view = v;
        });
        egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 0, bottom: 16 }).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| match st.view {
                ReviewView::Rows => {
                    if st.items.is_empty() {
                        ui.label(dim("Only structure changes are staged. See the SQL tab."));
                    }
                    for it in &st.items {
                        let (label, tone) = match it.kind {
                            super::table_tab::ReviewKind::Edit => ("Edit", pal.accent),
                            super::table_tab::ReviewKind::Add => ("Add", pal.success),
                            super::table_tab::ReviewKind::Remove => ("Remove", pal.danger),
                        };
                        egui::Frame::new()
                            .fill(if pal.dark { egui::Color32::from_white_alpha(6) } else { egui::Color32::from_black_alpha(6) })
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(tone.r(), tone.g(), tone.b(), 80)))
                            .corner_radius(egui::CornerRadius::same(10))
                            .inner_margin(egui::Margin::symmetric(14, 12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    env_chip(ui, label, tone, false);
                                    ui.label(RichText::new(&it.title).font(egui::FontId::monospace(12.5)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(faint(&it.note).size(12.0)));
                                });
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 16.0;
                                    for (k, old, new) in &it.fields {
                                        let mut job = egui::text::LayoutJob::default();
                                        let mono = egui::FontId::monospace(12.0);
                                        let fmt = |c: egui::Color32| egui::TextFormat { font_id: mono.clone(), color: c, ..Default::default() };
                                        job.append(&format!("{k} "), 0.0, fmt(pal.text_faint));
                                        if it.kind == super::table_tab::ReviewKind::Edit {
                                            let mut f = fmt(pal.text_dim);
                                            f.strikethrough = egui::Stroke::new(1.0, pal.text_dim);
                                            job.append(old.as_deref().unwrap_or("NULL"), 0.0, f);
                                            job.append(" → ", 0.0, fmt(pal.text_faint));
                                        }
                                        job.append(new.as_deref().unwrap_or("NULL"), 0.0, fmt(pal.text));
                                        ui.label(job);
                                    }
                                });
                            });
                        ui.add_space(8.0);
                    }
                }
                ReviewView::Sql => {
                    let (begin, commit, _) = sqlgen::transaction_words(info.cfg.driver.dialect());
                    let mut text = String::new();
                    if let Some(b) = begin {
                        text.push_str(&format!("{b};\n"));
                    }
                    for s in &st.stmts {
                        text.push_str(s.trim_end_matches(';'));
                        text.push_str(";\n");
                    }
                    if let Some(c) = commit {
                        text.push_str(&format!("{c};"));
                    }
                    ui.horizontal(|ui| {
                        ui.label(dim("Generated SQL"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Copy").clicked() {
                                ui.ctx().copy_text(text.clone());
                            }
                        });
                    });
                    sql_box(ui, &text, 320.0);
                }
                ReviewView::Impact => {
                    if st.impact_task.is_some() {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().color(pal.accent));
                            ui.label(dim("Checking what else these changes touch…"));
                        });
                    }
                    if st.impact.is_empty() && st.impact_task.is_none() {
                        ui.label(dim("Nothing outside these rows is affected: no cascades were found."));
                    }
                    for (warn, text) in &st.impact {
                        let tone = if *warn { pal.danger } else { pal.text_faint };
                        egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(tone.r(), tone.g(), tone.b(), 28)).corner_radius(egui::CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                                paint_icon(ui.painter(), r, if *warn { Icon::Bolt } else { Icon::Link }, readable_on_tint(tone));
                                ui.add(egui::Label::new(RichText::new(text).size(12.5).color(if *warn { readable_on_tint(tone) } else { pal.text_dim })).wrap());
                            });
                        });
                        ui.add_space(6.0);
                    }
                }
            });
        });
        // Footer
        egui::Frame::new().fill(if pal.dark { egui::Color32::from_black_alpha(50) } else { egui::Color32::from_black_alpha(8) }).inner_margin(egui::Margin::symmetric(24, 16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let typed_ok = !production || st.confirm.trim() == word;
            let pass_ok = !needs_password || st.password == info.cfg.password;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if production {
                        ui.label(RichText::new(format!("Type {word} to confirm a production write")).size(12.0).color(pal.text_dim));
                        ui.add(crate::ui::line_edit(&mut st.confirm).font(egui::FontId::monospace(13.0)).desired_width(260.0).hint_text(word.as_str()));
                    }
                    if needs_password {
                        ui.label(RichText::new("Database password (safe mode)").size(12.0).color(pal.text_dim));
                        ui.add(crate::ui::line_edit(&mut st.password).password(true).desired_width(260.0));
                    }
                    if let Some(e) = &st.error {
                        ui.label(RichText::new(e).size(12.0).color(pal.danger));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                    let label = format!("Commit {n} change{}", if n == 1 { "" } else { "s" });
                    let ok = typed_ok && pass_ok;
                    let fill = if ok { if production { pal.danger } else { pal.accent } } else { pal.surface_hover };
                    let text_color = if ok { if production { egui::Color32::WHITE } else { pal.on_accent } } else { pal.text_faint };
                    let b = egui::Button::new(RichText::new(label).font(semibold(13.0)).color(text_color)).fill(fill).corner_radius(egui::CornerRadius::same(6)).min_size(vec2(0.0, 38.0));
                    if ui.add_enabled(ok, b).clicked() {
                        *outcome = ModalOutcome::Approved(Gated::Changes(st.tab));
                    }
                    if secondary_button(ui, "  Back to editing  ").clicked() {
                        open = false;
                    }
                    if needs_password && !st.password.is_empty() && !pass_ok {
                        st.error = Some("Incorrect password".into());
                    } else {
                        st.error = None;
                    }
                });
            });
        });
    });
    if resp.should_close() {
        open = false;
    }
    open
}

pub fn create_user_sql(d: Dialect, name: &str, password: &str) -> String {
    match d {
        Dialect::Postgres => format!("CREATE ROLE {} LOGIN PASSWORD {}", sqlgen::quote_ident(d, name), sqlgen::lit(d, password)),
        Dialect::MySql => format!("CREATE USER {}@'%' IDENTIFIED BY {}", sqlgen::lit(d, name), sqlgen::lit(d, password)),
        Dialect::SqlServer => format!("CREATE LOGIN {} WITH PASSWORD = {}", sqlgen::quote_ident(d, name), sqlgen::lit(d, password)),
        Dialect::ClickHouse => format!("CREATE USER {} IDENTIFIED BY {}", sqlgen::quote_ident(d, name), sqlgen::lit(d, password)),
        Dialect::Mongo => format!("db.runCommand({{\"createUser\": {:?}, \"pwd\": {:?}, \"roles\": []}})", name, password),
        _ => String::new(),
    }
}

pub fn drop_user_sql(d: Dialect, name: &str, host: Option<&str>) -> String {
    match d {
        Dialect::MySql => format!("DROP USER {}@{}", sqlgen::lit(d, name), sqlgen::lit(d, host.unwrap_or("%"))),
        Dialect::SqlServer => format!("DROP LOGIN {}", sqlgen::quote_ident(d, name)),
        Dialect::Mongo => format!("db.runCommand({{\"dropUser\": {:?}}})", name),
        _ => format!("DROP ROLE {}", sqlgen::quote_ident(d, name)),
    }
}

fn nav_keys(ui: &mut egui::Ui, selected: &mut usize, len: usize) {
    if len == 0 {
        *selected = 0;
        return;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
        *selected = (*selected + 1) % len;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
        *selected = (*selected + len - 1) % len;
    }
    *selected = (*selected).min(len - 1);
}

fn open_anything(ctx: &egui::Context, query: &mut String, selected: &mut usize, info: &ModalInfo, outcome: &mut ModalOutcome) -> bool {
    let pal = p();
    enum Item {
        Obj(DbObject),
        Query(SavedQuery),
        Action(Icon, WsAction),
    }
    modal_window(ctx, "Open anything", 560.0, |ui| {
        let r = ui.add(crate::ui::line_edit(query).hint_text("Tables, views, functions, saved queries…").desired_width(f32::INFINITY));
        r.request_focus();
        let q = query.to_lowercase();
        let mut items: Vec<(String, Item)> = Vec::new();
        for o in info.objects {
            if q.is_empty() || super::workspace::fuzzy(&o.name.to_lowercase(), &q) {
                items.push((o.name.clone(), Item::Obj(o.clone())));
            }
        }
        for sq in info.queries.iter().filter(|s| s.connection.map(|c| c == info.cfg.id).unwrap_or(true)) {
            if q.is_empty() || super::workspace::fuzzy(&sq.name.to_lowercase(), &q) {
                items.push((sq.name.clone(), Item::Query(sq.clone())));
            }
        }
        let actions: [(&'static str, Icon, WsAction); 3] = [
            ("New query", Icon::Code, WsAction::OpenQuery { sql: String::new(), title: None, run: false }),
            ("Process list", Icon::Activity, WsAction::OpenAdmin(super::workspace::AdminKind::Processes)),
            ("Relationship graph", Icon::Graph, WsAction::OpenDiagram),
        ];
        for (label, icon, a) in actions {
            if q.is_empty() || super::workspace::fuzzy(&label.to_lowercase(), &q) {
                items.push((label.to_string(), Item::Action(icon, a)));
            }
        }
        // Exact-prefix matches first.
        items.sort_by_key(|(name, _)| !name.to_lowercase().starts_with(&q));
        items.truncate(60);
        nav_keys(ui, selected, items.len());
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let mut chosen: Option<usize> = if enter && !items.is_empty() { Some(*selected) } else { None };
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, (name, item)) in items.iter().enumerate() {
                let (icon, kind) = match item {
                    Item::Obj(o) => (match o.kind { ObjectKind::View | ObjectKind::MaterializedView => Icon::View, ObjectKind::Function | ObjectKind::Procedure => Icon::Function, _ => Icon::Table }, o.kind.section().trim_end_matches('s').to_lowercase()),
                    Item::Query(_) => (Icon::Star, "saved query".to_string()),
                    Item::Action(ic, _) => (*ic, "action".to_string()),
                };
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), egui::Sense::click());
                if i == *selected || resp.hovered() {
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), if i == *selected { mix(pal.surface, pal.accent, 0.2) } else { pal.surface_hover });
                }
                if i == *selected {
                    resp.scroll_to_me(None);
                }
                paint_icon(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, rect.center().y), vec2(15.0, 15.0)), icon, pal.text_dim);
                ui.painter().text(egui::pos2(rect.left() + 34.0, rect.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), pal.text);
                ui.painter().text(egui::pos2(rect.right() - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, kind, egui::FontId::proportional(11.0), pal.text_faint);
                if resp.clicked() {
                    chosen = Some(i);
                }
            }
        });
        if let Some(i) = chosen {
            if let Some((_, item)) = items.into_iter().nth(i) {
                *outcome = ModalOutcome::Actions(vec![match item {
                    Item::Obj(o) => WsAction::OpenObject { obj: o, structure: false, new_tab: true },
                    Item::Query(sq) => WsAction::OpenQuery { sql: sq.sql, title: Some(sq.name), run: false },
                    Item::Action(_, a) => a,
                }]);
            }
        }
    })
}

fn export_options_ui(ui: &mut egui::Ui, opts: &mut ExportOptions, sql_target: bool) {
    ui.horizontal(|ui| {
        for f in ExportFormat::ALL {
            if f == ExportFormat::Sql && !sql_target {
                continue;
            }
            if ui.add(egui::Button::selectable(opts.format == f, f.label())).clicked() {
                opts.format = f;
            }
        }
    });
    ui.add_space(6.0);
    match opts.format {
        ExportFormat::Csv => {
            ui.horizontal(|ui| {
                ui.label("Delimiter");
                for (c, l) in [(',', ","), (';', ";"), ('\t', "Tab"), ('|', "|")] {
                    if ui.add(egui::Button::selectable(opts.delimiter == c, l)).clicked() {
                        opts.delimiter = c;
                    }
                }
            });
            ui.checkbox(&mut opts.header, "Put field names in the first row");
            ui.checkbox(&mut opts.null_as_empty, "Convert NULL to EMPTY");
            ui.checkbox(&mut opts.newline_to_space, "Convert line break to space");
        }
        ExportFormat::Sql => {
            ui.checkbox(&mut opts.include_create, "Include CREATE TABLE");
            ui.horizontal(|ui| {
                ui.label("Rows per INSERT");
                ui.add(egui::DragValue::new(&mut opts.sql_batch).range(1..=10_000));
            });
        }
        _ => {}
    }
}

fn export_ui(ctx: &egui::Context, st: &mut ExportState, info: &ModalInfo, outcome: &mut ModalOutcome) -> bool {
    if let Some(r) = take(&mut st.task) {
        *outcome = match r {
            Ok(msg) => ModalOutcome::Toast(ToastKind::Success, msg),
            Err(e) => ModalOutcome::Toast(ToastKind::Error, format!("Export failed: {e:#}")),
        };
        return false;
    }
    let title = match &st.source {
        ExportSource::Table { table, .. } => format!("Export {}", table.name),
        ExportSource::Rows { rows, .. } => format!("Export {} rows", rows.len()),
    };
    modal_window(ctx, &title, 480.0, |ui| {
        let is_table = matches!(st.source, ExportSource::Table { .. });
        export_options_ui(ui, &mut st.opts, is_table);
        if is_table {
            ui.add_space(4.0);
            ui.checkbox(&mut st.all_rows, "All rows matching the current filters (otherwise the current page)");
        }
        if st.task.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(dim("Exporting…"));
            });
            return;
        }
        let (ok, cancel) = buttons(ui, "Export…", false);
        if cancel {
            *outcome = ModalOutcome::Close;
        }
        if !ok {
            return;
        }
        let base = match &st.source {
            ExportSource::Table { table, .. } => table.name.clone(),
            ExportSource::Rows { .. } => "result".into(),
        };
        let Some(path) = rfd::FileDialog::new().set_file_name(format!("{base}.{}", st.opts.format.extension())).save_file() else { return };
        let opts = st.opts.clone();
        match &st.source {
            ExportSource::Rows { columns, rows } => {
                let (columns, rows) = (columns.clone(), rows.clone());
                st.task = Some(Task::spawn(ui.ctx(), async move {
                    tokio::task::spawn_blocking(move || {
                        crate::export::write_file(&path, &columns, &rows, &opts, None)?;
                        Ok(format!("Exported {} rows to {}", rows.len(), path.display()))
                    })
                    .await?
                }));
            }
            ExportSource::Table { table, req } => {
                let mut req = req.clone();
                if st.all_rows {
                    req.offset = 0;
                    req.limit = 10_000_000;
                }
                let table = table.clone();
                let kind = info.cfg.driver;
                st.task = Some(info.session.spawn(ui.ctx(), move |d| async move {
                    let mut d = d.lock().await;
                    let cols = d.columns(&table).await.unwrap_or_default();
                    let data = d.browse(&table, &req, &cols).await?;
                    let ddl = if opts.include_create {
                        d.ddl(&DbObject { parent: None, arguments: None, schema: table.schema.clone(), name: table.name.clone(), kind: ObjectKind::Table }).await.ok()
                    } else {
                        None
                    };
                    drop(d);
                    let n = data.rows.len();
                    tokio::task::spawn_blocking(move || crate::export::write_file(&path, &data.columns, &data.rows, &opts, Some((kind.dialect(), &table, ddl.as_deref()))).map(|_| path)).await??;
                    Ok(format!("Exported {n} rows"))
                }));
            }
        }
    })
}

fn export_tables_ui(ctx: &egui::Context, st: &mut ExportTablesState, info: &ModalInfo, outcome: &mut ModalOutcome) -> bool {
    if let Some(r) = take(&mut st.task) {
        *outcome = match r {
            Ok(msg) => ModalOutcome::Toast(ToastKind::Success, msg),
            Err(e) => ModalOutcome::Toast(ToastKind::Error, format!("Export failed: {e:#}")),
        };
        return false;
    }
    modal_window(ctx, "Export tables", 520.0, |ui| {
        ui.horizontal(|ui| {
            for f in [ExportFormat::Sql, ExportFormat::Csv, ExportFormat::Json] {
                if ui.add(egui::Button::selectable(st.format == f, f.label())).clicked() {
                    st.format = f;
                }
            }
        });
        if st.format == ExportFormat::Sql {
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.structure, "Include table structure");
                ui.checkbox(&mut st.data, "Include table content");
            });
        } else {
            ui.label(faint("One file per table is written into the folder you choose."));
        }
        ui.horizontal(|ui| {
            if ui.small_button("Select all").clicked() {
                st.tables.iter_mut().for_each(|t| t.1 = true);
            }
            if ui.small_button("Select none").clicked() {
                st.tables.iter_mut().for_each(|t| t.1 = false);
            }
        });
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (t, on) in st.tables.iter_mut() {
                ui.checkbox(on, &t.name);
            }
        });
        if st.task.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(dim("Exporting…"));
            });
            return;
        }
        let (ok, cancel) = buttons(ui, "Export…", false);
        if cancel {
            *outcome = ModalOutcome::Close;
        }
        if !ok {
            return;
        }
        let chosen: Vec<DbObject> = st.tables.iter().filter(|t| t.1).map(|t| t.0.clone()).collect();
        if chosen.is_empty() {
            return;
        }
        let format = st.format;
        let (structure, data) = (st.structure, st.data);
        let dialect = info.cfg.driver.dialect();
        let dest = if format == ExportFormat::Sql {
            rfd::FileDialog::new().set_file_name(format!("{}.sql", info.cfg.display_name().replace(' ', "_"))).save_file()
        } else {
            rfd::FileDialog::new().pick_folder()
        };
        let Some(dest) = dest else { return };
        st.task = Some(info.session.spawn(ui.ctx(), move |d| async move {
            let mut d = d.lock().await;
            let mut sql = String::new();
            for obj in &chosen {
                let t = super::table_tab::table_ref(obj);
                if format == ExportFormat::Sql && structure {
                    sql.push_str(&format!("-- {}\n{}\n\n", obj.name, d.ddl(obj).await.unwrap_or_default()));
                }
                if format != ExportFormat::Sql || data {
                    let cols = d.columns(&t).await.unwrap_or_default();
                    let rows = d.browse(&t, &BrowseRequest { limit: 10_000_000, ..Default::default() }, &cols).await?;
                    match format {
                        ExportFormat::Sql => {
                            for chunk in rows.rows.chunks(500) {
                                sql.push_str(&sqlgen::insert_statement(dialect, &t, &rows.columns, chunk));
                                sql.push('\n');
                            }
                            sql.push('\n');
                        }
                        f => {
                            let path = dest.join(format!("{}.{}", obj.name, f.extension()));
                            crate::export::write_file(&path, &rows.columns, &rows.rows, &ExportOptions { format: f, ..Default::default() }, None)?;
                        }
                    }
                }
            }
            if format == ExportFormat::Sql {
                std::fs::write(&dest, sql)?;
            }
            Ok(format!("Exported {} tables to {}", chosen.len(), dest.display()))
        }));
    })
}

fn import_ui(ctx: &egui::Context, st: &mut ImportState, info: &ModalInfo, outcome: &mut ModalOutcome) -> bool {
    let pal = p();
    let d = info.cfg.driver.dialect();
    let title = if st.kind == ImportKind::Csv { "Import CSV" } else { "Import JSON" };
    modal_window(ctx, title, 640.0, |ui| {
        let mut reload = false;
        ui.horizontal(|ui| {
            if secondary_button(ui, "Choose file…").clicked() {
                let ext: &[&str] = if st.kind == ImportKind::Csv { &["csv", "tsv", "txt"] } else { &["json"] };
                if let Some(p) = rfd::FileDialog::new().add_filter(title, ext).pick_file() {
                    if st.target.is_empty() {
                        st.target = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        st.create_new = !info.objects.iter().any(|o| o.name == st.target);
                    }
                    if p.extension().map(|e| e == "tsv").unwrap_or(false) {
                        st.delimiter = '\t';
                    }
                    st.path = Some(p);
                    reload = true;
                }
            }
            if let Some(p) = &st.path {
                ui.label(faint(p.display().to_string()));
            }
        });
        if st.kind == ImportKind::Csv {
            ui.horizontal(|ui| {
                ui.label("Delimiter");
                for (c, l) in [(',', ","), (';', ";"), ('\t', "Tab"), ('|', "|")] {
                    if ui.add(egui::Button::selectable(st.delimiter == c, l)).clicked() {
                        st.delimiter = c;
                        reload = true;
                    }
                }
                if ui.checkbox(&mut st.header, "First line is header").changed() {
                    reload = true;
                }
            });
        }
        if reload {
            if let Some(path) = &st.path {
                let parsed = match st.kind {
                    ImportKind::Csv => crate::export::read_csv(path, st.delimiter as u8, st.header).map(|(h, r)| (h, r.into_iter().map(|row| row.into_iter().map(Some).collect()).collect())),
                    ImportKind::Json => crate::export::read_json(path),
                };
                match parsed {
                    Ok((h, r)) => {
                        st.columns = h;
                        st.rows = r;
                        st.error = None;
                    }
                    Err(e) => st.error = Some(format!("{e:#}")),
                }
            }
        }
        if let Some(e) = &st.error {
            ui.label(RichText::new(e).color(pal.danger));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Into table");
            egui::ComboBox::from_id_salt("import-target").width(220.0).selected_text(if st.target.is_empty() { "Choose…".to_string() } else { st.target.clone() }).show_ui(ui, |ui| {
                for o in info.objects.iter().filter(|o| o.kind == ObjectKind::Table || o.kind == ObjectKind::Collection) {
                    if ui.selectable_label(st.target == o.name && !st.create_new, &o.name).clicked() {
                        st.target = o.name.clone();
                        st.create_new = false;
                    }
                }
            });
            ui.checkbox(&mut st.create_new, "Create new table");
            if st.create_new {
                ui.add(crate::ui::line_edit(&mut st.target).desired_width(160.0).hint_text("table name"));
            }
        });
        if st.kind == ImportKind::Csv {
            ui.checkbox(&mut st.empty_as_null, "Treat empty values as NULL");
        }
        if st.create_new {
            ui.label(faint("A new table is created with all text columns, named after the file's header."));
        } else {
            ui.label(faint("Columns are matched by name."));
        }
        if !st.rows.is_empty() {
            ui.add_space(6.0);
            ui.label(dim(format!("{} rows · {} columns · preview of the first 100", st.rows.len(), st.columns.len())));
            let preview_rows: Vec<Vec<Cell>> = st.rows.iter().take(100).cloned().collect();
            ui.allocate_ui(vec2(ui.available_width(), 200.0), |ui| {
                let mut meta: Vec<grid::ColMeta> = st.columns.iter().map(|c| grid::ColMeta { name: c.clone(), ..Default::default() }).collect();
                grid::profile_all(&mut meta, &preview_rows);
                let src = grid::ResultSource { meta, rows: &preview_rows, sort: &[] };
                let mut gs = grid::GridState::default();
                let opts = grid::GridOptions { id: egui::Id::unique("import-preview"), editable: false, null_text: "NULL", font_size: 12.0, row_height: None };
                grid::show(ui, &src, &mut gs, &opts);
            });
        }
        let (ok, cancel) = buttons(ui, "Import", false);
        if cancel {
            *outcome = ModalOutcome::Close;
        }
        if ok && !st.rows.is_empty() && !st.target.trim().is_empty() {
            let t = TableRef { schema: st.schema.clone(), name: st.target.trim().to_string() };
            let rows: Vec<Vec<Cell>> = st
                .rows
                .iter()
                .map(|r| {
                    let mut v: Vec<Cell> = r.iter().map(|c| c.clone().filter(|s| !(st.empty_as_null && s.is_empty()))).collect();
                    v.resize(st.columns.len(), None);
                    v
                })
                .collect();
            let mut stmts = Vec::new();
            if d == Dialect::Mongo {
                for chunk in rows.chunks(500) {
                    let docs: Vec<serde_json::Value> = chunk
                        .iter()
                        .map(|r| serde_json::Value::Object(st.columns.iter().cloned().zip(r.iter().map(|v| v.clone().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null))).collect()))
                        .collect();
                    stmts.push(format!("db.getCollection({:?}).insertMany({})", t.name, serde_json::Value::Array(docs)));
                }
            } else {
                if st.create_new {
                    let cols: Vec<ColumnInfo> = st.columns.iter().map(|c| ColumnInfo { name: c.clone(), data_type: if d == Dialect::ClickHouse { "Nullable(String)".into() } else { "text".into() }, nullable: true, ..Default::default() }).collect();
                    let mut ddl = sqlgen::create_table_ddl(d, &t, &cols, &[]);
                    if d == Dialect::ClickHouse {
                        ddl = ddl.trim_end_matches(';').to_string() + " ENGINE = MergeTree ORDER BY tuple();";
                    }
                    stmts.push(ddl);
                }
                for chunk in rows.chunks(500) {
                    stmts.push(sqlgen::insert_statement(d, &t, &st.columns, chunk));
                }
            }
            *outcome = ModalOutcome::Actions(vec![WsAction::RunScript { stmts, label: format!("Imported {} rows into {}", rows.len(), t.name), refresh: st.create_new }]);
        }
    })
}

fn create_table_ui(ctx: &egui::Context, schema: &str, name: &mut String, columns: &mut Vec<ColumnInfo>, d: Dialect, outcome: &mut ModalOutcome) -> bool {
    let title = if d == Dialect::Mongo { "New collection" } else { "New table" };
    modal_window(ctx, title, 640.0, |ui| {
        ui.add(crate::ui::line_edit(name).hint_text("Name").desired_width(f32::INFINITY));
        if d != Dialect::Mongo {
            ui.add_space(8.0);
            let mut remove = None;
            egui::Grid::new("new-table-cols").num_columns(6).spacing([8.0, 6.0]).show(ui, |ui| {
                for h in ["Name", "Type", "Nullable", "Default", "PK", ""] {
                    ui.label(faint(h).size(11.0));
                }
                ui.end_row();
                for (i, c) in columns.iter_mut().enumerate() {
                    ui.add(crate::ui::line_edit(&mut c.name).desired_width(150.0));
                    ui.add(crate::ui::line_edit(&mut c.data_type).desired_width(140.0).font(egui::TextStyle::Monospace));
                    ui.checkbox(&mut c.nullable, "");
                    let mut def = c.default.clone().unwrap_or_default();
                    if ui.add(crate::ui::line_edit(&mut def).desired_width(110.0)).changed() {
                        c.default = (!def.is_empty()).then_some(def);
                    }
                    ui.checkbox(&mut c.primary_key, "");
                    if icon_button(ui, Icon::Trash, "Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                columns.remove(i);
            }
            if ui.button("+ Column").clicked() {
                columns.push(ColumnInfo { name: String::new(), data_type: if d == Dialect::MySql { "varchar(255)".into() } else { "text".into() }, nullable: true, ..Default::default() });
            }
        }
        let t = TableRef { schema: schema.to_string(), name: name.trim().to_string() };
        let ddl = if d == Dialect::Mongo {
            format!("db.runCommand({{\"create\": {:?}}})", name.trim())
        } else {
            let mut ddl = sqlgen::create_table_ddl(d, &t, &columns.iter().filter(|c| !c.name.trim().is_empty()).cloned().collect::<Vec<_>>(), &[]);
            if d == Dialect::ClickHouse {
                let order = columns.iter().find(|c| c.primary_key).map(|c| sqlgen::quote_ident(d, &c.name)).unwrap_or_else(|| "tuple()".into());
                ddl = ddl.trim_end_matches(';').to_string() + &format!(" ENGINE = MergeTree ORDER BY {order};");
            }
            ddl
        };
        if !name.trim().is_empty() {
            ui.add_space(8.0);
            sql_box(ui, &ddl, 160.0);
        }
        let (ok, cancel) = buttons(ui, "Create", false);
        if ok && !name.trim().is_empty() {
            *outcome = ModalOutcome::Actions(vec![
                WsAction::RunScript { stmts: vec![ddl], label: format!("Created {}", name.trim()), refresh: true },
            ]);
        } else if cancel {
            *outcome = ModalOutcome::Close;
        }
    })
}

/// Finds a client tool on PATH or in common install locations.
pub fn find_binary(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for extra in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/homebrew/opt/libpq/bin",
        "/usr/local/opt/libpq/bin",
        "/opt/homebrew/opt/mysql-client/bin",
        "/usr/local/opt/mysql-client/bin",
        "/Applications/Postgres.app/Contents/Versions/latest/bin",
        "/usr/bin",
        "C:\\Program Files\\PostgreSQL\\17\\bin",
        "C:\\Program Files\\PostgreSQL\\16\\bin",
        "C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin",
    ] {
        dirs.push(PathBuf::from(extra));
    }
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

struct DumpCommand {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    stdin: Option<PathBuf>,
}

fn dump_command(st: &BackupState, session: &Session, path: &std::path::Path) -> anyhow::Result<Option<DumpCommand>> {
    let cfg = &session.cfg;
    let (host, port) = session.endpoint.clone();
    let db = st.database.clone();
    let p = path.display().to_string();
    let pick = |default: &str| -> anyhow::Result<String> {
        if !st.binary.trim().is_empty() {
            return Ok(st.binary.trim().to_string());
        }
        find_binary(default).map(|p| p.display().to_string()).ok_or_else(|| anyhow::anyhow!("{default} could not be found. Install it, then try again (or set its path)."))
    };
    Ok(Some(match (cfg.driver.dialect(), st.restore) {
        (Dialect::Postgres, false) => {
            let mut args = vec!["-h".into(), host, "-p".into(), port.to_string(), "-U".into(), cfg.user.clone(), "-d".into(), db, "-f".into(), p];
            args.push(if st.format == DumpFormat::Custom { "-Fc".into() } else { "-Fp".into() });
            if st.schema_only {
                args.push("--schema-only".into());
            }
            DumpCommand { program: pick("pg_dump")?, args, env: vec![("PGPASSWORD".into(), cfg.password.clone())], stdin: None }
        }
        (Dialect::Postgres, true) => {
            let plain = path.extension().map(|e| e == "sql").unwrap_or(false);
            if plain {
                DumpCommand { program: pick("psql")?, args: vec!["-h".into(), host, "-p".into(), port.to_string(), "-U".into(), cfg.user.clone(), "-d".into(), db, "-v".into(), "ON_ERROR_STOP=1".into(), "-f".into(), p], env: vec![("PGPASSWORD".into(), cfg.password.clone())], stdin: None }
            } else {
                DumpCommand { program: pick("pg_restore")?, args: vec!["-h".into(), host, "-p".into(), port.to_string(), "-U".into(), cfg.user.clone(), "-d".into(), db, "--no-owner".into(), p], env: vec![("PGPASSWORD".into(), cfg.password.clone())], stdin: None }
            }
        }
        (Dialect::MySql, false) => {
            let mut args = vec!["-h".into(), host, "-P".into(), port.to_string(), "-u".into(), cfg.user.clone(), "--single-transaction".into(), "--routines".into(), format!("--result-file={p}")];
            if st.schema_only {
                args.push("--no-data".into());
            }
            args.push(db);
            DumpCommand { program: pick("mysqldump")?, args, env: vec![("MYSQL_PWD".into(), cfg.password.clone())], stdin: None }
        }
        (Dialect::MySql, true) => DumpCommand { program: pick("mysql")?, args: vec!["-h".into(), host, "-P".into(), port.to_string(), "-u".into(), cfg.user.clone(), db], env: vec![("MYSQL_PWD".into(), cfg.password.clone())], stdin: Some(path.to_path_buf()) },
        _ => return Ok(None),
    }))
}

fn backup_ui(ctx: &egui::Context, st: &mut BackupState, info: &ModalInfo, outcome: &mut ModalOutcome) -> bool {
    let pal = p();
    let d = info.cfg.driver.dialect();
    if let Some(r) = take(&mut st.task) {
        st.output = Some(match r {
            Ok(msg) => (true, msg),
            Err(e) => (false, format!("{e:#}")),
        });
    }
    let title = if st.restore { "Restore database" } else { "Backup database" };
    modal_window(ctx, title, 540.0, |ui| {
        ui.horizontal(|ui| {
            ui.label("Database");
            egui::ComboBox::from_id_salt("backup-db").selected_text(&st.database).show_ui(ui, |ui| {
                for db in info.databases {
                    ui.selectable_value(&mut st.database, db.clone(), db);
                }
            });
        });
        if !st.restore && d == Dialect::Postgres {
            ui.horizontal(|ui| {
                ui.label("Format");
                ui.selectable_value(&mut st.format, DumpFormat::Custom, "Custom (.dump)");
                ui.selectable_value(&mut st.format, DumpFormat::Plain, "Plain SQL");
            });
        }
        if !st.restore && matches!(d, Dialect::Postgres | Dialect::MySql) {
            ui.checkbox(&mut st.schema_only, "Structure only");
        }
        ui.horizontal(|ui| {
            if secondary_button(ui, if st.restore { "Choose backup file…" } else { "Save to…" }).clicked() {
                let ext = if d == Dialect::Postgres && st.format == DumpFormat::Custom && !st.restore { "dump" } else if d == Dialect::Sqlite { "sqlite" } else { "sql" };
                st.path = if st.restore {
                    rfd::FileDialog::new().pick_file()
                } else {
                    rfd::FileDialog::new().set_file_name(format!("{}-{}.{ext}", st.database, chrono::Local::now().format("%Y%m%d-%H%M"))).save_file()
                };
            }
            if let Some(p) = &st.path {
                ui.label(faint(p.display().to_string()));
            }
        });
        if matches!(d, Dialect::Postgres | Dialect::MySql) {
            ui.horizontal(|ui| {
                ui.label(faint("Client binary"));
                ui.add(crate::ui::line_edit(&mut st.binary).hint_text("auto-detect").desired_width(260.0));
            });
        }
        if d == Dialect::SqlServer {
            ui.label(faint("SQL Server writes and reads backup files on the server's own disk."));
        }
        if let Some((ok, msg)) = &st.output {
            ui.add(egui::Label::new(RichText::new(msg).monospace().size(11.5).color(if *ok { pal.success } else { pal.danger })).wrap());
        }
        if st.task.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(dim(if st.restore { "Restoring…" } else { "Dumping…" }));
            });
            return;
        }
        let (ok, cancel) = buttons(ui, if st.restore { "Start restore" } else { "Start backup" }, st.restore);
        if cancel {
            *outcome = ModalOutcome::Close;
        }
        if !ok {
            return;
        }
        let Some(path) = st.path.clone() else {
            st.output = Some((false, "Please choose a file first".into()));
            return;
        };
        let session = info.session.clone();
        match d {
            Dialect::Sqlite => {
                let p = path.display().to_string().replace('\'', "''");
                let restore = st.restore;
                st.task = Some(session.spawn(ui.ctx(), move |drv| async move {
                    let mut drv = drv.lock().await;
                    if restore {
                        let sql = std::fs::read_to_string(&p)?;
                        drv.execute(&sql).await?;
                        Ok("Restore database completed".into())
                    } else {
                        drv.execute(&format!("VACUUM INTO '{p}'")).await?;
                        Ok(format!("Backup saved to {p}"))
                    }
                }));
            }
            Dialect::SqlServer => {
                let db = sqlgen::quote_ident(d, &st.database);
                let p = sqlgen::lit(d, &path.display().to_string());
                let sql = if st.restore { format!("RESTORE DATABASE {db} FROM DISK = {p} WITH REPLACE") } else { format!("BACKUP DATABASE {db} TO DISK = {p}") };
                st.task = Some(session.spawn(ui.ctx(), move |drv| async move {
                    drv.lock().await.execute(&sql).await?;
                    Ok("Done".into())
                }));
            }
            _ => match dump_command(st, &session, &path) {
                Ok(Some(cmd)) => {
                    let restore = st.restore;
                    st.task = Some(crate::runtime::Task::spawn(ui.ctx(), async move {
                        let mut c = tokio::process::Command::new(&cmd.program);
                        c.args(&cmd.args).envs(cmd.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
                        if let Some(f) = &cmd.stdin {
                            c.stdin(std::fs::File::open(f)?);
                        }
                        let out = c.output().await?;
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                        if !out.status.success() {
                            anyhow::bail!("{} exited with {}\n{}", cmd.program, out.status, stderr.trim());
                        }
                        Ok(if restore { "Restore database completed".to_string() } else { format!("Backup database completed: {}", path.display()) })
                    }));
                }
                Ok(None) => st.output = Some((false, format!("{} doesn't support this feature", info.cfg.driver.label()))),
                Err(e) => st.output = Some((false, format!("{e:#}"))),
            },
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_sql_quotes_names_and_passwords() {
        assert_eq!(create_user_sql(Dialect::Postgres, "app", "p'w"), "CREATE ROLE \"app\" LOGIN PASSWORD 'p''w'");
        assert_eq!(create_user_sql(Dialect::MySql, "app", "x"), "CREATE USER 'app'@'%' IDENTIFIED BY 'x'");
        assert_eq!(drop_user_sql(Dialect::MySql, "app", Some("localhost")), "DROP USER 'app'@'localhost'");
    }
}
