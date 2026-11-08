//! Table tab: data browser (filters, sort, paging, inline edits, commit) and structure editor.

use super::grid::{self, CellView, ColMeta, GridEvent, GridSource, GridState, RowState};
use crate::db::Relation;
use super::workspace::{TabCtx, WsAction};
use super::{dim, faint, icon_button, p, paint_icon, primary_button, secondary_button, segmented, Icon, ToastKind};
use crate::db::sqlgen::StructureChange;
use crate::db::*;
use crate::runtime::{take, Task};
use egui::{vec2, Color32, CornerRadius, RichText, Stroke, Ui};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableView {
    Data,
    Structure,
    Relations,
    Profile,
}

/// One pending change as the review sheet shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewItem {
    pub kind: ReviewKind,
    /// "id = 10482" or "new row".
    pub title: String,
    /// A human hint for the row, e.g. its name column.
    pub note: String,
    /// (column, old value, new value).
    pub fields: Vec<(String, Option<String>, Option<String>)>,
    /// Key used to look up cascades for deletes.
    pub key: Vec<(String, Cell)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewKind {
    Edit,
    Add,
    Remove,
}

/// Whole-table column statistics from SQL.
#[derive(Debug, Clone, Default)]
pub struct ColumnStats {
    pub name: String,
    pub non_null: u64,
    pub distinct: u64,
    pub min: Option<String>,
    pub max: Option<String>,
}

#[derive(Default)]
pub struct Pending {
    pub edits: BTreeMap<(usize, usize), EditValue>,
    pub deleted: BTreeSet<usize>,
    pub inserted: Vec<Vec<Option<EditValue>>>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty() && self.deleted.is_empty() && self.inserted.is_empty()
    }

    pub fn count(&self) -> usize {
        let edited_rows: BTreeSet<usize> = self.edits.keys().map(|(r, _)| *r).filter(|r| !self.deleted.contains(r)).collect();
        edited_rows.len() + self.deleted.len() + self.inserted.len()
    }
}

struct Meta {
    columns: Vec<ColumnInfo>,
    fks: Vec<ForeignKey>,
    indexes: Vec<IndexInfo>,
    ddl: Option<String>,
}

#[derive(Default)]
struct StructureState {
    draft: Vec<ColumnInfo>,
    original_len: usize,
    dropped: BTreeSet<String>,
    new_index: Option<IndexDraft>,
    dropped_indexes: BTreeSet<String>,
    added_indexes: Vec<IndexDraft>,
    new_fk: Option<FkDraft>,
    dropped_fks: BTreeSet<String>,
    added_fks: Vec<FkDraft>,
    show_ddl: bool,
}

/// A foreign key being added; it references a table in this table's schema.
#[derive(Default, Clone)]
struct FkDraft {
    name: String,
    column: String,
    ref_table: String,
    ref_column: String,
    on_delete: String,
    on_update: String,
}

impl FkDraft {
    fn ready(&self) -> bool {
        [&self.name, &self.column, &self.ref_table, &self.ref_column].iter().all(|s| !s.trim().is_empty())
    }
}

#[derive(Default, Clone)]
struct IndexDraft {
    name: String,
    /// Comma separated, as typed.
    columns: String,
    unique: bool,
    method: String,
}

impl IndexDraft {
    fn column_list(&self) -> Vec<String> {
        self.columns.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect()
    }
}

pub struct QuickLook {
    pub row: usize,
    pub col: usize,
    pub viewer: super::viewer::ValueViewer,
}

static DEFAULT_EDIT: EditValue = EditValue::Default;

pub struct TableTab {
    pub id: u64,
    pub obj: DbObject,
    pub view: TableView,
    pub columns: Vec<ColumnInfo>,
    pub fks: Vec<ForeignKey>,
    pub indexes: Vec<IndexInfo>,
    pub ddl: Option<String>,
    meta_task: Option<Task<anyhow::Result<Meta>>>,
    pub data: QueryResult,
    data_task: Option<Task<anyhow::Result<QueryResult>>>,
    pub count: Option<u64>,
    count_task: Option<Task<anyhow::Result<u64>>>,
    pub req: BrowseRequest,
    draft_filters: Vec<Filter>,
    pub filters_open: bool,
    pub pending: Pending,
    pub grid: GridState,
    pub error: Option<String>,
    commit_task: Option<Task<anyhow::Result<()>>>,
    structure: StructureState,
    /// Single-click "preview" tabs are replaced by the next single click.
    pub preview: bool,
    grid_meta: Vec<ColMeta>,
    pub quick_look: Option<QuickLook>,
    loaded_once: bool,
    /// Foreign keys elsewhere that point at this table.
    pub incoming: Vec<Relation>,
    incoming_task: Option<Task<anyhow::Result<Vec<Relation>>>>,
    stats_task: Option<Task<anyhow::Result<(u64, Vec<ColumnStats>)>>>,
    pub table_stats: Option<(u64, Vec<ColumnStats>)>,
    /// Linked rows for the inspected row: (row index + page offset, links).
    linked: Option<(usize, Vec<LinkInfo>)>,
    linked_task: Option<(usize, Task<anyhow::Result<Vec<LinkInfo>>>)>,
    /// The toolbar's quick "contains" filter over every column.
    quick_filter: String,
    /// Row height: compact 26, cozy 32, roomy 40.
    density: u8,
}

/// A row this row points at, or a set of rows pointing at it.
#[derive(Debug, Clone)]
pub struct LinkInfo {
    pub outgoing: bool,
    pub schema: String,
    pub table: String,
    pub column: String,
    pub value: String,
    pub detail: String,
}

pub fn table_ref(obj: &DbObject) -> TableRef {
    TableRef { schema: obj.schema.clone(), name: obj.name.clone() }
}

impl TableTab {
    pub fn new(id: u64, obj: DbObject, view: TableView, page_size: usize) -> Self {
        let view = if obj.kind.has_rows() { view } else { TableView::Structure };
        Self {
            id,
            obj,
            view,
            columns: vec![],
            fks: vec![],
            indexes: vec![],
            ddl: None,
            meta_task: None,
            data: QueryResult::default(),
            data_task: None,
            count: None,
            count_task: None,
            req: BrowseRequest { limit: page_size, ..Default::default() },
            draft_filters: vec![],
            filters_open: false,
            pending: Pending::default(),
            grid: GridState::default(),
            error: None,
            commit_task: None,
            structure: StructureState::default(),
            preview: true,
            grid_meta: vec![],
            quick_look: None,
            loaded_once: false,
            incoming: vec![],
            incoming_task: None,
            stats_task: None,
            table_stats: None,
            linked: None,
            linked_task: None,
            quick_filter: String::new(),
            density: 0,
        }
    }

    pub fn title(&self) -> String {
        self.obj.display_name()
    }

    pub fn table(&self) -> TableRef {
        table_ref(&self.obj)
    }

    pub fn is_busy(&self) -> bool {
        self.data_task.is_some() || self.meta_task.is_some() || self.commit_task.is_some()
    }

    pub fn has_changes(&self) -> bool {
        !self.pending.is_empty() || self.structure_changes().len() > 0
    }

    pub fn editable(&self, read_only_conn: bool) -> bool {
        !read_only_conn && matches!(self.obj.kind, ObjectKind::Table | ObjectKind::Collection | ObjectKind::Keys)
    }

    pub fn add_filter(&mut self, f: Filter) {
        self.req.filters.push(f.clone());
        self.draft_filters = self.req.filters.clone();
        self.filters_open = true;
        self.req.offset = 0;
    }

    pub fn load_meta(&mut self, cx: &TabCtx) {
        let obj = self.obj.clone();
        self.meta_task = Some(cx.session.spawn(cx.ctx, move |d| async move {
            let mut d = d.lock().await;
            let t = table_ref(&obj);
            let columns = if obj.kind.has_rows() { d.columns(&t).await? } else { vec![] };
            let fks = if obj.kind == ObjectKind::Table { d.foreign_keys(&t).await.unwrap_or_default() } else { vec![] };
            let indexes = if matches!(obj.kind, ObjectKind::Table | ObjectKind::Collection) { d.indexes(&t).await.unwrap_or_default() } else { vec![] };
            let ddl = d.ddl(&obj).await.ok().filter(|s| !s.trim().is_empty());
            Ok(Meta { columns, fks, indexes, ddl })
        }));
        if self.obj.kind == ObjectKind::Table {
            let t = self.table();
            self.incoming_task = Some(cx.session.spawn(cx.ctx, move |d| async move { d.lock().await.referencing(&t).await }));
        }
    }

    pub fn reload(&mut self, cx: &TabCtx) {
        if !self.obj.kind.has_rows() {
            self.load_meta(cx);
            return;
        }
        if self.columns.is_empty() && self.meta_task.is_none() {
            self.load_meta(cx);
        }
        let t = self.table();
        let req = self.req.clone();
        let cols = self.columns.clone();
        let log = cx.session.log.clone();
        self.data_task = Some(cx.session.spawn(cx.ctx, move |d| async move {
            let r = d.lock().await.browse(&t, &req, &cols).await;
            if let Ok(mut l) = log.lock() {
                let text = r.as_ref().map(|r| r.sql.clone()).unwrap_or_else(|e| format!("-- browse failed: {e}"));
                l.push(crate::runtime::LogLine { at: chrono::Local::now(), text, ok: r.is_ok() });
            }
            r
        }));
        let t = self.table();
        let req = self.req.clone();
        let cols = self.columns.clone();
        self.count_task = Some(cx.session.spawn(cx.ctx, move |d| async move { d.lock().await.count(&t, &req, &cols).await }));
    }

    pub fn ensure_loaded(&mut self, cx: &TabCtx) {
        if !self.loaded_once {
            self.loaded_once = true;
            self.load_meta(cx);
            if self.obj.kind.has_rows() {
                self.reload(cx);
            }
        }
    }

    fn poll(&mut self, cx: &mut TabCtx) {
        if let Some(r) = take(&mut self.meta_task) {
            match r {
                Ok(m) => {
                    let need_reload = self.columns.is_empty() && !m.columns.is_empty() && self.req.filters.iter().any(|f| f.column.is_none());
                    self.columns = m.columns;
                    self.fks = m.fks;
                    self.indexes = m.indexes;
                    self.ddl = m.ddl;
                    self.reset_structure_draft();
                    self.rebuild_grid_meta();
                    if need_reload {
                        self.reload(cx);
                    }
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        if let Some(r) = take(&mut self.data_task) {
            match r {
                Ok(res) => {
                    self.data = res;
                    self.error = None;
                    self.pending = Pending::default();
                    self.grid.clear();
                    self.rebuild_grid_meta();
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        if let Some(r) = take(&mut self.count_task) {
            self.count = r.ok();
        }
        if let Some(r) = take(&mut self.incoming_task) {
            self.incoming = r.unwrap_or_default();
        }
        if let Some(r) = take(&mut self.stats_task) {
            match r {
                Ok(v) => self.table_stats = Some(v),
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        if let Some(r) = take(&mut self.commit_task) {
            match r {
                Ok(()) => {
                    cx.session.log("-- changes committed", true);
                    cx.actions.push(WsAction::Toast(ToastKind::Success, "Changes saved".into()));
                    self.pending = Pending::default();
                    self.reload(cx);
                }
                Err(e) => {
                    cx.session.log(format!("-- commit failed: {e:#}"), false);
                    self.error = Some(format!("{e:#}"));
                }
            }
        }
    }

    fn rebuild_grid_meta(&mut self) {
        let fk_cols: BTreeSet<&str> = self.fks.iter().map(|f| f.column.as_str()).collect();
        self.grid_meta = self
            .data
            .columns
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let info = self.columns.iter().find(|c| &c.name == name);
                ColMeta {
                    name: name.clone(),
                    data_type: info.map(|c| c.data_type.clone()).or_else(|| self.data.types.get(i).cloned()).unwrap_or_default(),
                    primary_key: info.map(|c| c.primary_key).unwrap_or(false),
                    foreign_key: fk_cols.contains(name.as_str()),
                    profile: None,
                }
            })
            .collect();
        grid::profile_all(&mut self.grid_meta, &self.data.rows);
    }

    fn key_columns(&self) -> Vec<usize> {
        let pk: Vec<usize> = self.grid_meta.iter().enumerate().filter(|(_, m)| m.primary_key).map(|(i, _)| i).collect();
        if pk.is_empty() { (0..self.grid_meta.len()).collect() } else { pk }
    }

    pub fn has_primary_key(&self) -> bool {
        self.grid_meta.iter().any(|m| m.primary_key)
    }

    pub fn changes(&self) -> Vec<RowChange> {
        let keys = self.key_columns();
        let key_of = |r: usize| -> Vec<(String, Cell)> { keys.iter().map(|&c| (self.grid_meta[c].name.clone(), self.data.rows[r].get(c).cloned().flatten())).collect() };
        let mut out = Vec::new();
        let mut by_row: BTreeMap<usize, Vec<(String, EditValue)>> = BTreeMap::new();
        for ((r, c), v) in &self.pending.edits {
            if !self.pending.deleted.contains(r) {
                by_row.entry(*r).or_default().push((self.grid_meta[*c].name.clone(), v.clone()));
            }
        }
        for (r, set) in by_row {
            out.push(RowChange::Update { key: key_of(r), set });
        }
        for &r in &self.pending.deleted {
            out.push(RowChange::Delete { key: key_of(r) });
        }
        for row in &self.pending.inserted {
            let values = row.iter().enumerate().map(|(c, v)| (self.grid_meta[c].name.clone(), v.clone().unwrap_or(EditValue::Default))).collect();
            out.push(RowChange::Insert { values });
        }
        out
    }

    /// The pending changes as before → after rows for the review sheet.
    pub fn review_items(&self) -> Vec<ReviewItem> {
        let keys = self.key_columns();
        let key_of = |r: usize| -> Vec<(String, Cell)> { keys.iter().map(|&c| (self.grid_meta[c].name.clone(), self.data.rows[r].get(c).cloned().flatten())).collect() };
        let title_of = |key: &[(String, Cell)]| -> String {
            if key.len() > 2 {
                return "row matched by all values".into();
            }
            key.iter().map(|(c, v)| format!("{c} = {}", v.as_deref().unwrap_or("NULL"))).collect::<Vec<_>>().join(", ")
        };
        let label_col = self.grid_meta.iter().position(|m| {
            let n = m.name.to_lowercase();
            !m.primary_key && (n.contains("name") || n.contains("title") || n.contains("email") || n == "label")
        });
        let note_of = |r: usize| label_col.and_then(|c| self.data.rows[r].get(c).cloned().flatten()).unwrap_or_default();
        let show = |v: &EditValue| match v {
            EditValue::Text(s) => Some(s.clone()),
            EditValue::Null => None,
            EditValue::Default => Some("DEFAULT".into()),
        };
        let mut out = Vec::new();
        let mut by_row: BTreeMap<usize, Vec<(usize, &EditValue)>> = BTreeMap::new();
        for ((r, c), v) in &self.pending.edits {
            if !self.pending.deleted.contains(r) {
                by_row.entry(*r).or_default().push((*c, v));
            }
        }
        for (r, cells) in by_row {
            let key = key_of(r);
            let fields = cells.into_iter().map(|(c, v)| (self.grid_meta[c].name.clone(), self.data.rows[r].get(c).cloned().flatten(), show(v))).collect();
            out.push(ReviewItem { kind: ReviewKind::Edit, title: title_of(&key), note: note_of(r), fields, key });
        }
        for row in &self.pending.inserted {
            let fields = row.iter().enumerate().filter_map(|(c, v)| v.as_ref().map(|v| (self.grid_meta[c].name.clone(), None, show(v)))).collect();
            out.push(ReviewItem { kind: ReviewKind::Add, title: "new row".into(), note: String::new(), fields, key: vec![] });
        }
        for &r in &self.pending.deleted {
            let key = key_of(r);
            let fields = (0..self.grid_meta.len())
                .filter(|&c| !self.grid_meta[c].primary_key)
                .take(3)
                .map(|c| (self.grid_meta[c].name.clone(), None, self.data.rows[r].get(c).cloned().flatten()))
                .collect();
            out.push(ReviewItem { kind: ReviewKind::Remove, title: title_of(&key), note: note_of(r), fields, key });
        }
        out
    }

    pub fn preview_statements(&self, kind: crate::model::DriverKind) -> Vec<String> {
        preview_changes(kind, &self.table(), &self.changes())
    }

    /// Called by the workspace after safe-mode approval.
    pub fn start_commit(&mut self, cx: &TabCtx) {
        let changes = self.changes();
        if changes.is_empty() {
            return;
        }
        for s in preview_changes(cx.session.kind(), &self.table(), &changes) {
            cx.session.log(s, true);
        }
        let t = self.table();
        self.commit_task = Some(cx.session.spawn(cx.ctx, move |d| async move { d.lock().await.apply_changes(&t, &changes).await }));
    }

    pub fn discard(&mut self) {
        self.pending = Pending::default();
        self.reset_structure_draft();
        self.grid.editing_cancel();
    }

    fn base_rows(&self) -> usize {
        self.data.rows.len()
    }

    fn apply_edit(&mut self, row: usize, col: usize, value: EditValue) {
        let base = self.base_rows();
        if row >= base {
            if let Some(r) = self.pending.inserted.get_mut(row - base) {
                r[col] = Some(value);
            }
            return;
        }
        if self.pending.deleted.contains(&row) {
            return;
        }
        let original = self.data.rows[row].get(col).cloned().flatten();
        let same = match (&value, &original) {
            (EditValue::Text(a), Some(b)) => a == b,
            (EditValue::Null, None) => true,
            _ => false,
        };
        if same {
            self.pending.edits.remove(&(row, col));
        } else {
            self.pending.edits.insert((row, col), value);
        }
    }

    /// Opens the inline cell editor for deterministic UI previews.
    pub fn script_begin_edit(&mut self, row: usize, col: &str) {
        let Some(c) = self.grid_meta.iter().position(|m| m.name == col) else { return };
        if row >= self.data.rows.len() { return; }
        let mut state = std::mem::take(&mut self.grid);
        state.select_row(row);
        state.begin_edit(&TableSource { tab: self }, row, c);
        self.grid = state;
    }

    /// Edits a cell by column name (used by the STRATA_SCRIPT development hook).
    pub fn script_edit(&mut self, row: usize, col: &str, value: &str) {
        if let Some(c) = self.grid_meta.iter().position(|m| m.name == col) {
            self.apply_edit(row, c, EditValue::Text(value.to_string()));
        }
    }

    pub fn insert_row(&mut self) {
        self.pending.inserted.push(vec![None; self.grid_meta.len()]);
        let r = self.base_rows() + self.pending.inserted.len() - 1;
        self.grid.select_row(r);
    }

    pub fn duplicate_rows(&mut self) {
        let rows = grid::selection_rows(&self.grid);
        for r in rows {
            let src = TableSource { tab: self };
            let mut vals: Vec<Option<EditValue>> = (0..self.grid_meta.len())
                .map(|c| match grid::cell_value(&src, r, c) {
                    Some(v) => Some(EditValue::Text(v)),
                    None => Some(EditValue::Null),
                })
                .collect();
            for (c, m) in self.grid_meta.iter().enumerate() {
                let generated = self.columns.iter().find(|x| x.name == m.name).map(|x| x.default.is_some() || !x.extra.is_empty()).unwrap_or(false);
                if m.primary_key && generated {
                    vals[c] = None;
                }
            }
            self.pending.inserted.push(vals);
        }
        let last = self.base_rows() + self.pending.inserted.len().saturating_sub(1);
        self.grid.select_row(last);
    }

    pub fn delete_selected(&mut self) {
        let base = self.base_rows();
        let mut rows = grid::selection_rows(&self.grid);
        rows.sort_unstable_by(|a, b| b.cmp(a));
        for r in rows {
            if r >= base {
                if r - base < self.pending.inserted.len() {
                    self.pending.inserted.remove(r - base);
                }
            } else if !self.pending.deleted.insert(r) {
                self.pending.deleted.remove(&r);
            }
        }
        self.grid.rows.clear();
    }

    // ---------- structure ----------

    fn reset_structure_draft(&mut self) {
        self.structure.draft = self.columns.clone();
        self.structure.original_len = self.columns.len();
        self.structure.dropped.clear();
        self.structure.dropped_indexes.clear();
        self.structure.added_indexes.clear();
        self.structure.new_index = None;
        self.structure.dropped_fks.clear();
        self.structure.added_fks.clear();
        self.structure.new_fk = None;
    }

    fn column_dropped(&self, i: usize) -> bool {
        i < self.structure.original_len && self.structure.dropped.contains(&self.columns[i].name)
    }

    /// In the order the engine needs them: constraints come off before columns change and go back on after.
    pub fn structure_changes(&self) -> Vec<StructureChange> {
        let s = &self.structure;
        let mut out = Vec::new();
        for name in &s.dropped_fks {
            out.push(StructureChange::DropForeignKey(name.clone()));
        }
        for name in &s.dropped_indexes {
            out.push(StructureChange::DropIndex(name.clone()));
        }
        // Compare key membership by position so renaming a key column doesn't rebuild the key.
        let pk_before: Vec<usize> = (0..s.original_len).filter(|&i| self.columns[i].primary_key).collect();
        let pk_after: Vec<usize> = (0..s.draft.len()).filter(|&i| s.draft[i].primary_key && !self.column_dropped(i) && !s.draft[i].name.trim().is_empty()).collect();
        let pk_changed = pk_before != pk_after;
        if pk_changed && !pk_before.is_empty() {
            let name = self.indexes.iter().find(|ix| ix.primary).map(|ix| ix.name.clone()).unwrap_or_else(|| format!("{}_pkey", self.obj.name));
            out.push(StructureChange::DropPrimaryKey { name });
        }
        for name in &s.dropped {
            out.push(StructureChange::DropColumn(name.clone()));
        }
        for (i, c) in s.draft.iter().enumerate() {
            if i < s.original_len {
                let o = &self.columns[i];
                if s.dropped.contains(&o.name) {
                    continue;
                }
                // Key membership is handled as one table-level change below.
                if (ColumnInfo { primary_key: c.primary_key, ..o.clone() }) != *c {
                    out.push(StructureChange::AlterColumn { original: o.clone(), updated: ColumnInfo { primary_key: o.primary_key, ..c.clone() } });
                }
            } else if !c.name.trim().is_empty() {
                out.push(StructureChange::AddColumn(ColumnInfo { primary_key: false, ..c.clone() }));
            }
        }
        if pk_changed && !pk_after.is_empty() {
            out.push(StructureChange::AddPrimaryKey { columns: pk_after.iter().map(|&i| s.draft[i].name.trim().to_string()).collect() });
        }
        for ix in &s.added_indexes {
            out.push(StructureChange::AddIndex { name: ix.name.clone(), columns: ix.column_list(), unique: ix.unique, method: ix.method.clone() });
        }
        for fk in &s.added_fks {
            out.push(StructureChange::AddForeignKey {
                name: fk.name.trim().to_string(),
                column: fk.column.clone(),
                ref_table: TableRef { schema: self.obj.schema.clone(), name: fk.ref_table.clone() },
                ref_column: fk.ref_column.clone(),
                on_delete: fk.on_delete.clone(),
                on_update: fk.on_update.clone(),
            });
        }
        out
    }

    pub fn structure_sql(&self, d: crate::model::Dialect) -> Vec<String> {
        self.structure_changes().iter().flat_map(|c| sqlgen::structure_sql(d, &self.table(), c)).collect()
    }

    // ---------- UI ----------

    pub fn ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, catalog: &super::editor::Catalog) {
        self.ensure_loaded(cx);
        self.poll(cx);
        let pal = p();
        let editable = self.editable(cx.session.cfg.read_only);

        // Toolbar: name and count, the view switch, the row filter, density; row actions on the right.
        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 8.0);
                ui.set_min_height(32.0);
                ui.label(RichText::new(&self.obj.name).font(super::mono_medium(14.0)).color(pal.text));
                if self.obj.kind.has_rows() {
                    let shown = self.data.rows.len();
                    let meta = match self.count {
                        Some(t) if t as usize == shown && self.req.offset == 0 => format!("{} rows", fmt_count(t)),
                        Some(t) => format!("{}–{} of {} rows", fmt_count((self.req.offset + 1) as u64), fmt_count((self.req.offset + shown) as u64), fmt_count(t)),
                        None => format!("{shown} rows"),
                    };
                    ui.label(RichText::new(meta).font(egui::FontId::monospace(12.0)).color(pal.text_faint));
                    let mut v = self.view;
                    let mut views = vec![(TableView::Data, "Data"), (TableView::Structure, "Structure")];
                    if self.obj.kind == ObjectKind::Table && !matches!(cx.session.dialect(), crate::model::Dialect::Redis | crate::model::Dialect::Elastic) {
                        views.push((TableView::Relations, "Relations"));
                    }
                    views.push((TableView::Profile, "Profile"));
                    if super::form::seg_well(ui, &mut v, &views, 0.0) {
                        self.view = v;
                    }
                }
                if self.view == TableView::Data && self.obj.kind.has_rows() {
                    let r = super::form::field(ui, &mut self.quick_filter, "Filter rows", Icon::Filter, 220.0);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let q = self.quick_filter.trim().to_string();
                        self.req.filters.retain(|f| !(f.column.is_none() && f.op == FilterOp::Contains));
                        if !q.is_empty() {
                            self.req.filters.push(Filter { column: None, op: FilterOp::Contains, value: q, enabled: true, ..Default::default() });
                        }
                        self.req.offset = 0;
                        self.draft_filters = self.req.filters.clone();
                        self.reload(cx);
                    }
                    let advanced = self.req.filters.iter().any(|f| f.enabled && !(f.column.is_none() && f.op == FilterOp::Contains));
                    if super::tool_button(ui, Icon::Filter, "Filter by column (⌘F)", self.filters_open || advanced).clicked() {
                        self.toggle_filters();
                    }
                } else if icon_button(ui, Icon::Refresh, "Reload structure").clicked() {
                    self.load_meta(cx);
                }
                if self.is_busy() {
                    ui.add(egui::Spinner::new().size(14.0).color(pal.text_dim));
                }
                if self.view == TableView::Data && self.obj.kind.has_rows() {
                    // Reserve the whole action group so the wrapping toolbar cannot overlap the filter.
                    let paging = self.req.offset > 0 || self.data.rows.len() >= self.req.limit;
                    let tools_width = (if editable { 400.0 } else { 332.0 }) + if paging { 64.0 } else { 0.0 };
                    ui.allocate_ui_with_layout(vec2(tools_width, 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let mut d = self.density;
                        super::form::seg_well(ui, &mut d, &[(0, "Compact"), (1, "Cozy"), (2, "Roomy")], 0.0);
                        self.density = d;
                        ui.add_space(8.0);
                        self.row_actions_ui(ui, cx, editable);
                    });
                }
            });
            if self.view == TableView::Data && self.req.filters.iter().any(|f| f.enabled && !(f.column.is_none() && f.op == FilterOp::Contains)) {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| self.filter_tokens_ui(ui, cx));
            }
        });
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, super::line()));

        if self.view == TableView::Data && self.filters_open {
            self.filters_ui(ui, cx);
        }
        if let Some(err) = self.error.clone() {
            error_banner(ui, &err, &mut self.error);
        }
        if self.view == TableView::Data && !self.has_primary_key() && !self.pending.is_empty() && self.obj.kind == ObjectKind::Table {
            egui::Frame::new().fill(pal.warning.gamma_multiply(0.12)).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                ui.label(RichText::new("This table has no primary key, so rows are matched by all their values. A change can hit more than one row.").size(12.0).color(super::readable_on_tint(pal.warning)));
            });
        }

        let body = ui.max_rect();
        match self.view {
            TableView::Data => self.data_ui(ui, cx, editable),
            TableView::Structure => self.structure_ui(ui, cx, editable, catalog),
            TableView::Relations => self.relations_ui(ui, cx),
            TableView::Profile => self.profile_ui(ui, cx),
        }
        self.changes_dock(ui, body, cx);
        self.quick_look_ui(ui.ctx(), editable);
    }

    fn row_actions_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, editable: bool) {
        if super::tool_button(ui, Icon::Export, "Export rows", false).clicked() {
            cx.actions.push(WsAction::ExportTable(self.table(), self.req.clone()));
        }
        if editable {
            if super::tool_button(ui, Icon::Trash, "Delete selected rows (⌫)", false).clicked() {
                self.delete_selected();
            }
            if super::tool_button(ui, Icon::Plus, "Insert row (⌘I)", false).clicked() {
                self.insert_row();
            }
        }
        if super::tool_button(ui, Icon::Refresh, "Reload (⌘R)", false).clicked() {
            self.reload(cx);
        }
        self.pager_ui(ui, cx);
    }

    /// Applied filters as tokens; clicking one (or "+ Filter") opens the editor.
    fn filter_tokens_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx) {
        let pal = p();
        let mut remove = None;
        for (i, f) in self.req.filters.iter().enumerate().filter(|(_, f)| f.enabled) {
            let col = f.column.clone().unwrap_or_else(|| "any column".into());
            let op = f.op.label().to_lowercase();
            let val = if f.op.needs_second_value() { format!("{} and {}", f.value, f.value2) } else if f.op.needs_value() { f.value.clone() } else { String::new() };
            let text = format!("{col} {op} {val}");
            let g = ui.painter().layout_no_wrap(text.trim().to_string(), egui::FontId::monospace(11.5), super::readable_on_tint(pal.accent2));
            let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 34.0, 28.0), egui::Sense::click());
            let tint = Color32::from_rgba_unmultiplied(pal.accent2.r(), pal.accent2.g(), pal.accent2.b(), 28);
            ui.painter().rect_filled(r, CornerRadius::same(9), tint);
            ui.painter().rect_stroke(r, CornerRadius::same(9), Stroke::new(1.0, pal.accent2.gamma_multiply(0.45)), egui::StrokeKind::Inside);
            ui.painter().galley(egui::pos2(r.left() + 10.0, r.center().y - g.size().y / 2.0), g, pal.text);
            let x = egui::Rect::from_center_size(egui::pos2(r.right() - 12.0, r.center().y), vec2(14.0, 14.0));
            let hover_x = ui.rect_contains_pointer(x);
            paint_icon(ui.painter(), x.shrink(2.0), Icon::Close, if hover_x { pal.text } else { pal.text_faint });
            if resp.clicked() {
                if hover_x {
                    remove = Some(i);
                } else {
                    self.filters_open = true;
                    self.draft_filters = self.req.filters.clone();
                }
            }
        }
        if let Some(i) = remove {
            self.req.filters.remove(i);
            self.draft_filters = self.req.filters.clone();
            self.req.offset = 0;
            self.reload(cx);
        }
        let add = ui.add(egui::Button::new(RichText::new("+ Filter").size(12.0).color(pal.text_dim)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, pal.border)).corner_radius(CornerRadius::same(9)).min_size(vec2(0.0, 28.0)));
        if add.on_hover_text("Filter rows (⌘F)").clicked() {
            if self.filters_open {
                self.draft_filters.push(Filter::default());
            } else {
                self.toggle_filters();
            }
        }
    }

    /// The floating "N staged changes" capsule at the bottom of the card.
    fn changes_dock(&mut self, ui: &mut Ui, body: egui::Rect, cx: &mut TabCtx) {
        let structure = self.structure_changes().len();
        let n = self.pending.count() + structure;
        if n == 0 {
            return;
        }
        let pal = p();
        let compact = body.width() < 740.0;
        let edited = self.pending.edits.keys().map(|(r, _)| *r).filter(|r| !self.pending.deleted.contains(r)).collect::<BTreeSet<_>>().len();
        let (added, removed) = (self.pending.inserted.len(), self.pending.deleted.len());
        let id = egui::Id::unique(("changes-dock", self.id));
        egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(egui::pos2(body.center().x, body.bottom() - 70.0)).pivot(egui::Align2::CENTER_TOP).show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(pal.sheet())
                .stroke(Stroke::new(1.0, pal.glass_stroke))
                .corner_radius(CornerRadius::same(255))
                .shadow(egui::Shadow { offset: [0, 12], blur: 36, spread: 0, color: Color32::from_black_alpha(if pal.dark { 120 } else { 40 }) })
                .inner_margin(egui::Margin { left: 16, right: 8, top: 8, bottom: 8 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 11.0, pal.accent);
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, n.to_string(), super::semibold(12.0), pal.on_accent);
                        ui.label(RichText::new(if n == 1 { "staged change" } else { "staged changes" }).font(super::medium(13.0)));
                        let part = |ui: &mut Ui, sym: &str, c: Color32, text: String| {
                            ui.label(RichText::new(sym).color(c).strong());
                            ui.add_space(-8.0);
                            ui.label(RichText::new(text).size(12.0).color(pal.text_dim));
                        };
                        if !compact && edited > 0 {
                            part(ui, "●", pal.accent, format!("{edited} edited"));
                        }
                        if !compact && added > 0 {
                            part(ui, "+", pal.success, format!("{added} added"));
                        }
                        if !compact && removed > 0 {
                            part(ui, "−", pal.danger, format!("{removed} removed"));
                        }
                        if !compact && structure > 0 {
                            part(ui, "◆", pal.accent2, format!("{structure} structure"));
                        }
                        if ui.add(egui::Button::new(RichText::new("Discard").size(12.5).color(pal.text_dim)).fill(Color32::TRANSPARENT).corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 34.0))).clicked() {
                            self.discard();
                        }
                        let commit = egui::Button::new(RichText::new(if compact { "Review & commit" } else { "Review & commit  ⌘S" }).font(super::semibold(12.5)).color(pal.on_accent)).fill(pal.accent).corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 34.0));
                        if ui.add(commit).clicked() {
                            cx.actions.push(WsAction::PreviewCommit(self.id));
                        }
                    });
                });
        });
    }

    fn relations_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx) {
        let pal = p();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::same(18)).show(ui, |ui| {
                ui.label(super::title(format!("How {} connects", self.obj.name), 16.0));
                ui.label(dim("Foreign keys this table declares, and the ones in other tables that point here."));
                ui.add_space(14.0);
                let row = |ui: &mut Ui, arrow: &str, table: &str, schema: &str, cols: String, rule: &str, actions: &mut Vec<WsAction>| {
                    super::card_frame().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(arrow).font(egui::FontId::monospace(14.0)).color(pal.accent2));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(table).font(super::medium(13.5)));
                                ui.label(RichText::new(cols).font(egui::FontId::monospace(11.5)).color(pal.text_dim));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Open").clicked() {
                                    actions.push(WsAction::OpenObject { obj: DbObject { parent: None, arguments: None, schema: schema.to_string(), name: table.to_string(), kind: ObjectKind::Table }, structure: false, new_tab: true });
                                }
                                if !rule.is_empty() {
                                    let c = if rule == "CASCADE" { pal.danger } else { pal.text_faint };
                                    ui.label(RichText::new(format!("on delete {}", rule.to_lowercase())).size(11.5).color(super::readable_on_tint(c)));
                                }
                            });
                        });
                    });
                    ui.add_space(6.0);
                };
                super::section_label(ui, &format!("References · {}", self.fks.len()));
                ui.add_space(6.0);
                if self.fks.is_empty() {
                    ui.label(faint("This table declares no foreign keys."));
                }
                for fk in self.fks.clone() {
                    row(ui, "→", &fk.ref_table, &fk.ref_schema, format!("{} → {}.{}", fk.column, fk.ref_table, fk.ref_column), "", cx.actions);
                }
                ui.add_space(14.0);
                super::section_label(ui, &format!("Referenced by · {}", self.incoming.len()));
                ui.add_space(6.0);
                if self.incoming.is_empty() {
                    ui.label(faint(if self.incoming_task.is_some() { "Loading…" } else { "No other table points at this one." }));
                }
                for r in self.incoming.clone() {
                    row(ui, "←", &r.table, &r.schema, format!("{}.{} → {}", r.table, r.column, r.ref_column), &r.on_delete, cx.actions);
                }
            });
        });
    }

    fn profile_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx) {
        let pal = p();
        let d = cx.session.dialect();
        let sql_profile = !matches!(d, crate::model::Dialect::Redis | crate::model::Dialect::Mongo | crate::model::Dialect::Elastic);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().fill(pal.content).stroke(Stroke::new(1.0, pal.border)).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(super::title("Table profile", 17.0));
                        let scope = match &self.table_stats {
                            Some((n, _)) => format!("Exact statistics for {} rows matching filters. Distribution uses the loaded page.", fmt_count(*n)),
                            None => format!("Statistics use the {} loaded rows.", fmt_count(self.data.rows.len() as u64)),
                        };
                        ui.label(dim(scope));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.stats_task.is_some() { ui.add(egui::Spinner::new().color(pal.accent)); }
                        if sql_profile && self.stats_task.is_none() && primary_button(ui, "Profile whole table").clicked() {
                            self.start_table_stats(cx);
                        }
                    });
                });
                ui.add_space(16.0);
                if self.grid_meta.is_empty() {
                    super::empty_state(ui, Icon::Columns, "No columns to profile", "Load a table to see null counts, distinct values and distributions.");
                    return;
                }
                egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_min_width(ui.available_width().max(760.0));
                    egui::Grid::new(("profile-summary", self.id)).striped(true).num_columns(6).spacing([14.0, 8.0]).min_col_width(72.0).max_col_width(240.0).show(ui, |ui| {
                        for heading in ["Column", "Nulls", "Distinct", "Minimum", "Maximum", "Distribution · loaded page"] {
                            ui.label(RichText::new(heading).size(11.5).color(pal.text_dim).strong());
                        }
                        ui.end_row();
                        for meta in &self.grid_meta {
                            let whole = self.table_stats.as_ref().and_then(|(total, stats)| stats.iter().find(|stat| stat.name == meta.name).map(|stat| (*total, stat)));
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    if meta.primary_key { let (r, _) = ui.allocate_exact_size(vec2(13.0, 13.0), egui::Sense::hover()); paint_icon(ui.painter(), r, Icon::Key, pal.accent); }
                                    ui.label(RichText::new(&meta.name).strong());
                                });
                                ui.label(RichText::new(&meta.data_type).font(egui::FontId::monospace(10.5)).color(pal.text_faint));
                            });
                            if let Some((total, stats)) = whole {
                                let nulls = total.saturating_sub(stats.non_null);
                                ui.label(RichText::new(format!("{} · {:.1}%", fmt_count(nulls), if total == 0 { 0.0 } else { nulls as f64 * 100.0 / total as f64 })).monospace().size(11.5));
                                ui.label(RichText::new(fmt_count(stats.distinct)).monospace().size(11.5));
                                ui.label(stats.min.as_deref().map(|v| grid::first_line(v, 22)).unwrap_or_else(|| "—".into())).on_hover_text(stats.min.as_deref().unwrap_or("No comparable value"));
                                ui.label(stats.max.as_deref().map(|v| grid::first_line(v, 22)).unwrap_or_else(|| "—".into())).on_hover_text(stats.max.as_deref().unwrap_or("No comparable value"));
                            } else if let Some(profile) = &meta.profile {
                                ui.label(RichText::new(format!("{:.1}%", profile.null_frac * 100.0)).monospace().size(11.5));
                                ui.label(RichText::new(fmt_count(profile.distinct as u64)).monospace().size(11.5));
                                ui.label("—"); ui.label("—");
                            } else {
                                for _ in 0..4 { ui.label("—"); }
                            }
                            ui.vertical(|ui| {
                                if let Some(profile) = &meta.profile {
                                    let (rect, response) = ui.allocate_exact_size(vec2(214.0, 30.0), egui::Sense::hover());
                                    let bars = profile.bars.len().max(1) as f32;
                                    let gap = 2.0;
                                    let width = ((rect.width() - gap * (bars - 1.0)) / bars).max(1.0);
                                    for (i, height) in profile.bars.iter().enumerate() {
                                        let x = rect.left() + i as f32 * (width + gap);
                                        let y = rect.bottom() - (height * rect.height()).max(2.0);
                                        // One measure, one hue: the bars differ by height, not by colour.
                                        let color = pal.chart[0];
                                        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x, y), egui::pos2(x + width, rect.bottom())), CornerRadius::same(2), color);
                                    }
                                    response.on_hover_text(format!("{} · {} distinct values in the loaded page", meta.name, fmt_count(profile.distinct as u64)));
                                    if profile.kind == grid::ProfileKind::Categorical {
                                        ui.horizontal_wrapped(|ui| {
                                            for (i, (value, count)) in profile.categories.iter().take(3).enumerate() {
                                                ui.horizontal(|ui| {
                                                    let (dot, _) = ui.allocate_exact_size(vec2(7.0, 7.0), egui::Sense::hover());
                                                    ui.painter().circle_filled(dot.center(), 3.0, pal.chart[0].gamma_multiply(1.0 - i as f32 * 0.25));
                                                    ui.label(RichText::new(grid::first_line(value, 14)).size(10.5));
                                                    ui.label(faint(count.to_string()).size(10.0));
                                                });
                                            }
                                        });
                                    }
                                } else {
                                    ui.label(faint("No loaded values"));
                                }
                            });
                            ui.end_row();
                        }
                    });
                });
            });
        });
    }

    fn start_table_stats(&mut self, cx: &TabCtx) {
        let d = cx.session.dialect();
        let t = self.table();
        let cols = self.columns.clone();
        let req = self.req.clone();
        let where_ = sqlgen::where_clause(d, &req, &cols);
        let parts: Vec<String> = cols
            .iter()
            .flat_map(|c| {
                let q = sqlgen::quote_ident(d, &c.name);
                let comparable = !c.data_type.to_lowercase().contains("json") && !c.data_type.to_lowercase().contains("blob") && !c.data_type.to_lowercase().contains("bytea");
                let txt = |e: String| match d {
                    crate::model::Dialect::Postgres => format!("{e}::text"),
                    crate::model::Dialect::MySql => format!("CAST({e} AS CHAR)"),
                    crate::model::Dialect::SqlServer => format!("CAST({e} AS NVARCHAR(200))"),
                    crate::model::Dialect::ClickHouse => format!("toString({e})"),
                    _ => format!("CAST({e} AS TEXT)"),
                };
                vec![
                    format!("COUNT({q})"),
                    if comparable { format!("COUNT(DISTINCT {q})") } else { "NULL".into() },
                    if comparable { txt(format!("MIN({q})")) } else { "NULL".into() },
                    if comparable { txt(format!("MAX({q})")) } else { "NULL".into() },
                ]
            })
            .collect();
        let sql = format!("SELECT COUNT(*), {} FROM {}{}", parts.join(", "), sqlgen::qualified(d, &t), where_);
        cx.session.log(sql.clone(), true);
        self.stats_task = Some(cx.session.spawn(cx.ctx, move |drv| async move {
            let r = drv.lock().await.query_one(&sql).await?;
            let row = r.rows.first().cloned().unwrap_or_default();
            let num = |i: usize| row.get(i).cloned().flatten().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u64;
            let total = num(0);
            let stats = cols
                .iter()
                .enumerate()
                .map(|(i, c)| ColumnStats { name: c.name.clone(), non_null: num(1 + i * 4), distinct: num(2 + i * 4), min: row.get(3 + i * 4).cloned().flatten(), max: row.get(4 + i * 4).cloned().flatten() })
                .collect();
            Ok((total, stats))
        }));
    }

    pub fn toggle_filters(&mut self) {
        self.filters_open = !self.filters_open;
        if self.filters_open && self.draft_filters.is_empty() {
            self.draft_filters = if self.req.filters.is_empty() { vec![Filter::default()] } else { self.req.filters.clone() };
        }
    }

    fn pager_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx) {
        let shown = self.data.rows.len();
        let total = self.count;
        let next_enabled = shown >= self.req.limit && total.map(|t| (self.req.offset + shown) < t as usize).unwrap_or(true);
        if !next_enabled && self.req.offset == 0 {
            return;
        }
        ui.add_enabled_ui(next_enabled, |ui| {
            if icon_button(ui, Icon::ChevronRight, "Next page").clicked() {
                self.req.offset += self.req.limit;
                self.reload(cx);
            }
        });
        ui.add_enabled_ui(self.req.offset > 0, |ui| {
            if icon_button(ui, Icon::ChevronLeft, "Previous page").clicked() {
                self.req.offset = self.req.offset.saturating_sub(self.req.limit);
                self.reload(cx);
            }
        });
    }

    fn filters_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx) {
        let pal = p();
        let names: Vec<String> = if self.columns.is_empty() { self.data.columns.clone() } else { self.columns.iter().map(|c| c.name.clone()).collect() };
        let mut apply = false;
        let well = if pal.dark { Color32::from_black_alpha(60) } else { Color32::from_black_alpha(8) };
        egui::Frame::new().fill(well).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            let mut remove = None;
            let n = self.draft_filters.len();
            for (i, f) in self.draft_filters.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut f.enabled, "");
                    egui::ComboBox::from_id_salt(("fcol", self.id, i)).width(150.0).selected_text(f.column.clone().unwrap_or_else(|| "Any column".into())).show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.column, None, "Any column");
                        for n in &names {
                            ui.selectable_value(&mut f.column, Some(n.clone()), n);
                        }
                    });
                    egui::ComboBox::from_id_salt(("fop", self.id, i)).width(110.0).selected_text(f.op.label()).show_ui(ui, |ui| {
                        for op in FilterOp::ALL {
                            ui.selectable_value(&mut f.op, op, op.label());
                        }
                    });
                    if f.op.needs_value() {
                        let hint = match f.op {
                            FilterOp::In | FilterOp::NotIn => "a, b, c",
                            FilterOp::Raw => "raw condition, e.g. age > 18",
                            _ => "value",
                        };
                        let r = ui.add(crate::ui::line_edit(&mut f.value).hint_text(hint).desired_width(if f.op.needs_second_value() { 140.0 } else { 260.0 }));
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            apply = true;
                        }
                        if f.op.needs_second_value() {
                            ui.label(dim("and"));
                            let r2 = ui.add(crate::ui::line_edit(&mut f.value2).desired_width(140.0));
                            if r2.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                apply = true;
                            }
                        }
                    }
                    if n > 1 && icon_button(ui, Icon::Close, "Remove filter").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.draft_filters.remove(i);
            }
            ui.horizontal(|ui| {
                if ui.button("+ Add filter").clicked() {
                    self.draft_filters.push(Filter::default());
                }
                if self.draft_filters.len() > 1 {
                    let mut any = self.req.match_any;
                    if segmented(ui, &mut any, &[(false, "Match all"), (true, "Match any")]) {
                        self.req.match_any = any;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, "Apply").clicked() {
                        apply = true;
                    }
                    if secondary_button(ui, "Clear").clicked() {
                        self.draft_filters = vec![Filter::default()];
                        self.req.filters.clear();
                        self.req.offset = 0;
                        self.reload(cx);
                    }
                });
            });
        });
        if apply {
            self.req.filters = self.draft_filters.clone();
            self.req.offset = 0;
            self.reload(cx);
        }
    }

    fn data_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, editable: bool) {
        if self.data.columns.is_empty() && self.data_task.is_some() {
            ui.centered_and_justified(|ui| ui.add(egui::Spinner::new().size(22.0)));
            return;
        }
        let opts = grid::GridOptions {
            id: egui::Id::unique(("table-grid", self.id)),
            editable,
            null_text: &cx.settings.null_display,
            font_size: cx.settings.grid_font_size,
            row_height: Some([22.0, 28.0, 36.0][self.density.min(2) as usize]),
            show_row_numbers: cx.settings.show_row_numbers,
            alternating_rows: cx.settings.alternating_rows,
        };
        let mut grid_state = std::mem::take(&mut self.grid);
        let src = TableSource { tab: self };
        let events = grid::show(ui, &src, &mut grid_state, &opts);
        self.grid = grid_state;
        for ev in events {
            self.handle_grid_event(ev, cx);
        }
    }

    fn handle_grid_event(&mut self, ev: GridEvent, cx: &mut TabCtx) {
        match ev {
            GridEvent::Edit { row, col, value } => self.apply_edit(row, col, value),
            GridEvent::Sort { col, ascending, add } => {
                let name = self.grid_meta[col].name.clone();
                let current = self.req.sort.iter().position(|(c, _)| *c == name);
                let next = match (ascending, current.map(|i| self.req.sort[i].1)) {
                    (Some(a), _) => Some(a),
                    (None, None) => Some(true),
                    (None, Some(true)) => Some(false),
                    (None, Some(false)) => None,
                };
                if !add {
                    self.req.sort.retain(|(c, _)| *c == name);
                }
                match (current.and_then(|_| self.req.sort.iter().position(|(c, _)| *c == name)), next) {
                    (Some(i), Some(a)) => self.req.sort[i].1 = a,
                    (Some(i), None) => {
                        self.req.sort.remove(i);
                    }
                    (None, Some(a)) => self.req.sort.push((name, a)),
                    (None, None) => {}
                }
                self.req.offset = 0;
                self.reload(cx);
            }
            GridEvent::FollowForeignKey { row, col } => {
                let name = &self.grid_meta[col].name;
                if let Some(fk) = self.fks.iter().find(|f| &f.column == name) {
                    let src = TableSource { tab: self };
                    let value = grid::cell_value(&src, row, col);
                    cx.actions.push(WsAction::OpenTableFiltered {
                        table: TableRef { schema: fk.ref_schema.clone(), name: fk.ref_table.clone() },
                        column: fk.ref_column.clone(),
                        value,
                    });
                }
            }
            GridEvent::CopyCells => {
                let src = TableSource { tab: self };
                let rows = grid::selection_rows(&self.grid);
                let text = if rows.len() > 1 {
                    grid::format_rows(&src, &rows, grid::CopyFormat::Tsv, &cx.settings.null_display, None)
                } else if let Some((r, c)) = self.grid.cursor {
                    grid::cell_value(&src, r, c).unwrap_or_else(|| cx.settings.null_display.clone())
                } else {
                    String::new()
                };
                super::copy(cx.ctx, text);
            }
            GridEvent::CopyRows(fmt) => {
                let src = TableSource { tab: self };
                let rows = grid::selection_rows(&self.grid);
                let d = cx.session.dialect();
                let t = self.table();
                super::copy(cx.ctx, grid::format_rows(&src, &rows, fmt, &cx.settings.null_display, Some((&d, &t))));
                cx.actions.push(WsAction::Toast(ToastKind::Info, format!("Copied {} row{}", rows.len(), if rows.len() == 1 { "" } else { "s" })));
            }
            GridEvent::CopyColumnName(c) => super::copy(cx.ctx, self.grid_meta[c].name.clone()),
            GridEvent::DeleteRows => self.delete_selected(),
            GridEvent::DuplicateRow => self.duplicate_rows(),
            GridEvent::InsertRow => self.insert_row(),
            GridEvent::FilterByValue { col, value } => {
                let column = Some(self.grid_meta[col].name.clone());
                let f = match value {
                    Some(v) => Filter { column, op: FilterOp::Eq, value: v, ..Default::default() },
                    None => Filter { column, op: FilterOp::IsNull, ..Default::default() },
                };
                self.add_filter(f);
                self.reload(cx);
            }
            GridEvent::QuickLook { row, col } => {
                let src = TableSource { tab: self };
                let text = grid::cell_value(&src, row, col).unwrap_or_default();
                self.quick_look = Some(QuickLook { row, col, viewer: super::viewer::ValueViewer::new(text) });
            }
            GridEvent::HideColumn(c) => {
                self.grid.hidden.insert(c);
            }
            GridEvent::SelectionChanged => {}
        }
    }

    fn quick_look_ui(&mut self, ctx: &egui::Context, editable: bool) {
        let Some(ql) = &mut self.quick_look else { return };
        let mut open = true;
        let mut save = None;
        let title = self.grid_meta.get(ql.col).map(|m| m.name.clone()).unwrap_or_default();
        egui::Window::new(format!("Quick look · {title}")).id(egui::Id::unique(("ql", self.id))).open(&mut open).default_size([600.0, 420.0]).collapsible(false).show(ctx, |ui| {
            ql.viewer.ui(ui, editable, egui::Id::unique(("ql-viewer", self.id)));
            if editable {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, "  Apply to cell  ").clicked() {
                        save = Some((ql.row, ql.col, ql.viewer.value_for_cell()));
                    }
                });
            }
        });
        if let Some((r, c, t)) = save {
            self.apply_edit(r, c, EditValue::Text(t));
            self.quick_look = None;
        } else if !open {
            self.quick_look = None;
        }
    }

    fn structure_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, editable: bool, catalog: &super::editor::Catalog) {
        let pal = p();
        let d = cx.session.dialect();
        let form = meta::structure_form(d);
        let can_alter = editable && meta::supports(d, StructureFeature::EditStructure) && matches!(self.obj.kind, ObjectKind::Table | ObjectKind::Collection);
        let alter_columns = can_alter && form.columns_editable;
        let edit_keys = alter_columns && form.keys_editable;
        if !self.obj.kind.has_rows() || self.obj.kind == ObjectKind::Keys {
            self.ddl_ui(ui);
            return;
        }
        let is_mongo = d == crate::model::Dialect::Mongo;
        let item = form.item.to_lowercase();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 20, top: 14, bottom: 24 }).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(super::title(form.title, 16.0));
                        ui.label(dim(format!("{}.{} · {} {item}s · {} indexes · {} foreign keys", self.obj.schema, self.obj.name, self.structure.draft.len(), self.indexes.len(), self.fks.len())).size(12.5));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if can_alter { super::env_chip(ui, "Editable", pal.success, false); }
                        else { ui.label(faint("Read only")); }
                    });
                });
                if is_mongo {
                    ui.label(faint("Fields and types are sampled from up to 100 documents. Indexes can be edited.").size(12.0));
                }
                ui.add_space(16.0);

                // ---- columns ----
                ui.horizontal(|ui| {
                    super::section_label(ui, &format!("{}s · {}", form.item, self.structure.draft.len()));
                    if alter_columns && ui.small_button(format!("+ {}", form.item)).clicked() {
                        self.structure.draft.push(ColumnInfo { name: String::new(), data_type: form.default_type.into(), nullable: true, ..Default::default() });
                    }
                });
                if edit_keys {
                    ui.label(faint("Click a key cell to toggle the primary key. Right-click it to add a foreign key.").size(11.5));
                }
                ui.add_space(6.0);
                let fk_columns: BTreeMap<String, String> = self
                    .fks
                    .iter()
                    .filter(|fk| !self.structure.dropped_fks.contains(&fk.name))
                    .map(|fk| (fk.column.clone(), format!("{}.{}", fk.ref_table, fk.ref_column)))
                    .chain(self.structure.added_fks.iter().map(|fk| (fk.column.clone(), format!("{}.{}", fk.ref_table, fk.ref_column))))
                    .collect();
                let mut drop_idx = None;
                let mut start_fk: Option<String> = None;
                egui::ScrollArea::horizontal().id_salt(("structure-cols", self.id)).auto_shrink([false, true]).show(ui, |ui| {
                    let width = ui.available_width().max(820.0);
                    ui.set_width(width);
                    let (show_null, show_default, show_comment) = (form.nullable.shown(), form.default.shown(), form.comment.shown());
                    let (dot_w, key_w, null_w, act_w) = (8.0, 44.0, 64.0, 30.0);
                    let (name_w, mut type_w, mut default_w) = if width < 1000.0 { (150.0, 210.0, 120.0) } else if width < 1400.0 { (190.0, 250.0, 170.0) } else { (220.0, 270.0, 200.0) };
                    let gap = ui.spacing().item_spacing.x;
                    let cells = 5 + show_null as usize + show_default as usize + show_comment as usize;
                    let used = dot_w + key_w + name_w + type_w + act_w + gap * cells as f32 + 12.0
                        + if show_null { null_w } else { 0.0 }
                        + if show_default { default_w } else { 0.0 };
                    // The last visible text field takes the leftover width.
                    let spare = (width - used).max(140.0);
                    let comment_w = if show_comment { spare } else { 0.0 };
                    if !show_comment {
                        if show_default { default_w += spare } else { type_w += spare }
                    }
                    let head = |ui: &mut Ui, w: f32, text: &str, centered: bool| {
                        let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), egui::Sense::hover());
                        let (pos, align) = if centered { (r.center(), egui::Align2::CENTER_CENTER) } else { (egui::pos2(r.left() + 9.0, r.center().y), egui::Align2::LEFT_CENTER) };
                        ui.painter().text(pos, align, text, egui::FontId::proportional(11.0), pal.text_faint);
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(6.0 + dot_w + gap);
                        head(ui, key_w, "Key", true);
                        head(ui, name_w, &format!("{} name", form.item), false);
                        head(ui, type_w, if is_mongo { "BSON types seen" } else { "Data type" }, false);
                        if show_null {
                            head(ui, null_w, if is_mongo { "Optional" } else { "Nullable" }, true);
                        }
                        if show_default {
                            head(ui, default_w, "Default", false);
                        }
                        if show_comment {
                            head(ui, comment_w, "Comment", false);
                        }
                    });
                    ui.add_space(2.0);
                    let new_only = format!("Can only be set on a new {item}");
                    for i in 0..self.structure.draft.len() {
                        let is_new = i >= self.structure.original_len;
                        let dropped = self.column_dropped(i);
                        let changed = is_new || self.columns.get(i) != Some(&self.structure.draft[i]);
                        let fk_target = fk_columns.get(&self.structure.draft[i].name).cloned();
                        let c = &mut self.structure.draft[i];
                        egui::Frame::new()
                            .fill(if i % 2 == 0 { pal.surface.gamma_multiply(0.32) } else { Color32::TRANSPARENT })
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(6, 3))
                            .show(ui, |ui| {
                            ui.set_width(width - 12.0);
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(dot_w, 32.0), egui::Sense::hover());
                                if changed || dropped {
                                    ui.painter().circle_filled(r.center(), 3.0, if dropped { pal.danger } else { pal.warning });
                                }
                                let enabled = alter_columns && !dropped;
                                let key_enabled = edit_keys && !dropped;
                                let (kr, key) = ui.allocate_exact_size(vec2(key_w, 32.0), if key_enabled { egui::Sense::click() } else { egui::Sense::hover() });
                                let icon_at = |dx: f32| egui::Rect::from_center_size(kr.center() + vec2(dx, 0.0), vec2(15.0, 15.0));
                                match (c.primary_key, fk_target.is_some()) {
                                    (true, true) => {
                                        paint_icon(ui.painter(), icon_at(-9.0), Icon::Key, pal.warning);
                                        paint_icon(ui.painter(), icon_at(9.0), Icon::Link, pal.accent2);
                                    }
                                    (true, false) => paint_icon(ui.painter(), icon_at(0.0), Icon::Key, pal.warning),
                                    (false, true) => paint_icon(ui.painter(), icon_at(0.0), Icon::Link, pal.accent2),
                                    (false, false) if key.hovered() && key_enabled => paint_icon(ui.painter(), icon_at(0.0), Icon::Key, pal.text_faint),
                                    _ => {}
                                }
                                let tip = match (&fk_target, c.primary_key) {
                                    (Some(t), true) => format!("Primary key · references {t}"),
                                    (Some(t), false) => format!("References {t}"),
                                    (None, true) => "Primary key".to_string(),
                                    (None, false) if key_enabled => "Click to make this part of the primary key".to_string(),
                                    _ => String::new(),
                                };
                                let key = if tip.is_empty() { key } else { key.on_hover_text(tip) };
                                if key_enabled {
                                    if key.clicked() {
                                        c.primary_key = !c.primary_key;
                                        // Every engine requires key columns to be NOT NULL.
                                        if c.primary_key {
                                            c.nullable = false;
                                        }
                                    }
                                    key.context_menu(|ui| {
                                        if ui.checkbox(&mut c.primary_key, "Primary key").changed() {
                                            if c.primary_key {
                                                c.nullable = false;
                                            }
                                            ui.close();
                                        }
                                        if ui.button("Add foreign key…").clicked() {
                                            start_fk = Some(c.name.clone());
                                            ui.close();
                                        }
                                    });
                                }
                                let attr = |a: meta::Attr| enabled && a.editable(is_new);
                                let hover = |r: egui::Response| r.on_disabled_hover_text(if enabled { new_only.as_str() } else { "" });
                                ui.add_enabled_ui(enabled, |ui| ui.add_sized(vec2(name_w, 32.0), crate::ui::line_edit(&mut c.name).desired_width(name_w)));
                                if form.types.is_empty() {
                                    ui.add_enabled_ui(attr(form.data_type), |ui| ui.add_sized(vec2(type_w, 32.0), crate::ui::line_edit(&mut c.data_type).desired_width(type_w).font(egui::TextStyle::Monospace)));
                                } else {
                                    hover(type_picker(ui, &mut c.data_type, type_w, form.types, attr(form.data_type)));
                                }
                                if show_null {
                                    let resp = ui
                                        .allocate_ui_with_layout(vec2(null_w, 32.0), egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                                            ui.add_enabled(attr(form.nullable), egui::Checkbox::without_text(&mut c.nullable))
                                        })
                                        .inner;
                                    hover(resp);
                                }
                                if show_default {
                                    let mut def = c.default.clone().unwrap_or_default();
                                    let resp = ui.add_enabled_ui(attr(form.default), |ui| ui.add_sized(vec2(default_w, 32.0), crate::ui::line_edit(&mut def).desired_width(default_w).hint_text("NULL"))).inner;
                                    if resp.changed() {
                                        c.default = (!def.trim().is_empty()).then_some(def);
                                    }
                                    hover(resp);
                                }
                                if show_comment {
                                    let mut com = c.comment.clone().unwrap_or_default();
                                    if ui.add_enabled_ui(attr(form.comment), |ui| ui.add_sized(vec2(comment_w, 32.0), crate::ui::line_edit(&mut com).desired_width(comment_w))).inner.changed() {
                                        c.comment = (!com.trim().is_empty()).then_some(com);
                                    }
                                }
                                if alter_columns {
                                    if icon_button(ui, if dropped { Icon::Refresh } else { Icon::Trash }, &if dropped { format!("Restore {item}") } else { format!("Drop {item}") }).clicked() {
                                        drop_idx = Some(i);
                                    }
                                }
                            });
                        });
                    }
                });
                if let Some(i) = drop_idx {
                    if i >= self.structure.original_len {
                        self.structure.draft.remove(i);
                    } else {
                        let name = self.columns[i].name.clone();
                        if !self.structure.dropped.remove(&name) {
                            self.structure.dropped.insert(name);
                        }
                    }
                }
                if let Some(column) = start_fk {
                    self.structure.new_fk = Some(FkDraft { column, on_delete: form.fk_actions.first().map(|a| a.to_string()).unwrap_or_default(), on_update: form.fk_actions.first().map(|a| a.to_string()).unwrap_or_default(), ..Default::default() });
                }

                // ---- indexes ----
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    super::section_label(ui, &format!("Indexes · {}", self.indexes.len() + self.structure.added_indexes.len()));
                    if can_alter && self.structure.new_index.is_none() && ui.small_button("+ Index").clicked() {
                        self.structure.new_index = Some(IndexDraft { name: format!("idx_{}_", self.obj.name), method: form.index_methods.first().map(|m| m.to_string()).unwrap_or_default(), ..Default::default() });
                    }
                });
                ui.add_space(6.0);
                egui::Grid::new(("idx", self.id)).striped(true).num_columns(5).min_col_width(60.0).spacing([20.0, 8.0]).show(ui, |ui| {
                    for h in ["Name", if is_mongo { "Keys" } else { "Columns" }, "Unique", if form.index_methods.is_empty() { "Method" } else { "Type" }, ""] {
                        ui.label(faint(h).size(11.0));
                    }
                    ui.end_row();
                    let mut toggle = None;
                    for ix in &self.indexes {
                        let dropped = self.structure.dropped_indexes.contains(&ix.name);
                        let name = RichText::new(&ix.name).monospace();
                        ui.label(if dropped { name.strikethrough().color(pal.danger) } else { name });
                        ui.label(&ix.columns);
                        ui.label(if ix.primary { "primary" } else if ix.unique { "yes" } else { "" });
                        ui.label(faint(&ix.method));
                        if can_alter && !ix.primary && icon_button(ui, if dropped { Icon::Refresh } else { Icon::Trash }, if dropped { "Keep index" } else { "Drop index" }).clicked() {
                            toggle = Some(ix.name.clone());
                        } else if !can_alter || ix.primary {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                    let mut remove = None;
                    for (i, ix) in self.structure.added_indexes.iter().enumerate() {
                        ui.label(RichText::new(&ix.name).monospace().color(pal.success));
                        ui.label(&ix.columns);
                        ui.label(if ix.unique { "yes" } else { "" });
                        ui.label(faint(if ix.method.is_empty() { "new" } else { &ix.method }));
                        if icon_button(ui, Icon::Trash, "Discard new index").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                    if let Some(i) = remove {
                        self.structure.added_indexes.remove(i);
                    }
                    if let Some(n) = toggle {
                        if !self.structure.dropped_indexes.remove(&n) {
                            self.structure.dropped_indexes.insert(n);
                        }
                    }
                });
                let mut add_index = false;
                let mut cancel = false;
                if let Some(ix) = &mut self.structure.new_index {
                    ui.add_space(8.0);
                    super::card_frame().show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(crate::ui::line_edit(&mut ix.name).desired_width(180.0).hint_text("index name"));
                            ui.add(crate::ui::line_edit(&mut ix.columns).desired_width(220.0).hint_text(form.index_hint));
                            if !form.index_methods.is_empty() {
                                egui::ComboBox::from_id_salt(("idx-method", self.id)).width(130.0).selected_text(ix.method.as_str()).show_ui(ui, |ui| {
                                    for m in form.index_methods {
                                        ui.selectable_value(&mut ix.method, m.to_string(), *m);
                                    }
                                });
                            }
                            if form.unique_index {
                                ui.checkbox(&mut ix.unique, "Unique");
                            }
                            if ui.add_enabled(!ix.name.trim().is_empty() && !ix.column_list().is_empty(), egui::Button::new("Add")).clicked() {
                                ix.name = ix.name.trim().to_string();
                                add_index = true;
                            }
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                        });
                    });
                }
                if add_index {
                    self.structure.added_indexes.extend(self.structure.new_index.take());
                } else if cancel {
                    self.structure.new_index = None;
                }

                // ---- foreign keys ----
                if !self.fks.is_empty() || edit_keys {
                    ui.add_space(22.0);
                    ui.horizontal(|ui| {
                        super::section_label(ui, &format!("Foreign keys · {}", self.fks.len() + self.structure.added_fks.len()));
                        if edit_keys && self.structure.new_fk.is_none() && ui.small_button("+ Foreign key").clicked() {
                            let first = form.fk_actions.first().map(|a| a.to_string()).unwrap_or_default();
                            self.structure.new_fk = Some(FkDraft { on_delete: first.clone(), on_update: first, ..Default::default() });
                        }
                    });
                    ui.add_space(6.0);
                    if self.fks.is_empty() && self.structure.added_fks.is_empty() && self.structure.new_fk.is_none() {
                        ui.label(faint("No foreign keys. Add one to link a column to another table's key.").size(12.0));
                    } else {
                        self.fk_list_ui(ui, cx, edit_keys);
                    }
                    if self.structure.new_fk.is_some() {
                        ui.add_space(8.0);
                        self.new_fk_ui(ui, &form, catalog);
                    }
                }

                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    super::section_label(ui, "Definition");
                    if ui.small_button(if self.structure.show_ddl { "Hide" } else { "Show" }).clicked() {
                        self.structure.show_ddl = !self.structure.show_ddl;
                    }
                });
                if self.structure.show_ddl {
                    self.ddl_block(ui);
                }
            });
        });
    }

    fn fk_list_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx, edit_keys: bool) {
        let pal = p();
        egui::Grid::new(("fks", self.id)).striped(true).num_columns(6).min_col_width(60.0).spacing([20.0, 8.0]).show(ui, |ui| {
            for h in ["Name", "Column", "References", "On delete", "On update", ""] {
                ui.label(faint(h).size(11.0));
            }
            ui.end_row();
            let mut toggle = None;
            for fk in &self.fks {
                let dropped = self.structure.dropped_fks.contains(&fk.name);
                let name = RichText::new(&fk.name).monospace();
                ui.label(if dropped { name.strikethrough().color(pal.danger) } else { name });
                ui.label(&fk.column);
                if ui.link(format!("{}.{}", fk.ref_table, fk.ref_column)).on_hover_text("Open the referenced table's structure").clicked() {
                    cx.actions.push(WsAction::OpenObject {
                        obj: DbObject { parent: None, arguments: None, schema: fk.ref_schema.clone(), name: fk.ref_table.clone(), kind: ObjectKind::Table },
                        structure: true,
                        new_tab: true,
                    });
                }
                ui.label("");
                ui.label("");
                if edit_keys && icon_button(ui, if dropped { Icon::Refresh } else { Icon::Trash }, if dropped { "Keep foreign key" } else { "Drop foreign key" }).clicked() {
                    toggle = Some(fk.name.clone());
                } else if !edit_keys {
                    ui.label("");
                }
                ui.end_row();
            }
            let mut remove = None;
            for (i, fk) in self.structure.added_fks.iter().enumerate() {
                ui.label(RichText::new(&fk.name).monospace().color(pal.success));
                ui.label(&fk.column);
                ui.label(format!("{}.{}", fk.ref_table, fk.ref_column));
                ui.label(faint(&fk.on_delete));
                ui.label(faint(&fk.on_update));
                if icon_button(ui, Icon::Trash, "Discard new foreign key").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
            if let Some(i) = remove {
                self.structure.added_fks.remove(i);
            }
            if let Some(n) = toggle {
                if !self.structure.dropped_fks.remove(&n) {
                    self.structure.dropped_fks.insert(n);
                }
            }
        });
    }

    /// The "add foreign key" form: this table's column → a table and column in the same schema.
    fn new_fk_ui(&mut self, ui: &mut Ui, form: &meta::StructureForm, catalog: &super::editor::Catalog) {
        let columns: Vec<String> = (0..self.structure.draft.len()).filter(|&i| !self.column_dropped(i)).map(|i| self.structure.draft[i].name.clone()).filter(|n| !n.trim().is_empty()).collect();
        let mut tables = catalog.tables.clone();
        tables.sort();
        let id = self.id;
        let table_name = self.obj.name.clone();
        let Some(fk) = &mut self.structure.new_fk else { return };
        let auto_name = format!("fk_{}_{}", table_name, if fk.column.is_empty() { "column" } else { &fk.column });
        let mut add = false;
        let mut cancel = false;
        super::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            ui.label(RichText::new("New foreign key").strong());
            ui.add_space(6.0);
            let label = |ui: &mut Ui, text: &str| {
                ui.add_sized(vec2(90.0, 30.0), egui::Label::new(dim(text).size(12.0)));
            };
            ui.horizontal(|ui| {
                label(ui, "Column");
                egui::ComboBox::from_id_salt(("fk-col", id)).width(200.0).selected_text(if fk.column.is_empty() { "Choose…" } else { fk.column.as_str() }).show_ui(ui, |ui| {
                    for c in &columns {
                        ui.selectable_value(&mut fk.column, c.clone(), c);
                    }
                });
            });
            ui.horizontal(|ui| {
                label(ui, "References");
                if tables.is_empty() {
                    ui.add(crate::ui::line_edit(&mut fk.ref_table).desired_width(200.0).hint_text("table"));
                } else {
                    let before = fk.ref_table.clone();
                    egui::ComboBox::from_id_salt(("fk-table", id)).width(200.0).selected_text(if fk.ref_table.is_empty() { "Choose a table…" } else { fk.ref_table.as_str() }).height(320.0).show_ui(ui, |ui| {
                        for t in &tables {
                            ui.selectable_value(&mut fk.ref_table, t.clone(), t);
                        }
                    });
                    // Point at the referenced table's id column when it has one.
                    if fk.ref_table != before {
                        let ref_cols = catalog.columns.get(&fk.ref_table);
                        fk.ref_column = ref_cols.and_then(|cols| cols.iter().find(|c| c.as_str() == "id").or(cols.first())).cloned().unwrap_or_default();
                    }
                }
                ui.label(faint("."));
                match catalog.columns.get(&fk.ref_table).filter(|c| !c.is_empty()) {
                    Some(ref_cols) => {
                        egui::ComboBox::from_id_salt(("fk-ref-col", id)).width(170.0).selected_text(if fk.ref_column.is_empty() { "column" } else { fk.ref_column.as_str() }).show_ui(ui, |ui| {
                            for c in ref_cols {
                                ui.selectable_value(&mut fk.ref_column, c.clone(), c);
                            }
                        });
                    }
                    None => {
                        ui.add(crate::ui::line_edit(&mut fk.ref_column).desired_width(170.0).hint_text("column"));
                    }
                }
            });
            if !form.fk_actions.is_empty() {
                ui.horizontal(|ui| {
                    label(ui, "On delete");
                    egui::ComboBox::from_id_salt(("fk-del", id)).width(130.0).selected_text(fk.on_delete.as_str()).show_ui(ui, |ui| {
                        for a in form.fk_actions {
                            ui.selectable_value(&mut fk.on_delete, a.to_string(), *a);
                        }
                    });
                    ui.add_space(12.0);
                    ui.label(dim("On update").size(12.0));
                    egui::ComboBox::from_id_salt(("fk-upd", id)).width(130.0).selected_text(fk.on_update.as_str()).show_ui(ui, |ui| {
                        for a in form.fk_actions {
                            ui.selectable_value(&mut fk.on_update, a.to_string(), *a);
                        }
                    });
                });
            }
            ui.horizontal(|ui| {
                label(ui, "Name");
                ui.add(crate::ui::line_edit(&mut fk.name).desired_width(300.0).hint_text(auto_name.as_str()));
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(98.0);
                let ready = !fk.column.is_empty() && !fk.ref_table.trim().is_empty() && !fk.ref_column.trim().is_empty();
                if ui.add_enabled(ready, egui::Button::new("Add foreign key")).clicked() {
                    if fk.name.trim().is_empty() {
                        fk.name = auto_name.clone();
                    }
                    add = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if add {
            self.structure.added_fks.extend(self.structure.new_fk.take().filter(FkDraft::ready));
        } else if cancel {
            self.structure.new_fk = None;
        }
    }

    fn ddl_ui(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::same(14)).show(ui, |ui| self.ddl_block(ui));
        });
    }

    fn ddl_block(&mut self, ui: &mut Ui) {
        let pal = p();
        let text = self.ddl.clone().unwrap_or_else(|| if self.meta_task.is_some() { "Loading…".into() } else { "-- definition unavailable".into() });
        ui.horizontal(|ui| {
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(text.clone());
            }
        });
        egui::Frame::new().fill(pal.surface).stroke(Stroke::new(1.0, pal.border)).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            let mut t = text.as_str();
            let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _w: f32| {
                let job = super::editor::highlight(buf.as_str(), egui::FontId::monospace(12.5), None);
                ui.fonts_mut(|f| f.layout_job(job))
            };
            ui.add(egui::TextEdit::multiline(&mut t).code_editor().desired_width(f32::INFINITY).layouter(&mut layouter));
        });
    }

    /// The inspector: the selected row's fields, JSON trees, geometry and linked rows.
    pub fn row_detail_ui(&mut self, ui: &mut Ui, cx: &mut TabCtx) {
        let pal = p();
        let editable = self.editable(cx.session.cfg.read_only);
        let Some((r, _)) = self.grid.cursor else {
            super::empty_state(ui, Icon::Rows, "No row selected", "Select a row to see all its fields");
            return;
        };
        let src = TableSource { tab: self };
        if r >= src.row_count() {
            return;
        }
        let state = src.row_state(r);
        let n = self.grid_meta.len();
        let pk_text: String = {
            let keys: Vec<usize> = self.grid_meta.iter().enumerate().filter(|(_, m)| m.primary_key).map(|(i, _)| i).collect();
            if keys.is_empty() || state == RowState::Inserted { format!("row {}", self.req.offset + r + 1) } else { keys.iter().map(|&c| grid::cell_value(&src, r, c).unwrap_or_else(|| "NULL".into())).collect::<Vec<_>>().join(" · ") }
        };
        let edited = (0..n).any(|c| src.cell_modified(r, c));
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Row {pk_text}")).font(super::medium(13.0)).color(pal.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let (label, tone) = match state {
                    RowState::Inserted => ("new", pal.success),
                    RowState::Deleted => ("removed", pal.danger),
                    RowState::Normal if edited => ("edited", pal.accent),
                    RowState::Normal => ("", pal.text_faint),
                };
                if !label.is_empty() {
                    super::env_chip(ui, label, tone, false);
                }
            });
        });
        ui.add_space(12.0);
        self.poll_linked(r, cx);
        let mut edits = Vec::new();
        let mut quick = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for c in 0..n {
                let meta = self.grid_meta[c].clone();
                let src = TableSource { tab: self };
                let value = grid::cell_value(&src, r, c);
                let modified = src.cell_modified(r, c);
                let original = if modified { self.data.rows.get(r).and_then(|row| row.get(c)).cloned().flatten() } else { None };
                // Field: a hairline, the name in mono (keys in their colours), the value under it.
                let top = ui.cursor().top();
                ui.painter().hline(ui.max_rect().x_range(), top + 0.5, Stroke::new(1.0, super::line()));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let name_color = if meta.primary_key { super::pk_color() } else if meta.foreign_key { super::fk_color() } else { pal.text_faint };
                    ui.label(RichText::new(&meta.name).font(egui::FontId::monospace(12.0)).color(name_color));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if modified {
                            ui.label(RichText::new("edited").font(egui::FontId::monospace(12.0)).color(pal.accent));
                        } else {
                            ui.label(RichText::new(meta.data_type.to_lowercase()).font(egui::FontId::monospace(12.0)).color(pal.text_faint));
                        }
                    });
                });
                ui.add_space(2.0);
                let json = value.as_deref().and_then(super::json_tree::parse_json);
                let geo = if json.is_none() { value.as_deref().filter(|_| looks_spatial(&meta.data_type) || value.as_deref().map(|v| v.len() >= 42).unwrap_or(false)).and_then(super::geo::parse) } else { None };
                if modified {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(original.as_deref().unwrap_or("NULL")).font(egui::FontId::monospace(12.0)).color(pal.text_faint).strikethrough());
                        ui.label(RichText::new("→").color(pal.text_faint));
                        ui.label(RichText::new(value.as_deref().unwrap_or("NULL")).font(egui::FontId::monospace(12.0)).color(super::readable_on_tint(pal.accent)));
                    });
                } else if let Some(mut j) = json {
                    egui::Frame::new().fill(if pal.dark { Color32::from_black_alpha(70) } else { Color32::from_black_alpha(8) }).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        super::json_tree::show(ui, &mut j, false, egui::Id::unique(("inspect-json", self.id, c)));
                    });
                    if ui.small_button(if editable { "Edit as tree…" } else { "Open…" }).clicked() {
                        quick = Some(c);
                    }
                } else if let Some((srid, g)) = geo {
                    super::geo::show(ui, &g, srid, 150.0);
                    ui.label(faint(super::geo::summary(&g)).size(11.5));
                } else {
                    let mut text = value.clone().unwrap_or_default();
                    let multiline = text.len() > 48 || text.contains('\n');
                    let te = if multiline { egui::TextEdit::multiline(&mut text).desired_rows(3) } else { crate::ui::line_edit(&mut text) };
                    let te = te.font(egui::FontId::monospace(13.0)).frame(egui::Frame::NONE).margin(egui::Margin::ZERO).text_color(if meta.foreign_key { super::fk_color() } else { pal.text }).desired_width(f32::INFINITY).hint_text(RichText::new(cx.settings.null_display.clone()).font(egui::FontId::monospace(13.0)).color(pal.text_faint)).interactive(editable && state != RowState::Deleted);
                    let resp = ui.add(te);
                    if resp.changed() {
                        edits.push((c, text));
                    }
                    if editable {
                        resp.context_menu(|ui| {
                            if ui.button("Set NULL").clicked() {
                                self.apply_edit(r, c, EditValue::Null);
                                ui.close();
                            }
                            if ui.button("Set DEFAULT").clicked() {
                                self.apply_edit(r, c, EditValue::Default);
                                ui.close();
                            }
                        });
                    }
                }
                ui.add_space(8.0);
            }
            let links = self.linked.as_ref().filter(|(k, _)| *k == r + self.req.offset).map(|(_, l)| l.clone()).unwrap_or_default();
            if !self.fks.is_empty() || !self.incoming.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Linked rows").font(super::medium(13.0)).color(pal.text));
                    if self.linked_task.is_some() {
                        ui.add(egui::Spinner::new().size(12.0).color(pal.accent));
                    }
                });
                ui.add_space(6.0);
                for l in links {
                    let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), egui::Sense::click());
                    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), super::motion::FAST as f32);
                    ui.painter().hline(rr.x_range(), rr.top() + 0.5, Stroke::new(1.0, super::line()));
                    if h > 0.0 {
                        ui.painter().rect_filled(rr, CornerRadius::same(4), pal.surface.gamma_multiply(h));
                    }
                    let arrow = if l.outgoing { "→" } else { "←" };
                    ui.painter().text(egui::pos2(rr.left() + 4.0, rr.top() + 12.0), egui::Align2::LEFT_CENTER, format!("{arrow} {}", l.table), egui::FontId::monospace(13.0), super::fk_color());
                    let detail = super::home::ellipsize_to(ui.painter(), &l.detail, egui::FontId::proportional(12.0), rr.width() - 28.0);
                    ui.painter().text(egui::pos2(rr.left() + 4.0, rr.top() + 31.0), egui::Align2::LEFT_CENTER, detail, egui::FontId::proportional(12.0), pal.text_dim);
                    super::home::paint_chevron(ui.painter(), egui::Rect::from_center_size(egui::pos2(rr.right() - 10.0, rr.center().y), vec2(12.0, 12.0)), 0.0, pal.text_faint);
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        cx.actions.push(WsAction::OpenTableFiltered { table: TableRef { schema: l.schema.clone(), name: l.table.clone() }, column: l.column.clone(), value: Some(l.value.clone()) });
                    }
                }
            }
        });
        for (c, t) in edits {
            self.apply_edit(r, c, EditValue::Text(t));
        }
        if let Some(c) = quick {
            let src = TableSource { tab: self };
            let text = grid::cell_value(&src, r, c).unwrap_or_default();
            self.quick_look = Some(QuickLook { row: r, col: c, viewer: super::viewer::ValueViewer::new(text) });
        }
    }

    /// Loads linked rows when the inspected row changes.
    fn poll_linked(&mut self, r: usize, cx: &TabCtx) {
        let key = r + self.req.offset;
        if let Some((k, task)) = &mut self.linked_task {
            if let Some(res) = task.poll() {
                let k = *k;
                self.linked_task = None;
                self.linked = Some((k, res.unwrap_or_default()));
            }
        }
        let fresh = self.linked.as_ref().map(|(k, _)| *k == key).unwrap_or(false) || self.linked_task.as_ref().map(|(k, _)| *k == key).unwrap_or(false);
        if fresh || r >= self.data.rows.len() || (self.fks.is_empty() && self.incoming.is_empty()) {
            return;
        }
        let d = cx.session.dialect();
        if matches!(d, crate::model::Dialect::Redis | crate::model::Dialect::Mongo | crate::model::Dialect::Elastic) {
            return;
        }
        let row = self.data.rows[r].clone();
        let names: Vec<String> = self.grid_meta.iter().map(|m| m.name.clone()).collect();
        let value_of = move |col: &str| names.iter().position(|n| n == col).and_then(|i| row.get(i).cloned().flatten());
        let outgoing: Vec<(ForeignKey, String)> = self.fks.iter().filter_map(|fk| value_of(&fk.column).map(|v| (fk.clone(), v))).collect();
        let incoming: Vec<(Relation, String)> = self.incoming.iter().filter_map(|rel| value_of(&rel.ref_column).map(|v| (rel.clone(), v))).collect();
        let task = cx.session.spawn(cx.ctx, move |drv| async move {
            let mut drv = drv.lock().await;
            let mut out = Vec::new();
            for (fk, v) in outgoing {
                let t = TableRef { schema: fk.ref_schema.clone(), name: fk.ref_table.clone() };
                let sql = match d {
                    crate::model::Dialect::SqlServer => format!("SELECT TOP 1 * FROM {} WHERE {} = {}", sqlgen::qualified(d, &t), sqlgen::quote_ident(d, &fk.ref_column), sqlgen::lit(d, &v)),
                    _ => format!("SELECT * FROM {} WHERE {} = {} LIMIT 1", sqlgen::qualified(d, &t), sqlgen::quote_ident(d, &fk.ref_column), sqlgen::lit(d, &v)),
                };
                let label = drv.query_one(&sql).await.ok().and_then(|res| {
                    let i = res.columns.iter().position(|c| {
                        let c = c.to_lowercase();
                        c.contains("name") || c.contains("title") || c.contains("email") || c == "label"
                    })?;
                    res.cell(0, i).map(str::to_string)
                });
                let detail = match label {
                    Some(l) => format!("{l} · {} {v}", fk.ref_column),
                    None => format!("{} = {v}", fk.ref_column),
                };
                out.push(LinkInfo { outgoing: true, schema: fk.ref_schema, table: fk.ref_table, column: fk.ref_column, value: v, detail });
            }
            for (rel, v) in incoming {
                let t = TableRef { schema: rel.schema.clone(), name: rel.table.clone() };
                let sql = format!("SELECT COUNT(*) FROM {} WHERE {} = {}", sqlgen::qualified(d, &t), sqlgen::quote_ident(d, &rel.column), sqlgen::lit(d, &v));
                let n = drv.query_one(&sql).await.ok().and_then(|r| r.cell(0, 0).and_then(|s| s.parse::<f64>().ok())).unwrap_or(0.0) as u64;
                let detail = match n {
                    0 => "no rows point here".to_string(),
                    1 => format!("1 row via {}", rel.column),
                    n => format!("{} rows via {}", fmt_count(n), rel.column),
                };
                out.push(LinkInfo { outgoing: false, schema: rel.schema, table: rel.table, column: rel.column, value: v, detail });
            }
            Ok(out)
        });
        self.linked_task = Some((key, task));
    }
}

fn looks_spatial(data_type: &str) -> bool {
    let t = data_type.to_lowercase();
    ["geometry", "geography", "point", "polygon", "linestring", "spatial"].iter().any(|k| t.contains(k))
}

impl GridState {
    pub fn editing_cancel(&mut self) {
        self.clear();
    }
}

/// Data type field: free text, so `varchar(80)` stays possible, with a menu of the engine's common types
/// behind the chevron at its right edge.
fn type_picker(ui: &mut Ui, value: &mut String, width: f32, groups: &[(&str, &[&str])], enabled: bool) -> egui::Response {
    let pal = p();
    ui.add_enabled_ui(enabled, |ui| {
        let field = ui.add_sized(
            vec2(width, 32.0),
            crate::ui::line_edit(value).desired_width(width).font(egui::TextStyle::Monospace).margin(egui::Margin { left: 8, right: 28, top: 4, bottom: 4 }),
        );
        let rect = egui::Rect::from_min_max(egui::pos2(field.rect.right() - 26.0, field.rect.top()), field.rect.right_bottom());
        let menu = ui.interact(rect, field.id.with("types"), egui::Sense::click());
        let tint = if !ui.is_enabled() { pal.text_faint.gamma_multiply(0.5) } else if menu.hovered() { pal.text } else { pal.text_faint };
        paint_icon(ui.painter(), egui::Rect::from_center_size(rect.center(), vec2(10.0, 10.0)), Icon::ChevronDown, tint);
        let menu = menu.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Common types");
        egui::Popup::menu(&menu).show(|ui| {
            ui.set_min_width(240.0);
            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                for (group, types) in groups {
                    ui.label(faint(*group).size(11.0));
                    for t in *types {
                        if ui.selectable_label(value.eq_ignore_ascii_case(t), RichText::new(*t).monospace()).clicked() {
                            *value = t.to_string();
                            ui.close();
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        });
        field
    })
    .inner
}

pub fn fmt_count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn error_banner(ui: &mut Ui, err: &str, slot: &mut Option<String>) {
    let pal = p();
    egui::Frame::new().fill(pal.danger.gamma_multiply(0.14)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(err).color(pal.danger).size(12.5).monospace()).wrap());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if icon_button(ui, Icon::Close, "Dismiss").clicked() {
                    *slot = None;
                }
                if ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(err.to_string());
                }
            });
        });
    });
}

struct TableSource<'a> {
    tab: &'a TableTab,
}

impl GridSource for TableSource<'_> {
    fn row_number(&self, row: usize) -> usize { self.tab.req.offset + row + 1 }
    fn columns(&self) -> &[ColMeta] {
        &self.tab.grid_meta
    }
    fn row_count(&self) -> usize {
        self.tab.data.rows.len() + self.tab.pending.inserted.len()
    }
    fn cell(&self, row: usize, col: usize) -> CellView<'_> {
        let base = self.tab.data.rows.len();
        if row >= base {
            return match self.tab.pending.inserted.get(row - base).and_then(|r| r.get(col)) {
                Some(Some(v)) => CellView::Edited(v),
                _ => CellView::Edited(&DEFAULT_EDIT),
            };
        }
        if let Some(v) = self.tab.pending.edits.get(&(row, col)) {
            return CellView::Edited(v);
        }
        CellView::Value(self.tab.data.rows[row].get(col).and_then(|v| v.as_deref()))
    }
    fn row_state(&self, row: usize) -> RowState {
        if row >= self.tab.data.rows.len() {
            RowState::Inserted
        } else if self.tab.pending.deleted.contains(&row) {
            RowState::Deleted
        } else {
            RowState::Normal
        }
    }
    fn cell_modified(&self, row: usize, col: usize) -> bool {
        self.tab.pending.edits.contains_key(&(row, col))
    }
    fn sort_of(&self, col: usize) -> Option<bool> {
        let name = &self.tab.grid_meta.get(col)?.name;
        self.tab.req.sort.iter().find(|(c, _)| c == name).map(|(_, a)| *a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_start_compact_and_row_numbers_follow_page_offset() {
        let mut t = tab();
        assert_eq!(t.density, 0);
        assert_eq!(TableSource { tab: &t }.row_number(0), 1);
        t.req.offset = 300;
        assert_eq!(TableSource { tab: &t }.row_number(0), 301);
        assert_eq!(TableSource { tab: &t }.row_number(9), 310);
    }

    fn tab() -> TableTab {
        let mut t = TableTab::new(1, DbObject { parent: None, arguments: None, schema: "public".into(), name: "users".into(), kind: ObjectKind::Table }, TableView::Data, 300);
        t.columns = vec![
            ColumnInfo { name: "id".into(), data_type: "integer".into(), primary_key: true, default: Some("nextval('x')".into()), ..Default::default() },
            ColumnInfo { name: "name".into(), data_type: "text".into(), nullable: true, ..Default::default() },
        ];
        t.data = QueryResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![vec![Some("1".into()), Some("Ann".into())], vec![Some("2".into()), None]],
            ..Default::default()
        };
        t.rebuild_grid_meta();
        t
    }

    #[test]
    fn edits_back_to_original_value_are_dropped() {
        let mut t = tab();
        t.apply_edit(0, 1, EditValue::Text("Bob".into()));
        assert_eq!(t.pending.count(), 1);
        t.apply_edit(0, 1, EditValue::Text("Ann".into()));
        assert!(t.pending.is_empty());
        t.apply_edit(1, 1, EditValue::Null);
        assert!(t.pending.is_empty(), "NULL over NULL is no change");
    }

    #[test]
    fn builds_update_delete_insert_with_primary_key() {
        let mut t = tab();
        t.apply_edit(0, 1, EditValue::Text("Bob".into()));
        t.grid.rows = [1].into();
        t.delete_selected();
        t.insert_row();
        let last = t.data.rows.len();
        t.apply_edit(last, 1, EditValue::Text("Cy".into()));
        let ch = t.changes();
        assert_eq!(ch.len(), 3);
        assert_eq!(ch[0], RowChange::Update { key: vec![("id".into(), Some("1".into()))], set: vec![("name".into(), EditValue::Text("Bob".into()))] });
        assert_eq!(ch[1], RowChange::Delete { key: vec![("id".into(), Some("2".into()))] });
        assert_eq!(ch[2], RowChange::Insert { values: vec![("id".into(), EditValue::Default), ("name".into(), EditValue::Text("Cy".into()))] });
    }

    #[test]
    fn edits_on_deleted_rows_are_not_sent() {
        let mut t = tab();
        t.apply_edit(0, 1, EditValue::Text("Bob".into()));
        t.grid.rows = [0].into();
        t.delete_selected();
        let ch = t.changes();
        assert_eq!(ch, vec![RowChange::Delete { key: vec![("id".into(), Some("1".into()))] }]);
    }

    #[test]
    fn duplicate_leaves_generated_key_to_default() {
        let mut t = tab();
        t.grid.rows = [0].into();
        t.duplicate_rows();
        let ch = t.changes();
        assert_eq!(ch, vec![RowChange::Insert { values: vec![("id".into(), EditValue::Default), ("name".into(), EditValue::Text("Ann".into()))] }]);
    }

    #[test]
    fn structure_changes_detect_alter_add_drop() {
        let mut t = tab();
        t.reset_structure_draft();
        t.structure.draft[1].data_type = "varchar(80)".into();
        t.structure.draft.push(ColumnInfo { name: "email".into(), data_type: "text".into(), nullable: true, ..Default::default() });
        t.structure.dropped.insert("id".into());
        let ch = t.structure_changes();
        // Dropping the key column takes the primary key off first.
        assert_eq!(ch[0], StructureChange::DropPrimaryKey { name: "users_pkey".into() });
        assert!(matches!(ch[1], StructureChange::DropColumn(ref n) if n == "id"));
        assert!(matches!(ch[2], StructureChange::AlterColumn { .. }));
        assert!(matches!(ch[3], StructureChange::AddColumn(ref c) if c.name == "email"));
        assert_eq!(ch.len(), 4, "no key is re-added: {ch:?}");
    }

    #[test]
    fn moving_the_primary_key_drops_the_named_constraint_then_adds_it() {
        let mut t = tab();
        t.indexes = vec![IndexInfo { name: "users_pk_custom".into(), columns: "id".into(), unique: true, primary: true, method: "btree".into() }];
        t.reset_structure_draft();
        t.structure.draft[0].primary_key = false;
        t.structure.draft[1].primary_key = true;
        t.structure.draft[1].nullable = false;
        let ch = t.structure_changes();
        assert_eq!(ch[0], StructureChange::DropPrimaryKey { name: "users_pk_custom".into() });
        // Nullability changes before the key is added; the key flag itself is never an ALTER COLUMN.
        assert!(matches!(&ch[1], StructureChange::AlterColumn { original, updated } if original.name == "name" && !updated.nullable && !updated.primary_key));
        assert_eq!(ch[2], StructureChange::AddPrimaryKey { columns: vec!["name".into()] });
        assert_eq!(ch.len(), 3, "{ch:?}");
    }

    #[test]
    fn renaming_a_key_column_keeps_the_key() {
        let mut t = tab();
        t.reset_structure_draft();
        t.structure.draft[0].name = "user_id".into();
        let ch = t.structure_changes();
        assert_eq!(ch.len(), 1, "{ch:?}");
        assert!(matches!(&ch[0], StructureChange::AlterColumn { updated, .. } if updated.name == "user_id"));
    }

    #[test]
    fn foreign_keys_drop_first_and_add_last() {
        let mut t = tab();
        t.fks = vec![ForeignKey { name: "old_fk".into(), column: "name".into(), ref_schema: "public".into(), ref_table: "people".into(), ref_column: "name".into() }];
        t.reset_structure_draft();
        t.structure.dropped_fks.insert("old_fk".into());
        t.structure.draft.push(ColumnInfo { name: "team_id".into(), data_type: "bigint".into(), nullable: true, ..Default::default() });
        t.structure.added_fks.push(FkDraft { name: "fk_users_team".into(), column: "team_id".into(), ref_table: "teams".into(), ref_column: "id".into(), on_delete: "CASCADE".into(), on_update: "NO ACTION".into() });
        let ch = t.structure_changes();
        assert_eq!(ch.first(), Some(&StructureChange::DropForeignKey("old_fk".into())));
        assert!(matches!(ch.last(), Some(StructureChange::AddForeignKey { ref_table, column, .. }) if ref_table.name == "teams" && ref_table.schema == "public" && column == "team_id"));
        let sql = t.structure_sql(crate::model::Dialect::Postgres);
        assert_eq!(sql.last().unwrap(), "ALTER TABLE \"public\".\"users\" ADD CONSTRAINT \"fk_users_team\" FOREIGN KEY (\"team_id\") REFERENCES \"public\".\"teams\" (\"id\") ON DELETE CASCADE ON UPDATE NO ACTION");
    }

    #[test]
    fn counts_format_with_separators() {
        assert_eq!(fmt_count(1234567), "1,234,567");
        assert_eq!(fmt_count(12), "12");
    }
}
