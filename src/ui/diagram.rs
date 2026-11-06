//! Schema maps. "Graph" lays tables out as nodes joined by their foreign keys (force-directed,
//! layered or circular) with neighbourhood focus; "ER diagram" shows column cards with links.

use super::workspace::{TabCtx, WsAction};
use super::{dim, mix, p, paint_icon, short_count, Icon};
use crate::db::{ColumnInfo, DbObject, ObjectKind, Relation, TableStat};
use crate::runtime::{take, Task};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Graph,
    Er,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Force,
    Layers,
    Circle,
}

struct Node {
    obj: DbObject,
    cols: Vec<ColumnInfo>,
    rows: Option<u64>,
    /// Graph position (world units).
    pos: Pos2,
    vel: Vec2,
    pinned: bool,
    /// ER card position.
    card: Pos2,
    /// ER card width: the widest card in its layout column, so a column lines up.
    card_w: f32,
}

struct Loaded {
    tables: Vec<(DbObject, Vec<ColumnInfo>)>,
    relations: Vec<Relation>,
    stats: Vec<TableStat>,
}

pub struct DiagramTab {
    schema: String,
    tables: Vec<DbObject>,
    task: Option<Task<anyhow::Result<Loaded>>>,
    nodes: Vec<Node>,
    relations: Vec<Relation>,
    /// (from, to, column pairs) between node indexes.
    edges: Vec<(usize, usize, Vec<String>)>,
    mode: Mode,
    layout: Layout,
    pan: Vec2,
    zoom: f32,
    dragging: Option<usize>,
    selected: Option<usize>,
    hovered: Option<usize>,
    depth: usize,
    hide_isolated: bool,
    search: String,
    /// Remaining force-layout steps (animates the settle).
    energy: u32,
    fit_pending: bool,
    error: Option<String>,
}

const NODE_W: f32 = 220.0;
const ROW_H: f32 = 19.0;
const HEAD_H: f32 = 30.0;
const MAX_TABLES: usize = 400;
const MAX_ER_COLUMNS: usize = 120;

impl DiagramTab {
    pub fn new(schema: String, tables: Vec<DbObject>) -> Self {
        Self {
            schema,
            tables,
            task: None,
            nodes: vec![],
            relations: vec![],
            edges: vec![],
            mode: Mode::Graph,
            layout: Layout::Force,
            pan: Vec2::ZERO,
            zoom: 1.0,
            dragging: None,
            selected: None,
            hovered: None,
            depth: 1,
            hide_isolated: false,
            search: String::new(),
            energy: 0,
            fit_pending: true,
            error: None,
        }
    }

    pub fn title(&self) -> String {
        format!("{} map", if self.schema.is_empty() { "Schema".to_string() } else { self.schema.clone() })
    }

    fn load(&mut self, cx: &TabCtx) {
        let tables: Vec<DbObject> = self.tables.iter().take(MAX_TABLES).cloned().collect();
        let schema = self.schema.clone();
        self.task = Some(cx.session.spawn(cx.ctx, move |d| async move {
            let mut d = d.lock().await;
            let mut relations = d.relations(&schema).await.unwrap_or_default();
            let stats = d.table_stats(&schema).await.unwrap_or_default();
            let all_cols = d.all_columns(&schema).await.unwrap_or_default();
            let mut by_table: HashMap<String, Vec<ColumnInfo>> = HashMap::new();
            for (t, c) in all_cols {
                by_table.entry(t).or_default().push(ColumnInfo { name: c, ..Default::default() });
            }
            let mut out = Vec::new();
            // Full column details (types, keys) only for schemas small enough to draw as cards.
            let detailed = tables.len() <= MAX_ER_COLUMNS;
            for t in tables {
                let r = super::table_tab::table_ref(&t);
                let cols = if detailed { d.columns(&r).await.unwrap_or_default() } else { by_table.remove(&t.name).unwrap_or_default() };
                if relations.is_empty() {
                    // Engines without a schema-wide FK listing: ask per table.
                    for fk in d.foreign_keys(&r).await.unwrap_or_default() {
                        relations.push(Relation { name: fk.name, schema: t.schema.clone(), table: t.name.clone(), column: fk.column, ref_schema: fk.ref_schema, ref_table: fk.ref_table, ref_column: fk.ref_column, on_delete: String::new() });
                    }
                }
                out.push((t, cols));
            }
            Ok(Loaded { tables: out, relations, stats })
        }));
    }

    fn ingest(&mut self, l: Loaded) {
        let stats: HashMap<String, TableStat> = l.stats.into_iter().map(|s| (s.name.clone(), s)).collect();
        let n = l.tables.len().max(1) as f32;
        self.nodes = l
            .tables
            .into_iter()
            .enumerate()
            .map(|(i, (obj, cols))| {
                // Seed on a spiral so the force layout starts untangled and deterministic.
                let a = i as f32 * 2.399_963;
                let r = 40.0 * (i as f32).sqrt() + 20.0;
                Node { rows: stats.get(&obj.name).and_then(|s| s.rows), obj, cols, pos: pos2(a.cos() * r, a.sin() * r), vel: Vec2::ZERO, pinned: false, card: Pos2::ZERO, card_w: NODE_W }
            })
            .collect();
        let index: HashMap<&str, usize> = self.nodes.iter().enumerate().map(|(i, n)| (n.obj.name.as_str(), i)).collect();
        let mut grouped: HashMap<(usize, usize), Vec<String>> = HashMap::new();
        for r in &l.relations {
            if let (Some(&a), Some(&b)) = (index.get(r.table.as_str()), index.get(r.ref_table.as_str())) {
                grouped.entry((a, b)).or_default().push(format!("{} → {}", r.column, r.ref_column));
            }
        }
        let mut edges: Vec<(usize, usize, Vec<String>)> = grouped.into_iter().map(|((a, b), c)| (a, b, c)).collect();
        edges.sort_by_key(|e| (e.0, e.1));
        self.edges = edges;
        self.relations = l.relations;
        let _ = n;
        self.apply_layout();
        self.layout_cards();
    }

    fn degree(&self, i: usize) -> usize {
        self.edges.iter().filter(|e| e.0 == i || e.1 == i).count()
    }

    fn neighbours(&self, start: usize, depth: usize) -> HashSet<usize> {
        let mut seen: HashSet<usize> = [start].into();
        let mut q: VecDeque<(usize, usize)> = [(start, 0)].into();
        while let Some((i, d)) = q.pop_front() {
            if d == depth {
                continue;
            }
            for e in &self.edges {
                let other = if e.0 == i { e.1 } else if e.1 == i { e.0 } else { continue };
                if seen.insert(other) {
                    q.push_back((other, d + 1));
                }
            }
        }
        seen
    }

    /// Longest-path layering: referenced tables (parents) above the tables that point at them.
    fn layers(&self) -> Vec<usize> {
        let n = self.nodes.len();
        let mut level = vec![0usize; n];
        for _ in 0..n.min(64) {
            let mut changed = false;
            for (from, to, _) in &self.edges {
                if from != to && level[*from] < level[*to] + 1 {
                    level[*from] = level[*to] + 1;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        level
    }

    fn apply_layout(&mut self) {
        match self.layout {
            Layout::Force => {
                self.energy = 600;
                self.shelve_unlinked();
            }
            Layout::Layers => {
                let level = self.layers();
                let mut per: HashMap<usize, Vec<usize>> = HashMap::new();
                for (i, l) in level.iter().enumerate() {
                    per.entry(*l).or_default().push(i);
                }
                for (l, items) in per {
                    let w = items.len() as f32 * 150.0;
                    for (k, i) in items.into_iter().enumerate() {
                        self.nodes[i].pos = pos2(k as f32 * 150.0 - w / 2.0, l as f32 * 140.0);
                        self.nodes[i].vel = Vec2::ZERO;
                    }
                }
                self.energy = 0;
            }
            Layout::Circle => {
                let n = self.nodes.len().max(1) as f32;
                let r = (n * 34.0).max(160.0);
                let mut order: Vec<usize> = (0..self.nodes.len()).collect();
                order.sort_by_key(|&i| std::cmp::Reverse(self.degree(i)));
                for (k, i) in order.into_iter().enumerate() {
                    let a = k as f32 / n * std::f32::consts::TAU;
                    self.nodes[i].pos = pos2(a.cos() * r, a.sin() * r);
                    self.nodes[i].vel = Vec2::ZERO;
                }
                self.energy = 0;
            }
        }
        self.fit_pending = true;
    }

    /// One Fruchterman–Reingold style step with cooling. Unlinked tables don't take part; they are
    /// shelved in rows under the linked cluster so they can't stretch the view.
    fn step(&mut self) {
        let n = self.nodes.len();
        if n == 0 {
            return;
        }
        let linked: Vec<bool> = (0..n).map(|i| self.degree(i) > 0).collect();
        let k = 150.0f32;
        let mut force = vec![Vec2::ZERO; n];
        for i in 0..n {
            if !linked[i] {
                continue;
            }
            for j in (i + 1)..n {
                if !linked[j] {
                    continue;
                }
                let d = self.nodes[i].pos - self.nodes[j].pos;
                let dist = d.length().max(5.0);
                if dist > k * 4.0 {
                    continue;
                }
                let f = d / dist * (k * k / dist);
                force[i] += f;
                force[j] -= f;
            }
        }
        for (a, b, _) in &self.edges {
            if a == b {
                continue;
            }
            let d = self.nodes[*a].pos - self.nodes[*b].pos;
            let dist = d.length().max(1.0);
            let f = d / dist * (dist * dist / k) * 0.6;
            force[*a] -= f;
            force[*b] += f;
        }
        let t = (self.energy as f32 / 600.0).max(0.02) * 22.0;
        for (i, node) in self.nodes.iter_mut().enumerate() {
            if node.pinned || !linked[i] {
                continue;
            }
            // A pull to the origin keeps separate components close together.
            let f = force[i] - node.pos.to_vec2() * 0.05;
            let len = f.length();
            if len > 0.0 {
                node.pos += f / len * len.min(t);
            }
        }
        self.energy = self.energy.saturating_sub(1);
        self.shelve_unlinked();
    }

    fn shelve_unlinked(&mut self) {
        let linked: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.degree(i) > 0).collect();
        let (minx, maxx, maxy) = linked.iter().fold((f32::MAX, f32::MIN, f32::MIN), |(a, b, c), &i| {
            let p = self.nodes[i].pos;
            (a.min(p.x), b.max(p.x), c.max(p.y))
        });
        let (minx, maxx, maxy) = if linked.is_empty() { (0.0, 600.0, -120.0) } else { (minx, maxx, maxy) };
        let per_row = (((maxx - minx) / 110.0).floor() as usize).max(6);
        let mut k = 0;
        for i in 0..self.nodes.len() {
            if self.degree(i) == 0 && !self.nodes[i].pinned {
                self.nodes[i].pos = pos2(minx + (k % per_row) as f32 * 110.0, maxy + 150.0 + (k / per_row) as f32 * 90.0);
                k += 1;
            }
        }
    }

    fn layout_cards(&mut self) {
        let level = self.layers();
        let mut per: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, l) in level.iter().enumerate() {
            per.entry(*l).or_default().push(i);
        }
        let mut levels: Vec<usize> = per.keys().copied().collect();
        levels.sort_unstable();
        // Each level is a column as wide as its widest card. A tall level wraps into more
        // columns so a schema with many unlinked tables doesn't become one endless strip.
        const MAX_COLUMN_H: f32 = 2400.0;
        let mut x = 0.0;
        for l in levels {
            // Split the level into columns first, then give each column one width.
            let mut columns: Vec<Vec<usize>> = vec![Vec::new()];
            let mut y = 0.0;
            for &i in &per[&l] {
                let h = Self::card_height(&self.nodes[i]);
                if y > 0.0 && y + h > MAX_COLUMN_H {
                    columns.push(Vec::new());
                    y = 0.0;
                }
                columns.last_mut().unwrap().push(i);
                y += h + 32.0;
            }
            for (k, col) in columns.iter().enumerate() {
                let w = col.iter().map(|&i| Self::card_width(&self.nodes[i])).fold(NODE_W, f32::max);
                let mut y = 0.0;
                for &i in col {
                    self.nodes[i].card = pos2(x, y);
                    self.nodes[i].card_w = w;
                    y += Self::card_height(&self.nodes[i]) + 32.0;
                }
                x += w + if k + 1 < columns.len() { 60.0 } else { 110.0 };
            }
        }
    }

    /// Card width from its longest line, so names and types fit side by side (monospace,
    /// about 0.6 em a character at 12 px). Very long names still end in "…".
    fn card_width(n: &Node) -> f32 {
        const CH: f32 = 7.3;
        let head = n.obj.name.chars().count() as f32 * CH + 20.0 + 48.0;
        let rows = n.cols.iter().take(18).map(|c| {
            let ty = super::short_type(&c.data_type).chars().count() + if c.primary_key { 5 } else { 0 };
            c.name.chars().count() as f32 * CH + 16.0 + ty as f32 * 6.7 + 20.0
        }).fold(0.0, f32::max);
        head.max(rows).clamp(NODE_W, 340.0)
    }

    fn card_height(n: &Node) -> f32 {
        HEAD_H + n.cols.len().min(18) as f32 * ROW_H + 8.0 + if n.cols.len() > 18 { ROW_H } else { 0.0 }
    }

    fn node_radius(&self, i: usize) -> f32 {
        let rows = self.nodes[i].rows.unwrap_or(0) as f32;
        (14.0 + rows.max(1.0).log10() * 4.0 + self.degree(i) as f32 * 1.5).clamp(14.0, 40.0)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut TabCtx) {
        let pal = p();
        if self.task.is_none() && self.nodes.is_empty() && self.error.is_none() {
            self.load(cx);
        }
        if let Some(r) = take(&mut self.task) {
            match r {
                Ok(l) => self.ingest(l),
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(32.0);
                ui.spacing_mut().item_spacing.x = 8.0;
                let schema = if self.schema.is_empty() { "schema".to_string() } else { self.schema.clone() };
                ui.label(egui::RichText::new(schema).font(super::mono_medium(13.0)).color(pal.text));
                let isolated = (0..self.nodes.len()).filter(|&i| self.degree(i) == 0).count();
                ui.label(egui::RichText::new(format!("{} tables · {} relations{}", self.nodes.len(), self.relations.len(), if isolated > 0 { format!(" · {isolated} unlinked") } else { String::new() })).font(FontId::monospace(12.0)).color(pal.text_faint));
                if self.mode == Mode::Graph {
                    let mut l = self.layout;
                    if super::form::seg_well(ui, &mut l, &[(Layout::Force, "Force"), (Layout::Layers, "Layers"), (Layout::Circle, "Circle")], 0.0) {
                        self.layout = l;
                        self.apply_layout();
                    }
                    ui.label(egui::RichText::new("Hide unlinked").color(pal.text_dim));
                    super::form::switch(ui, &mut self.hide_isolated, "Hide unlinked");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if super::btn(ui, "Export SVG", super::BtnKind::Secondary).clicked() {
                        self.export_svg(cx);
                    }
                    let mut m = self.mode;
                    if super::form::seg_well(ui, &mut m, &[(Mode::Er, "ER diagram"), (Mode::Graph, "Graph")], 0.0) {
                        self.mode = m;
                        self.fit_pending = true;
                    }
                    if super::btn(ui, "Fit", super::BtnKind::Ghost).on_hover_text("Fit everything in view").clicked() {
                        self.fit_pending = true;
                    }
                    let r = super::form::field(ui, &mut self.search, "Find a table", Icon::Search, 200.0);
                    if r.changed() && !self.search.is_empty() {
                        let q = self.search.to_lowercase();
                        if let Some(i) = self.nodes.iter().position(|n| n.obj.name.to_lowercase().contains(&q)) {
                            self.selected = Some(i);
                            self.center_on(i);
                        }
                    }
                });
            });
        });
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, super::line()));
        if self.task.is_some() {
            ui.centered_and_justified(|ui| ui.add(egui::Spinner::new().size(22.0).color(pal.accent)));
            return;
        }
        if let Some(e) = &self.error {
            ui.label(egui::RichText::new(e).color(pal.danger));
            return;
        }
        if self.nodes.is_empty() {
            super::empty_state(ui, Icon::Graph, "No tables to map", "This schema has no tables yet");
            return;
        }
        // Detail panel for the selected table.
        if let Some(sel) = self.selected.filter(|&i| i < self.nodes.len()) {
            egui::Panel::right(egui::Id::unique(("map-detail", self.schema.as_str()))).exact_size(260.0).frame(egui::Frame::new().inner_margin(egui::Margin::same(14))).show(ui, |ui| self.detail_ui(ui, sel, cx));
        }
        match self.mode {
            Mode::Graph => self.graph_ui(ui, cx),
            Mode::Er => self.er_ui(ui, cx),
        }
    }

    pub fn set_er(&mut self) {
        self.mode = Mode::Er;
        self.fit_pending = true;
    }

    /// Selects a table by name and shows it at 100% (STRATA_SCRIPT `node:`).
    pub fn select_named(&mut self, name: &str) {
        self.selected = self.nodes.iter().position(|n| n.obj.name == name);
        if let Some(i) = self.selected {
            self.zoom = 1.0;
            self.fit_pending = false;
            self.center_on(i);
        }
    }

    fn center_on(&mut self, i: usize) {
        let target = match self.mode {
            Mode::Graph => self.nodes[i].pos,
            Mode::Er => self.nodes[i].card + vec2(self.nodes[i].card_w / 2.0, 40.0),
        };
        self.pan = -target.to_vec2() * self.zoom;
    }

    fn detail_ui(&mut self, ui: &mut egui::Ui, i: usize, cx: &mut TabCtx) {
        let pal = p();
        let n = &self.nodes[i];
        let name = n.obj.name.clone();
        ui.horizontal(|ui| {
            // Long names end in "…" instead of running past the panel; hover shows the full name.
            let shown = super::home::ellipsize_to(ui.painter(), &name, super::semibold(15.0), ui.available_width() - 32.0);
            let truncated = shown != name;
            let label = ui.label(super::title(&shown, 15.0));
            if truncated {
                label.on_hover_text(&name);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if super::icon_button(ui, Icon::Close, "Close").clicked() {
                    self.selected = None;
                }
            });
        });
        ui.label(dim(format!("{} · {} columns", n.rows.map(|r| format!("~{} rows", short_count(r))).unwrap_or_else(|| "rows unknown".into()), n.cols.len())));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if super::primary_button(ui, "  Open  ").clicked() {
                cx.actions.push(WsAction::OpenObject { obj: self.nodes[i].obj.clone(), structure: false, new_tab: true });
            }
            if ui.button("Structure").clicked() {
                cx.actions.push(WsAction::OpenObject { obj: self.nodes[i].obj.clone(), structure: true, new_tab: true });
            }
        });
        ui.add_space(10.0);
        if self.mode == Mode::Graph {
            ui.horizontal(|ui| {
                ui.label(dim("Focus depth"));
                ui.add(egui::Slider::new(&mut self.depth, 1..=4).show_value(true));
            });
            ui.add_space(8.0);
        }
        let out: Vec<&Relation> = self.relations.iter().filter(|r| r.table == name).collect();
        let inc: Vec<&Relation> = self.relations.iter().filter(|r| r.ref_table == name && r.table != name).collect();
        super::section_label(ui, &format!("References · {}", out.len()));
        let mut jump = None;
        for r in out {
            if ui.add(egui::Button::new(egui::RichText::new(format!("→ {}  ({})", r.ref_table, r.column)).size(12.5)).frame(false)).clicked() {
                jump = self.nodes.iter().position(|n| n.obj.name == r.ref_table);
            }
        }
        ui.add_space(8.0);
        super::section_label(ui, &format!("Referenced by · {}", inc.len()));
        for r in inc {
            let rule = if r.on_delete == "CASCADE" { " · cascade" } else { "" };
            if ui.add(egui::Button::new(egui::RichText::new(format!("← {}  ({}){rule}", r.table, r.column)).size(12.5)).frame(false)).clicked() {
                jump = self.nodes.iter().position(|n| n.obj.name == r.table);
            }
        }
        if let Some(j) = jump {
            self.selected = Some(j);
            self.center_on(j);
        }
        let _ = pal;
    }

    fn canvas(&mut self, ui: &mut egui::Ui) -> (Rect, egui::Response) {
        let pal = p();
        let (canvas, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        let step = 26.0 * self.zoom;
        let origin = canvas.center() + self.pan;
        let mut gx = canvas.left() + (origin.x - canvas.left()).rem_euclid(step);
        while gx < canvas.right() {
            let mut gy = canvas.top() + (origin.y - canvas.top()).rem_euclid(step);
            while gy < canvas.bottom() {
                painter.circle_filled(pos2(gx, gy), 0.9, pal.separator);
                gy += step;
            }
            gx += step;
        }
        if let Some(hover) = resp.hover_pos() {
            let (scroll, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            let factor = if zoom_delta != 1.0 { zoom_delta } else if ui.input(|i| i.modifiers.command) && scroll.y != 0.0 { 1.0 + scroll.y * 0.002 } else { 1.0 };
            if factor != 1.0 {
                let old = self.zoom;
                self.zoom = (self.zoom * factor).clamp(0.15, 2.5);
                let local = hover - canvas.center() - self.pan;
                self.pan -= local * (self.zoom / old - 1.0);
            } else if scroll != Vec2::ZERO {
                self.pan += scroll;
            }
        }
        (canvas, resp)
    }

    fn fit(&mut self, canvas: Rect, points: &[(Pos2, Vec2)]) {
        let Some(first) = points.first() else { return };
        let mut bb = Rect::from_min_size(first.0, first.1);
        for (p, s) in points {
            bb = bb.union(Rect::from_min_size(*p, *s));
        }
        let bb = bb.expand(40.0);
        self.zoom = (canvas.width() / bb.width()).min(canvas.height() / bb.height()).clamp(0.15, 1.4);
        self.pan = -bb.center().to_vec2() * self.zoom;
    }

    fn graph_ui(&mut self, ui: &mut egui::Ui, cx: &mut TabCtx) {
        let pal = p();
        if self.energy > 0 {
            for _ in 0..4 {
                self.step();
            }
            ui.ctx().request_repaint();
        }
        let (canvas, resp) = self.canvas(ui);
        let visible: Vec<bool> = (0..self.nodes.len()).map(|i| !(self.hide_isolated && self.degree(i) == 0)).collect();
        if self.fit_pending && (self.energy < 300 || self.layout != Layout::Force) {
            let pts: Vec<(Pos2, Vec2)> = (0..self.nodes.len()).filter(|&i| visible[i]).map(|i| (self.nodes[i].pos - vec2(30.0, 30.0), vec2(60.0, 60.0))).collect();
            self.fit(canvas, &pts);
            self.fit_pending = self.energy > 0 && self.layout == Layout::Force;
        }
        let z = self.zoom;
        let origin = canvas.center() + self.pan;
        let to_screen = |p: Pos2| origin + p.to_vec2() * z;
        let hit = |me: &Self, at: Pos2| (0..me.nodes.len()).rev().find(|&i| visible[i] && to_screen(me.nodes[i].pos).distance(at) <= me.node_radius(i) * z.max(0.5) + 4.0);
        self.hovered = resp.hover_pos().and_then(|at| hit(self, at));
        if resp.drag_started() {
            let at = resp.interact_pointer_pos().unwrap_or_default();
            self.dragging = hit(self, at);
        }
        if resp.dragged() {
            match self.dragging {
                Some(i) => {
                    self.nodes[i].pos += resp.drag_delta() / z;
                    self.nodes[i].pinned = true;
                    if self.layout == Layout::Force {
                        self.energy = self.energy.max(60);
                    }
                }
                None => self.pan += resp.drag_delta(),
            }
        }
        if resp.drag_stopped() {
            self.dragging = None;
        }
        if resp.clicked() {
            self.selected = resp.interact_pointer_pos().and_then(|at| hit(self, at));
        }
        if resp.double_clicked() {
            if let Some(i) = resp.interact_pointer_pos().and_then(|at| hit(self, at)) {
                cx.actions.push(WsAction::OpenObject { obj: self.nodes[i].obj.clone(), structure: false, new_tab: true });
            }
        }
        let focus = self.selected.or(self.hovered).map(|i| self.neighbours(i, if self.selected.is_some() { self.depth } else { 1 }));
        let painter = ui.painter_at(canvas);
        // Edges first.
        for (a, b, cols) in &self.edges {
            if !visible[*a] || !visible[*b] {
                continue;
            }
            let lit = focus.as_ref().map(|f| f.contains(a) && f.contains(b)).unwrap_or(true);
            let (pa, pb) = (to_screen(self.nodes[*a].pos), to_screen(self.nodes[*b].pos));
            let color = if lit { pal.accent2.gamma_multiply(0.75) } else { pal.separator };
            if a == b {
                painter.circle_stroke(pa + vec2(0.0, -self.node_radius(*a) * z), 10.0 * z, Stroke::new(1.2, color));
                continue;
            }
            let dir = (pb - pa).normalized();
            let end = pb - dir * (self.node_radius(*b) * z + 3.0);
            let start = pa + dir * (self.node_radius(*a) * z);
            painter.line_segment([start, end], Stroke::new(if cols.len() > 1 { 2.2 } else { 1.4 }, color));
            let side = vec2(-dir.y, dir.x) * 4.0;
            painter.add(egui::Shape::convex_polygon(vec![end, end - dir * 9.0 + side, end - dir * 9.0 - side], color, Stroke::NONE));
            if lit && focus.is_some() && z > 0.5 {
                let mid = start + (end - start) * 0.5;
                painter.text(mid + vec2(0.0, -8.0), Align2::CENTER_BOTTOM, cols.join(", "), FontId::monospace(10.0), pal.text_faint);
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if !visible[i] {
                continue;
            }
            let c = to_screen(n.pos);
            let r = self.node_radius(i) * z.max(0.5);
            if !canvas.expand(r + 60.0).contains(c) {
                continue;
            }
            let lit = focus.as_ref().map(|f| f.contains(&i)).unwrap_or(true);
            let selected = self.selected == Some(i);
            let deg = self.degree(i);
            let base = if deg == 0 { pal.text_faint } else if selected { pal.accent } else { mix(pal.accent2, pal.accent, (deg as f32 / 8.0).min(1.0) * 0.6) };
            let fill = if lit { Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 60) } else { Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 14) };
            painter.circle_filled(c, r, pal.content);
            painter.circle_filled(c, r, fill);
            painter.circle_stroke(c, r, Stroke::new(if selected { 2.5 } else { 1.4 }, if lit { base } else { base.gamma_multiply(0.3) }));
            if n.pinned {
                painter.circle_filled(c + vec2(r * 0.7, -r * 0.7), 2.5, pal.text_faint);
            }
            if z > 0.35 || lit && focus.is_some() {
                let text_color = if lit { pal.text } else { pal.text_faint.gamma_multiply(0.5) };
                painter.text(c + vec2(0.0, r + 4.0), Align2::CENTER_TOP, &n.obj.name, FontId::proportional((12.0 * z.clamp(0.8, 1.3)).round()), text_color);
                if let (Some(rows), true) = (n.rows, z > 0.7) {
                    painter.text(c, Align2::CENTER_CENTER, short_count(rows), FontId::monospace(10.0), if lit { pal.text_dim } else { pal.text_faint.gamma_multiply(0.5) });
                }
            }
        }
        if self.hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        painter.text(canvas.left_bottom() + vec2(12.0, -10.0), Align2::LEFT_BOTTOM, "drag to arrange · scroll or pinch to zoom · click to focus · double-click to open", FontId::proportional(11.0), pal.text_faint);
        let (legend, _) = (canvas.right_bottom() - vec2(12.0, 10.0), ());
        painter.text(legend, Align2::RIGHT_BOTTOM, "size = rows · arrow points to the referenced table", FontId::proportional(11.0), pal.text_faint);
        let _ = paint_icon;
    }

    fn er_ui(&mut self, ui: &mut egui::Ui, cx: &mut TabCtx) {
        let pal = p();
        let (canvas, resp) = self.canvas(ui);
        if self.fit_pending {
            let pts: Vec<(Pos2, Vec2)> = self.nodes.iter().map(|n| (n.card, vec2(n.card_w, Self::card_height(n)))).collect();
            self.fit(canvas, &pts);
            self.fit_pending = false;
        }
        let z = self.zoom;
        let origin = canvas.center() + self.pan;
        let to_screen = |p: Pos2| origin + p.to_vec2() * z;
        let node_rect = |n: &Node| Rect::from_min_size(to_screen(n.card), vec2(n.card_w, Self::card_height(n)) * z);
        if resp.drag_started() {
            let at = resp.interact_pointer_pos().unwrap_or_default();
            self.dragging = self.nodes.iter().rposition(|n| node_rect(n).contains(at));
        }
        if resp.dragged() {
            match self.dragging {
                Some(i) => self.nodes[i].card += resp.drag_delta() / z,
                None => self.pan += resp.drag_delta(),
            }
        }
        if resp.drag_stopped() {
            self.dragging = None;
        }
        if resp.clicked() {
            self.selected = resp.interact_pointer_pos().and_then(|at| self.nodes.iter().rposition(|n| node_rect(n).contains(at)));
        }
        if resp.double_clicked() {
            if let Some(at) = resp.interact_pointer_pos() {
                if let Some(n) = self.nodes.iter().rev().find(|n| node_rect(n).contains(at)) {
                    cx.actions.push(WsAction::OpenObject { obj: n.obj.clone(), structure: false, new_tab: true });
                }
            }
        }
        let painter = ui.painter_at(canvas);
        let focus = self.selected.map(|i| self.neighbours(i, 1));
        let index: HashMap<&str, usize> = self.nodes.iter().enumerate().map(|(i, n)| (n.obj.name.as_str(), i)).collect();
        for rel in &self.relations {
            let (Some(&ai), Some(&bi)) = (index.get(rel.table.as_str()), index.get(rel.ref_table.as_str())) else { continue };
            let (a, b) = (&self.nodes[ai], &self.nodes[bi]);
            let row_y = |n: &Node, col: &str| {
                let idx = n.cols.iter().position(|c| c.name == col).unwrap_or(0).min(17);
                to_screen(n.card).y + (HEAD_H + idx as f32 * ROW_H + ROW_H / 2.0 + 4.0) * z
            };
            let (ra, rb) = (node_rect(a), node_rect(b));
            let from_right = ra.center().x < rb.center().x;
            let p0 = pos2(if from_right { ra.right() } else { ra.left() }, row_y(a, &rel.column));
            let p3 = pos2(if from_right { rb.left() } else { rb.right() }, row_y(b, &rel.ref_column));
            let bend = 60.0 * z * if from_right { 1.0 } else { -1.0 };
            // Quiet by default so a big schema doesn't turn into a tangle; a selected card
            // lights its own relations and dims the rest further.
            let color = match &focus {
                None => super::line2(),
                Some(f) if f.contains(&ai) && f.contains(&bi) && (Some(ai) == self.selected || Some(bi) == self.selected) => super::fk_color().gamma_multiply(0.85),
                Some(_) => super::line().gamma_multiply(0.6),
            };
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke([p0, p0 + vec2(bend, 0.0), p3 - vec2(bend, 0.0), p3], false, Color32::TRANSPARENT, Stroke::new(1.5, color)));
            painter.circle_filled(p3, 3.0 * z, color);
        }
        for (i, n) in self.nodes.iter().enumerate() {
            let r = node_rect(n);
            if !r.intersects(canvas) {
                continue;
            }
            let lit = focus.as_ref().map(|f| f.contains(&i)).unwrap_or(true);
            let selected = self.selected == Some(i);
            // Card: s1 with a hairline edge; header row in mono with the row count; keys keep their colours.
            let fade = if lit { 1.0 } else { 0.45 };
            painter.rect_filled(r, CornerRadius::same(6), pal.sidebar);
            painter.rect_stroke(r, CornerRadius::same(6), Stroke::new(if selected { 1.5 } else { 1.0 }, if selected { pal.grid_selection_border } else { super::line2() }), egui::StrokeKind::Inside);
            let head = Rect::from_min_size(r.min, vec2(r.width(), HEAD_H * z));
            painter.hline(head.x_range(), head.bottom(), Stroke::new(1.0, super::line()));
            let count = n.rows.map(short_count).unwrap_or_default();
            let count_font = FontId::monospace(12.0 * z);
            let count_w = if count.is_empty() { 0.0 } else { painter.layout_no_wrap(count.clone(), count_font.clone(), pal.text_faint).size().x + 12.0 * z };
            let title = super::home::ellipsize_to(&painter, &n.obj.name, super::mono_medium(12.0 * z), r.width() - 20.0 * z - count_w);
            painter.text(head.left_center() + vec2(10.0 * z, 0.0), Align2::LEFT_CENTER, title, super::mono_medium(12.0 * z), pal.text.gamma_multiply(fade));
            if !count.is_empty() {
                painter.text(head.right_center() - vec2(10.0 * z, 0.0), Align2::RIGHT_CENTER, count, count_font, pal.text_faint.gamma_multiply(fade));
            }
            let (name_font, type_font) = (FontId::monospace(12.0 * z), FontId::monospace(11.0 * z));
            let inner = r.width() - 20.0 * z;
            for (k, c) in n.cols.iter().take(18).enumerate() {
                let y = r.top() + (HEAD_H + 4.0 + k as f32 * ROW_H + ROW_H / 2.0) * z;
                let is_fk = self.relations.iter().any(|rel| rel.table == n.obj.name && rel.column == c.name);
                let name_color = if c.primary_key { super::pk_color() } else if is_fk { super::fk_color() } else { pal.text };
                // The name gets the room first; the type takes what is left, ending in "…".
                let name_w = painter.layout_no_wrap(c.name.clone(), name_font.clone(), pal.text).size().x;
                let key = if c.primary_key { "PK · " } else if is_fk { "FK · " } else { "" };
                let ty_room = inner - name_w.min(inner * 0.62) - 14.0 * z;
                let ty = if ty_room < 30.0 * z { String::new() } else { super::home::ellipsize_to(&painter, &format!("{key}{}", super::short_type(&c.data_type)), type_font.clone(), ty_room) };
                let ty_w = if ty.is_empty() { 0.0 } else { painter.layout_no_wrap(ty.clone(), type_font.clone(), pal.text_faint).size().x + 14.0 * z };
                let name = super::home::ellipsize_to(&painter, &c.name, name_font.clone(), inner - ty_w);
                painter.text(pos2(r.left() + 10.0 * z, y), Align2::LEFT_CENTER, name, name_font.clone(), name_color.gamma_multiply(fade));
                if !ty.is_empty() {
                    painter.text(pos2(r.right() - 10.0 * z, y), Align2::RIGHT_CENTER, ty, type_font.clone(), pal.text_faint.gamma_multiply(fade));
                }
            }
            if n.cols.len() > 18 {
                let y = r.top() + (HEAD_H + 4.0 + 18.0 * ROW_H + ROW_H / 2.0) * z;
                painter.text(pos2(r.left() + 12.0 * z, y), Align2::LEFT_CENTER, format!("+{} more", n.cols.len() - 18), FontId::proportional(11.0 * z), pal.text_faint);
            }
        }
        painter.text(canvas.left_bottom() + vec2(12.0, -10.0), Align2::LEFT_BOTTOM, "drag cards · scroll or pinch to zoom · click to focus · double-click to open", FontId::proportional(11.0), pal.text_faint);
    }

    /// Writes the current view (graph or cards) as a standalone SVG file.
    fn export_svg(&self, cx: &mut TabCtx) {
        let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}-map.svg", if self.schema.is_empty() { "schema" } else { &self.schema })).save_file() else { return };
        let svg = match self.mode {
            Mode::Graph => graph_svg(&self.nodes.iter().map(|n| (n.obj.name.clone(), n.pos, n.rows)).collect::<Vec<_>>(), &self.edges),
            Mode::Er => er_svg(&self.nodes.iter().map(|n| (n.obj.name.clone(), n.card, n.cols.clone())).collect::<Vec<_>>(), &self.relations),
        };
        match std::fs::write(&path, svg) {
            Ok(()) => cx.actions.push(WsAction::Toast(super::ToastKind::Success, format!("Saved {}", path.display()))),
            Err(e) => cx.actions.push(WsAction::Toast(super::ToastKind::Error, format!("Can't write {}: {e}", path.display()))),
        }
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// SVG of the graph: circles for tables, arrows for foreign keys.
pub fn graph_svg(nodes: &[(String, Pos2, Option<u64>)], edges: &[(usize, usize, Vec<String>)]) -> String {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (_, p, _) in nodes {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }
    if nodes.is_empty() {
        (minx, miny, maxx, maxy) = (0.0, 0.0, 100.0, 100.0);
    }
    let pad = 80.0;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" font-family=\"Geist, Inter, sans-serif\">\n<rect x=\"{}\" y=\"{}\" width=\"100%\" height=\"100%\" fill=\"#0B0E13\"/>\n<defs><marker id=\"a\" viewBox=\"0 0 10 10\" refX=\"10\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto\"><path d=\"M0 0L10 5L0 10z\" fill=\"#5CC8E0\"/></marker></defs>\n",
        minx - pad, miny - pad, maxx - minx + pad * 2.0, maxy - miny + pad * 2.0, minx - pad, miny - pad
    );
    for (a, b, cols) in edges {
        let (pa, pb) = (nodes[*a].1, nodes[*b].1);
        if a == b {
            continue;
        }
        let d = (pb - pa).normalized();
        let end = pb - d * 26.0;
        out.push_str(&format!("<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#5CC8E0\" stroke-opacity=\"0.7\" stroke-width=\"1.5\" marker-end=\"url(#a)\"><title>{}</title></line>\n", pa.x, pa.y, end.x, end.y, xml(&cols.join(", "))));
    }
    for (name, p, rows) in nodes {
        out.push_str(&format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"22\" fill=\"#12161C\" stroke=\"#F2B252\" stroke-width=\"1.5\"/>\n", p.x, p.y));
        out.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#E8ECF1\" font-size=\"12\" text-anchor=\"middle\">{}</text>\n", p.x, p.y + 40.0, xml(name)));
        if let Some(r) = rows {
            out.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#A3ADBA\" font-size=\"10\" text-anchor=\"middle\" dominant-baseline=\"middle\">{}</text>\n", p.x, p.y, short_count(*r)));
        }
    }
    out.push_str("</svg>\n");
    out
}

/// SVG of the ER cards with their column lists.
pub fn er_svg(cards: &[(String, Pos2, Vec<ColumnInfo>)], relations: &[Relation]) -> String {
    let h = |cols: &[ColumnInfo]| HEAD_H + cols.len().min(18) as f32 * ROW_H + 8.0;
    let (mut maxx, mut maxy) = (0.0f32, 0.0f32);
    let (mut minx, mut miny) = (f32::MAX, f32::MAX);
    for (_, p, cols) in cards {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x + NODE_W);
        maxy = maxy.max(p.y + h(cols));
    }
    if cards.is_empty() {
        (minx, miny) = (0.0, 0.0);
    }
    let pad = 40.0;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" font-family=\"Geist, Inter, sans-serif\">\n<rect x=\"{}\" y=\"{}\" width=\"100%\" height=\"100%\" fill=\"#0B0E13\"/>\n",
        minx - pad, miny - pad, maxx - minx + pad * 2.0, maxy - miny + pad * 2.0, minx - pad, miny - pad
    );
    let index: HashMap<&str, usize> = cards.iter().enumerate().map(|(i, c)| (c.0.as_str(), i)).collect();
    for r in relations {
        let (Some(&a), Some(&b)) = (index.get(r.table.as_str()), index.get(r.ref_table.as_str())) else { continue };
        let y = |i: usize, col: &str| cards[i].1.y + HEAD_H + cards[i].2.iter().position(|c| c.name == col).unwrap_or(0).min(17) as f32 * ROW_H + ROW_H / 2.0 + 4.0;
        let right = cards[a].1.x < cards[b].1.x;
        let x0 = if right { cards[a].1.x + NODE_W } else { cards[a].1.x };
        let x3 = if right { cards[b].1.x } else { cards[b].1.x + NODE_W };
        let bend = if right { 60.0 } else { -60.0 };
        let (y0, y3) = (y(a, &r.column), y(b, &r.ref_column));
        out.push_str(&format!("<path d=\"M{x0:.1} {y0:.1} C{:.1} {y0:.1} {:.1} {y3:.1} {x3:.1} {y3:.1}\" fill=\"none\" stroke=\"#5CC8E0\" stroke-opacity=\"0.75\" stroke-width=\"1.5\"/>\n", x0 + bend, x3 - bend));
    }
    for (name, p, cols) in cards {
        out.push_str(&format!("<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{NODE_W}\" height=\"{:.1}\" rx=\"14\" fill=\"#12161C\" stroke=\"#2A323D\"/>\n", p.x, p.y, h(cols)));
        out.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" fill=\"#E8ECF1\" font-size=\"13\" font-weight=\"600\">{}</text>\n", p.x + 12.0, p.y + 20.0, xml(name)));
        for (k, c) in cols.iter().take(18).enumerate() {
            let y = p.y + HEAD_H + 4.0 + k as f32 * ROW_H + 13.0;
            let color = if c.primary_key { "#F2B252" } else { "#A3ADBA" };
            out.push_str(&format!("<text x=\"{:.1}\" y=\"{y:.1}\" fill=\"{color}\" font-size=\"11.5\">{}</text>\n", p.x + 12.0, xml(&c.name)));
            out.push_str(&format!("<text x=\"{:.1}\" y=\"{y:.1}\" fill=\"#7D8794\" font-size=\"10\" text-anchor=\"end\" font-family=\"monospace\">{}</text>\n", p.x + NODE_W - 12.0, xml(&c.data_type.chars().take(16).collect::<String>())));
        }
    }
    out.push_str("</svg>\n");
    out
}

#[allow(dead_code)]
fn kind_is_table(k: ObjectKind) -> bool {
    k == ObjectKind::Table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab_with(edges: &[(&str, &str)], names: &[&str]) -> DiagramTab {
        let mut t = DiagramTab::new("public".into(), vec![]);
        let tables = names.iter().map(|n| (DbObject { parent: None, arguments: None, schema: "public".into(), name: n.to_string(), kind: ObjectKind::Table }, vec![])).collect();
        let relations = edges.iter().map(|(a, b)| Relation { table: a.to_string(), ref_table: b.to_string(), column: "x".into(), ref_column: "id".into(), ..Default::default() }).collect();
        t.ingest(Loaded { tables, relations, stats: vec![] });
        t
    }

    #[test]
    fn layers_put_referenced_tables_above_their_children() {
        let t = tab_with(&[("order_items", "orders"), ("orders", "customers"), ("order_items", "products")], &["customers", "orders", "order_items", "products"]);
        let level = t.layers();
        assert_eq!(level[0], 0, "customers is a root");
        assert_eq!(level[3], 0, "products is a root");
        assert_eq!(level[1], 1);
        assert_eq!(level[2], 2);
    }

    #[test]
    fn focus_depth_walks_both_directions() {
        let t = tab_with(&[("b", "a"), ("c", "b"), ("d", "c")], &["a", "b", "c", "d"]);
        assert_eq!(t.neighbours(1, 1), [0, 1, 2].into());
        assert_eq!(t.neighbours(0, 2), [0, 1, 2].into());
    }

    #[test]
    fn force_layout_separates_nodes() {
        let mut t = tab_with(&[("b", "a")], &["a", "b", "c"]);
        for _ in 0..400 {
            t.step();
        }
        for i in 0..3 {
            for j in (i + 1)..3 {
                assert!(t.nodes[i].pos.distance(t.nodes[j].pos) > 30.0, "nodes {i} and {j} overlap");
            }
        }
    }

    #[test]
    fn svg_export_escapes_names() {
        let svg = graph_svg(&[("a<b".into(), Pos2::ZERO, Some(5)), ("c".into(), pos2(100.0, 0.0), None)], &[(0, 1, vec!["x → id".into()])]);
        assert!(svg.contains("a&lt;b") && svg.starts_with("<svg") && svg.contains("marker-end"));
    }
}
