//! The native workbench: custom-painted chrome over the shared DBM engines.

pub mod dialog;
pub mod views;
pub mod widgets;

use std::collections::{HashMap, HashSet};

use dbm_engine::models::{
    ConnectionProfile, DatabaseEngine, DatabaseRef, FilterCondition, MutationBatch, OrderSpec,
    QueryHistoryEntry, QueryResponse, RowMutation, SaveProfileInput, SchemaNode, TablePage,
};
use dbm_workbench::format;
use dbm_workbench::pending_edits::{row_key, PendingEdits};
use uuid::Uuid;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::bridge::{self, EngineEvent};
use crate::render::{contains, rect, Renderer};
use crate::theme;
use widgets::TextField;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FieldId {
    Url,
    Name,
    Host,
    Port,
    Username,
    Database,
    Password,
    CaPath,
    Query(u64),
    TableCell(u64, usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ViewId {
    Sidebar,
    Schema(Uuid),
    ResultsX(u64),
    Editor(u64),
    Results(u64),
    History(u64),
    Table(u64),
    TableX(u64),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    NewConnection,
    SelectProfile(Uuid),
    ToggleProfile(Uuid),
    EditProfile(Uuid),
    Disconnect(Uuid),
    RefreshSchema(Uuid),
    OpenTable {
        profile: Uuid,
        schema: String,
        table: String,
    },
    ToggleNode(String),
    SelectTab(u64),
    CloseTab(u64),
    NewQuery,
    RunQuery(u64),
    RefreshQuery(u64),
    CopyTableCsv(u64),
    ExportTableCsv(u64),
    PagePrev(u64),
    PageNext(u64),
    SortColumn {
        tab: u64,
        column: String,
    },
    RefreshTable(u64),
    TableCell {
        tab: u64,
        row: usize,
        column: usize,
    },
    StageDelete(u64),
    SaveTable(u64),
    DiscardTable(u64),
    DiscardTableRow {
        tab: u64,
        row: usize,
    },
    ToggleMenu(Uuid),
    ToggleDatabases(Uuid),
    SelectDatabase(Uuid, String),
    ModalColor(String),
    ModalReadOnly,
    HistoryItem {
        tab: u64,
        index: usize,
    },
    ClickField {
        field: FieldId,
    },
    ModalEngine(DatabaseEngine),
    ModalTls(usize),
    ModalImportUrl,
    ModalTest,
    ModalSave,
    ModalDelete,
    ModalCancel,
    ConfirmAccept,
    ConfirmCancel,
    DismissError,
}

#[derive(Clone)]
pub struct Workspace {
    pub profile: ConnectionProfile,
    pub databases: Vec<DatabaseRef>,
}

impl Workspace {
    pub fn database_names(&self) -> Vec<String> {
        self.databases
            .iter()
            .map(|database| database.name.clone())
            .collect()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TabKind {
    Table { schema: String, table: String },
    Query { database: String },
}

#[derive(Clone)]
pub struct Tab {
    pub id: u64,
    pub title: String,
    pub kind: TabKind,
    pub profile_id: Uuid,
}

pub struct TableState {
    pub profile_id: Uuid,
    pub schema: String,
    pub table: String,
    pub page: Option<TablePage>,
    pub page_index: u32,
    pub limit: u32,
    pub order_by: Option<OrderSpec>,
    pub filters: Vec<FilterCondition>,
    pub columns: Vec<(String, String)>,
    pub loading: bool,
    pub saving: bool,
    pub saving_mutations: Option<Vec<RowMutation>>,
    pub status: String,
    pub pending: PendingEdits,
    pub selected_rows: HashSet<String>,
    pub selection_anchor: Option<usize>,
    pub preview_row: Option<String>,
}

pub struct QueryState {
    pub profile_id: Uuid,
    pub database: String,
    pub engine: DatabaseEngine,
    pub executed_sql: Option<String>,
    pub response: Option<QueryResponse>,
    pub history: Vec<QueryHistoryEntry>,
    pub running: bool,
    pub error: Option<String>,
    pub meta: String,
}

pub struct ProfileForm {
    pub input: SaveProfileInput,
    pub existing: Option<ConnectionProfile>,
    pub feedback: Option<(bool, String)>,
    pub busy: bool,
    pub tls_index: usize,
    pub read_only: bool,
    pub color: String,
}

pub enum ConfirmAction {
    DeleteProfile(Box<ConnectionProfile>),
    RunQuery { tab: u64, sql: String },
}

pub struct ConfirmState {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    pub danger: bool,
    pub action: ConfirmAction,
}

pub enum Modal {
    Profile(Box<ProfileForm>),
    Confirm(Box<ConfirmState>),
}

struct Region {
    rect: D2D_RECT_F,
    action: Action,
}

pub struct Layout {
    pub sidebar: D2D_RECT_F,
    pub topbar: D2D_RECT_F,
    pub tabs: D2D_RECT_F,
    pub content: D2D_RECT_F,
    pub width: f32,
    pub height: f32,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        let sidebar_width = theme::SIDEBAR_WIDTH.min(width * 0.45);
        Self {
            sidebar: rect(0.0, 0.0, sidebar_width, height),
            topbar: rect(sidebar_width, 0.0, width, theme::TOPBAR_HEIGHT),
            tabs: rect(
                sidebar_width,
                theme::TOPBAR_HEIGHT,
                width,
                theme::TOPBAR_HEIGHT + theme::TAB_STRIP_HEIGHT,
            ),
            content: rect(
                sidebar_width,
                theme::TOPBAR_HEIGHT + theme::TAB_STRIP_HEIGHT,
                width,
                height,
            ),
            width,
            height,
        }
    }
}

pub struct Ui {
    pub profiles: Vec<ConnectionProfile>,
    pub workspaces: HashMap<Uuid, Workspace>,
    pub schemas: HashMap<Uuid, Vec<SchemaNode>>,
    pub active_profile: Option<Uuid>,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<u64>,
    pub tables: HashMap<u64, TableState>,
    pub queries: HashMap<u64, QueryState>,
    pub expanded: HashSet<Uuid>,
    pub collapsed_nodes: HashSet<String>,
    pub fields: HashMap<FieldId, TextField>,
    pub scroll: HashMap<ViewId, f32>,
    pub modal: Option<Modal>,
    pub open_menu: Option<Uuid>,
    pub open_databases: Option<Uuid>,
    pub toast: Option<(String, f32)>,
    pub error: Option<(String, f32)>,
    regions: Vec<Region>,
    scroll_regions: Vec<(ViewId, D2D_RECT_F)>,
    field_rects: HashMap<FieldId, D2D_RECT_F>,
    hover: Option<Action>,
    pressed: Option<Action>,
    focus: Option<FieldId>,
    next_tab_id: u64,
    input_control: bool,
    input_shift: bool,
    pub time: f32,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        let mut ui = Self {
            profiles: Vec::new(),
            workspaces: HashMap::new(),
            schemas: HashMap::new(),
            active_profile: None,
            tabs: Vec::new(),
            active_tab: None,
            tables: HashMap::new(),
            queries: HashMap::new(),
            expanded: HashSet::new(),
            collapsed_nodes: HashSet::new(),
            fields: HashMap::new(),
            scroll: HashMap::new(),
            modal: None,
            open_menu: None,
            open_databases: None,
            toast: None,
            error: None,
            regions: Vec::new(),
            scroll_regions: Vec::new(),
            field_rects: HashMap::new(),
            hover: None,
            pressed: None,
            focus: None,
            next_tab_id: 1,
            input_control: false,
            input_shift: false,
            time: 0.0,
        };
        ui.load_profiles();
        ui
    }

    // ------------------------------------------------------------------
    // State helpers
    // ------------------------------------------------------------------

    pub fn profile(&self, profile_id: Uuid) -> Option<ConnectionProfile> {
        self.workspaces
            .get(&profile_id)
            .map(|workspace| workspace.profile.clone())
            .or_else(|| {
                self.profiles
                    .iter()
                    .find(|profile| profile.id == profile_id)
                    .cloned()
            })
    }

    pub fn profile_engine(&self, profile_id: Uuid) -> DatabaseEngine {
        self.profile(profile_id)
            .map_or(DatabaseEngine::Postgres, |profile| profile.engine)
    }

    pub fn workspace_database(&self, profile_id: Uuid) -> String {
        self.workspaces
            .get(&profile_id)
            .map(|workspace| workspace.profile.default_database.clone())
            .unwrap_or_else(|| {
                format::fallback_database(self.profile_engine(profile_id)).to_owned()
            })
    }

    pub fn active_table(&self) -> Option<(Uuid, String, String)> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| Some(tab.id) == self.active_tab)?;
        match &tab.kind {
            TabKind::Table { schema, table } => {
                Some((tab.profile_id, schema.clone(), table.clone()))
            }
            TabKind::Query { .. } => None,
        }
    }

    // ------------------------------------------------------------------
    // Engine work
    // ------------------------------------------------------------------

    pub fn load_profiles(&mut self) {
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let summaries = engine.profile_summaries()?;
                Ok(summaries
                    .into_iter()
                    .map(|summary| summary.profile)
                    .collect::<Vec<_>>())
            },
            |result| match result {
                Ok(profiles) => EngineEvent::Profiles(profiles),
                Err(error) => EngineEvent::Error(bridge::error_text(&error)),
            },
        );
    }

    pub fn connect_profile(&mut self, profile_id: Uuid) {
        let Some(profile) = self.profile(profile_id) else {
            return;
        };
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let session = engine.connect(profile.clone()).await?;
                let databases = session.list_databases().await?;
                let schema = session.schema_tree().await?;
                Ok((profile, databases, schema))
            },
            |result| match result {
                Ok((profile, databases, schema)) => EngineEvent::Connected {
                    profile,
                    databases,
                    schema,
                },
                Err(error) => EngineEvent::Error(bridge::error_text(&error)),
            },
        );
    }

    pub fn disconnect_profile(&mut self, profile_id: Uuid) {
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                engine.disconnect(profile_id).await;
                Ok(profile_id)
            },
            |_| EngineEvent::Toast("Disconnected.".to_owned()),
        );
        self.workspaces.remove(&profile_id);
        self.schemas.remove(&profile_id);
        self.close_tabs_for_profile(profile_id);
        if self.active_profile == Some(profile_id) {
            self.active_profile = None;
        }
    }

    pub fn switch_database(&mut self, profile_id: Uuid, database: String) {
        let Some(mut profile) = self.profile(profile_id) else {
            return;
        };
        profile.default_database = database;
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let session = engine.connect(profile.clone()).await?;
                let databases = session.list_databases().await?;
                let schema = session.schema_tree().await?;
                Ok((profile, databases, schema))
            },
            |result| match result {
                Ok((profile, databases, schema)) => EngineEvent::Connected {
                    profile,
                    databases,
                    schema,
                },
                Err(error) => EngineEvent::Error(bridge::error_text(&error)),
            },
        );
    }

    pub fn refresh_schema(&mut self, profile_id: Uuid) {
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let session = engine.session(profile_id).await?;
                session.schema_tree().await
            },
            move |result| match result {
                Ok(schema) => EngineEvent::Schema { profile_id, schema },
                Err(error) => EngineEvent::Error(bridge::error_text(&error)),
            },
        );
    }

    fn load_table_page(&mut self, tab: u64) {
        let Some(state) = self.tables.get_mut(&tab) else {
            return;
        };
        if state.loading || state.saving {
            return;
        }
        state.loading = true;
        state.status = "Loading…".to_owned();
        let request = dbm_engine::models::TablePageRequest {
            profile_id: state.profile_id,
            schema: state.schema.clone(),
            table: state.table.clone(),
            offset: state.page_index * state.limit,
            limit: state.limit,
            filters: state.filters.clone(),
            order_by: state.order_by.clone(),
            include_total: Some(true),
        };
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let session = engine.session(request.profile_id).await?;
                session.table_page(&request).await
            },
            move |result| EngineEvent::TablePage {
                tab,
                result: result.map_err(|error| bridge::error_text(&error)),
            },
        );
    }

    fn save_table(&mut self, tab: u64) {
        let Some(state) = self.tables.get_mut(&tab) else {
            return;
        };
        if state.pending.is_empty() || state.loading || state.saving {
            return;
        }
        state.saving = true;
        let mutations = state.pending.mutations();
        state.saving_mutations = Some(mutations.clone());
        let batch = MutationBatch {
            profile_id: state.profile_id,
            schema: state.schema.clone(),
            table: state.table.clone(),
            mutations,
        };
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let session = engine.session(batch.profile_id).await?;
                session.apply_mutations(&batch).await
            },
            move |result| EngineEvent::TableMutations {
                tab,
                result: result.map_err(|error| bridge::error_text(&error)),
            },
        );
    }

    /// The table a full-table select names, when this profile's schema tree
    /// has exactly one match. Redis commands never resolve to a table.
    fn resolve_table_select(&self, profile_id: Uuid, sql: &str) -> Option<(String, String)> {
        if self.profile_engine(profile_id) == DatabaseEngine::Redis {
            return None;
        }
        let tree = self.schemas.get(&profile_id).cloned().unwrap_or_default();
        dbm_workbench::table_select::resolve_full_table_select(sql, &tree)
    }

    fn run_query(&mut self, tab: u64, sql: String) {
        let Some(state) = self.queries.get_mut(&tab) else {
            return;
        };
        state.running = true;
        state.error = None;
        state.meta = "Running…".to_owned();
        let profile_id = state.profile_id;
        let database = state.database.clone();
        let engine = bridge::engine().clone();
        let sql_for_callback = sql.clone();
        bridge::spawn(
            async move {
                let session = engine.session(profile_id).await?;
                let response = session.run_query(&sql, Some(format::QUERY_ROW_LIMIT)).await;
                let duration_ms = response.as_ref().map_or(0, |response| response.duration_ms);
                let entry = QueryHistoryEntry {
                    id: Uuid::new_v4(),
                    profile_id,
                    database,
                    sql: sql.clone(),
                    executed_at: chrono::Utc::now(),
                    duration_ms,
                    success: response.is_ok(),
                };
                engine.store.add_history(&entry)?;
                response
            },
            move |result| EngineEvent::Query {
                tab,
                sql: sql_for_callback,
                result: result.map_err(|error| bridge::error_text(&error)),
            },
        );
    }

    fn load_history(&mut self, tab: u64) {
        let Some(state) = self.queries.get(&tab) else {
            return;
        };
        let (profile_id, database) = (state.profile_id, state.database.clone());
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                Ok(engine
                    .store
                    .list_history(profile_id, &database, 100)?
                    .into_iter()
                    .collect::<Vec<_>>())
            },
            move |result| EngineEvent::History {
                tab,
                entries: result.unwrap_or_default(),
            },
        );
    }

    fn export_csv(&mut self, tab: u64) {
        let Some(state) = self.tables.get(&tab) else {
            return;
        };
        let (profile_id, schema, table, filters, order_by) = (
            state.profile_id,
            state.schema.clone(),
            state.table.clone(),
            state.filters.clone(),
            state.order_by.clone(),
        );
        let Some(path) = crate::platform::save_csv_dialog(&format!(
            "{}.csv",
            format::safe_file_name(&format!("{schema}.{table}"))
        )) else {
            return;
        };
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let session = engine.session(profile_id).await?;
                let mut offset = 0_u32;
                let mut document = String::new();
                let mut rows_written = 0_u64;
                loop {
                    let page = session
                        .table_page(&dbm_engine::models::TablePageRequest {
                            profile_id,
                            schema: schema.clone(),
                            table: table.clone(),
                            offset,
                            limit: format::EXPORT_PAGE_SIZE,
                            filters: filters.clone(),
                            order_by: order_by.clone(),
                            include_total: Some(false),
                        })
                        .await?;
                    if offset == 0 {
                        document.push_str(&format::csv_line(
                            &page
                                .metadata
                                .columns
                                .iter()
                                .map(|column| serde_json::Value::String(column.name.clone()))
                                .collect::<Vec<_>>(),
                        ));
                    }
                    for row in &page.rows {
                        document.push('\n');
                        document.push_str(&format::csv_line(
                            &row[..row.len().min(page.metadata.columns.len())],
                        ));
                    }
                    rows_written += page.rows.len() as u64;
                    if !page.has_more {
                        break;
                    }
                    offset += format::EXPORT_PAGE_SIZE;
                }
                std::fs::write(&path, document)
                    .map_err(|error| dbm_engine::error::AppError::Storage(error.to_string()))?;
                Ok(rows_written)
            },
            |result| match result {
                Ok(rows) => EngineEvent::Exported { rows },
                Err(error) => EngineEvent::Error(bridge::error_text(&error)),
            },
        );
    }

    // ------------------------------------------------------------------
    // Events from the engine
    // ------------------------------------------------------------------

    pub fn handle_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Profiles(profiles) => self.profiles = profiles,
            EngineEvent::Connected {
                profile,
                databases,
                schema,
            } => {
                let profile_id = profile.id;
                self.workspaces.insert(
                    profile_id,
                    Workspace {
                        profile: profile.clone(),
                        databases,
                    },
                );
                self.schemas.insert(profile_id, schema);
                self.expanded.insert(profile_id);
                self.active_profile = Some(profile_id);
                self.open_query(profile_id);
            }
            EngineEvent::Schema { profile_id, schema } => {
                let (_, message) = format::describe_schema_refresh(
                    &self.schemas.get(&profile_id).cloned().unwrap_or_default(),
                    &schema,
                    if self.profile_engine(profile_id) == DatabaseEngine::Redis {
                        "Keyspace"
                    } else {
                        "Schema"
                    },
                );
                self.schemas.insert(profile_id, schema);
                self.show_toast(&message);
            }
            EngineEvent::TablePage { tab, result } => {
                if let Some(state) = self.tables.get_mut(&tab) {
                    state.loading = false;
                    match result {
                        Ok(page) => {
                            state.columns = page
                                .metadata
                                .columns
                                .iter()
                                .map(|column| (column.name.clone(), column.data_type.clone()))
                                .collect();
                            let total = page.total_rows.map_or_else(
                                || format!("{} rows on this page", page.rows.len()),
                                |total| format!("{total} rows"),
                            );
                            let first = if page.rows.is_empty() {
                                0
                            } else {
                                page.offset + 1
                            };
                            state.status = format!(
                                "{total} · showing rows {first}–{}",
                                page.offset + page.rows.len() as u32
                            );
                            state.page = Some(page);
                            state.selected_rows.clear();
                            state.selection_anchor = None;
                            state.preview_row = None;
                        }
                        Err(error) => {
                            state.status = "Load failed.".to_owned();
                            self.error = Some((error, self.time));
                        }
                    }
                }
            }
            EngineEvent::TableMutations { tab, result } => {
                let mut reload = false;
                if let Some(state) = self.tables.get_mut(&tab) {
                    state.saving = false;
                    let submitted = state.saving_mutations.take();
                    match result {
                        Ok(result) => {
                            let drafts_unchanged = submitted.as_ref().is_some_and(|submitted| {
                                mutations_equal(submitted, &state.pending.mutations())
                            });
                            if drafts_unchanged {
                                state.pending.clear();
                                state.selected_rows.clear();
                                state.selection_anchor = None;
                                state.preview_row = None;
                            }
                            reload = true;
                            if result.conflicts.is_empty() {
                                self.show_toast(&format!(
                                    "{} {} saved.",
                                    result.applied,
                                    if result.applied == 1 {
                                        "change"
                                    } else {
                                        "changes"
                                    }
                                ));
                            } else {
                                self.error = Some((
                                    format!(
                                        "{} row conflict(s); the table was refreshed.",
                                        result.conflicts.len()
                                    ),
                                    self.time,
                                ));
                            }
                        }
                        Err(error) => self.error = Some((error, self.time)),
                    }
                }
                if reload {
                    self.load_table_page(tab);
                }
            }
            EngineEvent::Query { tab, sql, result } => {
                let mut succeeded = false;
                let mut profile_id = None;
                if let Some(state) = self.queries.get_mut(&tab) {
                    state.running = false;
                    profile_id = Some(state.profile_id);
                    match result {
                        Ok(response) => {
                            succeeded = true;
                            state.executed_sql = Some(sql.clone());
                            state.meta = query_meta(&response);
                            state.response = Some(response);
                        }
                        Err(error) => {
                            state.meta = "Statement failed.".to_owned();
                            state.error = Some(error);
                            state.response = None;
                        }
                    }
                }
                // `SELECT * FROM table` opens the full table view, matching
                // the Tauri workbench.
                if succeeded {
                    if let Some(profile_id) = profile_id {
                        if let Some((schema, table)) = self.resolve_table_select(profile_id, &sql) {
                            self.open_table(profile_id, schema, table);
                        }
                    }
                }
                self.load_history(tab);
            }
            EngineEvent::History { tab, entries } => {
                if let Some(state) = self.queries.get_mut(&tab) {
                    state.history = entries;
                }
            }
            EngineEvent::Exported { rows } => {
                self.show_toast(&format!("Exported {rows} rows."));
            }
            EngineEvent::Error(message) => self.error = Some((message, self.time)),
            EngineEvent::Toast(message) => self.show_toast(&message),
        }
    }

    pub fn show_toast(&mut self, message: &str) {
        self.toast = Some((message.to_owned(), self.time));
    }

    // ------------------------------------------------------------------
    // Tabs
    // ------------------------------------------------------------------

    pub fn open_table(&mut self, profile_id: Uuid, schema: String, table: String) {
        if let Some(tab) = self.tabs.iter().find(|tab| {
            tab.profile_id == profile_id
                && tab.kind
                    == (TabKind::Table {
                        schema: schema.clone(),
                        table: table.clone(),
                    })
        }) {
            let tab_id = tab.id;
            self.active_tab = Some(tab_id);
            return;
        }
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tables.insert(
            tab_id,
            TableState {
                profile_id,
                schema: schema.clone(),
                table: table.clone(),
                page: None,
                page_index: 0,
                limit: format::MAX_PREVIEW_ROWS,
                order_by: None,
                filters: Vec::new(),
                columns: Vec::new(),
                loading: false,
                saving: false,
                saving_mutations: None,
                status: "Loading…".to_owned(),
                pending: PendingEdits::default(),
                selected_rows: HashSet::new(),
                selection_anchor: None,
                preview_row: None,
            },
        );
        self.tabs.push(Tab {
            id: tab_id,
            title: format!("{schema}.{table}"),
            kind: TabKind::Table { schema, table },
            profile_id,
        });
        self.active_tab = Some(tab_id);
        self.active_profile = Some(profile_id);
        self.load_table_page(tab_id);
    }

    pub fn open_query(&mut self, profile_id: Uuid) {
        let database = self.workspace_database(profile_id);
        let engine = self.profile_engine(profile_id);
        let title = self.next_query_title(profile_id);
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let editor = TextField::with_text(format::default_query_text(engine), true);
        self.fields.insert(FieldId::Query(tab_id), editor);
        self.queries.insert(
            tab_id,
            QueryState {
                profile_id,
                database: database.clone(),
                engine,
                executed_sql: None,
                response: None,
                history: Vec::new(),
                running: false,
                error: None,
                meta: "Results will appear here.".to_owned(),
            },
        );
        self.tabs.push(Tab {
            id: tab_id,
            title,
            kind: TabKind::Query { database },
            profile_id,
        });
        self.active_tab = Some(tab_id);
        self.active_profile = Some(profile_id);
        self.focus = Some(FieldId::Query(tab_id));
        self.load_history(tab_id);
    }

    fn next_query_title(&self, profile_id: Uuid) -> String {
        let existing: HashSet<&str> = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == profile_id && matches!(tab.kind, TabKind::Query { .. }))
            .map(|tab| tab.title.as_str())
            .collect();
        let mut number = 1;
        loop {
            let candidate = format!("Query {number}");
            if !existing.contains(candidate.as_str()) {
                return candidate;
            }
            number += 1;
        }
    }

    pub fn close_tab(&mut self, tab_id: u64) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        self.tabs.remove(index);
        self.tables.remove(&tab_id);
        self.queries.remove(&tab_id);
        self.fields.remove(&FieldId::Query(tab_id));
        if self.active_tab == Some(tab_id) {
            let next = self
                .tabs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| self.tabs.get(previous))
                })
                .map(|tab| tab.id);
            self.active_tab = next;
            if let Some(next) = next {
                self.active_profile = self
                    .tabs
                    .iter()
                    .find(|tab| tab.id == next)
                    .map(|tab| tab.profile_id);
            }
        }
    }

    pub fn close_tabs_for_profile(&mut self, profile_id: Uuid) {
        let ids: Vec<u64> = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == profile_id)
            .map(|tab| tab.id)
            .collect();
        for id in ids {
            self.close_tab(id);
        }
    }

    // ------------------------------------------------------------------
    // Painting
    // ------------------------------------------------------------------

    pub fn paint(&mut self, r: &mut Renderer) {
        self.regions.clear();
        self.scroll_regions.clear();
        self.field_rects.clear();
        let layout = Layout::new(r.width, r.height);
        let modal_open = self.modal.is_some();

        if !modal_open {
            self.paint_sidebar(r, &layout);
            self.paint_topbar(r, &layout);
            self.paint_tab_strip(r, &layout);
            self.paint_content(r, &layout);
        } else {
            self.paint_sidebar(r, &layout);
            self.paint_topbar(r, &layout);
            self.paint_tab_strip(r, &layout);
            self.paint_content(r, &layout);
            self.paint_modal(r, &layout);
        }

        self.paint_overlays(r, &layout);
    }

    pub fn region(&mut self, rect: D2D_RECT_F, action: Action) {
        self.regions.push(Region { rect, action });
    }

    pub fn scroll_region(&mut self, view: ViewId, rect: D2D_RECT_F) {
        self.scroll_regions.push((view, rect));
    }

    pub fn field_rect(&mut self, field: FieldId, rect: D2D_RECT_F) {
        self.field_rects.insert(field, rect);
    }

    pub fn remove_field_hit_region(&mut self, field: FieldId) {
        if self
            .regions
            .last()
            .is_some_and(|region| region.action == Action::ClickField { field })
        {
            self.regions.pop();
        }
        self.field_rects.remove(&field);
    }

    pub fn hovered(&self, action: &Action) -> bool {
        self.hover.as_ref() == Some(action)
    }

    pub fn pressed(&self, action: &Action) -> bool {
        self.pressed.as_ref() == Some(action)
    }

    pub fn focused(&self, field: FieldId) -> bool {
        self.focus == Some(field)
    }

    pub fn scroll_offset(&self, view: ViewId) -> f32 {
        self.scroll.get(&view).copied().unwrap_or(0.0)
    }

    pub fn set_scroll(&mut self, view: ViewId, offset: f32) {
        self.scroll.insert(view, offset.max(0.0));
    }

    fn table_editable(&self, tab: u64) -> bool {
        self.tables.get(&tab).is_some_and(|state| {
            !state.loading
                && !state.saving
                && !self
                    .profile(state.profile_id)
                    .is_some_and(|profile| profile.read_only)
                && state
                    .page
                    .as_ref()
                    .is_some_and(|page| !page.metadata.primary_key.is_empty())
        })
    }

    fn start_table_editor(&mut self, tab: u64, row_index: usize, column: usize) {
        if !self.table_editable(tab) {
            return;
        }
        let Some(state) = self.tables.get(&tab) else {
            return;
        };
        let Some(page) = &state.page else {
            return;
        };
        let Some(row) = page.rows.get(row_index) else {
            return;
        };
        let Some(column_meta) = page.metadata.columns.get(column) else {
            return;
        };
        if page.metadata.primary_key.contains(&column_meta.name)
            || state
                .pending
                .get(&page.metadata, row)
                .is_some_and(|pending| pending.deleted)
        {
            return;
        }
        let value = state.pending.values(&page.metadata, row);
        let text = value.get(column).map_or_else(String::new, |value| {
            if value.is_null() {
                String::new()
            } else {
                format::display_value(value)
            }
        });
        let field = FieldId::TableCell(tab, row_index, column);
        self.fields.insert(field, TextField::with_text(text, false));
        self.focus = Some(field);
    }

    fn finish_table_editor(&mut self, commit: bool) {
        let Some(FieldId::TableCell(tab, row_index, column)) = self.focus else {
            return;
        };
        let field = FieldId::TableCell(tab, row_index, column);
        let text = self.fields.remove(&field).map(|field| field.text);
        self.focus = None;
        if !commit {
            return;
        }
        let Some(text) = text else {
            return;
        };
        let result = self.tables.get_mut(&tab).and_then(|state| {
            let page = state.page.as_ref()?;
            let row = page.rows.get(row_index)?;
            let key = row_key(&page.metadata, row);
            let result = state.pending.set_cell(&page.metadata, row, column, &text);
            if result.is_ok() {
                state.preview_row = key;
            }
            Some(result)
        });
        if let Some(Err(error)) = result {
            self.error = Some((error, self.time));
        }
    }

    fn select_table_row(&mut self, tab: u64, row_index: usize) {
        let Some(state) = self.tables.get_mut(&tab) else {
            return;
        };
        if state.loading || state.saving {
            return;
        }
        let Some(page) = &state.page else {
            return;
        };
        let Some(row) = page.rows.get(row_index) else {
            return;
        };
        let key = table_row_key(&page.metadata, row, page.offset as usize + row_index);
        if self.input_shift {
            let anchor = state.selection_anchor.unwrap_or(row_index);
            let (start, end) = if anchor <= row_index {
                (anchor, row_index)
            } else {
                (row_index, anchor)
            };
            if !self.input_control {
                state.selected_rows.clear();
            }
            for index in start..=end {
                if let Some(row) = page.rows.get(index) {
                    state.selected_rows.insert(table_row_key(
                        &page.metadata,
                        row,
                        page.offset as usize + index,
                    ));
                }
            }
        } else if self.input_control {
            if !state.selected_rows.remove(&key) {
                state.selected_rows.insert(key.clone());
            }
            state.selection_anchor = Some(row_index);
        } else {
            state.selected_rows.clear();
            state.selected_rows.insert(key.clone());
            state.selection_anchor = Some(row_index);
        }
        if state.pending.get(&page.metadata, row).is_some() {
            state.preview_row = Some(key);
        }
    }

    fn stage_selected_rows(&mut self, tab: u64) {
        if !self.table_editable(tab) {
            return;
        }
        let result = self.tables.get_mut(&tab).and_then(|state| {
            let page = state.page.as_ref()?;
            let rows = page
                .rows
                .iter()
                .enumerate()
                .filter(|(index, row)| {
                    state.selected_rows.contains(&table_row_key(
                        &page.metadata,
                        row,
                        page.offset as usize + index,
                    ))
                })
                .map(|(_, row)| row.clone())
                .collect::<Vec<_>>();
            if rows.is_empty() {
                return None;
            }
            let result = state.pending.toggle_delete(&page.metadata, &rows);
            if result.is_ok() {
                state.preview_row = rows.first().and_then(|row| row_key(&page.metadata, row));
            }
            Some(result)
        });
        if let Some(Err(error)) = result {
            self.error = Some((error, self.time));
        }
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    pub fn on_mouse_move(&mut self, x: f32, y: f32) {
        let hover = self
            .regions
            .iter()
            .rev()
            .find(|region| contains(region.rect, x, y))
            .map(|region| region.action.clone());
        if hover != self.hover {
            self.hover = hover;
        }
    }

    pub fn on_mouse_down(&mut self, r: &mut Renderer, x: f32, y: f32) {
        let action = self
            .regions
            .iter()
            .rev()
            .find(|region| contains(region.rect, x, y))
            .map(|region| region.action.clone());
        if matches!(self.focus, Some(FieldId::TableCell(..)))
            && !matches!(action, Some(Action::ClickField { field, .. }) if Some(field) == self.focus)
        {
            self.finish_table_editor(true);
        }
        self.pressed = action.clone();
        if let Some(Action::ClickField { field, .. }) = action {
            self.focus = Some(field);
            self.set_caret_from_click(r, field, x, y);
        }
    }

    pub fn on_mouse_up(&mut self, r: &mut Renderer, x: f32, y: f32, control: bool, shift: bool) {
        let action = self
            .regions
            .iter()
            .rev()
            .find(|region| contains(region.rect, x, y))
            .map(|region| region.action.clone());
        let pressed = self.pressed.take();
        if let (Some(pressed), Some(released)) = (pressed, action) {
            if pressed == released {
                self.input_control = control;
                self.input_shift = shift;
                self.dispatch(r, pressed);
            }
        }
    }

    pub fn on_double_click(&mut self, x: f32, y: f32) {
        let action = self
            .regions
            .iter()
            .rev()
            .find(|region| contains(region.rect, x, y))
            .map(|region| region.action.clone());
        if let Some(Action::TableCell { tab, row, column }) = action {
            self.start_table_editor(tab, row, column);
        }
    }

    pub fn on_wheel(&mut self, x: f32, y: f32, delta: f32) {
        let view = self
            .scroll_regions
            .iter()
            .rev()
            .find(|(_, rect)| contains(*rect, x, y))
            .map(|(view, _)| *view);
        if let Some(view) = view {
            let offset = self.scroll_offset(view) - delta;
            self.set_scroll(view, offset);
        }
    }

    pub fn on_char(&mut self, character: char) {
        let Some(field) = self.focus else {
            return;
        };
        if let Some(text) = self.fields.get_mut(&field) {
            if character == '\r' || character == '\n' {
                if text.multiline {
                    text.insert('\n');
                }
            } else if !character.is_control() {
                text.insert(character);
            }
        }
    }

    pub fn on_key(&mut self, r: &mut Renderer, key: u32, extend: bool, control: bool) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT,
            VK_TAB, VK_UP,
        };
        if self.modal.is_some() {
            if key == VK_ESCAPE.0 as u32 {
                self.dispatch(r, Action::ModalCancel);
                return true;
            }
            if key == VK_RETURN.0 as u32 {
                let save = matches!(self.modal, Some(Modal::Profile(_)));
                if save {
                    self.dispatch(r, Action::ModalSave);
                } else {
                    self.dispatch(r, Action::ConfirmAccept);
                }
                return true;
            }
            return false;
        }
        if key == VK_ESCAPE.0 as u32 {
            if matches!(self.focus, Some(FieldId::TableCell(..))) {
                self.finish_table_editor(false);
                return true;
            }
            self.focus = None;
            return true;
        }
        let Some(field) = self.focus else {
            return false;
        };
        let multiline = self.fields.get(&field).is_some_and(|text| text.multiline);
        if key == VK_TAB.0 as u32 {
            self.focus = None;
            return true;
        }
        if key == VK_RETURN.0 as u32 {
            if matches!(field, FieldId::TableCell(..)) {
                self.finish_table_editor(true);
                return true;
            }
            if control {
                if let FieldId::Query(tab) = field {
                    let sql = self.query_target(tab);
                    if !sql.is_empty() {
                        self.run_query_or_confirm(r, tab, sql);
                    }
                }
                return true;
            }
            if multiline {
                if let Some(text) = self.fields.get_mut(&field) {
                    text.insert('\n');
                }
                return true;
            }
            return true;
        }
        let Some(text) = self.fields.get_mut(&field) else {
            return false;
        };
        match key {
            k if k == VK_BACK.0 as u32 => text.backspace(),
            k if k == VK_DELETE.0 as u32 => text.delete_forward(),
            k if k == VK_LEFT.0 as u32 => {
                if control {
                    text.move_caret(widgets::CaretMotion::WordLeft, extend);
                } else {
                    text.move_caret(widgets::CaretMotion::Left, extend);
                }
            }
            k if k == VK_RIGHT.0 as u32 => {
                if control {
                    text.move_caret(widgets::CaretMotion::WordRight, extend);
                } else {
                    text.move_caret(widgets::CaretMotion::Right, extend);
                }
            }
            k if k == VK_UP.0 as u32 && multiline => text.move_vertical(-1, extend),
            k if k == VK_DOWN.0 as u32 && multiline => text.move_vertical(1, extend),
            k if k == VK_HOME.0 as u32 => text.move_caret(widgets::CaretMotion::Home, extend),
            k if k == VK_END.0 as u32 => text.move_caret(widgets::CaretMotion::End, extend),
            _ => {}
        }
        true
    }

    fn set_caret_from_click(&mut self, r: &mut Renderer, field: FieldId, x: f32, y: f32) {
        let Some(rect) = self.field_rects.get(&field).copied() else {
            return;
        };
        let font = match field {
            FieldId::Query(_) => theme::Font::Mono,
            _ => theme::Font::Ui,
        };
        let view_scroll = match field {
            FieldId::Query(tab) => self.scroll_offset(ViewId::Editor(tab)),
            _ => 0.0,
        };
        let line_height = r.line_height(font);
        let Some(text) = self.fields.get_mut(&field) else {
            return;
        };
        let local = x - rect.left + text.scroll - 6.0;
        let index = if text.multiline {
            // Multi-line editors place the caret on the clicked line, not at
            // the start of the document.
            let lines: Vec<&str> = text.text.split('\n').collect();
            let line = (((y - rect.top - 8.0 + view_scroll) / line_height)
                .floor()
                .max(0.0)) as usize;
            let line = line.min(lines.len().saturating_sub(1));
            let before: usize = lines
                .iter()
                .take(line)
                .map(|value| value.chars().count() + 1)
                .sum();
            let column = if local >= r.text_width(lines[line], font).unwrap_or(0.0) {
                // Clicking past the end of the line lands on the last column
                // instead of relying on the hit test's trailing behavior.
                lines[line].chars().count()
            } else {
                r.caret_index(lines[line], local.max(0.0), font)
                    .unwrap_or(0)
            };
            before + column
        } else {
            if local >= r.text_width(&text.text, font).unwrap_or(0.0) {
                text.text.chars().count()
            } else {
                r.caret_index(&text.text, local.max(0.0), font).unwrap_or(0)
            }
        };
        let index = index.min(text.text.chars().count());
        text.caret = index;
        text.anchor = index;
    }

    /// Ctrl+A / Ctrl+C / Ctrl+X / Ctrl+V for the focused field.
    pub fn handle_shortcut(&mut self, r: &mut Renderer, key: u32) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_C, VK_V, VK_X};
        let Some(field) = self.focus else {
            return false;
        };
        let Some(text) = self.fields.get_mut(&field) else {
            return false;
        };
        match key {
            k if k == VK_A.0 as u32 => text.select_all(),
            k if k == VK_C.0 as u32 => {
                if let Some(selected) = text.selected_text() {
                    crate::platform::set_clipboard_text(&selected);
                }
            }
            k if k == VK_X.0 as u32 => {
                if let Some(selected) = text.selected_text() {
                    crate::platform::set_clipboard_text(&selected);
                    text.delete_selection();
                }
            }
            k if k == VK_V.0 as u32 => {
                if let Some(clipboard) = crate::platform::clipboard_text() {
                    text.insert_str(&clipboard);
                }
            }
            _ => return false,
        }
        let _ = r;
        true
    }

    /// The statement under the caret, or the selection.
    fn query_target(&self, tab: u64) -> String {
        let Some(state) = self.queries.get(&tab) else {
            return String::new();
        };
        let Some(editor) = self.fields.get(&FieldId::Query(tab)) else {
            return String::new();
        };
        let text = &editor.text;
        let (from, to) = match editor.selection() {
            Some((start, end)) => (start, end),
            None => (editor.caret, editor.caret),
        };
        let target = if state.engine == DatabaseEngine::Redis {
            dbm_workbench::sql_target::line_execution_target(text, from, to)
        } else {
            dbm_workbench::sql_target::sql_execution_target(text, from, to)
        };
        target.map_or_else(String::new, |target| target.sql)
    }

    fn run_query_or_confirm(&mut self, r: &mut Renderer, tab: u64, sql: String) {
        let engine = self
            .queries
            .get(&tab)
            .map_or(DatabaseEngine::Postgres, |state| state.engine);
        if format::requires_confirmation(&sql, engine) {
            self.modal = Some(Modal::Confirm(Box::new(ConfirmState {
                title: "Run destructive statement".to_owned(),
                body: "This query may change or remove many rows. Run it anyway?".to_owned(),
                confirm_label: "Run".to_owned(),
                danger: true,
                action: ConfirmAction::RunQuery { tab, sql },
            })));
            let _ = r;
            return;
        }
        self.run_query(tab, sql);
    }

    // ------------------------------------------------------------------
    // Actions
    // ------------------------------------------------------------------

    pub fn dispatch(&mut self, r: &mut Renderer, action: Action) {
        if !matches!(
            action,
            Action::ToggleMenu(_) | Action::ToggleDatabases(_) | Action::SelectDatabase(..)
        ) {
            self.open_menu = None;
            self.open_databases = None;
        }
        match action {
            Action::NewConnection => {
                self.open_profile_dialog(None);
            }
            Action::SelectProfile(profile_id) => {
                self.active_profile = Some(profile_id);
                self.expanded.insert(profile_id);
                if self.workspaces.contains_key(&profile_id) {
                    if let Some(tab) = self.tabs.iter().rfind(|tab| tab.profile_id == profile_id) {
                        self.active_tab = Some(tab.id);
                    } else {
                        self.open_query(profile_id);
                    }
                } else {
                    self.connect_profile(profile_id);
                }
            }
            Action::ToggleProfile(profile_id) => {
                if !self.expanded.remove(&profile_id) {
                    self.expanded.insert(profile_id);
                }
                self.active_profile = Some(profile_id);
            }
            Action::EditProfile(profile_id) => {
                if let Some(profile) = self.profile(profile_id) {
                    self.open_profile_dialog(Some(profile));
                }
            }
            Action::Disconnect(profile_id) => self.disconnect_profile(profile_id),
            Action::RefreshSchema(profile_id) => self.refresh_schema(profile_id),
            Action::OpenTable {
                profile,
                schema,
                table,
            } => self.open_table(profile, schema, table),
            Action::ToggleNode(key) => {
                if !self.collapsed_nodes.remove(&key) {
                    self.collapsed_nodes.insert(key);
                }
            }
            Action::SelectTab(tab_id) => {
                self.active_tab = Some(tab_id);
                if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
                    self.active_profile = Some(tab.profile_id);
                }
                self.focus = self
                    .queries
                    .contains_key(&tab_id)
                    .then_some(FieldId::Query(tab_id));
            }
            Action::CloseTab(tab_id) => self.close_tab(tab_id),
            Action::NewQuery => {
                if let Some(profile_id) = self.active_profile {
                    self.open_query(profile_id);
                }
            }
            Action::RunQuery(tab) => {
                let sql = self.query_target(tab);
                if !sql.is_empty() {
                    self.run_query_or_confirm(r, tab, sql);
                }
            }
            Action::RefreshQuery(tab) => {
                if let Some(state) = self.queries.get(&tab) {
                    if let Some(sql) = state.executed_sql.clone() {
                        self.run_query(tab, sql);
                    }
                }
            }
            Action::CopyTableCsv(tab) => {
                if let Some(state) = self.tables.get(&tab) {
                    if let Some(page) = &state.page {
                        let rows = visible_table_rows(state, page);
                        let document = table_csv_document(page, &rows);
                        crate::platform::set_clipboard_text(&document);
                        self.show_toast(&format!(
                            "Copied {} visible {} as CSV.",
                            rows.len(),
                            if rows.len() == 1 { "row" } else { "rows" }
                        ));
                    }
                }
            }
            Action::ExportTableCsv(tab) => {
                if self
                    .tables
                    .get(&tab)
                    .is_some_and(|state| !state.pending.is_empty())
                {
                    self.error = Some((
                        "Save or discard pending row changes before exporting.".to_owned(),
                        self.time,
                    ));
                } else {
                    self.export_csv(tab);
                }
            }
            Action::RefreshTable(tab) => {
                if self
                    .tables
                    .get(&tab)
                    .is_some_and(|state| !state.pending.is_empty())
                {
                    self.error = Some((
                        "Save or discard pending row changes before refreshing.".to_owned(),
                        self.time,
                    ));
                } else {
                    self.load_table_page(tab);
                }
            }
            Action::TableCell { tab, row, .. } => self.select_table_row(tab, row),
            Action::StageDelete(tab) => self.stage_selected_rows(tab),
            Action::SaveTable(tab) => self.save_table(tab),
            Action::DiscardTable(tab) => {
                if let Some(state) = self.tables.get_mut(&tab) {
                    if !state.loading && !state.saving {
                        state.pending.clear();
                        state.preview_row = None;
                    }
                }
            }
            Action::DiscardTableRow { tab, row } => {
                if let Some(state) = self.tables.get_mut(&tab) {
                    if !state.loading && !state.saving {
                        let Some(page) = &state.page else {
                            return;
                        };
                        if let Some(row) = page.rows.get(row) {
                            state.pending.discard_row(&page.metadata, row);
                            state.preview_row = None;
                        }
                    }
                }
            }
            Action::ToggleMenu(profile_id) => {
                self.open_menu = (self.open_menu != Some(profile_id)).then_some(profile_id);
            }
            Action::ToggleDatabases(profile_id) => {
                self.open_databases =
                    (self.open_databases != Some(profile_id)).then_some(profile_id);
            }
            Action::SelectDatabase(profile_id, database) => {
                self.open_databases = None;
                self.switch_database(profile_id, database);
            }
            Action::ModalColor(color) => {
                if let Some(Modal::Profile(form)) = &mut self.modal {
                    form.color = color;
                }
            }
            Action::ModalReadOnly => {
                if let Some(Modal::Profile(form)) = &mut self.modal {
                    form.read_only = !form.read_only;
                }
            }
            Action::PagePrev(tab) => {
                if let Some(state) = self.tables.get_mut(&tab) {
                    if state.loading || state.saving {
                        return;
                    }
                    state.page_index = state.page_index.saturating_sub(1);
                }
                self.load_table_page(tab);
            }
            Action::PageNext(tab) => {
                if let Some(state) = self.tables.get_mut(&tab) {
                    if state.loading || state.saving {
                        return;
                    }
                    state.page_index += 1;
                }
                self.load_table_page(tab);
            }
            Action::SortColumn { tab, column } => {
                if let Some(state) = self.tables.get_mut(&tab) {
                    if state.loading || state.saving {
                        return;
                    }
                    let descending = state
                        .order_by
                        .as_ref()
                        .is_some_and(|order| order.column == column && !order.descending);
                    state.order_by = Some(OrderSpec { column, descending });
                    state.page_index = 0;
                }
                self.load_table_page(tab);
            }
            Action::HistoryItem { tab, index } => {
                let sql = self
                    .queries
                    .get(&tab)
                    .and_then(|state| state.history.get(index))
                    .map(|entry| entry.sql.clone());
                if let Some(sql) = sql {
                    set_field(&mut self.fields, FieldId::Query(tab), &sql);
                    self.focus = Some(FieldId::Query(tab));
                }
            }
            Action::ClickField { field, .. } => {
                self.focus = Some(field);
            }
            Action::ModalEngine(engine) => {
                if let Some(Modal::Profile(form)) = &mut self.modal {
                    format::apply_engine_defaults(&mut form.input, engine);
                    let preset = format::preset(engine);
                    set_field(&mut self.fields, FieldId::Name, &form.input.name);
                    set_field(
                        &mut self.fields,
                        FieldId::Port,
                        &form.input.port.to_string(),
                    );
                    set_field(&mut self.fields, FieldId::Username, &form.input.username);
                    set_field(
                        &mut self.fields,
                        FieldId::Database,
                        &form.input.default_database,
                    );
                    let _ = preset;
                }
            }
            Action::ModalTls(index) => {
                if let Some(Modal::Profile(form)) = &mut self.modal {
                    form.tls_index = index;
                }
            }
            Action::ModalImportUrl => self.import_url(),
            Action::ModalTest => self.test_profile(),
            Action::ModalSave => self.save_profile(),
            Action::ModalDelete => {
                let profile = match &self.modal {
                    Some(Modal::Profile(form)) => form.existing.clone(),
                    _ => None,
                };
                if let Some(profile) = profile {
                    self.modal = Some(Modal::Confirm(Box::new(ConfirmState {
                        title: "Delete connection".to_owned(),
                        body: format!(
                            "Delete connection “{}”? Saved password and query history for this profile will be removed.",
                            profile.name
                        ),
                        confirm_label: "Delete".to_owned(),
                        danger: true,
                        action: ConfirmAction::DeleteProfile(Box::new(profile)),
                    })));
                }
            }
            Action::ModalCancel => {
                self.modal = None;
                self.focus = None;
                self.fields.clear();
            }
            Action::ConfirmAccept => {
                let Some(Modal::Confirm(confirm)) = self.modal.take() else {
                    return;
                };
                self.focus = None;
                match confirm.action {
                    ConfirmAction::DeleteProfile(profile) => self.delete_profile(*profile),
                    ConfirmAction::RunQuery { tab, sql } => self.run_query(tab, sql),
                }
            }
            Action::ConfirmCancel => {
                self.modal = None;
            }
            Action::DismissError => {
                self.error = None;
            }
        }
    }

    fn open_profile_dialog(&mut self, profile: Option<ConnectionProfile>) {
        let input = format::default_profile_input(profile.as_ref());
        let tls_index = match input.tls_mode {
            dbm_engine::models::TlsMode::Preferred => 0,
            dbm_engine::models::TlsMode::Required => 1,
            dbm_engine::models::TlsMode::Disabled => 2,
        };
        let read_only = input.read_only;
        let color = input
            .color
            .clone()
            .unwrap_or_else(|| format::DEFAULT_CONNECTION_COLOR.to_owned());
        self.fields.clear();
        let mut url = TextField::new(false);
        url.placeholder = format::preset(input.engine).url_placeholder.to_owned();
        let mut password = TextField::new(false);
        password.password = true;
        password.placeholder = if profile.is_some() {
            "Leave blank to keep saved password".to_owned()
        } else {
            "Stored in OS credential store".to_owned()
        };
        self.fields.insert(FieldId::Url, url);
        self.fields
            .insert(FieldId::Name, TextField::with_text(&input.name, false));
        self.fields
            .insert(FieldId::Host, TextField::with_text(&input.host, false));
        self.fields.insert(
            FieldId::Port,
            TextField::with_text(input.port.to_string(), false),
        );
        self.fields.insert(
            FieldId::Username,
            TextField::with_text(&input.username, false),
        );
        self.fields.insert(
            FieldId::Database,
            TextField::with_text(&input.default_database, false),
        );
        self.fields.insert(FieldId::Password, password);
        self.fields.insert(
            FieldId::CaPath,
            TextField::with_text(input.ca_cert_path.clone().unwrap_or_default(), false),
        );
        self.modal = Some(Modal::Profile(Box::new(ProfileForm {
            input,
            existing: profile,
            feedback: None,
            busy: false,
            tls_index,
            read_only,
            color,
        })));
        self.focus = Some(FieldId::Name);
    }

    fn read_form(&mut self) -> SaveProfileInput {
        let Some(Modal::Profile(form)) = &self.modal else {
            return SaveProfileInput {
                id: None,
                name: String::new(),
                color: None,
                engine: DatabaseEngine::Postgres,
                host: String::new(),
                port: 5432,
                username: String::new(),
                default_database: String::new(),
                tls_mode: dbm_engine::models::TlsMode::Preferred,
                ca_cert_path: None,
                ssh: None,
                read_only: false,
                password: None,
            };
        };
        let mut input = form.input.clone();
        input.name = field_text(&self.fields, FieldId::Name);
        input.host = field_text(&self.fields, FieldId::Host);
        input.port = field_text(&self.fields, FieldId::Port)
            .trim()
            .parse()
            .unwrap_or(input.port);
        input.username = field_text(&self.fields, FieldId::Username);
        input.default_database = field_text(&self.fields, FieldId::Database);
        input.ca_cert_path = {
            let text = field_text(&self.fields, FieldId::CaPath);
            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        };
        input.tls_mode = match form.tls_index {
            1 => dbm_engine::models::TlsMode::Required,
            2 => dbm_engine::models::TlsMode::Disabled,
            _ => dbm_engine::models::TlsMode::Preferred,
        };
        input.read_only = form.read_only;
        input.color = Some(form.color.clone());
        input.password = {
            let text = field_text(&self.fields, FieldId::Password);
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        };
        input
    }

    fn import_url(&mut self) {
        let value = field_text(&self.fields, FieldId::Url);
        let imported = match dbm_workbench::connection_url::parse_connection_url(&value) {
            Ok(imported) => imported,
            Err(message) => {
                self.set_form_feedback(false, &message);
                return;
            }
        };
        {
            let Some(Modal::Profile(form)) = &mut self.modal else {
                return;
            };
            form.input.engine = imported.engine;
            form.input.host = imported.host.clone();
            form.input.port = imported.port;
            form.input.username = imported.username.clone();
            form.input.default_database = imported.default_database.clone();
            form.input.tls_mode = imported.tls_mode.clone();
            form.tls_index = match imported.tls_mode {
                dbm_engine::models::TlsMode::Required => 1,
                dbm_engine::models::TlsMode::Disabled => 2,
                dbm_engine::models::TlsMode::Preferred => 0,
            };
            if let Some(password) = &imported.password {
                form.input.password = Some(password.clone());
                set_field(&mut self.fields, FieldId::Password, password);
            }
        }
        set_field(&mut self.fields, FieldId::Host, &imported.host);
        set_field(&mut self.fields, FieldId::Port, &imported.port.to_string());
        set_field(&mut self.fields, FieldId::Username, &imported.username);
        set_field(
            &mut self.fields,
            FieldId::Database,
            &imported.default_database,
        );
        self.set_form_feedback(
            true,
            "Connection URL imported. Review the details, then save and connect.",
        );
    }

    fn test_profile(&mut self) {
        let input = self.read_form();
        let existing = match &self.modal {
            Some(Modal::Profile(form)) => form.existing.clone(),
            _ => None,
        };
        let profile = match input.to_profile(existing.as_ref()) {
            Ok(profile) => profile,
            Err(error) => {
                self.set_form_feedback(false, &bridge::error_text(&error));
                return;
            }
        };
        self.set_form_busy(true, "Testing connection…");
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let password = resolve_password(&engine, &input, profile.id)?;
                let session = dbm_engine::session::DbSession::connect(profile, password).await?;
                session.close().await;
                Ok(())
            },
            |result| match result {
                Ok(()) => EngineEvent::Toast("Connection successful.".to_owned()),
                Err(error) => EngineEvent::Error(bridge::error_text(&error)),
            },
        );
    }

    fn save_profile(&mut self) {
        let input = self.read_form();
        let existing = match &self.modal {
            Some(Modal::Profile(form)) => form.existing.clone(),
            _ => None,
        };
        let profile = match input.to_profile(existing.as_ref()) {
            Ok(profile) => profile,
            Err(error) => {
                self.set_form_feedback(false, &bridge::error_text(&error));
                return;
            }
        };
        self.set_form_busy(true, "Testing connection before saving…");
        let engine = bridge::engine().clone();
        bridge::spawn(
            async move {
                let password = resolve_password(&engine, &input, profile.id)?;
                let session = dbm_engine::session::DbSession::connect(profile, password).await?;
                session.close().await;
                let saved = engine.store.save_profile(&input)?;
                if let Some(password) = input.password.as_deref().filter(|value| !value.is_empty())
                {
                    engine.credentials.save_password(saved.id, password)?;
                }
                engine.disconnect(saved.id).await;
                Ok(saved)
            },
            |result| match result {
                Ok(saved) => EngineEvent::Toast(format!("Connected to {}.", saved.name)),
                Err(error) => EngineEvent::Error(bridge::error_text(&error)),
            },
        );
        self.modal = None;
        self.focus = None;
        self.fields.clear();
    }

    fn delete_profile(&mut self, profile: ConnectionProfile) {
        let engine = bridge::engine().clone();
        let profile_id = profile.id;
        bridge::spawn(
            async move {
                engine.delete_profile(profile_id).await?;
                Ok(profile_id)
            },
            |_| EngineEvent::Toast("Connection deleted.".to_owned()),
        );
        self.workspaces.remove(&profile_id);
        self.schemas.remove(&profile_id);
        self.close_tabs_for_profile(profile_id);
        if self.active_profile == Some(profile_id) {
            self.active_profile = None;
        }
    }

    fn set_form_feedback(&mut self, success: bool, message: &str) {
        if let Some(Modal::Profile(form)) = &mut self.modal {
            form.feedback = Some((success, message.to_owned()));
        }
    }

    fn set_form_busy(&mut self, busy: bool, message: &str) {
        if let Some(Modal::Profile(form)) = &mut self.modal {
            form.busy = busy;
            form.feedback = Some((true, message.to_owned()));
        }
    }
}

pub fn field_text(fields: &HashMap<FieldId, TextField>, field: FieldId) -> String {
    fields
        .get(&field)
        .map_or_else(String::new, |text| text.text.clone())
}

pub fn set_field(fields: &mut HashMap<FieldId, TextField>, field: FieldId, value: &str) {
    if let Some(text) = fields.get_mut(&field) {
        text.set_text(value);
    }
}

pub fn table_row_key(
    metadata: &dbm_engine::models::TableMetadata,
    row: &[serde_json::Value],
    fallback_index: usize,
) -> String {
    row_key(metadata, row).unwrap_or_else(|| format!("page-row:{fallback_index}"))
}

pub fn visible_table_rows(state: &TableState, page: &TablePage) -> Vec<Vec<serde_json::Value>> {
    page.rows
        .iter()
        .filter(|row| {
            !state
                .pending
                .get(&page.metadata, row)
                .is_some_and(|pending| pending.deleted)
        })
        .map(|row| state.pending.values(&page.metadata, row))
        .collect()
}

pub fn table_csv_document(page: &TablePage, rows: &[Vec<serde_json::Value>]) -> String {
    let columns = page
        .metadata
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    format::csv_document(&columns, rows)
}

fn mutations_equal(left: &[RowMutation], right: &[RowMutation]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.original == right.original
                && left.changes == right.changes
                && left.primary_key == right.primary_key
                && left.xmin == right.xmin
                && left.deleted == right.deleted
        })
}

fn resolve_password(
    engine: &dbm_engine::state::AppState,
    input: &SaveProfileInput,
    profile_id: Uuid,
) -> Result<Option<String>, dbm_engine::error::AppError> {
    if let Some(password) = input.password.as_deref().filter(|value| !value.is_empty()) {
        return Ok(Some(password.to_owned()));
    }
    engine.credentials.get_password(profile_id)
}

pub fn query_meta(response: &QueryResponse) -> String {
    let mut meta = format!("{} rows", response.row_count);
    if let Some(affected) = response.affected_rows {
        meta.push_str(&format!(" · {affected} affected"));
    }
    meta.push_str(&format!(" · {} ms", response.duration_ms));
    if response.truncated {
        meta.push_str(" · truncated");
    }
    meta
}

pub fn column_width(data_type: &str) -> f32 {
    let data_type = data_type.to_lowercase();
    if data_type.contains("json") || data_type.contains("array") {
        320.0
    } else if data_type.contains("text")
        || data_type.contains("character")
        || data_type.contains("timestamp")
    {
        220.0
    } else {
        160.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_engine::models::{TableColumn, TableMetadata};
    use serde_json::json;

    fn table_state(page: TablePage) -> TableState {
        TableState {
            profile_id: Uuid::nil(),
            schema: "public".into(),
            table: "products".into(),
            page: Some(page),
            page_index: 0,
            limit: 200,
            order_by: None,
            filters: Vec::new(),
            columns: vec![
                ("id".into(), "integer".into()),
                ("name".into(), "text".into()),
            ],
            loading: false,
            saving: false,
            saving_mutations: None,
            status: String::new(),
            pending: PendingEdits::default(),
            selected_rows: HashSet::new(),
            selection_anchor: None,
            preview_row: None,
        }
    }

    fn page() -> TablePage {
        TablePage {
            metadata: TableMetadata {
                schema: "public".into(),
                table: "products".into(),
                columns: vec![
                    TableColumn {
                        name: "id".into(),
                        data_type: "integer".into(),
                        nullable: false,
                        default_value: None,
                        ordinal: 0,
                    },
                    TableColumn {
                        name: "name".into(),
                        data_type: "text".into(),
                        nullable: false,
                        default_value: None,
                        ordinal: 1,
                    },
                ],
                primary_key: vec!["id".into()],
                has_xmin: true,
            },
            columns: vec!["id".into(), "name".into(), "__dbm_xmin".into()],
            rows: vec![
                vec![json!(1), json!("Canvas"), json!("41")],
                vec![json!(2), json!("Runner"), json!("42")],
            ],
            total_rows: Some(2),
            offset: 0,
            limit: 200,
            has_more: false,
        }
    }

    #[test]
    fn copied_visible_rows_use_drafts_and_omit_deletions_and_xmin() {
        let page = page();
        let mut state = table_state(page.clone());
        state
            .pending
            .set_cell(&page.metadata, &page.rows[0], 1, "Canvas high top")
            .unwrap();
        state
            .pending
            .toggle_delete(&page.metadata, &[page.rows[1].clone()])
            .unwrap();
        let submitted = state.pending.mutations();

        assert_eq!(
            visible_table_rows(&state, &page),
            vec![vec![json!(1), json!("Canvas high top")]]
        );
        assert_eq!(
            table_csv_document(&page, &visible_table_rows(&state, &page)),
            "id,name\n1,Canvas high top"
        );
        assert!(mutations_equal(&submitted, &state.pending.mutations()));
        state
            .pending
            .set_cell(&page.metadata, &page.rows[0], 1, "Newer draft")
            .unwrap();
        assert!(!mutations_equal(&submitted, &state.pending.mutations()));
    }
}
