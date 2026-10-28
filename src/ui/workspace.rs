//! A workspace is one open connection: sidebar, tabs, toolbar, console, right panel and modals.

use super::assistant::Assistant;
use super::diagram::DiagramTab;
use super::dialogs::{self, Modal, ModalOutcome};
use super::editor::Catalog;
use super::grid::{self, GridState, ResultSource};
use super::metrics::MetricsTab;
use super::query_tab::{self, QueryTab};
use super::table_tab::{TableTab, TableView};
use super::*;
use crate::db::*;
use crate::model::*;
use crate::runtime::{take, Session, Task};
use crate::store::Store;
use egui::{vec2, RichText};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

pub enum WorkspaceCommand {
    Close(uuid::Uuid),
    Reconnect(uuid::Uuid),
    Quit,
}

#[derive(Clone, PartialEq)]
enum TransactionAction {
    CloseTab(u64), CloseOthers(u64), CloseWorkspace, Quit, Reconnect,
    Database(String), Schema(String), AutoCommit(u64, bool), Abandon(u64),
}

pub struct WorkspaceChoice {
    pub id: uuid::Uuid,
    pub name: String,
    pub color: egui::Color32,
}

/// App-wide state a workspace may read or change.
pub struct Shared<'a> {
    pub store: &'a Store,
    pub settings: &'a Settings,
    pub queries: &'a mut Vec<SavedQuery>,
    pub boards: &'a mut Vec<MetricsBoard>,
    pub toasts: &'a mut Toasts,
    pub edit_connection: &'a mut Option<uuid::Uuid>,
    pub workspace_choices: &'a [WorkspaceChoice],
    pub switch_workspace: &'a mut Option<uuid::Uuid>,
    pub show_home: &'a mut bool,
    pub open_settings: &'a mut bool,
    pub workspace_command: &'a mut Option<WorkspaceCommand>,
    pub create_connection: &'a mut Option<ConnectionConfig>,
    pub app_request: &'a mut Option<AppRequest>,
}

/// Requests the workspace hands up to the app shell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppRequest {
    About,
    CheckForUpdates,
}

pub struct TabCtx<'a> {
    pub ctx: &'a egui::Context,
    pub session: &'a Arc<Session>,
    pub settings: &'a Settings,
    pub store: &'a Store,
    pub actions: &'a mut Vec<WsAction>,
}

/// Work that may need safe-mode approval before it reaches the server.
#[derive(Debug, Clone, PartialEq)]
pub enum Gated {
    Query { tab: u64, sql: String },
    Changes(u64),
    Script { stmts: Vec<String>, label: String, refresh: bool },
}

pub enum WsAction {
    Toast(ToastKind, String),
    OpenDefinitionSql(DbObject),
    OpenObject { obj: DbObject, structure: bool, new_tab: bool },
    OpenTableFiltered { table: TableRef, column: String, value: Option<String> },
    OpenQuery { sql: String, title: Option<String>, run: bool },
    RunQuery { tab_id: u64, sql: String },
    SetAutoCommit { tab: u64, enabled: bool },
    ResolveUncertain(u64),
    RunScript { stmts: Vec<String>, label: String, refresh: bool },
    Gate(Gated),
    RequestCommit(u64),
    PreviewCommit(u64),
    SaveQuery(u64),
    ExportTable(TableRef, BrowseRequest),
    ExportResult { columns: Vec<String>, rows: Vec<Vec<Cell>> },
    QuickLookText { title: String, text: String },
    RecordHistory { sql: String, elapsed_ms: u64, ok: bool },
    RefreshObjects,
    SwitchDatabase(String),
    SwitchSchema(String),
    ToggleRowDetail,
    ShowModal(Box<Modal>),
    OpenAdmin(AdminKind),
    OpenMetrics(Option<uuid::Uuid>),
    OpenDiagram,
    KillProcess(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminKind {
    Processes,
    Users,
}

pub struct AdminTab {
    pub id: u64,
    pub kind: AdminKind,
    data: QueryResult,
    rows: Vec<Vec<Cell>>,
    task: Option<Task<anyhow::Result<QueryResult>>>,
    grid: GridState,
    error: Option<String>,
    auto_refresh: bool,
    last: Instant,
}

pub enum TabKind {
    Table(Box<TableTab>),
    Query(Box<QueryTab>),
    Admin(Box<AdminTab>),
    Metrics(Box<MetricsTab>),
    Diagram(Box<DiagramTab>),
}

pub struct Tab {
    pub id: u64,
    pub kind: TabKind,
}

impl Tab {
    fn title(&self) -> String {
        match &self.kind {
            TabKind::Table(t) => t.title(),
            TabKind::Query(q) => q.title.clone(),
            TabKind::Admin(a) => if a.kind == AdminKind::Processes { "Processes".into() } else { "Users".into() },
            TabKind::Metrics(m) => m.title(),
            TabKind::Diagram(d) => d.title(),
        }
    }

    fn icon(&self) -> Icon {
        match &self.kind {
            TabKind::Table(t) => match t.obj.kind {
                ObjectKind::View | ObjectKind::MaterializedView => Icon::View,
                ObjectKind::Function | ObjectKind::Procedure => Icon::Function,
                ObjectKind::Keys => Icon::Key,
                _ => Icon::Table,
            },
            TabKind::Query(_) => Icon::Code,
            TabKind::Admin(a) => if a.kind == AdminKind::Processes { Icon::Activity } else { Icon::Users },
            TabKind::Metrics(_) => Icon::Chart,
            TabKind::Diagram(_) => Icon::Graph,
        }
    }

    fn dirty(&self) -> bool {
        match &self.kind {
            TabKind::Table(t) => t.has_changes(),
            TabKind::Query(q) => q.has_pending_transaction() || (q.dirty && q.saved_id.is_some()),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarMode {
    Items,
    Queries,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPanel {
    None,
    RowDetail,
    Assistant,
}

struct InitData {
    databases: Vec<String>,
    current_db: String,
    schemas: Vec<String>,
    schema: String,
    objects: Vec<DbObject>,
}

pub struct Workspace {
    pub cfg: ConnectionConfig,
    pub session: Option<Arc<Session>>,
    connecting: Option<Task<anyhow::Result<Session>>>,
    pub connect_error: Option<String>,
    init_task: Option<Task<anyhow::Result<InitData>>>,
    context_ready: bool,
    objects_task: Option<Task<anyhow::Result<Vec<DbObject>>>>,
    definition_task: Option<Task<anyhow::Result<(DbObject, String)>>>,
    columns_task: Option<Task<anyhow::Result<Vec<(String, String)>>>>,
    script_task: Option<(String, bool, Task<anyhow::Result<()>>)>,
    kill_task: Option<Task<anyhow::Result<()>>>,
    ping_task: Option<Task<anyhow::Result<u64>>>,
    /// Round trip of the last keep-alive ping.
    pub latency_ms: Option<u64>,
    stats_task: Option<Task<anyhow::Result<Vec<TableStat>>>>,
    pub stats: std::collections::HashMap<String, TableStat>,
    pub databases: Vec<String>,
    pub current_db: String,
    pub schemas: Vec<String>,
    pub schema: String,
    pub objects: Vec<DbObject>,
    pub catalog: Catalog,
    database_picker: super::location_picker::LocationPicker,
    schema_picker: super::location_picker::LocationPicker,
    transaction_action: Option<TransactionAction>,
    resolving_transactions: bool,
    sidebar_search: String,
    sidebar_mode: SidebarMode,
    collapsed: HashSet<ObjectKind>,
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_id: u64,
    pub right: RightPanel,
    pub console: bool,
    assistant: Assistant,
    history: Vec<HistoryEntry>,
    history_loaded: bool,
    history_search: String,
    queries_search: String,
    pub modal: Option<Modal>,
    last_ping: Instant,
    pub status: ConnState,
    /// SQL to open and run once the connection is ready (`--run`).
    pub run_after_connect: Option<String>,
    /// SQL to open (not run) once the connection is ready.
    pending_query: Option<String>,
    /// Development hook: UI steps from STRATA_SCRIPT, replayed once connected (see `script_step`).
    script: std::collections::VecDeque<String>,
    script_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected,
    Failed,
    Lost,
}

impl Workspace {
    pub fn open(cfg: ConnectionConfig, ctx: &egui::Context) -> Self {
        let mut ws = Workspace {
            cfg: cfg.clone(),
            session: None,
            connecting: None,
            connect_error: None,
            init_task: None,
            context_ready: false,
            objects_task: None,
            definition_task: None,
            columns_task: None,
            script_task: None,
            kill_task: None,
            ping_task: None,
            latency_ms: None,
            stats_task: None,
            stats: Default::default(),
            databases: vec![],
            current_db: String::new(),
            schemas: vec![],
            schema: String::new(),
            objects: vec![],
            catalog: Catalog::default(),
            database_picker: Default::default(),
            schema_picker: Default::default(),
            transaction_action: None,
            resolving_transactions: false,
            sidebar_search: String::new(),
            sidebar_mode: SidebarMode::Items,
            collapsed: HashSet::new(),
            tabs: vec![],
            active: 0,
            next_id: 1,
            right: RightPanel::None,
            console: false,
            assistant: Assistant::default(),
            history: vec![],
            history_loaded: false,
            history_search: String::new(),
            queries_search: String::new(),
            modal: None,
            last_ping: Instant::now(),
            status: ConnState::Connecting,
            run_after_connect: None,
            pending_query: None,
            script: take_script(),
            script_at: Instant::now(),
        };
        ws.connect(ctx);
        ws
    }

    /// Opens `sql` in a query tab now, or once the connection is ready.
    pub fn open_after_connect(&mut self, sql: String, _ctx: &egui::Context) {
        if self.session.is_some() && self.init_task.is_none() {
            self.open_query(sql, None);
        } else {
            self.pending_query = Some(sql);
        }
    }

    pub fn id(&self) -> uuid::Uuid {
        self.cfg.id
    }

    pub fn connect(&mut self, ctx: &egui::Context) {
        let cfg = self.cfg.clone();
        self.status = ConnState::Connecting;
        self.connect_error = None;
        self.context_ready = false;
        self.connecting = Some(Task::spawn(ctx, async move { Session::open(cfg).await }));
    }

    pub fn reconnect(&mut self, ctx: &egui::Context) {
        if self.defer_transaction_action(TransactionAction::Reconnect) { return; }
        self.reset_query_connections();
        if let Some(s) = self.session.take() {
            let d = s.driver.clone();
            crate::runtime::rt().spawn(async move { d.lock().await.close().await });
        }
        self.connect(ctx);
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn load_init(&mut self, ctx: &egui::Context, switch_to: Option<String>) {
        let Some(s) = &self.session else { return };
        self.context_ready = false;
        let wanted_schema = self.schema.clone();
        self.init_task = Some(s.spawn(ctx, move |d| async move {
            let mut d = d.lock().await;
            if let Some(db) = switch_to {
                d.switch_database(&db).await?;
            }
            let databases = d.databases().await.unwrap_or_default();
            let current_db = d.current_database();
            let schemas = d.schemas().await.unwrap_or_default();
            let default = d.default_schema();
            let schema = [wanted_schema, default]
                .into_iter()
                .find(|s| !s.is_empty() && (schemas.is_empty() || schemas.contains(s)))
                .or_else(|| schemas.first().cloned())
                .unwrap_or_default();
            let objects = d.objects(&schema).await?;
            Ok(InitData { databases, current_db, schemas, schema, objects })
        }));
    }

    fn load_objects(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.session else { return };
        let schema = self.schema.clone();
        self.objects_task = Some(s.spawn(ctx, move |d| async move { d.lock().await.objects(&schema).await }));
    }

    fn load_catalog_columns(&mut self, ctx: &egui::Context) {
        let Some(s) = &self.session else { return };
        let schema = self.schema.clone();
        self.columns_task = Some(s.spawn(ctx, move |d| async move { d.lock().await.all_columns(&schema).await }));
    }

    fn set_objects(&mut self, objects: Vec<DbObject>, ctx: &egui::Context) {
        self.catalog.tables = objects.iter().filter(|o| o.kind.has_rows()).map(|o| o.name.clone()).collect();
        self.objects = objects;
        self.load_catalog_columns(ctx);
        if let Some(s) = &self.session {
            let schema = self.schema.clone();
            self.stats_task = Some(s.spawn(ctx, move |d| async move { d.lock().await.table_stats(&schema).await }));
        }
    }

    fn poll(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        if let Some(r) = take(&mut self.connecting) {
            match r {
                Ok(s) => {
                    self.session = Some(Arc::new(s));
                    self.status = ConnState::Connected;
                    self.last_ping = Instant::now();
                    self.load_init(ctx, None);
                }
                Err(e) => {
                    self.status = ConnState::Failed;
                    self.connect_error = Some(format!("{e:#}"));
                }
            }
        }
        if let Some(r) = take(&mut self.init_task) {
            match r {
                Ok(d) => {
                    self.reset_query_connections();
                    self.databases = d.databases;
                    self.current_db = d.current_db;
                    self.schemas = d.schemas;
                    self.schema = d.schema;
                    self.context_ready = true;
                    self.set_objects(d.objects, ctx);
                    if let Some(sql) = self.run_after_connect.take() {
                        self.handle(WsAction::OpenQuery { sql, title: None, run: true }, sh, ctx);
                    } else if let Some(sql) = self.pending_query.take() {
                        self.handle(WsAction::OpenQuery { sql, title: None, run: false }, sh, ctx);
                    } else if self.tabs.is_empty() {
                        if let Some(first) = self.objects.iter().find(|o| o.kind.has_rows()).cloned() {
                            self.open_object(first, false, false, sh.settings);
                        }
                    }
                }
                Err(e) => sh.toasts.error(format!("{e:#}")),
            }
        }
        if let Some(result) = take(&mut self.definition_task) {
            match result {
                Ok((object, sql)) => self.handle(WsAction::OpenQuery { sql, title: Some(format!("Definition · {}", object.display_name())), run: false }, sh, ctx),
                Err(error) => sh.toasts.error(format!("Cannot read definition: {error:#}")),
            }
        }
        if let Some(r) = take(&mut self.objects_task) {
            match r {
                Ok(o) => self.set_objects(o, ctx),
                Err(e) => sh.toasts.error(format!("{e:#}")),
            }
        }
        if let Some(r) = take(&mut self.stats_task) {
            self.stats = r.unwrap_or_default().into_iter().map(|t| (t.name.clone(), t)).collect();
        }
        if let Some(Ok(pairs)) = take(&mut self.columns_task) {
            self.catalog.columns.clear();
            for (t, c) in pairs {
                self.catalog.columns.entry(t).or_default().push(c);
            }
        }
        if let Some((_, _, task)) = &mut self.script_task {
            if let Some(r) = task.poll() {
                let (label, refresh, _) = self.script_task.take().expect("script task present");
                match r {
                    Ok(()) => {
                        sh.toasts.success(label);
                        if refresh {
                            self.load_init(ctx, None);
                        }
                        self.reload_active_tables(ctx, sh.settings, sh.store);
                    }
                    Err(e) => sh.toasts.error(format!("{e:#}")),
                }
            }
        }
        if let Some(r) = take(&mut self.kill_task) {
            match r {
                Ok(()) => sh.toasts.success("Process killed"),
                Err(e) => sh.toasts.error(format!("{e:#}")),
            }
        }
        if let Some(r) = take(&mut self.ping_task) {
            match r {
                Ok(ms) => {
                    self.latency_ms = Some(ms);
                    if self.status == ConnState::Lost {
                        self.status = ConnState::Connected;
                    }
                }
                Err(_) => self.status = ConnState::Lost,
            }
        }
        // Ping every 30 s to keep connections open; the first ping measures latency.
        let due = self.last_ping.elapsed().as_secs() >= 30 || (self.latency_ms.is_none() && self.status == ConnState::Connected);
        if (self.cfg.keep_alive || self.latency_ms.is_none()) && self.status != ConnState::Connecting && due && self.ping_task.is_none() {
            if let Some(s) = &self.session {
                // Skip the ping while a query holds the connection.
                if s.driver.try_lock().is_ok() {
                    self.ping_task = Some(s.spawn(ctx, |d| async move {
                        let t = Instant::now();
                        d.lock().await.ping().await?;
                        Ok(t.elapsed().as_millis() as u64)
                    }));
                }
            }
            self.last_ping = Instant::now();
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }

    fn reload_active_tables(&mut self, ctx: &egui::Context, settings: &Settings, store: &Store) {
        let Some(session) = self.session.clone() else { return };
        let mut actions = Vec::new();
        if let Some(Tab { kind: TabKind::Table(t), .. }) = self.tabs.get_mut(self.active) {
            let cx = TabCtx { ctx, session: &session, settings, store, actions: &mut actions };
            t.load_meta(&cx);
            t.reload(&cx);
        }
    }

    pub fn open_object(&mut self, obj: DbObject, structure: bool, new_tab: bool, settings: &Settings) {
        let structure = structure || !obj.kind.has_rows();
        if let Some(i) = self.tabs.iter().position(|t| matches!(&t.kind, TabKind::Table(tt) if tt.obj == obj)) {
            self.active = i;
            if let TabKind::Table(t) = &mut self.tabs[i].kind {
                if structure {
                    t.view = TableView::Structure;
                }
            }
            return;
        }
        let id = self.new_id();
        let mut tab = TableTab::new(id, obj, if structure { TableView::Structure } else { TableView::Data }, settings.page_size);
        tab.preview = !new_tab;
        let tab = Tab { id, kind: TabKind::Table(Box::new(tab)) };
        // Single-click opens a preview tab, replaced until it is touched.
        let replace = if new_tab { None } else { self.tabs.iter().position(|t| matches!(&t.kind, TabKind::Table(tt) if tt.preview && !tt.has_changes())) };
        match replace {
            Some(i) => {
                self.tabs[i] = tab;
                self.active = i;
            }
            None => {
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
            }
        }
    }

    pub fn open_query(&mut self, sql: String, title: Option<String>) -> u64 {
        let id = self.new_id();
        let n = self.tabs.iter().filter(|t| matches!(t.kind, TabKind::Query(_))).count() + 1;
        let title = title.unwrap_or_else(|| format!("Query {n}"));
        self.tabs.push(Tab { id, kind: TabKind::Query(Box::new(QueryTab::new(id, title, sql))) });
        self.active = self.tabs.len() - 1;
        id
    }

    #[cfg(target_os = "macos")]
    pub fn handle_native_menu(&mut self, action: &str, sh: &mut Shared, ctx: &egui::Context) {
        match action {
            "explorer" => self.sidebar_mode = SidebarMode::Items,
            "saved_queries" => self.sidebar_mode = SidebarMode::Queries,
            "history" => self.sidebar_mode = SidebarMode::History,
            "new_query" => { self.open_query(String::new(), None); }
            "close_connection" => *sh.workspace_command = Some(WorkspaceCommand::Close(self.cfg.id)),
            "reconnect" => self.reconnect(ctx),
            "console" => self.console = !self.console,
            "inspector" => self.right = if self.right == RightPanel::RowDetail { RightPanel::None } else { RightPanel::RowDetail },
            "new_database" => self.handle(WsAction::ShowModal(Box::new(Modal::NewDatabase { name: String::new(), encoding: String::new() })), sh, ctx),
            "new_table" => self.handle(WsAction::ShowModal(Box::new(Modal::create_table(self.schema.clone(), self.cfg.driver.dialect()))), sh, ctx),
            "refresh_objects" => self.load_objects(ctx),
            "graph" => self.handle(WsAction::OpenDiagram, sh, ctx),
            "run_query" => {
                let query = self.tabs.get(self.active).and_then(|tab| match &tab.kind { TabKind::Query(query) => Some((tab.id, query.sql.clone())), _ => None });
                if let Some((tab_id, sql)) = query {
                    self.handle(WsAction::RunQuery { tab_id, sql }, sh, ctx);
                }
            }
            "connection_info" => {
                let version = self.session.as_ref().and_then(|session| session.server_version.clone()).unwrap_or_else(|| "Version unavailable".into());
                self.handle(WsAction::QuickLookText { title: "Connection information".into(), text: format!("{}\n{}\nDatabase: {}\nSchema: {}\nServer: {}", self.cfg.display_name(), self.cfg.driver.label(), self.current_db, self.schema, version) }, sh, ctx);
            }
            "shortcuts" => self.handle(WsAction::QuickLookText { title: "Keyboard shortcuts".into(), text: "⌘T  New query\n⌘K  Find tables and commands\n⌘Enter  Run statement\n⌘Shift+Enter  Run all\n⌘S  Review changes or save query\n⌘R  Reload table\nSpace  Quick Look".into() }, sh, ctx),
            _ => {}
        }
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    pub fn close_tab(&mut self, i: usize) {
        if let Some(tab) = self.tabs.get(i) {
            if self.defer_transaction_action(TransactionAction::CloseTab(tab.id)) { return; }
        }
        if i < self.tabs.len() {
            self.tabs.remove(i);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            } else if self.active > i {
                self.active -= 1;
            }
        }
    }

    fn relevant_query(action: &TransactionAction, id: u64) -> bool {
        match action { TransactionAction::CloseTab(tab) | TransactionAction::AutoCommit(tab, _) | TransactionAction::Abandon(tab) => id == *tab, TransactionAction::CloseOthers(keep) => id != *keep, _ => true }
    }

    pub fn has_pending_transactions(&self) -> bool {
        self.tabs.iter().any(|tab| matches!(&tab.kind, TabKind::Query(q) if q.has_pending_transaction() || q.transaction_busy()))
    }

    pub fn request_close(&mut self, quit: bool) {
        if self.transaction_action.is_none() {
            self.transaction_action = Some(if quit { TransactionAction::Quit } else { TransactionAction::CloseWorkspace });
            self.resolving_transactions = false;
        }
    }

    fn cancel_active_query(&self) {
        if let Some(session) = &self.session {
            if let Some(Tab { kind: TabKind::Query(q), .. }) = self.tabs.get(self.active) { q.cancel(session); }
            else { session.cancel(); }
        }
    }

    fn defer_transaction_action(&mut self, action: TransactionAction) -> bool {
        if self.tabs.iter().any(|tab| Self::relevant_query(&action, tab.id) && matches!(&tab.kind, TabKind::Query(q) if q.has_pending_transaction() || q.transaction_busy())) {
            // Keep the first requested action while a resolution is in progress.
            if self.transaction_action.is_none() { self.transaction_action = Some(action); }
            true
        } else { false }
    }

    fn reset_query_connections(&mut self) {
        for tab in &mut self.tabs { if let TabKind::Query(q) = &mut tab.kind { q.reset_connection(); } }
    }

    pub fn poll_transactions(&mut self) {
        for tab in &mut self.tabs { if let TabKind::Query(q) = &mut tab.kind { q.poll_transaction(); } }
    }

    fn apply_transaction_action(&mut self, action: TransactionAction, ctx: &egui::Context, sh: &mut Shared) {
        match action {
            TransactionAction::CloseTab(id) => if let Some(i) = self.tab_index(id) { self.close_tab(i); },
            TransactionAction::CloseOthers(keep) => { self.tabs.retain(|tab| tab.id == keep); self.active = 0; }
            TransactionAction::CloseWorkspace => *sh.workspace_command = Some(WorkspaceCommand::Close(self.id())),
            TransactionAction::Quit => *sh.workspace_command = Some(WorkspaceCommand::Quit),
            TransactionAction::Reconnect => self.reconnect(ctx),
            TransactionAction::Database(db) => self.handle(WsAction::SwitchDatabase(db), sh, ctx),
            TransactionAction::Schema(schema) => self.handle(WsAction::SwitchSchema(schema), sh, ctx),
            TransactionAction::AutoCommit(id, enabled) => if let Some(i) = self.tab_index(id) { if let TabKind::Query(q) = &mut self.tabs[i].kind { q.auto_commit = enabled; q.reset_connection(); } },
            TransactionAction::Abandon(_) => {}
        }
    }

    fn transaction_dialog(&mut self, ctx: &egui::Context, sh: &mut Shared) {
        let Some(action) = self.transaction_action.clone() else { return };
        let relevant: Vec<usize> = self.tabs.iter().enumerate().filter(|(_, tab)| Self::relevant_query(&action, tab.id)).filter_map(|(i, tab)| matches!(tab.kind, TabKind::Query(_)).then_some(i)).collect();
        let busy = relevant.iter().any(|&i| matches!(&self.tabs[i].kind, TabKind::Query(q) if q.transaction_busy()));
        let pending = relevant.iter().any(|&i| matches!(&self.tabs[i].kind, TabKind::Query(q) if q.has_pending_transaction()));
        let failed = relevant.iter().any(|&i| matches!(&self.tabs[i].kind, TabKind::Query(q) if q.transaction_error.is_some()));
        if !busy && !pending && (!self.resolving_transactions || !failed) {
            self.transaction_action = None; self.resolving_transactions = false;
            self.apply_transaction_action(action, ctx, sh); return;
        }
        if self.resolving_transactions && !busy { self.resolving_transactions = false; }
        let mut choice = None;
        let abandon = matches!(action, TransactionAction::Abandon(_));
        let can_commit = relevant.iter().all(|&i| match &self.tabs[i].kind { TabKind::Query(q) => !q.has_pending_transaction() || q.transaction_state() == crate::query_transaction::TransactionState::Active, _ => true });
        let response = egui::Modal::new(egui::Id::unique(("pending-query-transaction", self.id()))).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading(if abandon { "Close uncertain connection?" } else { "Resolve pending transactions" });
            ui.label(if abandon { "The server outcome is unknown. Closing this connection cannot confirm whether earlier changes committed. Check the database before retrying." } else { "Choose Commit or Rollback before continuing. Cancel keeps your query tabs and pending work open." });
            for &i in &relevant {
                if let TabKind::Query(q) = &self.tabs[i].kind {
                    if q.has_pending_transaction() || q.transaction_busy() { ui.label(format!("• {}", q.title)); }
                    if let Some(error) = &q.transaction_error { ui.colored_label(p().danger, error); }
                }
            }
            if busy { ui.horizontal(|ui| { ui.spinner(); ui.label("Waiting for the running operation…"); }); }
            ui.horizontal(|ui| {
                if abandon {
                    if ui.add_enabled(!busy, egui::Button::new("Close connection")).clicked() { choice = Some(2); }
                } else {
                    if ui.add_enabled(!busy && can_commit && pending, egui::Button::new("Commit")).clicked() { choice = Some(1); }
                    if ui.add_enabled(!busy && pending, egui::Button::new("Rollback")).clicked() { choice = Some(0); }
                    let uncertain = relevant.iter().any(|&i| matches!(&self.tabs[i].kind, TabKind::Query(q) if q.transaction_state() == crate::query_transaction::TransactionState::Unknown));
                    if uncertain && ui.add_enabled(!busy, egui::Button::new("Close uncertain connections")).clicked() { choice = Some(2); }
                }
                if ui.add_enabled(!self.resolving_transactions, egui::Button::new("Cancel")).clicked() { choice = Some(3); }
            });
        });
        if response.should_close() && !self.resolving_transactions { choice = Some(3); }
        if let Some(choice) = choice {
            if choice == 3 { self.transaction_action = None; return; }
            self.resolving_transactions = true;
            for i in relevant {
                if let TabKind::Query(q) = &mut self.tabs[i].kind {
                    if q.has_pending_transaction() {
                        if choice == 2 && q.transaction_state() == crate::query_transaction::TransactionState::Unknown { q.abandon_transaction(ctx); }
                        else if choice != 2 { q.finish_transaction(ctx, choice == 1); }
                    }
                }
            }
        }
        if busy { ctx.request_repaint_after(std::time::Duration::from_millis(100)); }
    }

    // ---------- safe mode ----------

    fn gate(&mut self, g: Gated, sh: &mut Shared) {
        let (reads_only, preview) = match &g {
            Gated::Query { tab, sql } => {
                let managed = crate::query_transaction::supported(&self.cfg) && self.tab_index(*tab).is_some_and(|i| matches!(&self.tabs[i].kind, TabKind::Query(q) if !q.auto_commit));
                let reads_only = if managed { crate::query_transaction::is_managed_read_only(sql, self.cfg.driver.dialect()) } else { is_read_only(sql) };
                (reads_only, sql.clone())
            }
            Gated::Changes(id) => {
                let stmts = self.commit_preview(*id);
                (false, stmts.join(";\n"))
            }
            Gated::Script { stmts, .. } => (stmts.iter().all(|s| is_read_only(s)), stmts.join(";\n")),
        };
        if self.cfg.read_only && !reads_only {
            sh.toasts.error("This connection is read-only. Edit the connection to allow changes.");
            return;
        }
        let needs = match self.cfg.safe_mode {
            SafeMode::Silent => None,
            SafeMode::AlertAll => Some(false),
            SafeMode::AlertWrites => (!reads_only).then_some(false),
            SafeMode::PasswordAll => Some(true),
            SafeMode::PasswordWrites => (!reads_only).then_some(true),
        };
        match needs {
            None => self.run_gated(g, sh),
            Some(false) => self.modal = Some(Modal::confirm_send(preview, g, self.cfg.tag == Tag::Production)),
            Some(true) => self.modal = Some(Modal::password(preview, g)),
        }
    }

    fn open_review(&mut self, id: u64, ctx: &egui::Context) {
        let Some(i) = self.tab_index(id) else { return };
        let stmts = self.commit_preview(id);
        if self.cfg.read_only {
            self.modal = Some(Modal::Preview { title: "Read-only connection".into(), stmts, then: None });
            return;
        }
        let TabKind::Table(t) = &self.tabs[i].kind else { return };
        let items = t.review_items();
        let table = t.table();
        let incoming = t.incoming.clone();
        let d = self.cfg.driver.dialect();
        // Cascades: for each removed row, count the rows in tables that reference it.
        let deletes: Vec<Vec<(String, Cell)>> = items.iter().filter(|it| it.kind == super::table_tab::ReviewKind::Remove).map(|it| it.key.clone()).collect();
        let impact_task = if deletes.is_empty() || incoming.is_empty() {
            None
        } else {
            let incoming = incoming.clone();
            self.session.as_ref().map(|s| {
                s.spawn(ctx, move |drv| async move {
                    let mut out = Vec::new();
                    let mut drv = drv.lock().await;
                    for key in &deletes {
                        for rel in &incoming {
                            let Some((_, Some(v))) = key.iter().find(|(c, _)| *c == rel.ref_column) else { continue };
                            let child = TableRef { schema: rel.schema.clone(), name: rel.table.clone() };
                            let sql = format!("SELECT COUNT(*) FROM {} WHERE {} = {}", sqlgen::qualified(d, &child), sqlgen::quote_ident(d, &rel.column), sqlgen::lit(d, v));
                            let n = drv.query_one(&sql).await.ok().and_then(|r| r.cell(0, 0).and_then(|s| s.parse::<f64>().ok())).unwrap_or(0.0) as u64;
                            if n == 0 {
                                continue;
                            }
                            let rows = format!("{n} {} row{}", rel.table, if n == 1 { "" } else { "s" });
                            out.push(match rel.on_delete.as_str() {
                                "CASCADE" => (true, format!("Removing {} = {v} also deletes {rows} (ON DELETE CASCADE)", rel.ref_column)),
                                "SET NULL" => (true, format!("Removing {} = {v} sets {}.{} to NULL on {rows}", rel.ref_column, rel.table, rel.column)),
                                _ => (true, format!("{rows} still point at {} = {v}; the delete will fail unless they go first ({})", rel.ref_column, if rel.on_delete.is_empty() { "NO ACTION" } else { rel.on_delete.as_str() })),
                            });
                        }
                    }
                    Ok(out)
                })
            })
        };
        let mut impact = Vec::new();
        if !incoming.is_empty() {
            let names: Vec<String> = incoming.iter().map(|r| r.table.clone()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            impact.push((false, format!("Referenced by {}", names.join(", "))));
        }
        self.modal = Some(Modal::Review(Box::new(dialogs::ReviewState { tab: id, table, items, stmts, view: dialogs::ReviewView::Rows, impact, impact_task, confirm: String::new(), password: String::new(), error: None })));
    }

    fn commit_preview(&self, id: u64) -> Vec<String> {
        let Some(i) = self.tab_index(id) else { return vec![] };
        let kind = self.cfg.driver;
        match &self.tabs[i].kind {
            TabKind::Table(t) => {
                let mut v = t.structure_sql(kind.dialect());
                v.extend(t.preview_statements(kind));
                v
            }
            _ => vec![],
        }
    }

    fn run_gated(&mut self, g: Gated, sh: &mut Shared) {
        let Some(session) = self.session.clone() else { return };
        let ctx = sh_ctx();
        let mut actions = Vec::new();
        match g {
            Gated::Query { tab, sql } => {
                if self.transaction_action.is_some() || !self.context_ready { return; }
                let mut cfg = self.cfg.clone();
                cfg.database = self.current_db.clone();
                let schema = self.schema.clone();
                if let Some(i) = self.tab_index(tab) {
                    if let TabKind::Query(q) = &mut self.tabs[i].kind {
                        let cx = TabCtx { ctx: &ctx, session: &session, settings: sh.settings, store: sh.store, actions: &mut actions };
                        q.start(sql, &cx, cfg, schema);
                    }
                }
            }
            Gated::Changes(id) => {
                if let Some(i) = self.tab_index(id) {
                    if let TabKind::Table(t) = &mut self.tabs[i].kind {
                        let structure = t.structure_sql(session.dialect());
                        if !structure.is_empty() {
                            for s in &structure {
                                session.log(s.clone(), true);
                            }
                            let stmts = structure.clone();
                            self.script_task = Some((
                                "Structure updated".into(),
                                false,
                                session.spawn(&ctx, move |d| async move {
                                    let mut d = d.lock().await;
                                    for s in &stmts {
                                        d.execute(s).await?;
                                    }
                                    Ok(())
                                }),
                            ));
                        }
                        if !t.pending.is_empty() {
                            let cx = TabCtx { ctx: &ctx, session: &session, settings: sh.settings, store: sh.store, actions: &mut actions };
                            t.start_commit(&cx);
                        }
                    }
                }
            }
            Gated::Script { stmts, label, refresh } => {
                for s in &stmts {
                    session.log(s.clone(), true);
                }
                self.script_task = Some((
                    label,
                    refresh,
                    session.spawn(&ctx, move |d| async move {
                        let mut d = d.lock().await;
                        for s in &stmts {
                            d.execute(s).await?;
                        }
                        Ok(())
                    }),
                ));
            }
        }
        for a in actions {
            self.handle(a, sh, &ctx);
        }
    }

    // ---------- actions ----------

    fn handle(&mut self, a: WsAction, sh: &mut Shared, ctx: &egui::Context) {
        match a {
            WsAction::SetAutoCommit { tab, enabled } => {
                let action = TransactionAction::AutoCommit(tab, enabled);
                if !self.defer_transaction_action(action.clone()) { self.apply_transaction_action(action, ctx, sh); }
            }
            WsAction::ResolveUncertain(tab) => { self.transaction_action = Some(TransactionAction::Abandon(tab)); }
            WsAction::Toast(k, t) => sh.toasts.push(k, t),
            WsAction::OpenDefinitionSql(object) => {
                if let Some(session) = &self.session {
                    self.definition_task = Some(session.spawn(ctx, move |driver| async move {
                        let sql = driver.lock().await.ddl(&object).await?;
                        Ok((object, sql))
                    }));
                }
            }
            WsAction::OpenObject { obj, structure, new_tab } => self.open_object(obj, structure, new_tab, sh.settings),
            WsAction::OpenTableFiltered { table, column, value } => {
                let kind = self.objects.iter().find(|o| o.name == table.name && o.schema == table.schema).map(|o| o.kind).unwrap_or(ObjectKind::Table);
                let obj = DbObject { parent: None, arguments: None, schema: table.schema, name: table.name, kind };
                self.open_object(obj, false, true, sh.settings);
                if let Some(Tab { kind: TabKind::Table(t), .. }) = self.tabs.get_mut(self.active) {
                    let f = match value {
                        Some(v) => Filter { column: Some(column), op: FilterOp::Eq, value: v, ..Default::default() },
                        None => Filter { column: Some(column), op: FilterOp::IsNull, ..Default::default() },
                    };
                    t.req.filters.clear();
                    t.add_filter(f);
                    if let Some(s) = self.session.clone() {
                        let mut acts = Vec::new();
                        let cx = TabCtx { ctx, session: &s, settings: sh.settings, store: sh.store, actions: &mut acts };
                        t.ensure_loaded(&cx);
                        t.reload(&cx);
                    }
                }
            }
            WsAction::OpenQuery { sql, title, run } => {
                let id = self.open_query(sql.clone(), title);
                if run {
                    self.handle(WsAction::RunQuery { tab_id: id, sql }, sh, ctx);
                }
            }
            WsAction::RunQuery { tab_id, sql } => {
                if !self.context_ready { sh.toasts.info("Wait for the database connection to finish loading"); return; }
                if sql.trim().is_empty() {
                    return;
                }
                let params = if sh.settings.enable_query_params && !matches!(self.cfg.driver.dialect(), Dialect::Mongo | Dialect::Redis) { query_tab::param_names(&sql) } else { vec![] };
                if params.is_empty() {
                    self.gate(Gated::Query { tab: tab_id, sql }, sh);
                } else {
                    self.modal = Some(Modal::Params { tab: tab_id, sql, values: params.into_iter().map(|p| (p, String::new())).collect() });
                }
            }
            WsAction::RunScript { stmts, label, refresh } => self.gate(Gated::Script { stmts, label, refresh }, sh),
            WsAction::Gate(g) => self.gate(g, sh),
            WsAction::RequestCommit(id) => {
                if self.commit_preview(id).is_empty() {
                    return;
                }
                // Production writes always go through the review sheet.
                if self.cfg.tag == Tag::Production || self.cfg.safe_mode != SafeMode::Silent {
                    self.open_review(id, ctx);
                } else {
                    self.gate(Gated::Changes(id), sh);
                }
            }
            WsAction::PreviewCommit(id) => self.open_review(id, ctx),
            WsAction::SaveQuery(id) => {
                if let Some(i) = self.tab_index(id) {
                    if let TabKind::Query(q) = &mut self.tabs[i].kind {
                        if let Some(sid) = q.saved_id {
                            if let Some(sq) = sh.queries.iter_mut().find(|s| s.id == sid) {
                                sq.sql = q.sql.clone();
                                sq.updated_at = chrono::Utc::now();
                                q.dirty = false;
                                let _ = sh.store.save_queries(sh.queries);
                                sh.toasts.success("Query saved");
                                return;
                            }
                        }
                        self.modal = Some(Modal::SaveQuery { tab: id, name: q.title.clone(), shared: false });
                    }
                }
            }
            WsAction::ExportTable(t, req) => self.modal = Some(Modal::export_table(t, req, sh.settings.csv_delimiter)),
            WsAction::ExportResult { columns, rows } => self.modal = Some(Modal::export_rows(columns, rows, sh.settings.csv_delimiter)),
            WsAction::QuickLookText { title, text } => self.modal = Some(Modal::QuickLook { title, viewer: Box::new(super::viewer::ValueViewer::new(text)) }),
            WsAction::RecordHistory { sql, elapsed_ms, ok } => {
                let refresh = ok && crate::db::statements(&sql).iter().any(|statement| {
                    let keyword = statement.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                    matches!(keyword.as_str(), "CREATE" | "ALTER" | "DROP")
                });
                let e = HistoryEntry { at: chrono::Utc::now(), database: self.current_db.clone(), sql, elapsed_ms, ok };
                sh.store.append_history(self.cfg.id, &e);
                self.history.push(e);
                if refresh { self.load_objects(ctx); }
            }
            WsAction::RefreshObjects => self.load_objects(ctx),
            WsAction::SwitchDatabase(db) => {
                if self.defer_transaction_action(TransactionAction::Database(db.clone())) { return; }
                self.reset_query_connections();
                self.schema.clear();
                self.tabs.retain(|t| !matches!(t.kind, TabKind::Table(_) | TabKind::Diagram(_)));
                self.active = 0;
                self.load_init(ctx, Some(db));
            }
            WsAction::SwitchSchema(s) => {
                if self.defer_transaction_action(TransactionAction::Schema(s.clone())) { return; }
                self.reset_query_connections();
                self.schema = s;
                self.load_objects(ctx);
            }
            WsAction::ToggleRowDetail => self.right = if self.right == RightPanel::RowDetail { RightPanel::None } else { RightPanel::RowDetail },
            WsAction::ShowModal(m) => self.modal = Some(*m),
            WsAction::OpenAdmin(kind) => {
                if let Some(i) = self.tabs.iter().position(|t| matches!(&t.kind, TabKind::Admin(a) if a.kind == kind)) {
                    self.active = i;
                    return;
                }
                let id = self.new_id();
                self.tabs.push(Tab { id, kind: TabKind::Admin(Box::new(AdminTab { id, kind, data: QueryResult::default(), rows: vec![], task: None, grid: GridState::default(), error: None, auto_refresh: false, last: Instant::now() })) });
                self.active = self.tabs.len() - 1;
            }
            WsAction::OpenMetrics(board) => {
                let id = self.new_id();
                let board_id = match board {
                    Some(b) => b,
                    None => {
                        let b = MetricsBoard { id: uuid::Uuid::new_v4(), connection: self.cfg.id, name: "New board".into(), refresh_secs: 0, widgets: vec![] };
                        let bid = b.id;
                        sh.boards.push(b);
                        let _ = sh.store.save_boards(sh.boards);
                        bid
                    }
                };
                if let Some(i) = self.tabs.iter().position(|t| matches!(&t.kind, TabKind::Metrics(m) if m.board == board_id)) {
                    self.active = i;
                    return;
                }
                let name = sh.boards.iter().find(|b| b.id == board_id).map(|b| b.name.clone()).unwrap_or_default();
                self.tabs.push(Tab { id, kind: TabKind::Metrics(Box::new(MetricsTab::new(id, board_id, name))) });
                self.active = self.tabs.len() - 1;
            }
            WsAction::OpenDiagram => {
                if let Some(i) = self.tabs.iter().position(|t| matches!(t.kind, TabKind::Diagram(_))) {
                    self.active = i;
                    return;
                }
                let id = self.new_id();
                let tables: Vec<DbObject> = self.objects.iter().filter(|o| o.kind == ObjectKind::Table).cloned().collect();
                self.tabs.push(Tab { id, kind: TabKind::Diagram(Box::new(DiagramTab::new(self.schema.clone(), tables))) });
                self.active = self.tabs.len() - 1;
            }
            WsAction::KillProcess(pid) => {
                if let Some(s) = &self.session {
                    s.log(format!("-- kill {pid}"), true);
                    self.kill_task = Some(s.spawn(ctx, move |d| async move { d.lock().await.kill_process(&pid).await }));
                }
            }
        }
    }

    // ---------- UI ----------

    pub fn ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared) {
        let ctx = ui.ctx().clone();
        set_sh_ctx(&ctx);
        self.poll(&ctx, sh);
        if let Some(session) = self.session.clone() {
            let mut completed_actions = Vec::new();
            for tab in &mut self.tabs {
                if let TabKind::Query(q) = &mut tab.kind {
                    let mut cx = TabCtx { ctx: &ctx, session: &session, settings: sh.settings, store: sh.store, actions: &mut completed_actions };
                    q.poll(&mut cx);
                }
            }
            for action in completed_actions { self.handle(action, sh, &ctx); }
        }
        let mut actions: Vec<WsAction> = Vec::new();

        self.env_ribbon(&ctx);
        if !cfg!(target_os = "macos") { self.menu_bar_ui(ui, sh, &mut actions); }
        self.header_ui(ui, sh, &mut actions);

        if self.session.is_none() {
            self.connecting_ui(ui, sh);
            return;
        }
        let session = self.session.clone().expect("session checked above");

        if self.console {
            egui::Panel::bottom(egui::Id::unique(("console", self.cfg.id)))
                .resizable(true)
                .default_size(190.0)
                .frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 12 }))
                .show(ui, |ui| {
                    glass_frame().show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        console_ui(ui, &session);
                    });
                });
        }

        let narrow = ui.available_width() < 1120.0;
        egui::Panel::left(egui::Id::unique(("explorer-panel-v2", self.cfg.id)))
            .resizable(true)
            .show_separator_line(false)
            .default_size(264.0)
            .min_size(220.0)
            .max_size(if narrow { 252.0 } else { 360.0 })
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 0, top: 0, bottom: 12 }))
            .show(ui, |ui| {
                glass_frame().show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.sidebar_ui(ui, sh, &mut actions);
                });
            });

        if self.right != RightPanel::None && !narrow {
            egui::Panel::right(egui::Id::unique(("right", self.cfg.id)))
                .resizable(true)
                .show_separator_line(false)
                .default_size(340.0)
                .min_size(260.0)
                .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 12, top: 0, bottom: 12 }))
                .show(ui, |ui| {
                    glass_frame().inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        self.right_ui(ui, sh, &session, &mut actions);
                    });
                });
        }

        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 12 })).show(ui, |ui| {
            self.tab_bar_ui(ui, sh, &mut actions);
            ui.add_space(8.0);
            content_frame().show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.tabs.is_empty() {
                    empty_state(ui, Icon::Table, "Nothing open", "Pick a table in the explorer, or press ⌘T for a new query");
                    return;
                }
                let catalog = self.catalog.clone();
                let active = self.active.min(self.tabs.len() - 1);
                let mut cx = TabCtx { ctx: &ctx, session: &session, settings: sh.settings, store: sh.store, actions: &mut actions };
                match &mut self.tabs[active].kind {
                    TabKind::Table(t) => t.ui(ui, &mut cx, &catalog),
                    TabKind::Query(q) => q.ui(ui, &mut cx, &catalog, self.context_ready),
                    TabKind::Admin(a) => admin_ui(a, ui, &mut cx),
                    TabKind::Metrics(m) => m.ui(ui, &mut cx, sh.boards, sh.store),
                    TabKind::Diagram(d) => d.ui(ui, &mut cx),
                }
            });
        });

        if narrow && self.right != RightPanel::None {
            let mut open = true;
            egui::Window::new(if self.right == RightPanel::Assistant { "Assistant" } else { "Inspector" })
                .id(egui::Id::unique(("compact-inspector", self.cfg.id)))
                .open(&mut open)
                .frame(egui::Frame::window(ui.style()).fill(p().sidebar))
                .anchor(egui::Align2::RIGHT_TOP, vec2(-16.0, 116.0))
                .max_height((ctx.content_rect().height() - 144.0).max(200.0))
                .default_width(320.0)
                .max_width((ctx.content_rect().width() - 48.0).max(260.0))
                .vscroll(true)
                .show(&ctx, |ui| self.right_ui(ui, sh, &session, &mut actions));
            if !open { self.right = RightPanel::None; }
        }

        if self.transaction_action.is_none() {
            self.shortcuts(&ctx, sh, &mut actions);
            self.script_step(&ctx, sh, &mut actions);
        }

        if let Some(mut m) = self.modal.take() {
            let info = dialogs::ModalInfo { session: &session, cfg: &self.cfg, current_db: &self.current_db, objects: &self.objects, databases: &self.databases, queries: sh.queries };
            match dialogs::show(&mut m, &ctx, &info) {
                ModalOutcome::Keep => self.modal = Some(m),
                ModalOutcome::Close => {}
                ModalOutcome::Approved(g) => self.run_gated(g, sh),
                ModalOutcome::Actions(list) => actions.extend(list),
                ModalOutcome::SaveQuery { tab, name, shared } => {
                    if let Some(i) = self.tab_index(tab) {
                        if let TabKind::Query(q) = &mut self.tabs[i].kind {
                            let sq = SavedQuery { id: uuid::Uuid::new_v4(), name: name.clone(), sql: q.sql.clone(), connection: if shared { None } else { Some(self.cfg.id) }, favorite: true, updated_at: chrono::Utc::now() };
                            q.saved_id = Some(sq.id);
                            q.title = name;
                            q.dirty = false;
                            sh.queries.push(sq);
                            let _ = sh.store.save_queries(sh.queries);
                            sh.toasts.success("Saved to Queries");
                        }
                    }
                }
                ModalOutcome::Toast(k, t) => sh.toasts.push(k, t),
            }
        }

        for a in actions {
            self.handle(a, sh, &ctx);
        }
        self.transaction_dialog(&ctx, sh);
    }

    /// Replays one STRATA_SCRIPT step every ~0.7 s once the connection is idle. Steps:
    /// `open:TABLE`, `view:data|structure|relations|profile`, `select:ROW`, `inspect`, `edit:ROW:COL:VALUE`,
    /// `delete:ROW`, `insert`, `review`, `graph`, `er`, `node:TABLE`, `query:SQL`, `assistant`, `console`.
    fn script_step(&mut self, ctx: &egui::Context, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        if self.script.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let busy = self.init_task.is_some() || self.objects_task.is_some() || self.tabs.get(self.active).map(|t| matches!(&t.kind, TabKind::Table(tt) if tt.is_busy())).unwrap_or(false);
        if busy || self.script_at.elapsed().as_millis() < 700 {
            return;
        }
        self.script_at = Instant::now();
        let Some(step) = self.script.pop_front() else { return };
        let (cmd, arg) = step.split_once(':').unwrap_or((step.as_str(), ""));
        match cmd {
            "open" => {
                if let Some(o) = self.objects.iter().find(|o| o.name == arg).cloned() {
                    self.open_object(o, false, true, sh.settings);
                }
            }
            "window" => {
                if let Some((w, h)) = arg.split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>()) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(w, h)));
                    }
                }
            }
            "graph" => actions.push(WsAction::OpenDiagram),
            "query" => actions.push(WsAction::OpenQuery { sql: arg.replace("|", ";\n"), title: None, run: true }),
            "inspect" => self.right = RightPanel::RowDetail,
            "assistant" => self.right = RightPanel::Assistant,
            "console" => self.console = !self.console,
            "review" => {
                if let Some(t) = self.tabs.get(self.active) {
                    actions.push(WsAction::PreviewCommit(t.id));
                }
            }
            "er" | "node" => {
                if let Some(TabKind::Diagram(d)) = self.tabs.get_mut(self.active).map(|t| &mut t.kind) {
                    if cmd == "er" {
                        d.set_er();
                    } else {
                        d.select_named(arg);
                    }
                }
            }
            _ => {
                let Some(TabKind::Table(t)) = self.tabs.get_mut(self.active).map(|t| &mut t.kind) else { return };
                let mut parts = arg.splitn(3, ':');
                let row: usize = parts.next().and_then(|r| r.parse().ok()).unwrap_or(0);
                match cmd {
                    "view" => {
                        t.view = match arg {
                            "structure" => TableView::Structure,
                            "relations" => TableView::Relations,
                            "profile" => TableView::Profile,
                            _ => TableView::Data,
                        }
                    }
                    "select" => t.grid.select_row(row),
                    "delete" => {
                        t.grid.select_row(row);
                        t.delete_selected();
                    }
                    "insert" => t.insert_row(),
                    "edit" => {
                        let (col, value) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                        t.script_edit(row, col, value);
                    }
                    _ => {}
                }
            }
        }
    }

    /// A hairline in the environment color along the window's top edge.
    fn env_ribbon(&self, ctx: &egui::Context) {
        if self.cfg.tag == Tag::None {
            return;
        }
        let r = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("env-ribbon")));
        painter.rect_filled(egui::Rect::from_min_size(r.min, vec2(r.width(), 3.0)), egui::CornerRadius::ZERO, crate::theme::rgb(self.cfg.tag.color()));
    }

    fn menu_bar_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        egui::Panel::top(egui::Id::unique(("workspace-menubar", self.cfg.id)))
            .exact_size(32.0).show_separator_line(false)
            .frame(egui::Frame::new().fill(p().window).inner_margin(egui::Margin { left: if cfg!(target_os = "macos") { 88 } else { 12 }, right: 12, top: 2, bottom: 2 }))
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("New connection…").clicked() { *sh.show_home = true; ui.close(); }
                        if ui.button("New query  ⌘T").clicked() { actions.push(WsAction::OpenQuery { sql: String::new(), title: None, run: false }); ui.close(); }
                        if ui.button("Import data…").clicked() { actions.push(WsAction::ShowModal(Box::new(Modal::import_csv(self.schema.clone(), sh.settings.csv_delimiter)))); ui.close(); }
                        if ui.button("Export tables…").clicked() { actions.push(WsAction::ShowModal(Box::new(Modal::export_tables(self.objects.iter().filter(|o| o.kind == ObjectKind::Table).cloned().collect())))); ui.close(); }
                        ui.separator();
                        if ui.button("Settings…  ⌘,").clicked() { *sh.open_settings = true; ui.close(); }
                        if ui.button("Close connection").clicked() { *sh.workspace_command = Some(WorkspaceCommand::Close(self.cfg.id)); ui.close(); }
                    });
                    ui.menu_button("View", |ui| {
                        if ui.button("Explorer").clicked() { self.sidebar_mode = SidebarMode::Items; ui.close(); }
                        if ui.button("Saved queries").clicked() { self.sidebar_mode = SidebarMode::Queries; ui.close(); }
                        if ui.button("Query history").clicked() { self.sidebar_mode = SidebarMode::History; ui.close(); }
                        if ui.button("Inspector").clicked() { actions.push(WsAction::ToggleRowDetail); ui.close(); }
                        if ui.button("Console").clicked() { self.console = !self.console; ui.close(); }
                        if ui.button("Relationship graph").clicked() { actions.push(WsAction::OpenDiagram); ui.close(); }
                    });
                    ui.menu_button("Connections", |ui| {
                        if ui.button("All connections…").clicked() { *sh.show_home = true; ui.close(); }
                        for choice in sh.workspace_choices {
                            if ui.selectable_label(choice.id == self.cfg.id, &choice.name).clicked() { *sh.switch_workspace = Some(choice.id); ui.close(); }
                        }
                        ui.separator();
                        if ui.button("Reconnect").clicked() { *sh.workspace_command = Some(WorkspaceCommand::Reconnect(self.cfg.id)); ui.close(); }
                        if ui.button("Edit connection…").clicked() { *sh.edit_connection = Some(self.cfg.id); ui.close(); }
                    });
                    ui.menu_button("Database", |ui| self.more_menu(ui, sh, actions));
                    ui.menu_button("Query", |ui| {
                        if let Some(Tab { id, kind: TabKind::Query(q) }) = self.tabs.get(self.active) {
                            if ui.button("Run all statements").clicked() { actions.push(WsAction::RunQuery { tab_id: *id, sql: q.sql.clone() }); ui.close(); }
                            if ui.button("Save query…").clicked() { actions.push(WsAction::SaveQuery(*id)); ui.close(); }
                        } else { ui.label(dim("Open a query tab to run SQL")); }
                        if ui.button("Cancel running query").clicked() { self.cancel_active_query(); ui.close(); }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("Connection information").clicked() {
                            let version = self.session.as_ref().and_then(|s| s.server_version.clone()).unwrap_or_else(|| "Version unavailable".into());
                            actions.push(WsAction::QuickLookText { title: "Connection information".into(), text: format!("{}\n{}\nDatabase: {}\nSchema: {}\nServer: {}", self.cfg.display_name(), self.cfg.driver.label(), self.current_db, self.schema, version) }); ui.close();
                        }
                        if ui.button("Keyboard shortcuts").clicked() { actions.push(WsAction::QuickLookText { title: "Keyboard shortcuts".into(), text: "⌘T  New query\n⌘K  Find tables and commands\n⌘Enter  Run statement\n⌘Shift+Enter  Run all\n⌘S  Review changes or save query\n⌘R  Reload table\nSpace  Quick Look\nDouble-click a cell value to edit. Use the arrow to open a related row.".into() }); ui.close(); }
                        ui.separator();
                        if ui.button("Check for updates…").clicked() { *sh.app_request = Some(AppRequest::CheckForUpdates); ui.close(); }
                        if ui.button("About Strata").clicked() { *sh.app_request = Some(AppRequest::About); ui.close(); }
                    });
                });
            });
    }

    fn header_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        let compact = ui.available_width() < 1360.0;
        let top = 10;
        egui::Panel::top(egui::Id::unique(("header", self.cfg.id)))
            .exact_size(if compact { 98.0 + top as f32 } else { 54.0 + top as f32 })
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: if cfg!(target_os = "macos") { 96 } else { 16 }, right: 16, top, bottom: 10 }))
            .show(ui, |ui| {
                let bg = ui.interact(ui.max_rect(), egui::Id::unique(("header-drag", self.cfg.id)), egui::Sense::click_and_drag());
                if bg.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if bg.double_clicked() {
                    let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.allocate_ui_with_layout(vec2(140.0, 36.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(140.0);
                        let (r, response) = ui.allocate_exact_size(vec2(28.0, 28.0), egui::Sense::click());
                        super::home::paint_logo(ui.painter(), r);
                        if response.on_hover_text("Connections").clicked() { *sh.show_home = true; }
                        ui.label(title("Strata", 19.0));
                    });
                    let reserve = if compact { 156.0 } else { 380.0 };
                    let command_width = (ui.available_width() - reserve - 8.0).max(300.0);
                    egui::Frame::new().fill(p().sidebar).stroke(egui::Stroke::new(1.0, p().border))
                        .corner_radius(egui::CornerRadius::same(24)).inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.set_width(command_width - 14.0);
                            ui.horizontal(|ui| {
                                if compact {
                                    self.location_ui(ui, actions);
                                    self.server_identity_ui(ui, 142.0);
                                } else {
                                    ui.allocate_ui_with_layout(vec2(350.0, 32.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.set_min_width(350.0);
                                        self.location_ui(ui, actions);
                                    });
                                    self.server_identity_ui(ui, 142.0);
                                    self.jump_ui(ui, ui.available_width().max(100.0));
                                }
                            });
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.header_tools_ui(ui, sh, actions);
                        if !compact {
                            self.connection_status_ui(ui);
                        }
                    });
                });
                if compact {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        self.jump_ui(ui, (ui.available_width() - 280.0).max(100.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.connection_status_ui(ui));
                    });
                }
            });
    }

    fn location_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<WsAction>) {
        let pal = p();
        ui.spacing_mut().item_spacing.x = 6.0;
        if self.cfg.tag != Tag::None {
            env_chip(ui, &self.cfg.tag.label().to_uppercase(), crate::theme::rgb(self.cfg.tag.color()), true);
        } else {
            super::driver_tile(ui, self.cfg.driver, 26.0);
        }
        // Bound every breadcrumb so long connection/database names cannot displace actions.
        let db_name = if self.current_db.is_empty() { self.cfg.display_name() } else { self.current_db.clone() };
        let db_width = (ui.available_width() * 0.6).clamp(70.0, 170.0);
        let connection_name = self.cfg.display_name();
        let db_label = if connection_name == db_name { db_name.clone() } else { format!("{connection_name} · {db_name}") };
        let db_btn = ui.add_sized(vec2(db_width, 32.0), egui::Button::new(RichText::new(format!("{db_label} ▾")).font(medium(13.0)).color(pal.text)).truncate().frame(false))
            .on_hover_text(format!("{} · {}", self.cfg.display_name(), db_name));
        if db_btn.clicked() { self.database_picker.reset(); }
        egui::Popup::menu(&db_btn).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(200.0);
            ui.label(dim(self.cfg.display_name()));
            ui.separator();
            if let Some(db) = self.database_picker.show(ui, &self.databases, &self.current_db) {
                if db != self.current_db { actions.push(WsAction::SwitchDatabase(db)); }
                ui.close();
            }
        });
        let dialect = self.cfg.driver.dialect();
        if !self.schema.is_empty() && self.schema != self.current_db && !matches!(dialect, Dialect::MySql | Dialect::ClickHouse | Dialect::Mongo) {
            ui.label(faint("/"));
            let sb = ui.add_sized(vec2(ui.available_width().clamp(50.0, 90.0), 32.0), egui::Button::new(RichText::new(format!("{} ▾", self.schema)).color(pal.text_dim)).truncate().frame(false)).on_hover_text(&self.schema);
            if sb.clicked() { self.schema_picker.reset(); }
            egui::Popup::menu(&sb).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(180.0);
                if let Some(schema) = self.schema_picker.show(ui, &self.schemas, &self.schema) {
                    if schema != self.schema { actions.push(WsAction::SwitchSchema(schema)); }
                    ui.close();
                }
            });
        }
    }

    fn jump_ui(&mut self, ui: &mut egui::Ui, width: f32) {
        let pal = p();
        let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), egui::Sense::click());
        ui.painter().rect_filled(rect, egui::CornerRadius::same(8), if response.hovered() { pal.surface_hover } else { pal.content });
        ui.painter().rect_stroke(rect, egui::CornerRadius::same(8), egui::Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
        paint_icon(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, rect.center().y), vec2(14.0, 14.0)), Icon::Search, pal.text_dim);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Jump to anything"));
        let painter = ui.painter_at(rect.shrink2(vec2(32.0, 0.0)));
        painter.text(egui::pos2(rect.left() + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, "Jump to anything", egui::FontId::proportional(12.5), pal.text_dim);
        ui.painter().text(egui::pos2(rect.right() - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, "⌘K", egui::FontId::monospace(11.0), pal.text_faint);
        if response.on_hover_text("Find tables, queries and commands (⌘K)").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.modal = Some(Modal::open_anything());
        }
    }

    fn server_identity_ui(&self, ui: &mut egui::Ui, width: f32) {
        let Some(session) = &self.session else { return };
        let server = session.server_version.as_deref().unwrap_or("Version unavailable");
        let version = server.split(" on ").next().unwrap_or(server).trim();
        let version = ["Microsoft SQL Server", "Oracle Database", "PostgreSQL", "CockroachDB", "MariaDB"]
            .iter().find_map(|provider| version.strip_prefix(provider).map(str::trim))
            .unwrap_or_else(|| version.strip_prefix(self.cfg.driver.label()).map(str::trim).unwrap_or(version));
        let identity = format!("{} {version}", self.cfg.driver.label());
        ui.add_sized(vec2(width, 32.0), egui::Label::new(faint(identity)).truncate()).on_hover_text(server);
    }

    fn header_tools_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        if self.session.is_none() { return; }
        let more = icon_button(ui, Icon::More, "More actions");
        egui::Popup::menu(&more).show(|ui| self.more_menu(ui, sh, actions));
        if icon_button_colored(ui, Icon::Sparkle, "Assistant", None, self.right == RightPanel::Assistant).clicked() {
            self.right = if self.right == RightPanel::Assistant { RightPanel::None } else { RightPanel::Assistant };
        }
        if icon_button_colored(ui, Icon::Sidebar, "Inspector", None, self.right == RightPanel::RowDetail).clicked() {
            actions.push(WsAction::ToggleRowDetail);
        }
        if icon_button_colored(ui, Icon::Terminal, "Console (⌘⇧C)", None, self.console).clicked() {
            self.console = !self.console;
        }
    }

    fn connection_status_ui(&mut self, ui: &mut egui::Ui) {
        let pal = p();
        let (dot, text) = match self.status {
            ConnState::Connected => (pal.success, self.latency_ms.map(|ms| format!("{ms} ms")).unwrap_or_else(|| "Online".into())),
            ConnState::Connecting => (pal.warning, "Connecting".into()),
            ConnState::Failed => (pal.danger, "Failed".into()),
            ConnState::Lost => (pal.danger, "Lost".into()),
        };
        status_dot_chip(ui, &text, dot).on_hover_text(if self.cfg.ssh.enabled { format!("Through SSH {}", self.cfg.ssh.host) } else { self.cfg.subtitle() });
        let (color, label) = match self.cfg.safe_mode {
            SafeMode::Silent if self.cfg.read_only => (pal.success, "Read-only"),
            SafeMode::Silent => (pal.warning, "Direct writes"),
            SafeMode::AlertAll | SafeMode::AlertWrites => (pal.success, "Review writes"),
            SafeMode::PasswordAll | SafeMode::PasswordWrites => (pal.success, "Password required"),
        };
        let chip = status_chip(ui, Icon::Shield, label, color);
        egui::Popup::menu(&chip).show(|ui| {
            for mode in SafeMode::ALL {
                if ui.selectable_label(self.cfg.safe_mode == mode, mode.label()).clicked() {
                    self.cfg.safe_mode = mode;
                    ui.close();
                }
            }
        });
    }

    fn more_menu(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() - 80.0).max(200.0)).show(ui, |ui| self.more_menu_contents(ui, sh, actions));
    }

    fn more_menu_contents(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        ui.set_min_width(240.0);
        let Some(s) = self.session.clone() else { return };
        let d = s.dialect();
        fn item(ui: &mut egui::Ui, actions: &mut Vec<WsAction>, label: &str, a: WsAction) {
            if ui.button(label).clicked() {
                actions.push(a);
                ui.close();
            }
        }
        item(ui, actions, "New query  ⌘T", WsAction::OpenQuery { sql: String::new(), title: None, run: false });
        item(ui, actions, "Relationship graph", WsAction::OpenDiagram);
        ui.menu_button("Metrics boards", |ui| {
            for b in sh.boards.iter().filter(|b| b.connection == self.cfg.id) {
                if ui.button(&b.name).clicked() {
                    actions.push(WsAction::OpenMetrics(Some(b.id)));
                    ui.close();
                }
            }
            if ui.button("+ New board").clicked() {
                actions.push(WsAction::OpenMetrics(None));
                ui.close();
            }
        });
        ui.separator();
        item(ui, actions, "Import from CSV…", WsAction::ShowModal(Box::new(Modal::import_csv(self.schema.clone(), sh.settings.csv_delimiter))));
        item(ui, actions, "Import from JSON…", WsAction::ShowModal(Box::new(Modal::import_json(self.schema.clone()))));
        item(ui, actions, "Import SQL dump…", WsAction::ShowModal(Box::new(Modal::import_sql())));
        item(ui, actions, "Export tables…", WsAction::ShowModal(Box::new(Modal::export_tables(self.objects.iter().filter(|o| o.kind == ObjectKind::Table).cloned().collect()))));
        if !matches!(d, Dialect::Redis | Dialect::Mongo | Dialect::Elastic) {
            ui.separator();
            item(ui, actions, "Backup database…", WsAction::ShowModal(Box::new(Modal::backup(self.current_db.clone()))));
            item(ui, actions, "Restore database…", WsAction::ShowModal(Box::new(Modal::restore(self.current_db.clone()))));
        }
        ui.separator();
        if meta::supports(d, StructureFeature::Databases) && !matches!(d, Dialect::Redis) {
            item(ui, actions, "New database…", WsAction::ShowModal(Box::new(Modal::NewDatabase { name: String::new(), encoding: String::new() })));
        }
        if d == Dialect::Postgres || d == Dialect::SqlServer {
            item(ui, actions, "New schema…", WsAction::ShowModal(Box::new(Modal::NewSchema { name: String::new() })));
        }
        if !matches!(d, Dialect::Redis | Dialect::Elastic) {
            item(ui, actions, "New table…", WsAction::ShowModal(Box::new(Modal::create_table(self.schema.clone(), d))));
        }
        for kind in crate::db::objects::supported(d) {
            item(ui, actions, &format!("New {}…", crate::db::objects::label(kind).to_lowercase()), WsAction::ShowModal(Box::new(Modal::CreateObject(Box::new(crate::db::objects::ObjectDraft::new(kind, self.schema.clone()))))));
        }
        ui.separator();
        item(ui, actions, "Process list", WsAction::OpenAdmin(AdminKind::Processes));
        if !matches!(d, Dialect::Sqlite | Dialect::Redis | Dialect::Elastic) {
            item(ui, actions, "Users & roles", WsAction::OpenAdmin(AdminKind::Users));
        }
        ui.separator();
        if ui.button("Copy connection URL").clicked() {
            ui.ctx().copy_text(self.cfg.to_url(false));
            ui.close();
        }
        if ui.button("Edit connection…").clicked() {
            *sh.edit_connection = Some(self.cfg.id);
            ui.close();
        }
    }

    fn connecting_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared) {
        let pal = p();
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.22);
                ui.allocate_ui(vec2(520.0, 320.0), |ui| {
                    glass_frame().inner_margin(egui::Margin::same(28)).show(ui, |ui| {
                        ui.set_width(464.0);
                        ui.vertical_centered(|ui| {
                            super::driver_tile(ui, self.cfg.driver, 54.0);
                            ui.add_space(12.0);
                            ui.label(title(self.cfg.display_name(), 20.0));
                            ui.label(dim(self.cfg.subtitle()));
                            ui.add_space(16.0);
                            if let Some(err) = self.connect_error.clone() {
                                egui::Frame::new().fill(pal.danger.gamma_multiply(0.12)).corner_radius(egui::CornerRadius::same(12)).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new("Can't connect").font(semibold(13.0)).color(readable_on_tint(pal.danger)));
                                    ui.add(egui::Label::new(RichText::new(&err).monospace().size(12.0).color(pal.text)).wrap());
                                });
                                ui.add_space(14.0);
                                ui.horizontal(|ui| {
                                    ui.add_space((ui.available_width() - 230.0).max(0.0) / 2.0);
                                    if primary_button(ui, "  Try again  ").clicked() {
                                        self.connect(&ctx);
                                    }
                                    if secondary_button(ui, "  Edit connection  ").clicked() {
                                        *sh.edit_connection = Some(self.cfg.id);
                                    }
                                });
                            } else {
                                ui.add(egui::Spinner::new().size(22.0).color(pal.accent));
                                ui.add_space(6.0);
                                ui.label(faint(if self.cfg.ssh.enabled { "Opening the SSH tunnel and connecting…" } else { "Connecting…" }));
                            }
                        });
                    });
                });
            });
        });
    }

    fn workspace_switcher_ui(&self, ui: &mut egui::Ui, sh: &mut Shared) {
        let pal = p();
        egui::ScrollArea::horizontal().id_salt(("workspace-switcher", self.cfg.id))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for choice in sh.workspace_choices {
                        let active = choice.id == self.cfg.id;
                        let (rect, response) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::click());
                        let initials: String = choice.name.chars().filter(|c| c.is_alphanumeric()).take(2).collect::<String>().to_uppercase();
                        ui.painter().rect_filled(rect, egui::CornerRadius::same(11), if active { mix(pal.surface, choice.color, 0.12) } else { pal.surface });
                        ui.painter().rect_stroke(rect, egui::CornerRadius::same(11), egui::Stroke::new(if active { 2.0 } else { 1.0 }, if active { choice.color } else { pal.border }), egui::StrokeKind::Inside);
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initials, semibold(12.0), readable_on_tint(choice.color));
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &choice.name));
                        let response = response.on_hover_text(&choice.name);
                        if response.clicked() { *sh.switch_workspace = Some(choice.id); }
                        if response.middle_clicked() { *sh.workspace_command = Some(WorkspaceCommand::Close(choice.id)); }
                        response.context_menu(|ui| {
                            if ui.button("Reconnect").clicked() {
                                *sh.workspace_command = Some(WorkspaceCommand::Reconnect(choice.id));
                                ui.close();
                            }
                            if ui.button("Close connection").clicked() {
                                *sh.workspace_command = Some(WorkspaceCommand::Close(choice.id));
                                ui.close();
                            }
                        });
                    }
                    let (rect, response) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::click());
                    ui.painter().rect_stroke(rect, egui::CornerRadius::same(11), egui::Stroke::new(1.0, pal.border), egui::StrokeKind::Inside);
                    paint_icon(ui.painter(), rect.shrink(11.0), Icon::Plus, pal.text_dim);
                    if response.on_hover_text("Open another connection").clicked() { *sh.show_home = true; }
                });
            });
    }

    fn sidebar_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        self.workspace_switcher_ui(ui, sh);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.menu_button("+ Create", |ui| {
                egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() - 100.0).max(200.0)).show(ui, |ui| {
                let d = self.cfg.driver.dialect();
                if !matches!(d, Dialect::Redis | Dialect::Elastic) && ui.button("New table…").clicked() { actions.push(WsAction::ShowModal(Box::new(Modal::create_table(self.schema.clone(), d)))); ui.close(); }
                if meta::supports(d, StructureFeature::Databases) && d != Dialect::Redis && ui.button("New database…").clicked() { actions.push(WsAction::ShowModal(Box::new(Modal::NewDatabase { name: String::new(), encoding: String::new() }))); ui.close(); }
                if matches!(d, Dialect::Postgres | Dialect::SqlServer) && ui.button("New schema…").clicked() { actions.push(WsAction::ShowModal(Box::new(Modal::NewSchema { name: String::new() }))); ui.close(); }
                for kind in crate::db::objects::supported(d) {
                    if ui.button(format!("New {}…", crate::db::objects::label(kind).to_lowercase())).clicked() {
                        actions.push(WsAction::ShowModal(Box::new(Modal::CreateObject(Box::new(crate::db::objects::ObjectDraft::new(kind, self.schema.clone())))))); ui.close();
                    }
                }
                if d == Dialect::Sqlite && ui.button("New database file…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("SQLite database", &["sqlite", "db"]).set_file_name("database.sqlite").save_file() {
                        let mut cfg = ConnectionConfig::new(DriverKind::Sqlite); cfg.path = path.display().to_string();
                        *sh.create_connection = Some(cfg);
                    }
                    ui.close();
                }
                });
            });
            if icon_button(ui, Icon::Refresh, "Refresh objects").clicked() { actions.push(WsAction::RefreshObjects); }
        });
        ui.add_space(8.0);
        egui::Panel::bottom(egui::Id::unique(("explorer-navigation", self.cfg.id)))
            .exact_size(34.0).show_separator_line(false).frame(egui::Frame::NONE)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (mode, icon, label) in [(SidebarMode::Items, Icon::Table, "Explorer"), (SidebarMode::Queries, Icon::Code, "Saved queries"), (SidebarMode::History, Icon::Clock, "History")] {
                        if icon_button_colored(ui, icon, label, None, self.sidebar_mode == mode).clicked() { self.sidebar_mode = mode; }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Gear, "Settings (⌘,)").clicked() { *sh.open_settings = true; }
                    });
                });
            });
        match self.sidebar_mode {
            SidebarMode::Items => self.items_ui(ui, sh, actions),
            SidebarMode::Queries => self.queries_ui(ui, sh, actions),
            SidebarMode::History => self.history_ui(ui, sh, actions),
        }
    }

    fn items_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        let pal = p();
        let w = ui.available_width();
        let n_tables = self.objects.iter().filter(|o| o.kind.has_rows()).count();
        let search = search_field(ui, &mut self.sidebar_search, &format!("Filter {n_tables} items"), w);
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::F)) {
            search.request_focus();
        }
        ui.add_space(10.0);
        if self.init_task.is_some() || self.objects_task.is_some() {
            ui.add(egui::Spinner::new().size(14.0).color(pal.accent));
        }
        let q = self.sidebar_search.to_lowercase();
        let active_obj = match self.tabs.get(self.active).map(|t| &t.kind) {
            Some(TabKind::Table(t)) => Some(t.obj.clone()),
            _ => None,
        };
        let dialect = self.cfg.driver.dialect();
        let max_rows = self.stats.values().filter_map(|s| s.rows).max().unwrap_or(0);
        let mut toggle_pin: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let pinned: Vec<DbObject> = self.cfg.pinned.iter().filter_map(|n| self.objects.iter().find(|o| &o.name == n).cloned()).filter(|o| q.is_empty() || fuzzy(&o.name.to_lowercase(), &q)).collect();
            if !pinned.is_empty() {
                section_label(ui, "Pinned");
                ui.add_space(4.0);
                for obj in pinned {
                    let selected = active_obj.as_ref() == Some(&obj);
                    if let Some(p) = self.object_row(ui, &obj, selected, true, max_rows, dialect, sh, actions) {
                        toggle_pin = Some(p);
                    }
                }
                ui.add_space(10.0);
            }
            let mut kinds: Vec<ObjectKind> = self.objects.iter().map(|o| o.kind).collect();
            kinds.dedup();
            for kind in kinds {
                let items: Vec<DbObject> = self.objects.iter().filter(|o| o.kind == kind && (q.is_empty() || fuzzy(&o.name.to_lowercase(), &q))).cloned().collect();
                if items.is_empty() {
                    continue;
                }
                let collapsed = self.collapsed.contains(&kind) && q.is_empty();
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), egui::Sense::click());
                paint_icon(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.left() + 6.0, r.center().y), vec2(10.0, 10.0)), if collapsed { Icon::ChevronRight } else { Icon::ChevronDown }, pal.text_faint);
                let label = if kind == ObjectKind::Table && !self.schema.is_empty() && self.schema != self.current_db { format!("{} · {}", self.schema, kind.section()) } else { kind.section().to_string() };
                ui.painter().text(egui::pos2(r.left() + 16.0, r.center().y), egui::Align2::LEFT_CENTER, label.to_uppercase(), semibold(10.5), pal.text_faint);
                let right = if kind == ObjectKind::Table && !self.stats.is_empty() { "rows · size".to_string() } else { items.len().to_string() };
                ui.painter().text(egui::pos2(r.right() - 4.0, r.center().y), egui::Align2::RIGHT_CENTER, right, egui::FontId::proportional(10.5), pal.text_faint);
                if resp.clicked() && !self.collapsed.remove(&kind) {
                    self.collapsed.insert(kind);
                }
                if collapsed {
                    ui.add_space(4.0);
                    continue;
                }
                for obj in items {
                    let selected = active_obj.as_ref() == Some(&obj);
                    if let Some(p) = self.object_row(ui, &obj, selected, false, max_rows, dialect, sh, actions) {
                        toggle_pin = Some(p);
                    }
                }
                ui.add_space(8.0);
            }
            if self.objects.is_empty() && self.init_task.is_none() && self.objects_task.is_none() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(faint("No items in this schema")));
            }
        });
        if let Some(name) = toggle_pin {
            if let Some(i) = self.cfg.pinned.iter().position(|n| *n == name) {
                self.cfg.pinned.remove(i);
            } else {
                self.cfg.pinned.push(name);
            }
        }
    }

    /// One explorer row. Returns a table name when its pin was toggled.
    #[allow(clippy::too_many_arguments)]
    fn object_row(&self, ui: &mut egui::Ui, obj: &DbObject, selected: bool, pinned_section: bool, max_rows: u64, dialect: Dialect, sh: &mut Shared, actions: &mut Vec<WsAction>) -> Option<String> {
        let pal = p();
        let mut pin = None;
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), egui::Sense::hover());
        let resp = ui.interact(r, ui.scope_id().with(("object-row", &obj.schema, &obj.name, &obj.parent, &obj.arguments, pinned_section)), egui::Sense::click());
        if selected {
            ui.painter().rect_filled(r, egui::CornerRadius::same(10), egui::Color32::from_rgba_unmultiplied(pal.accent.r(), pal.accent.g(), pal.accent.b(), 26));
            ui.painter().rect_stroke(r, egui::CornerRadius::same(10), egui::Stroke::new(1.0, pal.accent.gamma_multiply(0.4)), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, egui::CornerRadius::same(10), pal.surface_hover);
        }
        let icon = if pinned_section {
            Icon::Star
        } else {
            match obj.kind {
                ObjectKind::View | ObjectKind::MaterializedView => Icon::View,
                ObjectKind::Function | ObjectKind::Procedure => Icon::Function,
                ObjectKind::Keys => Icon::Key,
                ObjectKind::Sequence => Icon::Layers,
                _ => Icon::Table,
            }
        };
        let icon_color = if pinned_section { pal.accent } else if selected { pal.accent } else { pal.text_faint };
        paint_icon(ui.painter(), egui::Rect::from_center_size(egui::pos2(r.left() + 15.0, r.center().y), vec2(14.0, 14.0)), icon, icon_color);
        let stat = self.stats.get(&obj.name).filter(|_| obj.kind.has_rows());
        let right_w = if stat.is_some() { 116.0 } else { 30.0 };
        let painter = ui.painter_at(egui::Rect::from_min_max(r.min, egui::pos2(r.right() - right_w, r.bottom())));
        painter.text(egui::pos2(r.left() + 30.0, r.center().y), egui::Align2::LEFT_CENTER, obj.display_name(), egui::FontId::proportional(13.0), if selected { pal.text } else { mix(pal.text, pal.text_dim, 0.3) });
        if let Some(st) = stat {
            if let Some(n) = st.rows {
                ui.painter().text(egui::pos2(r.right() - 74.0, r.center().y), egui::Align2::RIGHT_CENTER, short_count(n), egui::FontId::monospace(11.0), pal.text_faint);
                let bar = egui::Rect::from_min_size(egui::pos2(r.right() - 68.0, r.center().y - 2.0), vec2(36.0, 4.0));
                ui.painter().rect_filled(bar, egui::CornerRadius::same(2), pal.surface_hover);
                let frac = if max_rows == 0 { 0.0 } else { ((n.max(1) as f32).log10() / (max_rows.max(10) as f32).log10()).clamp(0.08, 1.0) };
                ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(36.0 * frac, 4.0)), egui::CornerRadius::same(2), if selected { pal.accent } else { pal.text_faint.gamma_multiply(0.8) });
            }
        }
        let resp = match stat.and_then(|s| s.bytes) {
            Some(b) => resp.on_hover_text(format!("{} · {}", obj.name, human_bytes(b))),
            None => resp,
        };
        let cmd = ui.input(|i| i.modifiers.command);
        if resp.double_clicked() {
            actions.push(WsAction::OpenObject { obj: obj.clone(), structure: false, new_tab: true });
        } else if resp.clicked() {
            actions.push(WsAction::OpenObject { obj: obj.clone(), structure: false, new_tab: cmd });
        }
        let options_rect = egui::Rect::from_min_max(egui::pos2(r.right() - 24.0, r.top()), r.right_bottom());
        let options = ui.interact(options_rect, resp.id.with("options"), egui::Sense::click());
        if selected || resp.hovered() || options.hovered() { paint_icon(ui.painter(), options_rect.shrink2(vec2(5.0, 8.0)), Icon::More, pal.text_dim); }
        egui::Popup::menu(&options).show(|ui| self.object_menu(ui, obj, dialect, sh, actions));
        egui::Popup::context_menu(&resp).show(|ui| {
            if obj.kind.has_rows() {
                let pinned = self.cfg.pinned.contains(&obj.name);
                if ui.button(if pinned { "Unpin" } else { "Pin to top" }).clicked() {
                    pin = Some(obj.name.clone());
                    ui.close();
                }
                ui.separator();
            }
            self.object_menu(ui, obj, dialect, sh, actions);
        });
        pin
    }

    fn object_menu(&self, ui: &mut egui::Ui, obj: &DbObject, d: Dialect, _sh: &mut Shared, actions: &mut Vec<WsAction>) {
        ui.set_min_width(220.0);
        let t = TableRef { schema: obj.schema.clone(), name: obj.name.clone() };
        let mut push = |ui: &mut egui::Ui, label: &str, a: WsAction| {
            if ui.button(label).clicked() {
                actions.push(a);
                ui.close();
            }
        };
        if obj.kind.has_rows() {
            push(ui, "Open in new tab", WsAction::OpenObject { obj: obj.clone(), structure: false, new_tab: true });
            push(ui, "Open structure", WsAction::OpenObject { obj: obj.clone(), structure: true, new_tab: true });
        } else {
            push(ui, "Show definition", WsAction::OpenObject { obj: obj.clone(), structure: true, new_tab: true });
        }
        push(ui, "Open definition in SQL editor", WsAction::OpenDefinitionSql(obj.clone()));
        if ui.button("Copy name").clicked() {
            ui.ctx().copy_text(obj.name.clone());
            ui.close();
        }
        if obj.kind.has_rows() && !matches!(d, Dialect::Redis | Dialect::Mongo) {
            let q = sqlgen::qualified(d, &t);
            let select = if d == Dialect::SqlServer { format!("SELECT TOP 100 * FROM {q};") } else { format!("SELECT * FROM {q} LIMIT 100;") };
            push(ui, "Query in SQL editor", WsAction::OpenQuery { sql: select, title: Some(obj.name.clone()), run: false });
        }
        if obj.kind.has_rows() {
            push(ui, "Export…", WsAction::ExportTable(t.clone(), BrowseRequest { limit: usize::MAX / 4, ..Default::default() }));
        }
        if obj.kind == ObjectKind::Table && !matches!(d, Dialect::Redis | Dialect::Elastic) {
            ui.separator();
            if d != Dialect::Mongo {
                push(ui, "Rename…", WsAction::ShowModal(Box::new(Modal::Rename { obj: obj.clone(), name: obj.name.clone() })));
                push(ui, "Clone table…", WsAction::ShowModal(Box::new(Modal::Clone { obj: obj.clone(), name: format!("{}_copy", obj.name), with_data: false })));
            }
            push(ui, "Truncate…", WsAction::ShowModal(Box::new(Modal::Truncate { obj: obj.clone(), cascade: false, restart: false })));
        }
        let maintenance = crate::db::objects::maintenance(d, obj);
        if !maintenance.is_empty() {
            ui.menu_button("Maintenance", |ui| {
                for (label, sql) in maintenance {
                    push(ui, label, WsAction::ShowModal(Box::new(Modal::Preview { title: label.into(), stmts: vec![sql.clone()], then: Some(Gated::Script { stmts: vec![sql], label: label.into(), refresh: true }) })));
                }
            });
        }
        if obj.kind == ObjectKind::Table {
            ui.menu_button("Add object", |ui| {
                for kind in crate::db::objects::supported(d).into_iter().filter(|k| matches!(k, ObjectKind::Index | ObjectKind::Constraint | ObjectKind::Trigger)) {
                    let mut draft = crate::db::objects::ObjectDraft::new(kind, obj.schema.clone()); draft.table = obj.name.clone();
                    push(ui, crate::db::objects::label(kind), WsAction::ShowModal(Box::new(Modal::CreateObject(Box::new(draft)))));
                }
            });
        }
        if !matches!(obj.kind, ObjectKind::Keys) && !matches!(d, Dialect::Redis | Dialect::Elastic) {
            push(ui, "Drop…", WsAction::ShowModal(Box::new(Modal::Drop { obj: obj.clone(), cascade: false })));
        }
    }

    fn queries_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        let pal = p();
        let w = ui.available_width();
        search_field(ui, &mut self.queries_search, "Search saved queries", w);
        ui.add_space(6.0);
        let q = self.queries_search.to_lowercase();
        let mut delete = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut any = false;
            for sq in sh.queries.iter().filter(|s| s.connection.map(|c| c == self.cfg.id).unwrap_or(true)).filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q) || s.sql.to_lowercase().contains(&q)) {
                any = true;
                let resp = card_frame().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
                        paint_icon(ui.painter(), r, Icon::Star, if s_conn(sq).is_none() { pal.accent } else { pal.warning });
                        ui.label(RichText::new(&sq.name).strong().size(12.5));
                    });
                    ui.label(RichText::new(sqlgen::ellipsize(&sq.sql, 90)).monospace().size(11.0).color(pal.text_dim));
                });
                let resp = resp.response.interact(egui::Sense::click());
                if resp.clicked() {
                    let mut open_existing = false;
                    for (i, t) in self.tabs.iter().enumerate() {
                        if let TabKind::Query(qt) = &t.kind {
                            if qt.saved_id == Some(sq.id) {
                                open_existing = true;
                                actions.push(WsAction::Toast(ToastKind::Info, format!("“{}” is already open", sq.name)));
                                let _ = i;
                            }
                        }
                    }
                    if !open_existing {
                        actions.push(WsAction::OpenQuery { sql: sq.sql.clone(), title: Some(sq.name.clone()), run: false });
                    }
                }
                resp.context_menu(|ui| {
                    if ui.button("Open & run").clicked() {
                        actions.push(WsAction::OpenQuery { sql: sq.sql.clone(), title: Some(sq.name.clone()), run: true });
                        ui.close();
                    }
                    if ui.button("Copy SQL").clicked() {
                        ui.ctx().copy_text(sq.sql.clone());
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        delete = Some(sq.id);
                        ui.close();
                    }
                });
                ui.add_space(4.0);
            }
            if !any {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(faint("No saved queries"));
                    ui.label(faint("Save one from a query tab with ⌘S").size(11.0));
                });
            }
        });
        if let Some(id) = delete {
            sh.queries.retain(|q| q.id != id);
            let _ = sh.store.save_queries(sh.queries);
        }
        // Remember queries opened from saved ones.
        if let Some(TabKind::Query(qt)) = self.tabs.last_mut().map(|t| &mut t.kind) {
            if qt.saved_id.is_none() {
                if let Some(sq) = sh.queries.iter().find(|s| s.name == qt.title && s.sql == qt.sql) {
                    qt.saved_id = Some(sq.id);
                }
            }
        }
    }

    fn history_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        let pal = p();
        if !self.history_loaded {
            self.history = sh.store.load_history(self.cfg.id, sh.settings.history_days);
            self.history_loaded = true;
        }
        let w = ui.available_width();
        ui.horizontal(|ui| {
            search_field(ui, &mut self.history_search, "Search history", w - 34.0);
            if icon_button(ui, Icon::Trash, "Clear history").clicked() {
                sh.store.clear_history(self.cfg.id);
                self.history.clear();
            }
        });
        ui.add_space(6.0);
        let q = self.history_search.to_lowercase();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut last_day = String::new();
            for e in self.history.iter().rev().filter(|e| q.is_empty() || e.sql.to_lowercase().contains(&q)).take(500) {
                let local = e.at.with_timezone(&chrono::Local);
                let day = local.format("%A, %e %B").to_string();
                if day != last_day {
                    section_label(ui, &day);
                    last_day = day;
                }
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, egui::CornerRadius::same(6), pal.surface_hover);
                }
                let painter = ui.painter_at(r.shrink(4.0));
                painter.text(r.left_top() + vec2(6.0, 5.0), egui::Align2::LEFT_TOP, sqlgen::ellipsize(&e.sql, 70), egui::FontId::monospace(11.0), if e.ok { pal.text } else { pal.danger });
                painter.text(r.left_bottom() + vec2(6.0, -5.0), egui::Align2::LEFT_BOTTOM, format!("{} · {} ms · {}", local.format("%H:%M:%S"), e.elapsed_ms, e.database), egui::FontId::proportional(10.5), pal.text_faint);
                if resp.double_clicked() {
                    actions.push(WsAction::OpenQuery { sql: e.sql.clone(), title: None, run: false });
                }
                let resp = resp.on_hover_text(&e.sql);
                resp.context_menu(|ui| {
                    if ui.button("Open in new tab").clicked() {
                        actions.push(WsAction::OpenQuery { sql: e.sql.clone(), title: None, run: false });
                        ui.close();
                    }
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(e.sql.clone());
                        ui.close();
                    }
                });
            }
        });
    }

    fn tab_bar_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        let pal = p();
        let mut close = None;
        egui::ScrollArea::horizontal().id_salt(("tabs", self.cfg.id)).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (i, tab) in self.tabs.iter().enumerate() {
                    let selected = i == self.active;
                    let title = tab.title();
                    let italic = matches!(&tab.kind, TabKind::Table(t) if t.preview);
                    let pending = match &tab.kind {
                        TabKind::Table(t) => t.pending.count() + t.structure_changes().len(),
                        _ => 0,
                    };
                    let font = if selected { medium(12.5) } else { egui::FontId::proportional(12.5) };
                    let text_w = ui.painter().layout_no_wrap(title.clone(), font.clone(), pal.text).size().x.min(200.0);
                    let badge_w = if pending > 0 { 22.0 } else { 0.0 };
                    let w = 14.0 + 16.0 + 8.0 + text_w + badge_w + 28.0;
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 34.0), egui::Sense::click());
                    let fill = if selected { pal.surface_hover } else if resp.hovered() { pal.glass } else { egui::Color32::TRANSPARENT };
                    ui.painter().rect_filled(r, egui::CornerRadius::same(255), fill);
                    if selected {
                        ui.painter().rect_stroke(r, egui::CornerRadius::same(255), egui::Stroke::new(1.0, mix(pal.border, pal.text, 0.12)), egui::StrokeKind::Inside);
                    }
                    let icon_c = egui::pos2(r.left() + 20.0, r.center().y);
                    if selected && matches!(tab.kind, TabKind::Table(_)) {
                        ui.painter().circle_filled(icon_c, 3.5, pal.accent);
                    } else {
                        paint_icon(ui.painter(), egui::Rect::from_center_size(icon_c, vec2(14.0, 14.0)), tab.icon(), if selected { pal.accent } else { pal.text_faint });
                    }
                    let mut job = egui::text::LayoutJob::simple_singleline(title, font, if selected { pal.text } else { pal.text_dim });
                    if italic {
                        job.sections[0].format.italics = true;
                    }
                    job.wrap.max_width = 200.0;
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    let galley = ui.painter().layout_job(job);
                    let tx = r.left() + 32.0;
                    let gw = galley.size().x;
                    ui.painter().galley(egui::pos2(tx, r.center().y - galley.size().y / 2.0), galley, pal.text);
                    if pending > 0 {
                        ui.painter().text(egui::pos2(tx + gw + 8.0, r.center().y), egui::Align2::LEFT_CENTER, pending.to_string(), egui::FontId::monospace(11.0), pal.accent);
                    }
                    let cr = egui::Rect::from_center_size(egui::pos2(r.right() - 16.0, r.center().y), vec2(18.0, 18.0));
                    let hover_close = ui.rect_contains_pointer(cr);
                    if tab.dirty() && !hover_close && pending == 0 {
                        ui.painter().circle_filled(cr.center(), 3.5, pal.warning);
                    } else if resp.hovered() || selected {
                        if hover_close {
                            ui.painter().rect_filled(cr, egui::CornerRadius::same(255), pal.surface);
                        }
                        paint_icon(ui.painter(), cr.shrink(4.0), Icon::Close, pal.text_dim);
                    }
                    if resp.clicked() {
                        if hover_close {
                            close = Some(i);
                        } else {
                            self.active = i;
                        }
                    }
                    if resp.middle_clicked() {
                        close = Some(i);
                    }
                    resp.context_menu(|ui| {
                        if let TabKind::Table(table) = &tab.kind {
                            self.object_menu(ui, &table.obj, self.cfg.driver.dialect(), sh, actions);
                            ui.separator();
                        }
                        if ui.button("Close tab").clicked() {
                            close = Some(i);
                            ui.close();
                        }
                        if ui.button("Close other tabs").clicked() {
                            close = Some(usize::MAX - i);
                            ui.close();
                        }
                    });
                }
                let (r, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, egui::CornerRadius::same(255), pal.glass);
                }
                paint_icon(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(14.0, 14.0)), Icon::Plus, pal.text_dim);
                if resp.on_hover_text("New query (⌘T)").clicked() {
                    actions.push(WsAction::OpenQuery { sql: String::new(), title: None, run: false });
                }
            });
        });
        // Pin a preview tab once its data is edited.
        if let Some(Tab { kind: TabKind::Table(t), .. }) = self.tabs.get_mut(self.active) {
            if t.has_changes() {
                t.preview = false;
            }
        }
        if let Some(i) = close {
            if i > self.tabs.len() {
                let keep = usize::MAX - i;
                let id = self.tabs[keep].id;
                if !self.defer_transaction_action(TransactionAction::CloseOthers(id)) {
                    self.tabs.retain(|tab| tab.id == id);
                    self.active = 0;
                }
            } else {
                self.close_tab(i);
            }
        }
    }

    fn right_ui(&mut self, ui: &mut egui::Ui, sh: &mut Shared, session: &Arc<Session>, actions: &mut Vec<WsAction>) {
        let ctx = ui.ctx().clone();
        match self.right {
            RightPanel::RowDetail => {
                if let Some(Tab { kind: TabKind::Table(t), .. }) = self.tabs.get_mut(self.active) {
                    let mut cx = TabCtx { ctx: &ctx, session, settings: sh.settings, store: sh.store, actions };
                    t.row_detail_ui(ui, &mut cx);
                } else {
                    empty_state(ui, Icon::Rows, "Nothing to inspect", "Open a table and select a row");
                }
            }
            RightPanel::Assistant => {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    paint_icon(ui.painter(), r, Icon::Sparkle, p().accent);
                    ui.label(title("Assistant", 15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, "Close").clicked() {
                            self.right = RightPanel::None;
                        }
                    });
                });
                ui.add_space(8.0);
                let mut cx = TabCtx { ctx: &ctx, session, settings: sh.settings, store: sh.store, actions };
                let catalog = self.catalog.clone();
                self.assistant.ui(ui, &mut cx, &catalog, sh.store);
            }
            RightPanel::None => {}
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, sh: &mut Shared, actions: &mut Vec<WsAction>) {
        use egui::{Key, Modifiers};
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let typing = ctx.memory(|m| m.focused().is_some());
        if ctx.input_mut(|i| i.consume_key(cmd, Key::T)) {
            actions.push(WsAction::OpenQuery { sql: String::new(), title: None, run: false });
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::E)) {
            actions.push(WsAction::OpenQuery { sql: String::new(), title: None, run: false });
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::W)) && !self.tabs.is_empty() {
            self.close_tab(self.active);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::P)) {
            self.modal = Some(Modal::open_anything());
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::K)) && self.databases.len() > 1 {
            self.modal = Some(Modal::SwitchDatabase { query: String::new(), selected: 0 });
        }
        if ctx.input_mut(|i| i.consume_key(cmd_shift, Key::C)) {
            self.console = !self.console;
        }
        if ctx.input_mut(|i| i.consume_key(cmd_shift, Key::R)) {
            self.reconnect(ctx);
            sh.toasts.info("Reconnecting…");
        }
        if ctx.input_mut(|i| i.consume_key(cmd_shift, Key::OpenBracket)) && self.active > 0 {
            self.active -= 1;
        }
        if ctx.input_mut(|i| i.consume_key(cmd_shift, Key::CloseBracket)) && self.active + 1 < self.tabs.len() {
            self.active += 1;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Period)) {
            self.cancel_active_query();
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        let id = tab.id;
        match &mut tab.kind {
            TabKind::Table(t) => {
                if ctx.input_mut(|i| i.consume_key(cmd, Key::S)) {
                    actions.push(WsAction::RequestCommit(id));
                }
                if ctx.input_mut(|i| i.consume_key(cmd, Key::R)) {
                    actions.push(WsAction::RefreshObjects);
                    if let Some(s) = self.session.clone() {
                        let mut acts = Vec::new();
                        let cx = TabCtx { ctx, session: &s, settings: sh.settings, store: sh.store, actions: &mut acts };
                        t.reload(&cx);
                    }
                }
                if ctx.input_mut(|i| i.consume_key(cmd, Key::F)) {
                    t.toggle_filters();
                }
                if ctx.input_mut(|i| i.consume_key(cmd, Key::Num1)) {
                    t.view = TableView::Data;
                }
                if ctx.input_mut(|i| i.consume_key(cmd, Key::Num2)) {
                    t.view = TableView::Structure;
                }
                let editable = t.editable(self.cfg.read_only);
                if !typing && editable && ctx.input_mut(|i| i.consume_key(cmd, Key::I)) {
                    t.insert_row();
                }
                if !typing && editable && ctx.input_mut(|i| i.consume_key(cmd, Key::D)) {
                    t.duplicate_rows();
                }
            }
            TabKind::Query(_) => {
                if ctx.input_mut(|i| i.consume_key(cmd, Key::S)) {
                    actions.push(WsAction::SaveQuery(id));
                }
            }
            _ => {}
        }
    }
}

fn s_conn(q: &SavedQuery) -> Option<uuid::Uuid> {
    q.connection
}

/// The STRATA_SCRIPT steps, handed to the first workspace only.
fn take_script() -> std::collections::VecDeque<String> {
    static TAKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if TAKEN.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Default::default();
    }
    if !cfg!(debug_assertions) { return Default::default(); }
    std::env::var("STRATA_SCRIPT").map(|s| s.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default()
}

/// Subsequence match: "usr" matches "users".
pub fn fuzzy(hay: &str, needle: &str) -> bool {
    if hay.contains(needle) {
        return true;
    }
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

fn console_ui(ui: &mut egui::Ui, session: &Arc<Session>) {
    let pal = p();
    ui.horizontal(|ui| {
        section_label(ui, "Console");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Clear").clicked() {
                if let Ok(mut l) = session.log.lock() {
                    l.clear();
                }
            }
        });
    });
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        if let Ok(lines) = session.log.lock() {
            for l in lines.iter() {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(l.at.format("%H:%M:%S").to_string()).monospace().size(11.0).color(pal.text_faint));
                    ui.add(egui::Label::new(RichText::new(&l.text).monospace().size(11.5).color(if l.ok { pal.text_dim } else { pal.danger })).wrap());
                });
            }
        }
    });
}

fn admin_ui(a: &mut AdminTab, ui: &mut egui::Ui, cx: &mut TabCtx) {
    let pal = p();
    if a.task.is_none() && (a.data.columns.is_empty() && a.error.is_none() || (a.auto_refresh && a.last.elapsed().as_secs() >= 3)) {
        let kind = a.kind;
        a.last = Instant::now();
        a.task = Some(cx.session.spawn(cx.ctx, move |d| async move {
            let mut d = d.lock().await;
            if kind == AdminKind::Processes { d.processes().await } else { d.users().await }
        }));
    }
    if let Some(r) = take(&mut a.task) {
        match r {
            Ok(res) => {
                a.rows = res.rows.clone();
                a.data = res;
                a.error = None;
            }
            Err(e) => a.error = Some(format!("{e:#}")),
        }
    }
    if a.auto_refresh {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    }
    egui::Frame::new().fill(pal.content).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(if a.kind == AdminKind::Processes { "Process list" } else { "Users & roles" }).strong());
            if icon_button(ui, Icon::Refresh, "Refresh").clicked() {
                a.data.columns.clear();
                a.error = None;
            }
            if a.task.is_some() {
                ui.add(egui::Spinner::new().size(13.0));
            }
            if a.kind == AdminKind::Processes {
                ui.checkbox(&mut a.auto_refresh, "Auto refresh");
                let sel = a.grid.cursor.map(|(r, _)| r);
                if let Some(r) = sel {
                    let pid = a.rows.get(r).and_then(|row| row.first().cloned().flatten());
                    if let Some(pid) = pid {
                        if danger_button(ui, &format!("Kill {pid}")).clicked() {
                            cx.actions.push(WsAction::ShowModal(Box::new(Modal::ConfirmAction { title: format!("Kill process {pid}?"), body: "The session's running statement is cancelled and its connection closed.".into(), action: WsAction::KillProcess(pid), danger: true })));
                        }
                    }
                }
            } else {
                let d = cx.session.dialect();
                if ui.button("+ New user").clicked() {
                    cx.actions.push(WsAction::ShowModal(Box::new(Modal::CreateUser { name: String::new(), password: String::new(), dialect: d })));
                }
                if let Some((r, _)) = a.grid.cursor {
                    if let Some(name) = a.rows.get(r).and_then(|row| row.first().cloned().flatten()) {
                        let host = a.data.col("host").and_then(|c| a.rows.get(r).and_then(|row| row.get(c).cloned().flatten()));
                        if danger_button(ui, &format!("Drop {name}")).clicked() {
                            let stmt = dialogs::drop_user_sql(d, &name, host.as_deref());
                            cx.actions.push(WsAction::ShowModal(Box::new(Modal::ConfirmAction { title: format!("Drop user {name}?"), body: stmt.clone(), action: WsAction::RunScript { stmts: vec![stmt], label: format!("Dropped {name}"), refresh: false }, danger: true })));
                        }
                    }
                }
            }
        });
    });
    if let Some(e) = &a.error {
        ui.label(RichText::new(e).color(pal.danger));
        return;
    }
    let opts = grid::GridOptions { id: egui::Id::unique(("admin", a.id)), editable: false, row_numbers: false, alternating: true, null_text: "NULL", font_size: cx.settings.grid_font_size, row_offset: 0, profiles: false };
    let src = ResultSource { meta: grid::result_meta(&a.data), rows: &a.rows, sort: &[] };
    for ev in grid::show(ui, &src, &mut a.grid, &opts) {
        if let grid::GridEvent::CopyCells = ev {
            if let Some((r, c)) = a.grid.cursor {
                cx.ctx.copy_text(a.rows.get(r).and_then(|row| row.get(c).cloned().flatten()).unwrap_or_default());
            }
        }
    }
}

// The egui context for code paths (gated execution) that run outside a `ui` call.
thread_local! {
    static SH_CTX: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

fn set_sh_ctx(ctx: &egui::Context) {
    SH_CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
}

fn sh_ctx() -> egui::Context {
    SH_CTX.with(|c| c.borrow().clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn navigation_and_closure_preserve_pending_query_work() {
        let ctx = egui::Context::default();
        let mut cfg = ConnectionConfig::new(DriverKind::Sqlite);
        cfg.path = std::env::temp_dir().join(format!("strata-navigation-{}.sqlite", uuid::Uuid::new_v4())).display().to_string();
        let mut workspace = Workspace::open(cfg.clone(), &ctx);
        workspace.connecting = None;
        let session = Arc::new(Session::open(cfg.clone()).await.unwrap());
        workspace.session = Some(session.clone());
        workspace.current_db = "original".into();
        let store = Store::for_test(std::env::temp_dir().join(format!("strata-navigation-store-{}", uuid::Uuid::new_v4())));
        let settings = Settings::default();
        let id = workspace.open_query("SELECT 1".into(), None);
        let mut actions = Vec::new();
        let mut cx = TabCtx { ctx: &ctx, session: &session, settings: &settings, store: &store, actions: &mut actions };
        let TabKind::Query(q) = &mut workspace.tabs[0].kind else { panic!("query tab") };
        q.start("SELECT 1".into(), &cx, cfg, "main".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while q.is_running() && std::time::Instant::now() < deadline { tokio::time::sleep(std::time::Duration::from_millis(5)).await; q.poll(&mut cx); }
        assert!(q.has_pending_transaction());
        assert!(!q.is_running());
        workspace.close_tab(0);
        assert_eq!(workspace.tabs.len(), 1);
        assert!(workspace.transaction_action == Some(TransactionAction::CloseTab(id)));
        let mut queries = Vec::new(); let mut boards = Vec::new(); let mut toasts = Toasts::default(); let mut edit = None;
        let mut switch = None; let mut home = false; let mut settings_open = false; let mut command = None; let mut create = None; let mut app_request = None;
        let mut sh = Shared { store: &store, settings: &settings, queries: &mut queries, boards: &mut boards, toasts: &mut toasts, edit_connection: &mut edit, workspace_choices: &[], switch_workspace: &mut switch, show_home: &mut home, open_settings: &mut settings_open, workspace_command: &mut command, create_connection: &mut create, app_request: &mut app_request };
        let mut initial = ctx.run_ui(egui::RawInput::default(), |_ui| workspace.transaction_dialog(&ctx, &mut sh));
        initial.textures_delta.clear();
        let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let mut output = ctx.run_ui(egui::RawInput { events: vec![escape], ..Default::default() }, |_ui| workspace.transaction_dialog(&ctx, &mut sh));
        output.textures_delta.clear();
        assert!(workspace.transaction_action.is_none(), "Escape cancels the close request");
        assert!(workspace.has_pending_transactions());
        workspace.handle(WsAction::SwitchDatabase("other".into()), &mut sh, &ctx);
        assert_eq!(workspace.current_db, "original");
        assert!(workspace.transaction_action == Some(TransactionAction::Database("other".into())));
        workspace.transaction_action = None;
        workspace.handle(WsAction::SetAutoCommit { tab: id, enabled: true }, &mut sh, &ctx);
        assert!(matches!(&workspace.tabs[0].kind, TabKind::Query(q) if !q.auto_commit));
        assert!(workspace.transaction_action == Some(TransactionAction::AutoCommit(id, true)));
        workspace.transaction_action = None;
        workspace.reconnect(&ctx);
        assert!(workspace.transaction_action == Some(TransactionAction::Reconnect));
        workspace.transaction_action = None;
        workspace.close_tab(0);
        let TabKind::Query(q) = &mut workspace.tabs[0].kind else { panic!("query tab") };
        q.finish_transaction(&ctx, false);
        while q.transaction_busy() && std::time::Instant::now() < deadline { tokio::time::sleep(std::time::Duration::from_millis(5)).await; q.poll_transaction(); }
        assert!(!q.has_pending_transaction());
        workspace.transaction_dialog(&ctx, &mut sh);
        assert!(workspace.tabs.is_empty(), "close continues only after successful resolution");
        let id = workspace.open_query("SELECT 1".into(), None);
        workspace.context_ready = false;
        workspace.handle(WsAction::RunQuery { tab_id: id, sql: "SELECT 1".into() }, &mut sh, &ctx);
        workspace.run_gated(Gated::Query { tab: id, sql: "SELECT 1".into() }, &mut sh);
        assert!(matches!(&workspace.tabs[0].kind, TabKind::Query(q) if !q.is_running() && !q.has_pending_transaction()), "neither Run nor a delayed approval may execute during a database transition");
    }

    #[test]
    fn selected_table_has_context_menu_and_options_button() {
        let ctx = egui::Context::default();
        let mut cfg = ConnectionConfig::new(DriverKind::Sqlite); cfg.path = ":memory:".into();
        let mut workspace = Workspace::open(cfg, &ctx);
        workspace.connecting = None;
        let store = Store::for_test(std::env::temp_dir().join("strata-menu-test"));
        let settings = Settings::default();
        let object = DbObject { schema: "main".into(), name: "orders".into(), kind: ObjectKind::Table, parent: None, arguments: None };
        let frame = |events, time| {
            let mut queries = Vec::new(); let mut boards = Vec::new(); let mut toasts = Toasts::default(); let mut edit = None;
            let mut switch = None; let mut home = false; let mut settings_open = false; let mut command = None; let mut create = None;
            let mut shared = Shared { store: &store, settings: &settings, queries: &mut queries, boards: &mut boards, toasts: &mut toasts, edit_connection: &mut edit, workspace_choices: &[], switch_workspace: &mut switch, show_home: &mut home, open_settings: &mut settings_open, workspace_command: &mut command, create_connection: &mut create, app_request: &mut None };
            let mut actions = Vec::new(); let mut row_rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0,400.0))), events, time: Some(time), ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let id = ui.scope_id().with(("object-row", &object.schema, &object.name, &object.parent, &object.arguments, false));
                    workspace.object_row(ui, &object, true, false, 0, Dialect::Sqlite, &mut shared, &mut actions);
                    row_rect = ctx.read_response(id).expect("object row rendered").rect;
                });
            });
            fn text(shape: &egui::epaint::Shape, all: &mut Vec<String>) {
                match shape { egui::epaint::Shape::Text(shape) => all.push(shape.galley.text().to_string()), egui::epaint::Shape::Vec(shapes) => for shape in shapes { text(shape,all); }, _ => {} }
            }
            let mut labels = Vec::new(); for shape in &output.shapes { text(&shape.shape, &mut labels); }
            output.textures_delta.clear();
            (row_rect, labels)
        };
        frame(Vec::new(), 0.0);
        let (rect, _) = frame(Vec::new(), 0.01);
        let point = egui::pos2(rect.left()+60.0, rect.center().y);
        let button = |pos, button, pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE };
        frame(vec![egui::Event::PointerMoved(point), button(point, egui::PointerButton::Secondary, true)], 0.1);
        frame(vec![button(point, egui::PointerButton::Secondary, false)], 0.12);
        let (_, labels) = frame(Vec::new(), 0.14);
        assert!(labels.iter().any(|label| label == "Open structure"), "right-click menu: {labels:?}");
        assert!(labels.iter().any(|label| label == "Drop…"), "right-click must expose object operations");
        frame(vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }], 0.2);
        let point = egui::pos2(rect.right()-12.0, rect.center().y);
        frame(vec![egui::Event::PointerMoved(point), button(point, egui::PointerButton::Primary, true)], 0.3);
        frame(vec![button(point, egui::PointerButton::Primary, false)], 0.32);
        let (_, labels) = frame(Vec::new(), 0.34);
        assert!(labels.iter().any(|label| label == "Open structure"), "options-button menu: {labels:?}");
    }

    #[test]
    fn fuzzy_matches_subsequences() {
        assert!(fuzzy("user_accounts", "usac"));
        assert!(fuzzy("orders", "ord"));
        assert!(!fuzzy("orders", "xyz"));
    }
}
