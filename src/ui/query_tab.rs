//! SQL query tab: the editor on top, then one result card per statement (table, chart or plan).

use super::editor::{Catalog, SqlEditor};
use super::grid::{self, GridEvent, GridState, ResultSource};
use super::workspace::{TabCtx, WsAction};
use super::{dim, faint, icon_button, p, paint_icon, segmented, Icon, ToastKind};
use crate::db::QueryResult;
use crate::model::Dialect;
use crate::model::ConnectionConfig;
use crate::query_transaction::{self, ManagedQuery, TransactionState};
use crate::runtime::{take, Task};
use egui::{vec2, Color32, CornerRadius, RichText, Stroke, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellView {
    Table,
    Chart,
    Plan,
}

/// One statement's outcome.
pub struct ResultView {
    pub result: QueryResult,
    pub rows: Vec<Vec<Option<String>>>,
    pub grid: GridState,
    pub sort: Vec<(usize, bool)>,
    /// Column metadata with value profiles, computed once per result.
    pub meta: Vec<grid::ColMeta>,
    pub view: CellView,
    pub collapsed: bool,
    pub chart: Option<ChartSpec>,
    pub plan: Vec<PlanNode>,
}

/// A label column plus numeric series, when the rows chart well.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSpec {
    pub label: usize,
    pub series: Vec<usize>,
    pub line: bool,
}

/// One line of a query plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanNode {
    pub depth: usize,
    pub label: String,
    /// Total cost (Postgres) or 1.0 (SQLite, which has none).
    pub cost: f64,
    pub rows: Option<f64>,
    pub warn: Option<String>,
}

impl ResultView {
    fn new(result: QueryResult, dialect: Dialect) -> Self {
        let rows = result.rows.clone();
        let mut meta = grid::result_meta(&result);
        grid::profile_all(&mut meta, &rows);
        let plan = parse_plan(&result, dialect);
        let chart = chart_spec(&meta, rows.len());
        let view = if !plan.is_empty() { CellView::Plan } else { CellView::Table };
        Self { result, rows, grid: GridState::default(), sort: vec![], meta, view, collapsed: false, chart, plan }
    }
}

/// Bar or line chart when the first text/date column labels one or more numeric columns.
pub fn chart_spec(meta: &[grid::ColMeta], rows: usize) -> Option<ChartSpec> {
    use grid::ProfileKind as K;
    if !(2..=2000).contains(&rows) || meta.len() < 2 {
        return None;
    }
    let kind = |i: usize| meta[i].profile.as_ref().map(|p| p.kind).unwrap_or_default();
    let label = (0..meta.len()).find(|&i| matches!(kind(i), K::Temporal | K::Categorical | K::Text | K::Unique))?;
    let series: Vec<usize> = (0..meta.len()).filter(|&i| i != label && kind(i) == K::Numeric).take(4).collect();
    if series.is_empty() {
        return None;
    }
    Some(ChartSpec { label, series, line: kind(label) == K::Temporal || rows > 40 })
}

/// Reads Postgres' text EXPLAIN or SQLite's EXPLAIN QUERY PLAN into indented nodes.
pub fn parse_plan(r: &QueryResult, d: Dialect) -> Vec<PlanNode> {
    match d {
        Dialect::Postgres if r.columns.len() == 1 && r.columns[0].eq_ignore_ascii_case("QUERY PLAN") => {
            let mut out = Vec::new();
            for row in &r.rows {
                let Some(line) = row.first().cloned().flatten() else { continue };
                let indent = line.len() - line.trim_start().len();
                let t = line.trim_start().trim_start_matches("->").trim();
                if t.is_empty() || !(t.contains("(cost=") || out.is_empty()) {
                    continue;
                }
                let label = t.split("  (").next().unwrap_or(t).trim().to_string();
                let num = |key: &str| -> Option<f64> {
                    let i = t.find(key)? + key.len();
                    let rest = &t[i..];
                    let end = rest.find(|c: char| c == ' ' || c == ')').unwrap_or(rest.len());
                    rest[..end].split("..").last()?.parse().ok()
                };
                let cost = num("cost=").unwrap_or(0.0);
                let rows = num("rows=");
                let warn = (label.starts_with("Seq Scan") && rows.unwrap_or(0.0) > 10_000.0).then(|| format!("{label} reads every row; an index on the filtered column would avoid it"));
                out.push(PlanNode { depth: indent / 2, label, cost, rows, warn });
            }
            out
        }
        Dialect::Sqlite if r.col("detail").is_some() && r.col("parent").is_some() => {
            let (id_c, parent_c, detail_c) = (r.col("id").unwrap_or(0), r.col("parent").unwrap_or(1), r.col("detail").unwrap_or(3));
            let mut depth_of: std::collections::HashMap<String, usize> = Default::default();
            r.rows
                .iter()
                .map(|row| {
                    let id = row.get(id_c).cloned().flatten().unwrap_or_default();
                    let parent = row.get(parent_c).cloned().flatten().unwrap_or_default();
                    let depth = depth_of.get(&parent).map(|d| d + 1).unwrap_or(0);
                    depth_of.insert(id, depth);
                    let label = row.get(detail_c).cloned().flatten().unwrap_or_default();
                    let warn = (label.starts_with("SCAN") && !label.contains("USING")).then(|| format!("{label}: a full scan; consider an index"));
                    PlanNode { depth, label, cost: 1.0, rows: None, warn }
                })
                .collect()
        }
        _ => vec![],
    }
}

pub struct QueryTab {
    pub id: u64,
    pub title: String,
    pub sql: String,
    pub editor: SqlEditor,
    pub results: Vec<ResultView>,
    /// Statements that returned no rows: (ok, text).
    pub messages: Vec<(bool, String)>,
    task: Option<Task<anyhow::Result<Vec<QueryResult>>>>,
    pub auto_commit: bool,
    managed: Option<ManagedQuery>,
    finish_task: Option<Task<anyhow::Result<()>>>,
    finish_message: &'static str,
    pub transaction_error: Option<String>,
    running_sql: String,
    pub saved_id: Option<uuid::Uuid>,
    pub dirty: bool,
    /// Elapsed milliseconds of recent runs of this tab, newest last.
    pub runs: Vec<u64>,
    ask: String,
    ask_task: Option<Task<anyhow::Result<String>>>,
    ask_answer: Option<Result<String, String>>,
}

impl QueryTab {
    pub fn new(id: u64, title: String, sql: String) -> Self {
        Self {
            id,
            title,
            sql,
            editor: SqlEditor::new(egui::Id::unique(("sql-editor", id))),
            results: vec![],
            messages: vec![],
            task: None,
            auto_commit: false,
            managed: None,
            finish_task: None,
            finish_message: "",
            transaction_error: None,
            running_sql: String::new(),
            saved_id: None,
            dirty: false,
            runs: vec![],
            ask: String::new(),
            ask_task: None,
            ask_answer: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.task.is_some()
    }

    pub fn transaction_busy(&self) -> bool { self.task.is_some() || self.finish_task.is_some() }
    pub fn has_pending_transaction(&self) -> bool {
        self.managed.as_ref().is_some_and(|query| query.pending()) || (!self.auto_commit && self.managed.is_some() && self.is_running())
    }
    pub fn transaction_state(&self) -> TransactionState { self.managed.as_ref().map(|query| query.state()).unwrap_or(TransactionState::Idle) }
    pub fn reset_connection(&mut self) { if !self.has_pending_transaction() && !self.transaction_busy() { self.managed = None; } }
    pub fn cancel(&self, session: &crate::runtime::Session) {
        if let Some(query) = &self.managed { if !self.auto_commit { query.cancel(); return; } }
        session.cancel();
    }
    pub fn finish_transaction(&mut self, ctx: &egui::Context, commit: bool) {
        if self.transaction_busy() { return; }
        let Some(query) = self.managed.clone() else { return };
        self.transaction_error = None;
        self.finish_message = if commit { "Transaction committed" } else { "Transaction rolled back" };
        self.finish_task = Some(Task::spawn(ctx, async move { query.finish(commit).await }));
    }
    pub fn abandon_transaction(&mut self, ctx: &egui::Context) {
        if self.transaction_busy() { return; }
        let Some(query) = self.managed.clone() else { return };
        self.transaction_error = None;
        self.finish_message = "Connection closed; the previous transaction outcome remains unknown";
        self.finish_task = Some(Task::spawn(ctx, async move { query.abandon().await; Ok(()) }));
    }
    pub fn poll_transaction(&mut self) {
        if let Some(result) = take(&mut self.finish_task) {
            match result {
                Ok(()) => self.messages.push((true, self.finish_message.into())),
                Err(error) => { let error = format!("{error:#}"); self.transaction_error = Some(error.clone()); self.messages.push((false, error)); }
            }
        }
    }

    /// The text that "Run current" would execute: the selection, or the statement under the caret.
    pub fn current_sql(&self) -> String {
        if let Some(sel) = self.editor.selected_text(&self.sql) {
            return sel.to_string();
        }
        match self.editor.current_statement(&self.sql) {
            Some((s, e)) => self.sql[s..e].trim().to_string(),
            None => self.sql.trim().to_string(),
        }
    }

    /// Called by the workspace once parameters are bound and safe mode approved.
    pub fn start(&mut self, sql: String, cx: &TabCtx, cfg: ConnectionConfig, schema: String) {
        if sql.trim().is_empty() || self.transaction_busy() {
            return;
        }
        cx.session.log(sql.clone(), true);
        self.running_sql = sql.clone();
        let timeout = cx.settings.query_timeout_secs;
        if !self.auto_commit && query_transaction::supported(&cfg) {
            let query = self.managed.get_or_insert_with(|| ManagedQuery::new(cfg, schema)).clone();
            query.prepare_run();
            self.task = Some(Task::spawn(cx.ctx, async move { query.execute(&sql, timeout).await }));
            return;
        }
        self.task = Some(cx.session.spawn(cx.ctx, move |d| async move {
            let mut d = d.lock().await;
            if timeout > 0 {
                match tokio::time::timeout(std::time::Duration::from_secs(timeout), d.execute(&sql)).await {
                    Ok(r) => r,
                    Err(_) => anyhow::bail!("Query timed out after {timeout} seconds"),
                }
            } else {
                d.execute(&sql).await
            }
        }));
    }

    pub(super) fn poll(&mut self, cx: &mut TabCtx) {
        self.poll_transaction();
        if let Some(r) = take(&mut self.ask_task) {
            self.ask_answer = Some(r.map_err(|e| format!("{e:#}")));
        }
        let Some(task) = &self.task else { return };
        let elapsed = task.started.elapsed().as_millis() as u64;
        let Some(r) = take(&mut self.task) else { return };
        let sql = std::mem::take(&mut self.running_sql);
        let dialect = cx.session.dialect();
        self.runs.push(elapsed);
        if self.runs.len() > 24 {
            self.runs.remove(0);
        }
        match r {
            Ok(results) => {
                self.messages.clear();
                let mut views = Vec::new();
                for (i, res) in results.into_iter().enumerate() {
                    if res.has_rows() {
                        views.push(ResultView::new(res, dialect));
                    } else {
                        let what = match (res.affected, &res.message) {
                            (Some(n), _) => format!("{n} row{} affected", if n == 1 { "" } else { "s" }),
                            (None, Some(m)) => m.clone(),
                            _ => "OK".into(),
                        };
                        self.messages.push((true, format!("[{}] {what} · {} ms · {}", i + 1, res.elapsed_ms, crate::db::sqlgen::ellipsize(&res.sql, 160))));
                    }
                }
                self.results = views;
                cx.actions.push(WsAction::RecordHistory { sql, elapsed_ms: elapsed, ok: true });
                if crate::db::statements(&self.sql).iter().any(|s| is_ddl(s)) {
                    cx.actions.push(WsAction::RefreshObjects);
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                cx.session.log(format!("-- error: {msg}"), false);
                self.messages = vec![(false, msg)];
                self.results.clear();
                cx.actions.push(WsAction::RecordHistory { sql, elapsed_ms: elapsed, ok: false });
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, catalog: &Catalog, context_ready: bool) {
        self.poll(cx);
        let pal = p();
        let dialect = cx.session.dialect();

        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 10 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if self.is_running() {
                    let b = egui::Button::new(RichText::new("Stop  ⌘.").font(super::semibold(12.5)).color(Color32::WHITE)).fill(pal.danger).corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 32.0));
                    if ui.add(b).clicked() {
                        self.cancel(cx.session);
                    }
                    ui.add(egui::Spinner::new().size(14.0).color(pal.accent));
                    if let Some(t) = &self.task {
                        ui.label(RichText::new(format!("{:.1}s", t.started.elapsed().as_secs_f32())).font(egui::FontId::monospace(12.0)).color(pal.text_dim));
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                } else {
                    let b = egui::Button::new(RichText::new("▶  Run  ⌘↩").font(super::semibold(12.5)).color(pal.on_accent)).fill(pal.accent).corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 32.0));
                    if ui.add_enabled(context_ready && !self.transaction_busy(), b).on_hover_text("Run the statement under the caret, or the selection").clicked() {
                        cx.actions.push(WsAction::RunQuery { tab_id: self.id, sql: self.current_sql() });
                    }
                    let b = egui::Button::new(RichText::new("Run all  ⌘⇧↩").size(12.5).color(pal.text)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, pal.border)).corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 32.0));
                    if ui.add_enabled(context_ready && !self.transaction_busy(), b).clicked() {
                        cx.actions.push(WsAction::RunQuery { tab_id: self.id, sql: self.sql.clone() });
                    }
                }
                ui.add_space(4.0);
                if let Some(prefix) = explain_prefix(dialect) {
                    if icon_button(ui, Icon::Activity, "Explain: show the query plan").clicked() {
                        let sql = format!("{prefix} {}", self.current_sql());
                        cx.actions.push(WsAction::RunQuery { tab_id: self.id, sql });
                    }
                }
                if !matches!(dialect, Dialect::Redis | Dialect::Mongo) && icon_button(ui, Icon::Wand, "Format (⌘⇧F)").clicked() {
                    self.editor.format(&mut self.sql, cx.settings.uppercase_keywords, cx.settings.tab_width);
                    self.dirty = true;
                }
                if super::icon_button_colored(ui, Icon::Star, if self.saved_id.is_some() { "Saved to Queries (⌘S)" } else { "Save to Queries (⌘S)" }, None, self.saved_id.is_some()).clicked() {
                    cx.actions.push(WsAction::SaveQuery(self.id));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let line = self.sql[..char_byte(&self.sql, self.editor.cursor_char)].matches('\n').count() + 1;
                    ui.label(RichText::new(format!("Ln {line}")).font(egui::FontId::monospace(11.5)).color(pal.text_faint));
                    if self.runs.len() > 1 {
                        run_sparkline(ui, &self.runs);
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                let supported = query_transaction::supported(&cx.session.cfg);
                let mut auto_commit = self.auto_commit || !supported;
                if ui.add_enabled(supported && !self.transaction_busy(), egui::Checkbox::new(&mut auto_commit, "Auto-commit")).on_hover_text(if supported { "When off, changes stay pending until Commit or Rollback" } else { "Persistent manual transactions are unavailable for this connection" }).changed() {
                    cx.actions.push(WsAction::SetAutoCommit { tab: self.id, enabled: auto_commit });
                }
                if supported && !self.auto_commit {
                    let state = self.transaction_state();
                    if ui.add_enabled(state == TransactionState::Active && !self.transaction_busy(), egui::Button::new("Commit")).clicked() { self.finish_transaction(cx.ctx, true); }
                    if ui.add_enabled(self.has_pending_transaction() && !self.transaction_busy(), egui::Button::new("Rollback")).clicked() { self.finish_transaction(cx.ctx, false); }
                    let status = if self.finish_task.is_some() { "Finishing transaction…" } else { match state { TransactionState::Idle => "Transaction starts on Run", TransactionState::Active => "Pending transaction", TransactionState::Failed => "Failed transaction — Rollback required", TransactionState::Unknown => "Outcome unknown — Rollback or close connection" } };
                    ui.label(RichText::new(status).size(11.5).color(if state == TransactionState::Idle { pal.text_dim } else { pal.warning }));
                    if state == TransactionState::Unknown && !self.transaction_busy() && ui.button("Close uncertain connection…").clicked() {
                        cx.actions.push(WsAction::ResolveUncertain(self.id));
                    }
                }
            });
        });
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, pal.separator));

        let has_results = !self.results.is_empty() || !self.messages.is_empty();
        let mut ev = Default::default();
        let mut editor_panel = |ui: &mut Ui, me: &mut Self, ev: &mut super::editor::EditorEvents| {
            if cx.settings.ai_enabled {
                egui::Panel::bottom(egui::Id::unique(("ask", me.id))).frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 6, bottom: 10 })).show(ui, |ui| me.ask_ui(ui, cx, catalog));
            }
            *ev = me.editor.show(ui, &mut me.sql, catalog, cx.settings.editor_font_size, cx.settings.uppercase_keywords, cx.settings.auto_close_brackets);
        };
        if has_results {
            egui::Panel::top(egui::Id::unique(("qsplit", self.id)))
                .resizable(true)
                .default_size(ui.available_height() * 0.42)
                .min_size(90.0)
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)))
                .show(ui, |ui| editor_panel(ui, self, &mut ev));
        } else {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)).show(ui, |ui| {
                ui.set_min_height(ui.available_height());
                editor_panel(ui, self, &mut ev);
            });
        }
        if ev.changed {
            self.dirty = true;
        }
        if ev.run_current && context_ready && !self.transaction_busy() {
            cx.actions.push(WsAction::RunQuery { tab_id: self.id, sql: self.current_sql() });
        }
        if ev.run_all && context_ready && !self.transaction_busy() {
            cx.actions.push(WsAction::RunQuery { tab_id: self.id, sql: self.sql.clone() });
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::F)) {
            self.editor.format(&mut self.sql, cx.settings.uppercase_keywords, cx.settings.tab_width);
        }
        if has_results {
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, pal.separator));
            self.results_ui(ui, cx);
        }
    }

    fn ask_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, catalog: &Catalog) {
        let pal = p();
        let well = if pal.dark { Color32::from_black_alpha(70) } else { Color32::from_black_alpha(8) };
        egui::Frame::new().fill(well).stroke(Stroke::new(1.0, pal.glass_stroke)).corner_radius(CornerRadius::same(14)).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 22.0), egui::Sense::hover());
                paint_icon(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(15.0, 15.0)), Icon::Sparkle, pal.accent);
                let resp = ui.add(crate::ui::line_edit(&mut self.ask).hint_text(faint("Ask in plain words, e.g. customers with 3+ orders this month")).frame(egui::Frame::NONE).desired_width(ui.available_width() - 30.0));
                if self.ask_task.is_some() {
                    ui.add(egui::Spinner::new().size(13.0).color(pal.accent));
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.ask.trim().is_empty() && self.ask_task.is_none() {
                    let fut = super::assistant::ask_for_sql(cx.settings, cx.store, cx.session.cfg.driver.label(), &cx.session.cfg.database, catalog, self.ask.trim());
                    self.ask_task = Some(Task::spawn(cx.ctx, fut));
                    self.ask_answer = None;
                }
            });
            match self.ask_answer.clone() {
                Some(Ok(sql)) => {
                    ui.add_space(6.0);
                    egui::Frame::new().fill(pal.editor_bg).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let mut t = sql.as_str();
                        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _w: f32| ui.fonts_mut(|f| f.layout_job(super::editor::highlight(buf.as_str(), egui::FontId::monospace(12.5), None)));
                        ui.add(egui::TextEdit::multiline(&mut t).code_editor().desired_rows(1).desired_width(f32::INFINITY).frame(egui::Frame::NONE).layouter(&mut layouter));
                    });
                    ui.horizontal(|ui| {
                        ui.label(faint("Only table and column names were sent. No row data left this machine.").size(11.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if super::primary_button(ui, "  Insert  ").clicked() {
                                if !self.sql.trim().is_empty() && !self.sql.trim_end().ends_with(';') {
                                    self.sql.push(';');
                                }
                                if !self.sql.is_empty() {
                                    self.sql.push_str("\n\n");
                                }
                                self.sql.push_str(sql.trim());
                                self.dirty = true;
                                self.ask_answer = None;
                                self.ask.clear();
                            }
                            if ui.button("Dismiss").clicked() {
                                self.ask_answer = None;
                            }
                        });
                    });
                }
                Some(Err(e)) => {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(e).size(12.0).color(pal.danger));
                        if ui.small_button("Dismiss").clicked() {
                            self.ask_answer = None;
                        }
                    });
                }
                None => {}
            }
        });
    }

    fn results_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx) {
        let pal = p();
        let many = self.results.len() + usize::from(!self.messages.is_empty()) > 1;
        egui::ScrollArea::vertical().id_salt(("cells", self.id)).auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                if !self.messages.is_empty() {
                    let failed = self.messages.iter().any(|(ok, _)| !ok);
                    let tone = if failed { pal.danger } else { pal.success };
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(tone.r(), tone.g(), tone.b(), 22))
                        .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(tone.r(), tone.g(), tone.b(), 70)))
                        .corner_radius(CornerRadius::same(14))
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for (ok, m) in &self.messages {
                                ui.horizontal_top(|ui| {
                                    ui.label(RichText::new(if *ok { "✓" } else { "✗" }).color(if *ok { pal.success } else { pal.danger }));
                                    ui.add(egui::Label::new(RichText::new(m).font(egui::FontId::monospace(12.0)).color(if *ok { pal.text } else { super::readable_on_tint(pal.danger) })).wrap());
                                });
                            }
                            if failed {
                                ui.horizontal(|ui| {
                                    if ui.small_button("Copy error").clicked() {
                                        ui.ctx().copy_text(self.messages.iter().map(|(_, m)| m.clone()).collect::<Vec<_>>().join("\n"));
                                    }
                                });
                            }
                        });
                    ui.add_space(10.0);
                }
                let n = self.results.len();
                for idx in 0..n {
                    self.cell_ui(ui, cx, idx, many);
                    ui.add_space(12.0);
                }
            });
        });
    }

    fn cell_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, idx: usize, many: bool) {
        let pal = p();
        let id = self.id;
        let rv = &mut self.results[idx];
        egui::Frame::new().fill(pal.grid_even).stroke(Stroke::new(1.0, pal.glass_stroke)).corner_radius(CornerRadius::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 8, top: 6, bottom: 6 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let (r, resp) = ui.allocate_exact_size(vec2(14.0, 22.0), egui::Sense::click());
                    paint_icon(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(10.0, 10.0)), if rv.collapsed { Icon::ChevronRight } else { Icon::ChevronDown }, pal.text_faint);
                    if resp.clicked() {
                        rv.collapsed = !rv.collapsed;
                    }
                    ui.label(RichText::new("✓").color(pal.success));
                    ui.label(RichText::new(format!("{} ms · {} row{}", rv.result.elapsed_ms, rv.rows.len(), if rv.rows.len() == 1 { "" } else { "s" })).font(egui::FontId::monospace(11.5)).color(pal.text_dim));
                    let w = (ui.available_width() - 230.0).max(40.0);
                    ui.add_sized([w, 18.0], egui::Label::new(RichText::new(crate::db::sqlgen::ellipsize(&rv.result.sql, 120)).font(egui::FontId::monospace(11.5)).color(pal.text_faint)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Export, "Export this result…").clicked() {
                            cx.actions.push(WsAction::ExportResult { columns: rv.result.columns.clone(), rows: rv.rows.clone() });
                        }
                        let mut views = vec![(CellView::Table, "Table")];
                        if rv.chart.is_some() {
                            views.push((CellView::Chart, "Chart"));
                        }
                        if !rv.plan.is_empty() {
                            views.push((CellView::Plan, "Plan"));
                        }
                        if views.len() > 1 {
                            let mut v = rv.view;
                            if segmented(ui, &mut v, &views) {
                                rv.view = v;
                            }
                        }
                    });
                });
            });
            if rv.collapsed {
                return;
            }
            let row_h = (cx.settings.grid_font_size * 1.85).round();
            let natural = row_h * (rv.rows.len() as f32 + 3.2) + 20.0;
            let height = if many { natural.clamp(140.0, 380.0) } else { (ui.available_height() - 12.0).max(natural.min(380.0)).max(200.0) };
            match rv.view {
                CellView::Chart => {
                    if let Some(spec) = rv.chart.clone() {
                        egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| chart_ui(ui, rv, &spec, id, idx, height.min(320.0)));
                    }
                }
                CellView::Plan => {
                    egui::Frame::new().inner_margin(egui::Margin::same(14)).show(ui, |ui| plan_ui(ui, &rv.plan));
                }
                CellView::Table => {
                    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
                        let opts = grid::GridOptions {
                            id: egui::Id::unique(("result-grid", id, idx)),
                            editable: false,
                            row_numbers: cx.settings.show_row_numbers,
                            alternating: cx.settings.alternating_rows,
                            null_text: &cx.settings.null_display,
                            font_size: cx.settings.grid_font_size,
                            row_offset: 0,
                            profiles: cx.settings.column_profiles,
                        };
                        let src = ResultSource { meta: rv.meta.clone(), rows: &rv.rows, sort: &rv.sort };
                        let events = grid::show(ui, &src, &mut rv.grid, &opts);
                        for ev in events {
                            handle_grid_event(rv, ev, cx);
                        }
                    });
                }
            }
        });
    }
}

fn handle_grid_event(rv: &mut ResultView, ev: GridEvent, cx: &mut TabCtx) {
    match ev {
        GridEvent::Sort { col, ascending, .. } => {
            let cur = rv.sort.first().filter(|(c, _)| *c == col).map(|(_, a)| *a);
            let asc = ascending.unwrap_or(!matches!(cur, Some(true)));
            rv.sort = vec![(col, asc)];
            grid::sort_rows(&mut rv.rows, col, asc);
        }
        GridEvent::CopyCells => {
            let src = ResultSource { meta: rv.meta.clone(), rows: &rv.rows, sort: &rv.sort };
            let rows = grid::selection_rows(&rv.grid);
            let text = if rows.len() > 1 {
                grid::format_rows(&src, &rows, grid::CopyFormat::Tsv, &cx.settings.null_display, None)
            } else if let Some((r, c)) = rv.grid.cursor {
                grid::cell_value(&src, r, c).unwrap_or_else(|| cx.settings.null_display.clone())
            } else {
                String::new()
            };
            super::copy(cx.ctx, text);
        }
        GridEvent::CopyRows(fmt) => {
            let src = ResultSource { meta: rv.meta.clone(), rows: &rv.rows, sort: &rv.sort };
            let rows = grid::selection_rows(&rv.grid);
            super::copy(cx.ctx, grid::format_rows(&src, &rows, fmt, &cx.settings.null_display, None));
            cx.actions.push(WsAction::Toast(ToastKind::Info, format!("Copied {} rows", rows.len())));
        }
        GridEvent::CopyColumnName(c) => super::copy(cx.ctx, rv.result.columns[c].clone()),
        GridEvent::HideColumn(c) => {
            rv.grid.hidden.insert(c);
        }
        GridEvent::QuickLook { row, col } => {
            let v = rv.rows.get(row).and_then(|r| r.get(col)).cloned().flatten().unwrap_or_default();
            cx.actions.push(WsAction::QuickLookText { title: rv.result.columns[col].clone(), text: v });
        }
        _ => {}
    }
}

fn chart_ui(ui: &mut Ui, rv: &ResultView, spec: &ChartSpec, id: u64, idx: usize, height: f32) {
    let pal = p();
    let labels: Vec<String> = rv.rows.iter().map(|r| r.get(spec.label).cloned().flatten().unwrap_or_default()).collect();
    let num = |r: &Vec<Option<String>>, c: usize| r.get(c).cloned().flatten().and_then(|s| s.trim().parse::<f64>().ok());
    let lbl = labels.clone();
    let plot = egui_plot::Plot::new(("cell-chart", id, idx))
        .height(height)
        .allow_drag(false)
        .allow_scroll(false)
        .show_background(false)
        .legend(egui_plot::Legend::default())
        .x_axis_formatter(move |m, _| lbl.get(m.value.round() as usize).filter(|_| (m.value - m.value.round()).abs() < 1e-6).map(|s| grid::first_line(s, 14)).unwrap_or_default());
    plot.show(ui, |pu| {
        for (si, &c) in spec.series.iter().enumerate() {
            let color = pal.chart[si % 5];
            let name = rv.result.columns[c].clone();
            if spec.line {
                let pts: Vec<[f64; 2]> = rv.rows.iter().enumerate().filter_map(|(i, r)| num(r, c).map(|y| [i as f64, y])).collect();
                pu.line(egui_plot::Line::new(name, pts).color(color).width(2.0));
            } else {
                let w = 0.8 / spec.series.len() as f64;
                let bars: Vec<egui_plot::Bar> = rv
                    .rows
                    .iter()
                    .enumerate()
                    .filter_map(|(i, r)| num(r, c).map(|y| egui_plot::Bar::new(i as f64 - 0.4 + w * (si as f64 + 0.5), y).width(w * 0.9).fill(color).name(labels[i].clone())))
                    .collect();
                pu.bar_chart(egui_plot::BarChart::new(name, bars).color(color));
            }
        }
    });
}

fn plan_ui(ui: &mut Ui, plan: &[PlanNode]) {
    let pal = p();
    let max = plan.iter().map(|n| n.cost).fold(0.0f64, f64::max).max(1e-9);
    for n in plan {
        ui.horizontal(|ui| {
            ui.add_space(n.depth as f32 * 16.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&n.label).font(egui::FontId::monospace(12.5)).color(if n.warn.is_some() { super::readable_on_tint(pal.accent) } else { pal.text }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut t = String::new();
                        if n.cost != 1.0 || n.rows.is_some() {
                            t = format!("cost {:.1}", n.cost);
                        }
                        if let Some(r) = n.rows {
                            t.push_str(&format!(" · ~{} rows", super::short_count(r as u64)));
                        }
                        ui.label(RichText::new(t).font(egui::FontId::monospace(11.0)).color(pal.text_faint));
                    });
                });
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), egui::Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(3), pal.surface_hover);
                let frac = (n.cost / max).clamp(0.02, 1.0) as f32;
                ui.painter().rect_filled(egui::Rect::from_min_size(r.min, vec2(r.width() * frac, r.height())), CornerRadius::same(3), if n.warn.is_some() { pal.accent } else { pal.accent2 });
            });
        });
        if let Some(w) = &n.warn {
            ui.horizontal(|ui| {
                ui.add_space(n.depth as f32 * 16.0);
                ui.label(RichText::new(w).size(11.5).color(super::readable_on_tint(pal.accent)));
            });
        }
        ui.add_space(8.0);
    }
    if plan.is_empty() {
        ui.label(dim("No plan rows"));
    }
}

/// Tiny bars of this tab's recent run times.
fn run_sparkline(ui: &mut Ui, runs: &[u64]) {
    let pal = p();
    let (r, resp) = ui.allocate_exact_size(vec2(runs.len() as f32 * 5.0, 18.0), egui::Sense::hover());
    let max = *runs.iter().max().unwrap_or(&1) as f32;
    for (i, ms) in runs.iter().enumerate() {
        let h = (*ms as f32 / max.max(1.0) * r.height()).max(2.0);
        let x = r.left() + i as f32 * 5.0;
        let c = if i + 1 == runs.len() { pal.accent } else { pal.text_faint.gamma_multiply(0.6) };
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x, r.bottom() - h), egui::pos2(x + 3.0, r.bottom())), CornerRadius::same(1), c);
    }
    resp.on_hover_text(format!("Last {} runs · latest {} ms", runs.len(), runs.last().unwrap_or(&0)));
}

fn char_byte(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

pub fn explain_prefix(d: Dialect) -> Option<&'static str> {
    match d {
        Dialect::Postgres | Dialect::MySql | Dialect::ClickHouse => Some("EXPLAIN"),
        Dialect::Sqlite => Some("EXPLAIN QUERY PLAN"),
        _ => None,
    }
}

pub fn is_ddl(s: &str) -> bool {
    let up = s.trim_start().to_ascii_uppercase();
    ["CREATE ", "DROP ", "ALTER ", "RENAME ", "TRUNCATE "].iter().any(|k| up.starts_with(k))
}

/// `:name` parameters in the order they first appear (Postgres `::casts` are ignored).
pub fn param_names(sql: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let b = sql.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'\'' || c == b'"' || c == b'`' {
            quote = Some(c);
        } else if c == b'-' && b.get(i + 1) == Some(&b'-') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b':' && (i == 0 || b[i - 1] != b':') && b.get(i + 1).map(|n| n.is_ascii_alphabetic() || *n == b'_').unwrap_or(false) {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let name = sql[start..j].to_string();
            if !out.contains(&name) {
                out.push(name);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

/// Replaces `:name` with literal values (numbers stay bare, everything else is quoted).
pub fn bind_params(sql: &str, values: &[(String, String)], d: Dialect) -> String {
    let mut out = String::with_capacity(sql.len());
    let b = sql.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == b'\'' || c == b'"' || c == b'`' {
            quote = Some(c);
        } else if c == b':' && (i == 0 || b[i - 1] != b':') && b.get(i + 1).map(|n| n.is_ascii_alphabetic() || *n == b'_').unwrap_or(false) {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let name = &sql[i + 1..j];
            if let Some((_, v)) = values.iter().find(|(n, _)| n == name) {
                let literal = if v.parse::<f64>().is_ok() || v.eq_ignore_ascii_case("null") || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
                    v.clone()
                } else {
                    crate::db::sqlgen::lit(d, v)
                };
                out.push_str(&literal);
                i = j;
                continue;
            }
        }
        let ch = sql[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_params_outside_strings_and_casts() {
        let sql = "select * from t where a = :id and b = ':nope' and c::text = :name -- :hidden\n and d = :id";
        assert_eq!(param_names(sql), vec!["id", "name"]);
    }

    #[test]
    fn binds_numbers_bare_and_text_quoted() {
        let sql = "select :id, :name, ':id', x::int";
        let out = bind_params(sql, &[("id".into(), "42".into()), ("name".into(), "O'Neil".into())], Dialect::Postgres);
        assert_eq!(out, "select 42, 'O''Neil', ':id', x::int");
    }

    #[test]
    fn detects_ddl() {
        assert!(is_ddl("  create table x()"));
        assert!(!is_ddl("select 1"));
    }
}
