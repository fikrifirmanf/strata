//! Metrics boards: a grid of widgets (scoreboard, table, bar, line, pie), each backed by a read-only query.

use super::grid::{self, GridState, ResultSource};
use super::workspace::TabCtx;
use super::{dim, faint, p, primary_button, Icon};
use crate::db::{is_read_only, QueryResult};
use crate::model::{MetricWidget, MetricsBoard, WidgetKind};
use crate::runtime::Task;
use crate::store::Store;
use egui::{vec2, Color32, RichText, Stroke};
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

enum WidgetData {
    Loading(Task<anyhow::Result<QueryResult>>),
    Ready(QueryResult),
    Failed(String),
}

pub struct MetricsTab {
    pub id: u64,
    pub board: Uuid,
    name: String,
    data: HashMap<Uuid, WidgetData>,
    grids: HashMap<Uuid, GridState>,
    editing: Option<MetricWidget>,
    last_refresh: Option<Instant>,
}

fn numeric(s: Option<&str>) -> Option<f64> {
    s.and_then(|v| v.trim().parse::<f64>().ok())
}

impl MetricsTab {
    pub fn new(id: u64, board: Uuid, name: String) -> Self {
        Self { id, board, name, data: HashMap::new(), grids: HashMap::new(), editing: None, last_refresh: None }
    }

    pub fn title(&self) -> String {
        self.name.clone()
    }

    fn refresh(&mut self, board: &MetricsBoard, cx: &TabCtx) {
        self.last_refresh = Some(Instant::now());
        for w in &board.widgets {
            self.run_widget(w, cx);
        }
    }

    fn run_widget(&mut self, w: &MetricWidget, cx: &TabCtx) {
        if w.sql.trim().is_empty() {
            self.data.insert(w.id, WidgetData::Failed("No query yet — edit the widget to add one.".into()));
            return;
        }
        if !is_read_only(&w.sql) {
            self.data.insert(w.id, WidgetData::Failed("Metrics widgets only run read-only queries.".into()));
            return;
        }
        let sql = w.sql.clone();
        self.data.insert(w.id, WidgetData::Loading(cx.session.spawn(cx.ctx, move |d| async move { d.lock().await.query_one(&sql).await })));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut TabCtx, boards: &mut Vec<MetricsBoard>, store: &Store) {
        let pal = p();
        let Some(bi) = boards.iter().position(|b| b.id == self.board) else {
            super::empty_state(ui, Icon::Chart, "Board not found", "It may have been deleted");
            return;
        };
        for v in self.data.values_mut() {
            if let WidgetData::Loading(t) = v {
                if let Some(r) = t.poll() {
                    *v = match r {
                        Ok(q) => WidgetData::Ready(q),
                        Err(e) => WidgetData::Failed(format!("{e:#}")),
                    };
                }
            }
        }
        let board_snapshot = boards[bi].clone();
        let due = match (self.last_refresh, board_snapshot.refresh_secs) {
            (None, _) => true,
            (Some(t), s) if s > 0 => t.elapsed().as_secs() >= s as u64,
            _ => false,
        };
        if due {
            self.refresh(&board_snapshot, cx);
        }
        if board_snapshot.refresh_secs > 0 {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }

        let mut changed = false;
        // Header: board name in mono, freshness, then auto refresh and Add widget on the right.
        let head = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(32.0);
                ui.spacing_mut().item_spacing.x = 12.0;
                let b = &mut boards[bi];
                let w = ui.painter().layout_no_wrap(b.name.clone(), super::mono_medium(14.0), pal.text).size().x.clamp(80.0, 360.0) + 8.0;
                if ui.add(crate::ui::line_edit(&mut b.name).font(super::mono_medium(14.0)).frame(egui::Frame::NONE).desired_width(w)).changed() {
                    self.name = b.name.clone();
                    changed = true;
                }
                if let Some(t) = self.last_refresh {
                    ui.label(RichText::new(format!("updated {}s ago", t.elapsed().as_secs())).font(egui::FontId::monospace(12.0)).color(pal.text_faint));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if super::btn(ui, "Add widget", super::BtnKind::Secondary).clicked() {
                        self.editing = Some(MetricWidget { id: Uuid::new_v4(), title: "New widget".into(), kind: WidgetKind::Scoreboard, sql: String::new(), wide: false });
                    }
                    if super::tool_button(ui, Icon::Refresh, "Refresh all", false).clicked() {
                        self.refresh(&board_snapshot, cx);
                    }
                    let mut auto = b.refresh_secs > 0;
                    if super::form::switch(ui, &mut auto, "Auto refresh") {
                        b.refresh_secs = if auto { 30 } else { 0 };
                        changed = true;
                    }
                    ui.label(RichText::new(if auto { format!("Auto refresh · {}s", b.refresh_secs) } else { "Auto refresh".into() }).color(pal.text_dim));
                });
            });
        });
        let hr = head.response.rect;
        ui.painter().hline(hr.x_range(), hr.bottom() - 0.5, Stroke::new(1.0, super::line()));

        if board_snapshot.widgets.is_empty() {
            super::empty_state(ui, Icon::Chart, "No widgets yet", "Add a scoreboard, table or chart backed by a query");
        }

        // Pack widgets into rows of three units; wide widgets take two.
        let mut rows: Vec<Vec<&MetricWidget>> = Vec::new();
        let mut used = 0;
        for w in &board_snapshot.widgets {
            let units = if w.wide { 2 } else { 1 };
            if rows.is_empty() || used + units > 3 {
                rows.push(Vec::new());
                used = 0;
            }
            rows.last_mut().expect("row exists").push(w);
            used += units;
        }
        let mut edit = None;
        let mut remove = None;
        let mut rerun = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                let gap = 10.0;
                let unit = (ui.available_width() - gap * 2.0) / 3.0;
                for (ri, row) in rows.into_iter().enumerate() {
                    // A row of stats is short; a row with a chart or table is tall.
                    let height = if row.iter().all(|w| w.kind == WidgetKind::Scoreboard) { 98.0 } else { 196.0 };
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (ci, w) in row.into_iter().enumerate() {
                            let width = if w.wide { unit * 2.0 + gap } else { unit };
                            // Tiles rise in one after another.
                            let t = super::motion::ease_out(super::motion::entered(ui.ctx(), egui::Id::unique(("tile-in", w.id)), super::motion::ENTER + 0.04 * (ri * 3 + ci) as f64));
                            ui.allocate_ui(vec2(width, height), |ui| {
                                ui.set_opacity(t);
                                egui::Frame::new().fill(pal.sidebar).stroke(Stroke::new(1.0, super::line())).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(12)).show(ui, |ui| ui.vertical(|ui| {
                                    ui.set_width(width - 26.0);
                                    ui.set_height(height - 26.0);
                                    let hovered = ui.rect_contains_pointer(ui.max_rect().expand(12.0));
                                    let tools = ui.ctx().animate_bool_with_time(egui::Id::unique(("tile-tools", w.id)), hovered, super::motion::FAST as f32);
                                    ui.horizontal(|ui| {
                                        ui.set_height(16.0);
                                        ui.label(RichText::new(&w.title).font(super::medium(12.0)).color(pal.text));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.set_opacity(tools);
                                            ui.spacing_mut().item_spacing.x = 0.0;
                                            if super::form::mini_icon(ui, Icon::Trash, "Remove widget").clicked() {
                                                remove = Some(w.id);
                                            }
                                            if super::form::mini_icon(ui, Icon::Edit, "Edit widget").clicked() {
                                                edit = Some(w.clone());
                                            }
                                            if super::form::mini_icon(ui, Icon::Refresh, "Refresh").clicked() {
                                                rerun = Some(w.clone());
                                            }
                                        });
                                    });
                                    ui.add_space(6.0);
                                    self.widget_body(ui, w, cx);
                                }));
                            });
                        }
                    });
                    ui.add_space(gap);
                }
            });
        });
        if let Some(w) = rerun {
            self.run_widget(&w, cx);
        }
        if let Some(w) = edit {
            self.editing = Some(w);
        }
        if let Some(id) = remove {
            boards[bi].widgets.retain(|w| w.id != id);
            changed = true;
        }
        if let Some(saved) = self.editor_ui(ui.ctx()) {
            let b = &mut boards[bi];
            match b.widgets.iter_mut().find(|w| w.id == saved.id) {
                Some(w) => *w = saved.clone(),
                None => b.widgets.push(saved.clone()),
            }
            self.run_widget(&saved, cx);
            changed = true;
        }
        if changed {
            let _ = store.save_boards(boards);
        }
    }

    fn widget_body(&mut self, ui: &mut egui::Ui, w: &MetricWidget, cx: &TabCtx) {
        let pal = p();
        let r = match self.data.get(&w.id) {
            None | Some(WidgetData::Loading(_)) => {
                ui.centered_and_justified(|ui| ui.add(egui::Spinner::new()));
                return;
            }
            Some(WidgetData::Failed(e)) => {
                ui.add(egui::Label::new(RichText::new(e).color(pal.danger).size(12.0)).wrap());
                return;
            }
            Some(WidgetData::Ready(r)) => r,
        };
        ui.set_opacity(super::motion::entered(ui.ctx(), egui::Id::unique(("tile-data", w.id)), super::motion::ENTER));
        if r.rows.is_empty() {
            ui.label(dim("No rows"));
            return;
        }
        match w.kind {
            WidgetKind::Scoreboard => {
                // Bento stat: the number leads, set in the display face, label underneath.
                // Stat: the number in mono, what it counts underneath.
                let v = r.cell(0, 0).unwrap_or("—").to_string();
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(v).font(super::mono_medium(32.0)).color(pal.text));
                ui.add_space(6.0);
                let mut sub = r.columns.first().cloned().unwrap_or_default().replace('_', " ");
                if let Some(extra) = r.cell(0, 1) {
                    sub = format!("{sub}, {} {extra}", r.columns.get(1).cloned().unwrap_or_default().replace('_', " "));
                }
                ui.label(RichText::new(sub).font(egui::FontId::monospace(12.0)).color(pal.text_faint));
            }
            WidgetKind::Table => {
                let gs = self.grids.entry(w.id).or_default();
                let opts = grid::GridOptions { id: egui::Id::unique(("metric-grid", w.id)), editable: false, null_text: "NULL", font_size: cx.settings.grid_font_size - 0.5, row_height: None };
                let src = ResultSource { meta: grid::result_meta(r), rows: &r.rows, sort: &[] };
                grid::show(ui, &src, gs, &opts);
            }
            WidgetKind::Bar => bars(ui, r),
            WidgetKind::Line => line_chart(ui, r),
            WidgetKind::Pie => pie(ui, r),
        }
    }

    fn editor_ui(&mut self, ctx: &egui::Context) -> Option<MetricWidget> {
        let w = self.editing.as_mut()?;
        let mut open = true;
        let mut saved = None;
        egui::Window::new("Widget").id(egui::Id::unique(("widget-editor", self.id))).open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
            ui.add(crate::ui::line_edit(&mut w.title).hint_text("Title").desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                for k in WidgetKind::ALL {
                    ui.selectable_value(&mut w.kind, k, k.label());
                }
            });
            ui.checkbox(&mut w.wide, "Double width");
            ui.label(faint(match w.kind {
                WidgetKind::Scoreboard => "First column of the first row is the big number.",
                WidgetKind::Table => "Shows the result as a table.",
                WidgetKind::Bar | WidgetKind::Pie => "First column is the label, second column the value.",
                WidgetKind::Line => "First column is the x axis; every numeric column after it becomes a line.",
            }));
            ui.add(egui::TextEdit::multiline(&mut w.sql).code_editor().desired_rows(6).desired_width(f32::INFINITY).hint_text("SELECT count(*) AS users FROM users"));
            ui.horizontal(|ui| {
                if primary_button(ui, "Save").clicked() {
                    saved = Some(w.clone());
                }
            });
        });
        if saved.is_some() || !open {
            self.editing = None;
        }
        saved
    }
}

/// Horizontal bars: label, a bar in the category's hue, the value. Up to eight categories.
fn bars(ui: &mut egui::Ui, r: &QueryResult) {
    let pal = p();
    let items: Vec<(String, f64, String)> = r
        .rows
        .iter()
        .filter_map(|row| {
            let raw = row.get(1).and_then(|v| v.as_deref()).or_else(|| row.first().and_then(|v| v.as_deref()));
            let y = numeric(raw)?;
            Some((row.first().cloned().flatten().unwrap_or_default(), y, raw.unwrap_or("").to_string()))
        })
        .take(8)
        .collect();
    let max = items.iter().map(|i| i.1).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
    let full = ui.available_width();
    let label_w = 72.0_f32.min(full * 0.3);
    let value_w = 72.0;
    for (i, (label, v, raw)) in items.iter().enumerate() {
        let (row, resp) = ui.allocate_exact_size(vec2(full, 22.0), egui::Sense::hover());
        let font = egui::FontId::monospace(12.0);
        let shown = super::home::ellipsize_to(ui.painter(), label, font.clone(), label_w - 8.0);
        ui.painter().text(egui::pos2(row.left(), row.center().y), egui::Align2::LEFT_CENTER, shown, font.clone(), pal.text);
        let track = egui::Rect::from_min_max(egui::pos2(row.left() + label_w, row.center().y - 4.0), egui::pos2(row.right() - value_w - 8.0, row.center().y + 4.0));
        // Bars grow from the baseline as the tile enters.
        let grow = super::motion::ease_out(super::motion::entered(ui.ctx(), resp.id.with("grow"), super::motion::SETTLE + 0.03 * i as f64));
        let w = (track.width() * (*v / max).max(0.0) as f32 * grow).max(2.0);
        ui.painter().rect_filled(egui::Rect::from_min_size(track.min, vec2(w, track.height())), egui::CornerRadius::same(2), pal.chart[i % 5]);
        ui.painter().text(egui::pos2(row.right(), row.center().y), egui::Align2::RIGHT_CENTER, raw, font, pal.text);
        resp.on_hover_text(format!("{label}: {raw}"));
    }
}

/// Line chart: two faint rules, 2 px lines, a dot on the last point, the first and last x labels.
fn line_chart(ui: &mut egui::Ui, r: &QueryResult) {
    let pal = p();
    let x_numeric = r.rows.iter().all(|row| numeric(row.first().and_then(|v| v.as_deref())).is_some());
    let series: Vec<usize> = (1..r.columns.len()).filter(|&c| r.rows.iter().any(|row| numeric(row.get(c).and_then(|v| v.as_deref())).is_some())).collect();
    if series.is_empty() || r.rows.len() < 2 {
        ui.label(dim("Needs a label column and at least one numeric column over two or more rows."));
        return;
    }
    let xs: Vec<f64> = r.rows.iter().enumerate().map(|(i, row)| if x_numeric { numeric(row.first().and_then(|v| v.as_deref())).unwrap_or(i as f64) } else { i as f64 }).collect();
    let (x0, x1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
    let vals = || series.iter().flat_map(|&c| r.rows.iter().filter_map(move |row| numeric(row.get(c).and_then(|v| v.as_deref()))));
    let (y0, y1) = (vals().fold(f64::MAX, f64::min), vals().fold(f64::MIN, f64::max));
    let (y0, y1) = if (y1 - y0).abs() < f64::EPSILON { (y0 - 1.0, y1 + 1.0) } else { (y0, y1) };
    let (area, resp) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    let plot = egui::Rect::from_min_max(area.min + vec2(0.0, 8.0), area.max - vec2(8.0, 22.0));
    for k in 1..=2 {
        let y = plot.top() + plot.height() * k as f32 / 3.0;
        ui.painter().hline(plot.x_range(), y, Stroke::new(1.0, super::line()));
    }
    let map = |x: f64, y: f64| egui::pos2(plot.left() + ((x - x0) / (x1 - x0).max(f64::EPSILON)) as f32 * plot.width(), plot.bottom() - ((y - y0) / (y1 - y0)) as f32 * plot.height());
    let reveal = super::motion::ease_out(super::motion::entered(ui.ctx(), resp.id.with("draw"), super::motion::ENTER));
    for (si, &c) in series.iter().enumerate() {
        let pts: Vec<egui::Pos2> = r.rows.iter().enumerate().filter_map(|(i, row)| Some(map(xs[i], numeric(row.get(c).and_then(|v| v.as_deref()))?))).collect();
        let n = ((pts.len() as f32 * reveal).ceil() as usize).clamp(2, pts.len());
        let color = pal.chart[si % 5];
        ui.painter().add(egui::Shape::line(pts[..n].to_vec(), Stroke::new(2.0, color)));
        if n == pts.len() {
            ui.painter().circle_filled(*pts.last().expect("two or more points"), 3.5, color);
        }
    }
    let label = |i: usize| r.rows.get(i).and_then(|row| row.first().cloned().flatten()).unwrap_or_default();
    let font = egui::FontId::monospace(12.0);
    ui.painter().text(egui::pos2(plot.left(), area.bottom()), egui::Align2::LEFT_BOTTOM, label(0), font.clone(), pal.text_faint);
    ui.painter().text(egui::pos2(plot.right(), area.bottom()), egui::Align2::RIGHT_BOTTOM, label(r.rows.len() - 1), font, pal.text_faint);
    if let Some(pos) = resp.hover_pos() {
        let i = ((pos.x - plot.left()) / plot.width() * (r.rows.len() - 1) as f32).round().clamp(0.0, (r.rows.len() - 1) as f32) as usize;
        let text: Vec<String> = series.iter().map(|&c| format!("{}: {}", r.columns[c], r.cell(i, c).unwrap_or("—"))).collect();
        resp.on_hover_text(format!("{}\n{}", label(i), text.join("\n")));
    }
}

fn pie(ui: &mut egui::Ui, r: &QueryResult) {
    let pal = p();
    let all: Vec<(String, f64)> = r.rows.iter().filter_map(|row| Some((row.first().cloned().flatten().unwrap_or_default(), numeric(row.get(1).and_then(|v| v.as_deref()))?.max(0.0)))).collect();
    let slices = fold_slices(all);
    let total: f64 = slices.iter().map(|s| s.1).sum();
    if total <= 0.0 {
        ui.label(dim("Nothing to chart"));
        return;
    }
    let size = ui.available_height().clamp(96.0, 180.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
        let c = rect.center();
        let radius = size / 2.0 - 4.0;
        let mut start = -std::f64::consts::FRAC_PI_2;
        for (i, (_, v)) in slices.iter().enumerate() {
            let sweep = v / total * std::f64::consts::TAU;
            let steps = ((sweep / 0.05).ceil() as usize).max(2);
            let mut pts = vec![c];
            for s in 0..=steps {
                let a = start + sweep * s as f64 / steps as f64;
                pts.push(c + vec2(a.cos() as f32, a.sin() as f32) * radius);
            }
            let color = slice_color(i, &slices);
            // Fan of triangles keeps each slice convex for the tessellator.
            for t in 1..pts.len() - 1 {
                ui.painter().add(egui::Shape::convex_polygon(vec![pts[0], pts[t], pts[t + 1]], color, Stroke::NONE));
            }
            start += sweep;
        }
        ui.painter().circle_filled(c, radius * 0.6, pal.sidebar);
        ui.vertical(|ui| {
            for (i, (label, v)) in slices.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, slice_color(i, &slices));
                    ui.label(RichText::new(format!("{label}  {:.1}%", v / total * 100.0)).size(12.0));
                });
            }
        });
    });
}

const OTHER: &str = "Other";

/// Five categorical hues at most: anything past the fourth slice folds into "Other".
fn fold_slices(mut all: Vec<(String, f64)>) -> Vec<(String, f64)> {
    if all.len() <= 5 {
        return all;
    }
    all.sort_by(|a, b| b.1.total_cmp(&a.1));
    let rest: f64 = all[4..].iter().map(|s| s.1).sum();
    all.truncate(4);
    all.push((OTHER.to_string(), rest));
    all
}

/// Slices keep their hue in fixed order; "Other" is neutral.
fn slice_color(i: usize, slices: &[(String, f64)]) -> Color32 {
    let pal = p();
    if slices.len() > 4 && i == 4 && slices[4].0 == OTHER { pal.text_faint } else { pal.chart[i.min(4)] }
}

#[cfg(test)]
mod tests {
    use super::fold_slices;

    #[test]
    fn more_than_five_slices_fold_the_smallest_into_other() {
        let s: Vec<(String, f64)> = [("a", 1.0), ("b", 9.0), ("c", 3.0), ("d", 7.0), ("e", 5.0), ("f", 2.0)].iter().map(|(n, v)| (n.to_string(), *v)).collect();
        let out = fold_slices(s);
        let names: Vec<&str> = out.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["b", "d", "e", "c", "Other"]);
        assert_eq!(out[4].1, 3.0);
    }

    #[test]
    fn five_or_fewer_slices_keep_their_order() {
        let s: Vec<(String, f64)> = [("x", 1.0), ("y", 2.0)].iter().map(|(n, v)| (n.to_string(), *v)).collect();
        assert_eq!(fold_slices(s.clone()), s);
    }
}
