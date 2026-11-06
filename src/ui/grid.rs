//! Virtualized data grid with inline editing, change tinting, sorting and context actions.

use super::p;
use crate::db::EditValue;
use chrono::Datelike;
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Key, Modifiers, Rect, Sense, Stroke, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Normal,
    Inserted,
    Deleted,
}

pub enum CellView<'a> {
    Value(Option<&'a str>),
    Edited(&'a EditValue),
}

#[derive(Debug, Clone, Default)]
pub struct ColMeta {
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
    pub foreign_key: bool,
    /// Shape of the loaded values, drawn under the header.
    pub profile: Option<ColProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileKind {
    #[default]
    Empty,
    Numeric,
    Temporal,
    Categorical,
    Unique,
    Text,
}

/// What a page of values looks like: null share, a 10-bar histogram or category frequencies.
#[derive(Debug, Clone, Default)]
pub struct ColProfile {
    pub kind: ProfileKind,
    pub null_frac: f32,
    pub distinct: usize,
    /// Bar heights in 0..=1.
    pub bars: Vec<f32>,
    /// Categories by descending frequency (categorical columns only).
    pub categories: Vec<(String, usize)>,
    /// Render values as colored pills (few short categories).
    pub pills: bool,
}

fn date_days(s: &str) -> Option<f64> {
    let d = chrono::NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()?;
    Some(d.num_days_from_ce() as f64 + s.get(11..13).and_then(|h| h.parse::<f64>().ok()).unwrap_or(0.0) / 24.0)
}

fn histogram(values: &[f64], bins: usize) -> Vec<f32> {
    let (lo, hi) = values.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    if values.is_empty() || !(hi > lo) {
        return vec![1.0; bins.min(values.len().max(1))];
    }
    let mut counts = vec![0usize; bins];
    for v in values {
        let i = (((v - lo) / (hi - lo)) * bins as f64).floor() as usize;
        counts[i.min(bins - 1)] += 1;
    }
    let max = *counts.iter().max().unwrap_or(&1) as f32;
    counts.into_iter().map(|c| c as f32 / max).collect()
}

/// Profiles one column of a result page.
pub fn profile_column(meta: &ColMeta, rows: &[Vec<Option<String>>], col: usize) -> ColProfile {
    use std::collections::HashMap;
    let total = rows.len();
    let vals: Vec<&str> = rows.iter().filter_map(|r| r.get(col).and_then(|v| v.as_deref())).collect();
    let mut p = ColProfile { null_frac: if total == 0 { 0.0 } else { 1.0 - vals.len() as f32 / total as f32 }, ..Default::default() };
    if vals.is_empty() {
        return p;
    }
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for v in &vals {
        *freq.entry(v).or_default() += 1;
    }
    p.distinct = freq.len();
    let nums: Vec<f64> = vals.iter().filter_map(|v| v.trim().parse::<f64>().ok()).collect();
    let dates: Vec<f64> = if nums.len() * 5 < vals.len() * 4 { vals.iter().filter_map(|v| date_days(v)).collect() } else { vec![] };
    let few = p.distinct <= 12 && vals.len() >= 4 && p.distinct * 2 <= vals.len();
    if meta.primary_key || (p.distinct == vals.len() && vals.len() > 12 && nums.len() * 5 < vals.len() * 4 && dates.len() * 5 < vals.len() * 4) {
        p.kind = ProfileKind::Unique;
        p.bars = vec![1.0; 10];
    } else if few && nums.len() * 5 < vals.len() * 4 {
        let mut cats: Vec<(String, usize)> = freq.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        cats.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let max = cats[0].1 as f32;
        p.bars = cats.iter().map(|(_, n)| *n as f32 / max).collect();
        p.pills = cats.len() <= 6 && cats.iter().all(|(c, _)| c.chars().count() <= 18 && !c.contains('\n'));
        p.categories = cats;
        p.kind = ProfileKind::Categorical;
    } else if nums.len() * 5 >= vals.len() * 4 {
        p.kind = ProfileKind::Numeric;
        p.bars = histogram(&nums, 10);
    } else if dates.len() * 5 >= vals.len() * 4 {
        p.kind = ProfileKind::Temporal;
        p.bars = histogram(&dates, 10);
    } else {
        p.kind = ProfileKind::Text;
        let lens: Vec<f64> = vals.iter().map(|v| v.chars().count() as f64).collect();
        p.bars = histogram(&lens, 10);
    }
    p
}

pub fn profile_all(meta: &mut [ColMeta], rows: &[Vec<Option<String>>]) {
    for c in 0..meta.len() {
        let prof = profile_column(&meta[c], rows, c);
        meta[c].profile = Some(prof);
    }
}

pub trait GridSource {
    fn columns(&self) -> &[ColMeta];
    fn row_count(&self) -> usize;
    fn cell(&self, row: usize, col: usize) -> CellView<'_>;
    fn row_state(&self, _row: usize) -> RowState {
        RowState::Normal
    }
    fn cell_modified(&self, _row: usize, _col: usize) -> bool {
        false
    }
    fn sort_of(&self, _col: usize) -> Option<bool> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFormat {
    Tsv,
    Csv,
    Json,
    SqlInsert,
    Markdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GridEvent {
    Edit { row: usize, col: usize, value: EditValue },
    Sort { col: usize, ascending: Option<bool>, add: bool },
    FollowForeignKey { row: usize, col: usize },
    CopyCells,
    CopyRows(CopyFormat),
    CopyColumnName(usize),
    DeleteRows,
    DuplicateRow,
    InsertRow,
    FilterByValue { col: usize, value: Option<String> },
    QuickLook { row: usize, col: usize },
    HideColumn(usize),
    SelectionChanged,
}

#[derive(Default)]
pub struct GridState {
    pub cursor: Option<(usize, usize)>,
    pub rows: BTreeSet<usize>,
    anchor: Option<usize>,
    editing: Option<(usize, usize, String, bool)>,
    focused: bool,
    pub hidden: BTreeSet<usize>,
    pub near_end: bool,
    scroll_to: Option<usize>,
    /// The gliding cursor ring: the cell it last sat on, and the offset it is travelling from.
    ring_cell: Option<(usize, usize)>,
    ring_last: Option<Rect>,
    ring_from: Option<(egui::Vec2, egui::Vec2, f64)>,
}

impl GridState {
    pub fn clear(&mut self) {
        self.cursor = None;
        self.rows.clear();
        self.anchor = None;
        self.editing = None;
    }

    pub fn select_row(&mut self, row: usize) {
        self.cursor = Some((row, self.cursor.map(|c| c.1).unwrap_or(0)));
        self.rows = [row].into();
        self.anchor = Some(row);
        self.scroll_to = Some(row);
        self.focused = true;
    }
}

fn is_numeric_type(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    ["int", "numeric", "decimal", "float", "double", "real", "serial", "money", "number"].iter().any(|k| t.contains(k))
}

pub fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(max).collect();
    if line.chars().count() > max || s.contains('\n') {
        out.push('…');
    }
    out
}

/// Cell borders: a light grey, a step brighter than the app's hairline so rows and columns read.
fn grid_line() -> Color32 {
    if p().dark { Color32::from_white_alpha(30) } else { Color32::from_black_alpha(26) }
}

/// Shown in an empty-string cell, in the NULL placeholder style.
pub const EMPTY_TEXT: &str = "empty";

pub struct GridOptions<'a> {
    pub id: egui::Id,
    pub editable: bool,
    pub null_text: &'a str,
    pub font_size: f32,
    /// Row height from the density control; `None` follows the font size.
    pub row_height: Option<f32>,
}

/// Keeps one trackpad gesture on one axis. A vertical swipe always drifts a little sideways, and
/// with the outer horizontal scroll area that drift slid the columns while scrolling rows.
fn lock_scroll_axis(ui: &Ui, id: egui::Id) {
    // Gap between scroll events that ends a gesture.
    const GESTURE_GAP: f64 = 0.25;
    let (d, now) = ui.input(|i| (i.smooth_scroll_delta, i.time));
    if d == egui::Vec2::ZERO || !ui.rect_contains_pointer(ui.max_rect()) {
        return;
    }
    let held: Option<(bool, f64)> = ui.ctx().data(|m| m.get_temp(id));
    let horizontal = match held {
        Some((h, last)) if now - last < GESTURE_GAP => h,
        _ => d.x.abs() > d.y.abs() * 2.0,
    };
    ui.ctx().input_mut(|i| if horizontal { i.smooth_scroll_delta.y = 0.0 } else { i.smooth_scroll_delta.x = 0.0 });
    ui.ctx().data_mut(|m| m.insert_temp(id, (horizontal, now)));
}

/// Where a cell's text sits and how it looks. Reading and typing share it, so nothing moves
/// when an edit starts: same rect, font, colour and alignment, only the ring and caret change.
struct CellLayout {
    font: FontId,
    color: Color32,
    right: bool,
}

impl CellLayout {
    fn for_column(meta: &ColMeta, font: &FontId, mono: &FontId) -> CellLayout {
        let t = meta.data_type.to_ascii_lowercase();
        let pal = p();
        if is_numeric_type(&t) && !meta.foreign_key {
            CellLayout { font: mono.clone(), color: pal.grid_number, right: true }
        } else if meta.foreign_key {
            CellLayout { font: mono.clone(), color: pal.grid_text, right: true }
        } else if t.contains("json") || t.contains("date") || t.contains("time") || t.contains("uuid") {
            CellLayout { font: mono.clone(), color: pal.text_dim, right: false }
        } else {
            CellLayout { font: font.clone(), color: pal.grid_text, right: false }
        }
    }

    /// The text box inside a cell: 12 px in from each side, one line tall, centred vertically.
    /// A foreign-key cell keeps room for its arrow whether or not it is shown.
    fn text_rect(&self, ui: &Ui, cell: Rect, fk: bool) -> Rect {
        let line = ui.fonts_mut(|f| f.row_height(&self.font));
        let inner = cell.with_max_x(cell.right() - if fk { 28.0 } else { 12.0 }).with_min_x(cell.left() + 12.0);
        Rect::from_center_size(inner.center(), vec2(inner.width().max(1.0), line))
    }

    fn anchor(&self, r: Rect) -> (egui::Pos2, Align2) {
        if self.right { (r.right_center(), Align2::RIGHT_CENTER) } else { (r.left_center(), Align2::LEFT_CENTER) }
    }
}

pub fn show(ui: &mut Ui, src: &dyn GridSource, st: &mut GridState, opt: &GridOptions) -> Vec<GridEvent> {
    let pal = p();
    let mut events = Vec::new();
    let cols = src.columns();
    let visible: Vec<usize> = (0..cols.len()).filter(|c| !st.hidden.contains(c)).collect();
    let n_rows = src.row_count();
    let font = FontId::proportional(opt.font_size);
    let mono = FontId::monospace(opt.font_size - 0.5);
    let row_h = opt.row_height.unwrap_or((opt.font_size + 19.0).round());
    let layouts: Vec<CellLayout> = cols.iter().map(|m| CellLayout::for_column(m, &font, &mono)).collect();
    // Screen rect of the cursor cell this frame, and the body's clip, for the gliding ring.
    let mut cursor_rect: Option<Rect> = None;
    let mut body_clip: Option<Rect> = None;

    keyboard(ui, src, st, opt, &visible, n_rows, &mut events);
    let shift_held = ui.input(|i| i.modifiers.shift);
    lock_scroll_axis(ui, opt.id.with("scroll_axis"));

    // TableBuilder only scrolls vertically; wide result sets scroll sideways in this outer area.
    egui::ScrollArea::horizontal().id_salt(opt.id.with("hscroll")).auto_shrink([false, false]).show(ui, |ui| {
        // Swiss grid: horizontal rules only. The resize handle still shows on hover and drag.
        ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;
        // Cells butt against each other so header fill, rules and selection run unbroken; text carries its own inset.
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let max_scroll_height = (ui.available_height() - 40.0 - ui.spacing().scroll.bar_width - 8.0).max(0.0);
        let mut table = TableBuilder::new(ui)
            .id_salt(opt.id)
            .striped(false)
            .resizable(true)
            .sense(Sense::click())
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .max_scroll_height(max_scroll_height)
            .stick_to_bottom(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        if let Some(r) = st.scroll_to.take() {
            table = table.scroll_to_row(r, None);
        }
        for (i, &c) in visible.iter().enumerate() {
            // Size from the header and a sample of values ("fit content").
            let sample = (0..n_rows.min(40))
                .map(|r| match src.cell(r, c) {
                    CellView::Value(Some(s)) => s.lines().next().unwrap_or("").chars().count(),
                    CellView::Edited(EditValue::Text(s)) => s.chars().count(),
                    _ => opts_null_len(opt),
                })
                .max()
                .unwrap_or(0);
            let header_chars = cols[c].name.chars().count() + 2;
            let w = sample.max(header_chars) as f32 * opt.font_size * 0.6 + 24.0 + if cols[c].foreign_key { 16.0 } else { 0.0 };
            // The last column takes the rest of the width, so the grid never stops short.
            table = if i + 1 == visible.len() {
                table.column(Column::remainder().at_least(w.clamp(70.0, 340.0)).clip(true))
            } else {
                table.column(Column::initial(w.clamp(70.0, 340.0)).at_least(40.0).clip(true).resizable(true))
            };
        }

        let scroll = table
            .header(40.0, |mut header| {
                for (vi, &c) in visible.iter().enumerate() {
                    let meta = &cols[c];
                    let lay = &layouts[c];
                    let (_, resp) = header.col(|ui| {
                        let r = ui.max_rect().expand2(vec2(4.0, 0.0));
                        ui.painter().rect_filled(r, CornerRadius::ZERO, pal.grid_header);
                        let hovered = ui.rect_contains_pointer(ui.max_rect());
                        if hovered {
                            ui.painter().rect_filled(ui.max_rect(), CornerRadius::ZERO, pal.surface);
                        }
                        let cell = ui.max_rect();
                        ui.painter().hline(r.x_range(), cell.bottom() - 0.5, Stroke::new(1.0, grid_line()));
                        if vi + 1 < visible.len() {
                            ui.painter().vline(cell.right() - 0.5, r.y_range(), Stroke::new(1.0, grid_line()));
                        }
                        // Name only: no type, no strip. Keys keep their colour.
                        let name_color = if meta.primary_key { super::pk_color() } else if meta.foreign_key { super::fk_color() } else { pal.grid_header_text };
                        let sorted = src.sort_of(c);
                        let inner = r.shrink2(vec2(12.0, 0.0)).with_max_x(r.right() - if sorted.is_some() { 26.0 } else { 12.0 });
                        let painter = ui.painter_at(inner);
                        let (pos, align) = lay.anchor(inner);
                        painter.text(pos, align, &meta.name, super::medium(opt.font_size), name_color);
                        if let Some(asc) = sorted {
                            let ar = Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(8.0, 8.0));
                            let pts = if asc { vec![ar.left_bottom(), ar.right_bottom(), ar.center_top()] } else { vec![ar.left_top(), ar.right_top(), ar.center_bottom()] };
                            ui.painter().add(egui::Shape::convex_polygon(pts, pal.text_dim, Stroke::NONE));
                        }
                    });
                    let resp = resp.on_hover_text(format!("{} · {}", meta.name, meta.data_type.to_lowercase()));
                    if resp.clicked() {
                        events.push(GridEvent::Sort { col: c, ascending: None, add: shift_held });
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Sort ascending").clicked() {
                            events.push(GridEvent::Sort { col: c, ascending: Some(true), add: false });
                            ui.close();
                        }
                        if ui.button("Sort descending").clicked() {
                            events.push(GridEvent::Sort { col: c, ascending: Some(false), add: false });
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Copy column name").clicked() {
                            events.push(GridEvent::CopyColumnName(c));
                            ui.close();
                        }
                        if ui.button("Hide this column").clicked() {
                            events.push(GridEvent::HideColumn(c));
                            ui.close();
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(row_h, n_rows, |mut row| {
                    let r = row.index();
                    let state = src.row_state(r);
                    let row_selected = st.rows.contains(&r);
                    let base_bg = match state {
                        RowState::Inserted => pal.inserted,
                        RowState::Deleted => pal.deleted,
                        RowState::Normal if row_selected => pal.grid_selection,
                        // Plain rows take the surface they sit on (ground in a tab, surface 1 in a bento tile).
                        RowState::Normal => egui::Color32::TRANSPARENT,
                    };
                    for (vi, &c) in visible.iter().enumerate() {
                        let meta = &cols[c];
                        let lay = &layouts[c];
                        let is_cursor = st.cursor == Some((r, c));
                        let editing_here = matches!(&st.editing, Some((er, ec, _, _)) if *er == r && *ec == c);
                        let (_, resp) = row.col(|ui| {
                            let rect = ui.max_rect().expand2(vec2(4.0, 1.0));
                            body_clip.get_or_insert(ui.clip_rect());
                            let modified = src.cell_modified(r, c) && state == RowState::Normal;
                            ui.painter().rect_filled(rect, CornerRadius::ZERO, if modified { pal.modified } else { base_bg });
                            // Hairline borders: under each row, and between columns (not after the last).
                            // Drawn inside the cell's clip: on its edge they were half cut off.
                            let cell = ui.max_rect();
                            ui.painter().hline(rect.x_range(), cell.bottom() - 0.5, Stroke::new(1.0, grid_line()));
                            if vi + 1 < visible.len() {
                                ui.painter().vline(cell.right() - 0.5, rect.y_range(), Stroke::new(1.0, grid_line()));
                            }
                            if vi == 0 && state == RowState::Inserted {
                                ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), CornerRadius::ZERO, pal.text_dim);
                            }
                            if modified {
                                let t = rect.left_top();
                                ui.painter().add(egui::Shape::convex_polygon(vec![t, t + vec2(6.0, 0.0), t + vec2(0.0, 6.0)], pal.accent, Stroke::NONE));
                            }
                            if is_cursor && st.focused {
                                cursor_rect = Some(rect.shrink(1.0));
                            }
                            let text_rect = lay.text_rect(ui, rect, meta.foreign_key);
                            if editing_here {
                                if let Some((_, _, buf, first)) = &mut st.editing {
                                    let id = opt.id.with(("edit", r, c));
                                    let te = crate::ui::line_edit(buf)
                                        .id(id)
                                        .font(lay.font.clone())
                                        .text_color(lay.color)
                                        .frame(egui::Frame::NONE)
                                        .margin(egui::Margin::ZERO)
                                        .align(if lay.right { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER })
                                        .desired_width(text_rect.width());
                                    let out = ui.put(text_rect, te);
                                    if *first {
                                        out.request_focus();
                                        *first = false;
                                    }
                                }
                                cursor_rect = Some(rect.shrink(1.0));
                                return;
                            }
                            let (text, color, f) = match src.cell(r, c) {
                                CellView::Value(None) => (opt.null_text.to_string(), pal.grid_placeholder, mono.clone()),
                                // An empty string is a value, not NULL: say so in the placeholder style, lowercase so the two never read alike.
                                CellView::Value(Some("")) => (EMPTY_TEXT.into(), pal.grid_placeholder, mono.clone()),
                                CellView::Edited(EditValue::Text(s)) if s.is_empty() => (EMPTY_TEXT.into(), pal.grid_placeholder, mono.clone()),
                                CellView::Value(Some(s)) => (first_line(s, 300), lay.color, lay.font.clone()),
                                CellView::Edited(EditValue::Null) => (opt.null_text.to_string(), pal.grid_placeholder, mono.clone()),
                                CellView::Edited(EditValue::Default) => ("DEFAULT".into(), pal.grid_placeholder, mono.clone()),
                                CellView::Edited(EditValue::Text(s)) => (first_line(s, 300), lay.color, lay.font.clone()),
                            };
                            let linked = meta.foreign_key && state != RowState::Deleted && match src.cell(r, c) { CellView::Value(Some(s)) => !s.is_empty(), CellView::Edited(EditValue::Text(s)) => !s.is_empty(), _ => false };
                            if linked {
                                let arrow = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(20.0, rect.height().min(24.0)));
                                let response = ui.interact(arrow, opt.id.with(("foreign-key", r, c)), Sense::click());
                                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Open referenced row"));
                                if response.hovered() {
                                    ui.painter().rect_filled(arrow, CornerRadius::same(4), pal.surface_hover);
                                }
                                // ↗: the row this value points at.
                                let a = Rect::from_center_size(arrow.center(), vec2(9.0, 9.0));
                                let st = Stroke::new(1.5, super::fk_color());
                                ui.painter().line_segment([a.left_bottom(), a.right_top()], st);
                                ui.painter().add(egui::Shape::line(vec![a.left_top() + vec2(2.0, 0.0), a.right_top(), a.right_bottom() - vec2(0.0, 2.0)], st));
                                if response.on_hover_text("Open referenced row").clicked() {
                                    events.push(GridEvent::FollowForeignKey { row: r, col: c });
                                }
                            }
                            let painter = ui.painter_at(text_rect.expand2(vec2(0.0, 4.0)));
                            let (pos, align) = lay.anchor(text_rect);
                            let drawn = painter.text(pos, align, &text, f, color);
                            if state == RowState::Deleted {
                                let w = drawn.width().min(text_rect.width());
                                let x0 = if lay.right { text_rect.right() - w } else { text_rect.left() };
                                painter.line_segment([pos2(x0, pos.y), pos2(x0 + w, pos.y)], Stroke::new(1.0, pal.danger));
                            }
                        });
                        if editing_here {
                            continue;
                        }
                        if resp.clicked() {
                            click(st, r, c, resp.ctx.input(|i| i.modifiers));
                            events.push(GridEvent::SelectionChanged);
                        }
                        if resp.double_clicked() && opt.editable && state != RowState::Deleted {
                            st.editing = Some((r, c, cell_string(src, r, c), true));
                        }
                        if resp.secondary_clicked() && !st.rows.contains(&r) {
                            click(st, r, c, Modifiers::NONE);
                            events.push(GridEvent::SelectionChanged);
                        }
                        resp.context_menu(|ui| context_menu(ui, src, st, opt, r, c, &mut events));
                    }
                });
            });
        st.near_end = scroll.content_size.y - scroll.state.clamped_offset().y - scroll.inner_rect.height() <= row_h * 8.0;
    });

    paint_cursor_ring(ui, st, cursor_rect, body_clip);

    // Finish an inline edit on Enter / focus loss; cancel on Escape.
    if let Some((r, c, buf, first)) = st.editing.clone() {
        let id = opt.id.with(("edit", r, c));
        let has_focus = ui.memory(|m| m.has_focus(id));
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let tab = ui.input(|i| i.key_pressed(Key::Tab));
        let esc = ui.input(|i| i.key_pressed(Key::Escape));
        if esc {
            st.editing = None;
        } else if !first && (!has_focus || enter || tab) {
            let original = cell_string(src, r, c);
            let was_null = matches!(src.cell(r, c), CellView::Value(None) | CellView::Edited(EditValue::Null));
            if buf != original || (was_null && !buf.is_empty()) {
                events.push(GridEvent::Edit { row: r, col: c, value: EditValue::Text(buf) });
            }
            st.editing = None;
            if tab {
                if let Some(pos) = visible.iter().position(|&x| x == c) {
                    if let Some(&next) = visible.get(pos + 1) {
                        st.cursor = Some((r, next));
                        st.editing = Some((r, next, cell_string(src, r, next), true));
                    }
                }
            }
        }
    }
    events
}

/// Draws the cell cursor on top of the grid. When it moves to a new cell it glides there over
/// `motion::FAST`, measured as an offset from the new cell so scrolling never makes it lag.
fn paint_cursor_ring(ui: &Ui, st: &mut GridState, target: Option<Rect>, clip: Option<Rect>) {
    let (Some(target), Some(clip)) = (target, clip) else {
        st.ring_last = None;
        return;
    };
    let now = ui.input(|i| i.time);
    if st.ring_cell != st.cursor {
        if let (Some(last), Some(_)) = (st.ring_last, st.ring_cell) {
            st.ring_from = Some((last.min - target.min, last.size() - target.size(), now));
        }
        st.ring_cell = st.cursor;
    }
    let mut rect = target;
    if let Some((dpos, dsize, t0)) = st.ring_from {
        let t = super::motion::progress(now - t0, super::motion::FAST);
        if t >= 1.0 {
            st.ring_from = None;
        } else {
            let k = 1.0 - super::motion::ease_out(t);
            rect = Rect::from_min_size(target.min + dpos * k, target.size() + dsize * k);
            ui.ctx().request_repaint();
        }
    }
    st.ring_last = Some(target);
    let color = if st.editing.is_some() { p().accent } else { p().grid_selection_border };
    ui.painter().with_clip_rect(clip).rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.5, color), egui::StrokeKind::Inside);
}

fn opts_null_len(opt: &GridOptions) -> usize {
    opt.null_text.chars().count()
}

pub fn cell_string(src: &dyn GridSource, r: usize, c: usize) -> String {
    match src.cell(r, c) {
        CellView::Value(v) => v.unwrap_or("").to_string(),
        CellView::Edited(EditValue::Text(s)) => s.clone(),
        CellView::Edited(_) => String::new(),
    }
}

pub fn cell_value(src: &dyn GridSource, r: usize, c: usize) -> Option<String> {
    match src.cell(r, c) {
        CellView::Value(v) => v.map(str::to_string),
        CellView::Edited(EditValue::Text(s)) => Some(s.clone()),
        CellView::Edited(EditValue::Null) => None,
        CellView::Edited(EditValue::Default) => Some("DEFAULT".into()),
    }
}

fn click(st: &mut GridState, r: usize, c: usize, m: Modifiers) {
    st.focused = true;
    st.editing = None;
    if m.shift {
        let a = st.anchor.unwrap_or(r);
        st.rows = (a.min(r)..=a.max(r)).collect();
    } else if m.command {
        if !st.rows.insert(r) {
            st.rows.remove(&r);
        }
        st.anchor = Some(r);
    } else {
        st.rows = [r].into();
        st.anchor = Some(r);
    }
    st.cursor = Some((r, c));
}

fn keyboard(ui: &mut Ui, src: &dyn GridSource, st: &mut GridState, opt: &GridOptions, visible: &[usize], n_rows: usize, events: &mut Vec<GridEvent>) {
    if st.editing.is_some() || !st.focused || n_rows == 0 {
        return;
    }
    // Another widget (search, editor) owns the keyboard.
    if ui.memory(|m| m.focused().is_some()) {
        return;
    }
    if ui.input(|i| i.pointer.any_pressed()) && !ui.ui_contains_pointer() {
        st.focused = false;
        return;
    }
    let Some((r, c)) = st.cursor else { return };
    let vi = visible.iter().position(|&x| x == c).unwrap_or(0);
    let mut moved = None;
    ui.input_mut(|i| {
        let shift = i.modifiers.shift;
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::SHIFT, Key::ArrowDown) {
            moved = Some(((r + 1).min(n_rows - 1), c, shift));
        } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::SHIFT, Key::ArrowUp) {
            moved = Some((r.saturating_sub(1), c, shift));
        } else if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
            moved = Some((r, visible.get(vi + 1).copied().unwrap_or(c), false));
        } else if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
            moved = Some((r, visible.get(vi.saturating_sub(1)).copied().unwrap_or(c), false));
        } else if i.consume_key(Modifiers::NONE, Key::PageDown) {
            moved = Some(((r + 20).min(n_rows - 1), c, false));
        } else if i.consume_key(Modifiers::NONE, Key::PageUp) {
            moved = Some((r.saturating_sub(20), c, false));
        }
    });
    if let Some((nr, nc, shift)) = moved {
        if shift {
            let a = st.anchor.unwrap_or(r);
            st.rows = (a.min(nr)..=a.max(nr)).collect();
        } else {
            st.rows = [nr].into();
            st.anchor = Some(nr);
        }
        st.cursor = Some((nr, nc));
        st.scroll_to = Some(nr);
        events.push(GridEvent::SelectionChanged);
        return;
    }
    if opt.editable && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) && src.row_state(r) != RowState::Deleted {
        st.editing = Some((r, c, cell_string(src, r, c), true));
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space)) {
        events.push(GridEvent::QuickLook { row: r, col: c });
    }
    if opt.editable && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace)) {
        events.push(GridEvent::DeleteRows);
    }
    if ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
        events.push(GridEvent::CopyCells);
    }
    if opt.editable {
        let pasted = ui.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(s) = e { Some(s.clone()) } else { None }));
        if let Some(s) = pasted {
            // Paste a TSV block starting at the cursor cell.
            for (dr, line) in s.trim_end_matches('\n').lines().enumerate() {
                for (dc, val) in line.split('\t').enumerate() {
                    let (Some(&col), row) = (visible.get(vi + dc), r + dr) else { continue };
                    if row < n_rows {
                        let v = if val == opt.null_text { EditValue::Null } else { EditValue::Text(val.to_string()) };
                        events.push(GridEvent::Edit { row, col, value: v });
                    }
                }
            }
        }
    }
}

fn context_menu(ui: &mut Ui, src: &dyn GridSource, st: &mut GridState, opt: &GridOptions, r: usize, c: usize, events: &mut Vec<GridEvent>) {
    ui.set_min_width(210.0);
    fn item(ui: &mut Ui, events: &mut Vec<GridEvent>, label: &str, shortcut: &str, ev: GridEvent) {
        let resp = ui.add(egui::Button::new(label).shortcut_text(shortcut));
        if resp.clicked() {
            events.push(ev);
            ui.close();
        }
    }
    item(ui, events, "Copy", "⌘C", GridEvent::CopyCells);
    ui.menu_button("Copy rows as", |ui| {
        for (f, l) in [(CopyFormat::Tsv, "Tab-separated"), (CopyFormat::Csv, "CSV"), (CopyFormat::Json, "JSON"), (CopyFormat::SqlInsert, "SQL INSERT"), (CopyFormat::Markdown, "Markdown table")] {
            if ui.button(l).clicked() {
                events.push(GridEvent::CopyRows(f));
                ui.close();
            }
        }
    });
    item(ui, events, "Quick look", "Space", GridEvent::QuickLook { row: r, col: c });
    ui.separator();
    item(ui, events, "Filter with this value", "", GridEvent::FilterByValue { col: c, value: cell_value(src, r, c) });
    if src.columns()[c].foreign_key {
        item(ui, events, "Open referenced row", "", GridEvent::FollowForeignKey { row: r, col: c });
    }
    if opt.editable {
        ui.separator();
        item(ui, events, "Edit cell", "↩", GridEvent::QuickLook { row: usize::MAX, col: c });
        item(ui, events, "Set NULL", "", GridEvent::Edit { row: r, col: c, value: EditValue::Null });
        item(ui, events, "Set EMPTY", "", GridEvent::Edit { row: r, col: c, value: EditValue::Text(String::new()) });
        item(ui, events, "Set DEFAULT", "", GridEvent::Edit { row: r, col: c, value: EditValue::Default });
        ui.separator();
        item(ui, events, "Insert row", "⌘I", GridEvent::InsertRow);
        item(ui, events, "Duplicate row", "⌘D", GridEvent::DuplicateRow);
        item(ui, events, if st.rows.len() > 1 { "Delete rows" } else { "Delete row" }, "⌫", GridEvent::DeleteRows);
    }
    // "Edit cell" is routed through QuickLook with row = MAX to keep the event enum small.
    if let Some(GridEvent::QuickLook { row: usize::MAX, col }) = events.last().cloned() {
        events.pop();
        st.editing = Some((r, col, cell_string(src, r, col), true));
    }
}

/// Serializes the selected rows for the clipboard.
pub fn format_rows(src: &dyn GridSource, rows: &[usize], fmt: CopyFormat, null_text: &str, table: Option<(&crate::model::Dialect, &crate::db::TableRef)>) -> String {
    let cols: Vec<&ColMeta> = src.columns().iter().collect();
    let names: Vec<String> = cols.iter().map(|c| c.name.clone()).collect();
    let data: Vec<Vec<Option<String>>> = rows.iter().map(|&r| (0..cols.len()).map(|c| cell_value(src, r, c)).collect()).collect();
    crate::export::render_text(&names, &data, fmt, null_text, table)
}

pub fn selection_rows(st: &GridState) -> Vec<usize> {
    if st.rows.is_empty() {
        st.cursor.map(|c| vec![c.0]).unwrap_or_default()
    } else {
        st.rows.iter().copied().collect()
    }
}

/// A read-only source over a plain result set.
pub struct ResultSource<'a> {
    pub meta: Vec<ColMeta>,
    pub rows: &'a [Vec<Option<String>>],
    pub sort: &'a [(usize, bool)],
}

impl GridSource for ResultSource<'_> {
    fn columns(&self) -> &[ColMeta] {
        &self.meta
    }
    fn row_count(&self) -> usize {
        self.rows.len()
    }
    fn cell(&self, row: usize, col: usize) -> CellView<'_> {
        CellView::Value(self.rows.get(row).and_then(|r| r.get(col)).and_then(|v| v.as_deref()))
    }
    fn sort_of(&self, col: usize) -> Option<bool> {
        self.sort.iter().find(|(c, _)| *c == col).map(|(_, a)| *a)
    }
}

pub fn result_meta(r: &crate::db::QueryResult) -> Vec<ColMeta> {
    r.columns
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut data_type = r.types.get(i).cloned().unwrap_or_default();
            // Untyped results (SQLite expressions): a column whose sampled values are all numbers reads as numeric.
            if data_type.is_empty() || data_type.eq_ignore_ascii_case("text") && n.contains('(') {
                let mut seen = 0;
                let all_numbers = r.rows.iter().take(50).filter_map(|row| row.get(i).cloned().flatten()).all(|v| {
                    seen += 1;
                    v.trim().parse::<f64>().is_ok()
                });
                if all_numbers && seen > 0 {
                    data_type = "numeric".into();
                }
            }
            ColMeta { name: n.clone(), data_type, ..Default::default() }
        })
        .collect()
}

/// Sorts a result set locally (query results are not re-queried for sorting).
pub fn sort_rows(rows: &mut [Vec<Option<String>>], col: usize, asc: bool) {
    rows.sort_by(|a, b| {
        let (x, y) = (a.get(col).cloned().flatten(), b.get(col).cloned().flatten());
        let o = match (x.as_deref().and_then(|s| s.parse::<f64>().ok()), y.as_deref().and_then(|s| s.parse::<f64>().ok())) {
            (Some(p), Some(q)) => p.partial_cmp(&q).unwrap_or(std::cmp::Ordering::Equal),
            _ => x.cmp(&y),
        };
        if asc { o } else { o.reverse() }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ForeignKeySource { columns: Vec<ColMeta> }

    impl GridSource for ForeignKeySource {
        fn columns(&self) -> &[ColMeta] { &self.columns }
        fn row_count(&self) -> usize { 1 }
        fn cell(&self, _: usize, _: usize) -> CellView<'_> { CellView::Value(Some("2281")) }
    }

    fn foreign_key_frame(ctx: &egui::Context, state: &mut GridState, time: f64, events: Vec<egui::Event>) -> Vec<GridEvent> {
        let source = ForeignKeySource { columns: vec![ColMeta { name: "customer_id".into(), data_type: "integer".into(), foreign_key: true, ..Default::default() }] };
        let options = GridOptions { id: egui::Id::unique("fk-regression"), editable: true, null_text: "NULL", font_size: 13.0, row_height: None };
        let mut result = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 400.0))), time: Some(time), events, ..Default::default() }, |ui| {
            egui::CentralPanel::default().show(ui, |ui| { result = show(ui, &source, state, &options); });
        });
        output.textures_delta.clear();
        result
    }

    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    #[test]
    fn foreign_key_value_edits_while_arrow_navigates() {
        let ctx = egui::Context::default();
        let mut state = GridState::default();
        foreign_key_frame(&ctx, &mut state, 0.0, vec![]);
        foreign_key_frame(&ctx, &mut state, 0.01, vec![]);
        let arrow = ctx.read_response(egui::Id::unique("fk-regression").with(("foreign-key", 0usize, 0usize))).expect("foreign-key arrow rendered").rect;
        let value = pos2(arrow.left() - 30.0, arrow.center().y);
        for (time, pressed) in [(0.1, true), (0.12, false), (0.2, true), (0.22, false)] {
            let events = foreign_key_frame(&ctx, &mut state, time, vec![egui::Event::PointerMoved(value), pointer_button(value, pressed)]);
            assert!(!events.iter().any(|event| matches!(event, GridEvent::FollowForeignKey { .. })), "clicking a value must keep the current table open");
        }
        assert!(matches!(state.editing, Some((0, 0, _, _))), "double-clicking the value must start inline editing");
        state.editing = None;
        foreign_key_frame(&ctx, &mut state, 0.5, vec![]);
        foreign_key_frame(&ctx, &mut state, 0.6, vec![egui::Event::PointerMoved(arrow.center()), pointer_button(arrow.center(), true)]);
        let events = foreign_key_frame(&ctx, &mut state, 0.62, vec![pointer_button(arrow.center(), false)]);
        assert!(events.contains(&GridEvent::FollowForeignKey { row: 0, col: 0 }), "the arrow must still open the related row");
    }

    #[test]
    fn numeric_sort_beats_lexical() {
        let mut rows = vec![vec![Some("10".into())], vec![Some("9".into())], vec![None], vec![Some("100".into())]];
        sort_rows(&mut rows, 0, true);
        let got: Vec<Option<&str>> = rows.iter().map(|r| r[0].as_deref()).collect();
        assert_eq!(got, vec![None, Some("9"), Some("10"), Some("100")]);
    }

    #[test]
    fn first_line_marks_truncation() {
        assert_eq!(first_line("abc\ndef", 10), "abc…");
        assert_eq!(first_line("abcdef", 3), "abc…");
        assert_eq!(first_line("ok", 3), "ok");
    }

    #[test]
    fn shift_click_selects_range() {
        let mut st = GridState::default();
        click(&mut st, 2, 0, Modifiers::NONE);
        click(&mut st, 5, 0, Modifiers::SHIFT);
        assert_eq!(st.rows.iter().copied().collect::<Vec<_>>(), vec![2, 3, 4, 5]);
        click(&mut st, 3, 0, Modifiers::COMMAND);
        assert!(!st.rows.contains(&3));
    }
}
