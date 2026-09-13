//! The native workbench shell: connection sidebar, tab strip, and content pane.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use dbm_engine::models::{ConnectionProfile, DatabaseEngine, SchemaNode};
use dbm_engine::state::AppState;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use uuid::Uuid;

use crate::bridge;
use crate::state::{Tab, TabId, TabKind, Workspace};
use crate::theme;
use crate::ui::{dialogs, profile_dialog, query_view, table_view};
use dbm_workbench::format;

const SIDEBAR_WIDTH: i32 = 280;
const COLLAPSED_SIDEBAR_WIDTH: i32 = 52;

pub struct Ui {
    pub engine: Arc<AppState>,
    pub window: gtk::ApplicationWindow,
    weak: Weak<RefCell<Ui>>,

    shell: gtk::Paned,
    sidebar: gtk::Box,
    sidebar_body: gtk::Box,
    brand_mark: gtk::Box,
    brand_copy: gtk::Box,
    collapse_button: gtk::Button,
    connections_box: gtk::Box,

    identity_box: gtk::Box,
    identity_dot: gtk::Box,
    identity_name: gtk::Label,
    identity_meta: gtk::Label,
    breadcrumb: gtk::Label,

    banner: gtk::Revealer,
    banner_label: gtk::Label,
    banner_generation: Rc<Cell<u64>>,

    welcome_title: gtk::Label,
    welcome_body: gtk::Label,

    tab_strip: gtk::Box,
    new_query_button: gtk::Button,
    stack: gtk::Stack,

    toast: gtk::Revealer,
    toast_label: gtk::Label,
    toast_generation: Rc<Cell<u64>>,

    pub profiles: Vec<ConnectionProfile>,
    pub workspaces: std::collections::HashMap<Uuid, Workspace>,
    pub schemas: std::collections::HashMap<Uuid, Vec<SchemaNode>>,
    pub active_profile: Option<Uuid>,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<TabId>,
    next_tab_id: TabId,
    sidebar_collapsed: bool,
}

impl Ui {
    pub fn new(engine: Arc<AppState>, window: gtk::ApplicationWindow) -> Rc<RefCell<Self>> {
        let brand_mark = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        brand_mark.add_css_class("brand-mark");
        brand_mark.append(&gtk::Label::new(Some("DB")));

        let brand_copy = gtk::Box::new(gtk::Orientation::Vertical, 0);
        brand_copy.add_css_class("brand-copy");
        let brand_title = gtk::Label::new(Some("DBM"));
        brand_title.add_css_class("brand-title");
        brand_title.set_xalign(0.0);
        let brand_subtitle = gtk::Label::new(Some("database manager"));
        brand_subtitle.add_css_class("brand-subtitle");
        brand_subtitle.set_xalign(0.0);
        brand_copy.append(&brand_title);
        brand_copy.append(&brand_subtitle);

        let collapse_button = gtk::Button::with_label("‹");
        collapse_button.add_css_class("icon-button");
        collapse_button.set_tooltip_text(Some("Collapse sidebar"));

        let brand_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        brand_row.add_css_class("brand-row");
        brand_row.append(&brand_mark);
        brand_row.append(&brand_copy);
        let brand_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        brand_spacer.set_hexpand(true);
        brand_row.append(&brand_spacer);
        brand_row.append(&collapse_button);

        let connections_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let connections_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&connections_box)
            .build();

        let sidebar_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let section_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        section_row.set_margin_top(10);
        section_row.set_margin_bottom(4);
        section_row.set_margin_start(14);
        section_row.set_margin_end(10);
        let section_title = gtk::Label::new(Some("CONNECTIONS"));
        section_title.add_css_class("section-title");
        section_title.set_xalign(0.0);
        let new_connection_button = gtk::Button::with_label("New connection");
        new_connection_button.add_css_class("new-connection");
        let section_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        section_spacer.set_hexpand(true);
        section_row.append(&section_title);
        section_row.append(&section_spacer);
        section_row.append(&new_connection_button);

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.add_css_class("sidebar-footer");
        let local_chip = gtk::Label::new(Some("LOCAL ONLY"));
        local_chip.add_css_class("chip");
        local_chip.add_css_class("local");
        footer.append(&local_chip);

        sidebar_body.append(&section_row);
        sidebar_body.append(&connections_scroll);
        sidebar_body.append(&footer);

        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar.add_css_class("sidebar");
        sidebar.set_width_request(SIDEBAR_WIDTH);
        sidebar.set_halign(gtk::Align::Start);
        sidebar.append(&brand_row);
        sidebar.append(&sidebar_body);

        // Top bar
        let identity_dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        identity_dot.add_css_class("dot");
        identity_dot.set_valign(gtk::Align::Center);
        let identity_name = gtk::Label::new(None);
        identity_name.add_css_class("identity-name");
        identity_name.set_xalign(0.0);
        let identity_meta = gtk::Label::new(None);
        identity_meta.add_css_class("identity-meta");
        identity_meta.set_xalign(0.0);
        let identity_copy = gtk::Box::new(gtk::Orientation::Vertical, 0);
        identity_copy.append(&identity_name);
        identity_copy.append(&identity_meta);
        let identity_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        identity_box.set_valign(gtk::Align::Center);
        identity_box.append(&identity_dot);
        identity_box.append(&identity_copy);

        let breadcrumb = gtk::Label::new(Some("No active connection"));
        breadcrumb.add_css_class("breadcrumb");
        breadcrumb.set_xalign(0.0);
        breadcrumb.set_visible(false);

        let topbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        topbar.add_css_class("topbar");
        topbar.append(&identity_box);
        topbar.append(&breadcrumb);

        // Error banner
        let banner_label = gtk::Label::new(None);
        banner_label.set_xalign(0.0);
        banner_label.set_wrap(true);
        banner_label.set_hexpand(true);
        let banner_dismiss = gtk::Button::with_label("×");
        banner_dismiss.add_css_class("icon-button");
        let banner_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        banner_content.add_css_class("banner");
        banner_content.add_css_class("error");
        banner_content.append(&banner_label);
        banner_content.append(&banner_dismiss);
        let banner = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .child(&banner_content)
            .build();
        let banner_generation = Rc::new(Cell::new(0_u64));
        {
            let banner = banner.clone();
            banner_dismiss.connect_clicked(move |_| banner.set_reveal_child(false));
        }

        // Tab strip
        let tab_strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab_strip.add_css_class("tab-strip");
        let tab_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .child(&tab_strip)
            .build();
        let new_query_button = gtk::Button::with_label("＋");
        new_query_button.add_css_class("tab-new");
        new_query_button.set_tooltip_text(Some("New query"));
        new_query_button.set_visible(false);
        let tab_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab_row.append(&tab_scroll);
        tab_row.append(&new_query_button);

        // Content
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .vexpand(true)
            .hexpand(true)
            .build();
        let welcome_title = gtk::Label::new(Some("No connection selected"));
        welcome_title.add_css_class("title");
        let welcome_body =
            gtk::Label::new(Some("Create a connection from the sidebar to get started."));
        welcome_body.add_css_class("muted");
        welcome_body.set_wrap(true);
        welcome_body.set_justify(gtk::Justification::Center);
        let welcome = gtk::Box::new(gtk::Orientation::Vertical, 8);
        welcome.set_valign(gtk::Align::Center);
        welcome.set_halign(gtk::Align::Center);
        welcome.append(&welcome_title);
        welcome.append(&welcome_body);
        stack.add_named(&welcome, Some("welcome"));
        stack.set_visible_child_name("welcome");

        let main_pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_pane.set_hexpand(true);
        main_pane.append(&topbar);
        main_pane.append(&banner);
        main_pane.append(&tab_row);
        main_pane.append(&stack);

        let shell = gtk::Paned::new(gtk::Orientation::Horizontal);
        shell.set_position(SIDEBAR_WIDTH);
        shell.set_resize_start_child(false);
        shell.set_shrink_start_child(false);
        shell.set_hexpand(true);
        shell.set_start_child(Some(&sidebar));
        shell.set_end_child(Some(&main_pane));

        // Toast overlay
        let toast_label = gtk::Label::new(None);
        toast_label.set_wrap(true);
        let toast_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        toast_content.add_css_class("toast");
        toast_content.append(&toast_label);
        let toast = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .child(&toast_content)
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_bottom(18)
            .margin_end(18)
            .build();
        let toast_generation = Rc::new(Cell::new(0_u64));

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&shell));
        overlay.add_overlay(&toast);
        window.set_child(Some(&overlay));

        let ui = Rc::new(RefCell::new(Self {
            engine,
            window,
            weak: Weak::new(),
            shell: shell.clone(),
            sidebar,
            sidebar_body,
            brand_mark,
            brand_copy,
            collapse_button: collapse_button.clone(),
            connections_box,
            identity_box,
            identity_dot,
            identity_name,
            identity_meta,
            breadcrumb,
            banner,
            banner_label,
            banner_generation,
            welcome_title,
            welcome_body,
            tab_strip,
            new_query_button: new_query_button.clone(),
            stack,
            toast,
            toast_label,
            toast_generation,
            profiles: Vec::new(),
            workspaces: std::collections::HashMap::new(),
            schemas: std::collections::HashMap::new(),
            active_profile: None,
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 1,
            sidebar_collapsed: false,
        }));
        ui.borrow_mut().weak = Rc::downgrade(&ui);

        {
            let ui = ui.clone();
            collapse_button.connect_clicked(move |_| {
                let collapsed = ui.borrow().sidebar_collapsed;
                ui.borrow_mut().set_sidebar_collapsed(!collapsed);
            });
        }
        {
            let ui = ui.clone();
            new_connection_button.connect_clicked(move |_| {
                profile_dialog::open(&ui, None);
            });
        }
        {
            let ui = ui.clone();
            new_query_button.connect_clicked(move |_| {
                if let Some(profile_id) = ui.borrow().active_profile {
                    ui.borrow_mut().open_query(profile_id);
                }
            });
        }

        ui.borrow().load_profiles();
        ui
    }

    pub fn this(&self) -> Rc<RefCell<Self>> {
        self.weak.upgrade().expect("the workbench must stay alive")
    }

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

    pub fn workspace_database(&self, profile_id: Uuid) -> String {
        self.workspaces
            .get(&profile_id)
            .map(|workspace| workspace.profile.default_database.clone())
            .unwrap_or_else(|| {
                self.profile(profile_id).map_or_else(
                    || format::fallback_database(DatabaseEngine::Postgres).to_owned(),
                    |profile| format::fallback_database(profile.engine).to_owned(),
                )
            })
    }

    pub fn profile_engine(&self, profile_id: Uuid) -> DatabaseEngine {
        self.profile(profile_id)
            .map_or(DatabaseEngine::Postgres, |profile| profile.engine)
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
    // Async work
    // ------------------------------------------------------------------

    pub fn load_profiles(&self) {
        let engine = self.engine.clone();
        let this = self.this();
        bridge::spawn(
            async move {
                let summaries = engine.profile_summaries()?;
                Ok(summaries
                    .into_iter()
                    .map(|summary| summary.profile)
                    .collect::<Vec<_>>())
            },
            move |result| {
                let mut ui = this.borrow_mut();
                match result {
                    Ok(profiles) => {
                        ui.profiles = profiles;
                        ui.rebuild_sidebar();
                        ui.update_welcome();
                    }
                    Err(error) => ui.show_error(&format::error_message(&error)),
                }
            },
        );
    }

    pub fn connect_profile(&self, profile_id: Uuid) {
        let profile = self
            .profile(profile_id)
            .or_else(|| self.engine.store.get_profile(profile_id).ok().flatten());
        let Some(profile) = profile else {
            return;
        };
        let engine = self.engine.clone();
        let this = self.this();
        bridge::spawn(
            async move {
                let session = engine.connect(profile.clone()).await?;
                let databases = session.list_databases().await?;
                let schema = session.schema_tree().await?;
                Ok((profile, databases, schema))
            },
            move |result| {
                let mut ui = this.borrow_mut();
                match result {
                    Ok((profile, databases, schema)) => {
                        ui.workspaces
                            .insert(profile.id, Workspace { profile, databases });
                        ui.schemas.insert(profile_id, schema);
                        ui.active_profile = Some(profile_id);
                        ui.rebuild_sidebar();
                        ui.open_query(profile_id);
                        ui.show_toast("Connected.");
                    }
                    Err(error) => {
                        ui.show_error(&format::error_message(&error));
                        ui.rebuild_sidebar();
                    }
                }
            },
        );
    }

    pub fn disconnect_profile(&self, profile_id: Uuid) {
        let engine = self.engine.clone();
        let this = self.this();
        bridge::spawn(
            async move {
                engine.disconnect(profile_id).await;
                Ok(())
            },
            move |_| {
                let mut ui = this.borrow_mut();
                ui.workspaces.remove(&profile_id);
                ui.schemas.remove(&profile_id);
                ui.close_tabs_for_profile(profile_id);
                if ui.active_profile == Some(profile_id) {
                    ui.active_profile = None;
                }
                ui.rebuild_sidebar();
                ui.update_identity();
                ui.update_welcome();
            },
        );
    }

    pub fn switch_database(&self, profile_id: Uuid, database: String) {
        let Some(mut profile) = self.profile(profile_id) else {
            return;
        };
        profile.default_database = database;
        let engine = self.engine.clone();
        let this = self.this();
        bridge::spawn(
            async move {
                let session = engine.connect(profile.clone()).await?;
                let databases = session.list_databases().await?;
                let schema = session.schema_tree().await?;
                Ok((profile, databases, schema))
            },
            move |result| {
                let mut ui = this.borrow_mut();
                match result {
                    Ok((profile, databases, schema)) => {
                        ui.workspaces
                            .insert(profile.id, Workspace { profile, databases });
                        ui.schemas.insert(profile_id, schema);
                        ui.rebuild_sidebar();
                        ui.update_identity();
                        ui.update_welcome();
                    }
                    Err(error) => ui.show_error(&format::error_message(&error)),
                }
            },
        );
    }

    pub fn refresh_schema(&self, profile_id: Uuid) {
        let engine = self.engine.clone();
        let this = self.this();
        let previous = self.schemas.get(&profile_id).cloned().unwrap_or_default();
        let kind = if self.profile_engine(profile_id) == DatabaseEngine::Redis {
            "Keyspace"
        } else {
            "Schema"
        };
        bridge::spawn(
            async move {
                let session = engine.session(profile_id).await?;
                session.schema_tree().await
            },
            move |result| {
                let mut ui = this.borrow_mut();
                match result {
                    Ok(tree) => {
                        let (_, message) = format::describe_schema_refresh(&previous, &tree, kind);
                        ui.schemas.insert(profile_id, tree);
                        ui.rebuild_sidebar();
                        ui.show_toast(&message);
                    }
                    Err(error) => ui.show_error(&format::error_message(&error)),
                }
            },
        );
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
            self.set_active_tab(tab_id);
            return;
        }
        let widget = table_view::build(
            self.this(),
            self.engine.clone(),
            profile_id,
            schema.clone(),
            table.clone(),
        );
        self.add_tab(Tab {
            id: 0,
            title: format!("{schema}.{table}"),
            kind: TabKind::Table { schema, table },
            profile_id,
            widget,
        });
    }

    pub fn open_query(&mut self, profile_id: Uuid) {
        let database = self.workspace_database(profile_id);
        let engine_kind = self.profile_engine(profile_id);
        let title = self.next_query_title(profile_id);
        let sql = format::default_query_text(engine_kind).to_owned();
        let widget = query_view::build(
            self.this(),
            self.engine.clone(),
            profile_id,
            database.clone(),
            engine_kind,
            sql,
        );
        self.add_tab(Tab {
            id: 0,
            title,
            kind: TabKind::Query { database },
            profile_id,
            widget,
        });
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

    fn add_tab(&mut self, mut tab: Tab) {
        tab.id = self.next_tab_id;
        self.next_tab_id += 1;
        self.stack.add_named(&tab.widget, Some(&tab.id.to_string()));
        let tab_id = tab.id;
        self.tabs.push(tab);
        self.set_active_tab(tab_id);
    }

    pub fn set_active_tab(&mut self, tab_id: TabId) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
            return;
        };
        let profile_id = tab.profile_id;
        self.stack.set_visible_child_name(&tab_id.to_string());
        self.active_tab = Some(tab_id);
        self.active_profile = Some(profile_id);
        self.update_identity();
        self.update_welcome();
        self.rebuild_tab_strip();
        self.rebuild_sidebar();
    }

    pub fn close_tab(&mut self, tab_id: TabId) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let tab = self.tabs.remove(index);
        self.stack.remove(&tab.widget);
        if self.active_tab == Some(tab_id) {
            self.active_tab = None;
            let next = self
                .tabs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| self.tabs.get(previous))
                })
                .map(|tab| tab.id);
            match next {
                Some(next) => self.set_active_tab(next),
                None => {
                    self.stack.set_visible_child_name("welcome");
                    self.update_identity();
                    self.update_welcome();
                }
            }
        }
        self.rebuild_tab_strip();
        self.rebuild_sidebar();
    }

    pub fn close_tabs_for_profile(&mut self, profile_id: Uuid) {
        let tab_ids: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|tab| tab.profile_id == profile_id)
            .map(|tab| tab.id)
            .collect();
        for tab_id in tab_ids {
            self.close_tab(tab_id);
        }
    }

    pub fn open_connection_editor(&self, profile: ConnectionProfile) {
        profile_dialog::open(&self.this(), Some(profile));
    }

    // ------------------------------------------------------------------
    // Sidebar
    // ------------------------------------------------------------------

    pub fn rebuild_sidebar(&mut self) {
        while let Some(child) = self.connections_box.first_child() {
            self.connections_box.remove(&child);
        }
        if self.profiles.is_empty() {
            let empty = gtk::Label::new(Some("No saved connections."));
            empty.add_css_class("empty-note");
            empty.set_xalign(0.0);
            self.connections_box.append(&empty);
            return;
        }
        let groups: Vec<gtk::Widget> = self
            .profiles
            .iter()
            .map(|profile| self.connection_group(profile))
            .collect();
        for group in groups {
            self.connections_box.append(&group);
        }
    }

    fn connection_group(&self, profile: &ConnectionProfile) -> gtk::Widget {
        let profile_id = profile.id;
        let workspace = self.workspaces.get(&profile_id);
        let connected = workspace.is_some();
        let active = self.active_profile == Some(profile_id);
        let expanded = active && connected;

        let group = gtk::Box::new(gtk::Orientation::Vertical, 0);
        group.add_css_class("connection-group");
        let color_class = theme::color_class(format::profile_color(profile));
        if active {
            group.add_css_class("active");
            group.add_css_class(&format!("{color_class}-accent"));
        }

        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("dot");
        dot.add_css_class(&color_class);
        dot.set_valign(gtk::Align::Center);

        let name = gtk::Label::new(Some(&profile.name));
        name.add_css_class("connection-name");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let meta_text = if profile.username.is_empty() {
            format!(
                "{} · {}",
                format::engine_label(profile.engine),
                profile.host
            )
        } else {
            format!(
                "{} · {}@{}",
                format::engine_label(profile.engine),
                profile.username,
                profile.host
            )
        };
        let meta = gtk::Label::new(Some(&meta_text));
        meta.add_css_class("connection-meta");
        meta.set_xalign(0.0);
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let copy = gtk::Box::new(gtk::Orientation::Vertical, 0);
        copy.set_hexpand(true);
        copy.append(&name);
        copy.append(&meta);

        let select = gtk::Button::new();
        select.add_css_class("connection-row");
        select.set_hexpand(true);
        if active {
            select.add_css_class("active");
        }
        let select_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        select_content.append(&dot);
        select_content.append(&copy);
        select.set_child(Some(&select_content));
        select.set_tooltip_text(Some(if connected {
            "Select connection"
        } else {
            "Connect"
        }));

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let actions = self.connection_actions(profile, connected, expanded);
        row.append(&select);
        row.append(&actions);
        group.append(&row);

        if expanded {
            group.append(&self.workspace_panel(profile));
        }

        {
            let ui = self.this();
            select.connect_clicked(move |_| {
                let mut ui = ui.borrow_mut();
                if ui.workspaces.contains_key(&profile_id) {
                    ui.active_profile = Some(profile_id);
                    if let Some(tab) = ui.tabs.iter().rfind(|tab| tab.profile_id == profile_id) {
                        let tab_id = tab.id;
                        ui.set_active_tab(tab_id);
                    } else {
                        ui.open_query(profile_id);
                    }
                } else {
                    ui.connect_profile(profile_id);
                }
            });
        }

        group.upcast()
    }

    fn connection_actions(
        &self,
        profile: &ConnectionProfile,
        connected: bool,
        expanded: bool,
    ) -> gtk::Widget {
        let profile_id = profile.id;
        let profile = profile.clone();
        let menu_button = gtk::Button::with_label("⋯");
        menu_button.add_css_class("icon-button");
        menu_button.set_tooltip_text(Some("Connection actions"));

        let popover = gtk::Popover::new();
        popover.set_parent(&menu_button);
        popover.set_has_arrow(true);
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 2);
        menu.set_width_request(180);

        let edit = gtk::Button::with_label("Edit connection");
        edit.add_css_class("link-button");
        edit.set_halign(gtk::Align::Fill);
        {
            let ui = self.this();
            let popover = popover.clone();
            let profile = profile.clone();
            edit.connect_clicked(move |_| {
                popover.popdown();
                ui.borrow().open_connection_editor(profile.clone());
            });
        }
        menu.append(&edit);

        if connected {
            let disconnect = gtk::Button::with_label("Disconnect");
            disconnect.add_css_class("link-button");
            disconnect.set_halign(gtk::Align::Fill);
            let ui = self.this();
            let popover = popover.clone();
            disconnect.connect_clicked(move |_| {
                popover.popdown();
                ui.borrow().disconnect_profile(profile_id);
            });
            menu.append(&disconnect);
        }

        popover.set_child(Some(&menu));
        {
            let popover = popover.clone();
            menu_button.connect_clicked(move |_| popover.popup());
        }
        {
            let popover = popover.clone();
            menu_button.connect_destroy(move |_| popover.unparent());
        }

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        if expanded || connected {
            let toggle = gtk::Button::with_label(if expanded { "⌃" } else { "⌄" });
            toggle.add_css_class("icon-button");
            toggle.set_tooltip_text(Some(if expanded {
                "Collapse connection"
            } else {
                "Expand connection"
            }));
            let ui = self.this();
            toggle.connect_clicked(move |_| {
                let mut ui = ui.borrow_mut();
                if ui.active_profile == Some(profile_id) {
                    ui.active_profile = None;
                } else {
                    ui.active_profile = Some(profile_id);
                }
                ui.rebuild_sidebar();
            });
            actions.append(&toggle);
        }
        actions.append(&menu_button);
        actions.upcast()
    }

    fn workspace_panel(&self, profile: &ConnectionProfile) -> gtk::Widget {
        let profile_id = profile.id;
        let workspace = self
            .workspaces
            .get(&profile_id)
            .cloned()
            .expect("workspace panel requires a connection");

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 6);
        panel.add_css_class("workspace-panel");

        let database_label = gtk::Label::new(Some(if profile.engine == DatabaseEngine::Redis {
            "DATABASE INDEX"
        } else {
            "DATABASE"
        }));
        database_label.add_css_class("panel-label");
        database_label.set_xalign(0.0);
        panel.append(&database_label);

        let names = workspace.database_names();
        let dropdown =
            gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
        if let Some(index) = names
            .iter()
            .position(|name| name == &workspace.profile.default_database)
        {
            dropdown.set_selected(index as u32);
        }
        {
            let ui = self.this();
            let current = workspace.profile.default_database.clone();
            dropdown.connect_selected_notify(move |dropdown| {
                let index = dropdown.selected();
                let Some(name) = dropdown
                    .model()
                    .and_downcast::<gtk::StringList>()
                    .and_then(|model| model.string(index))
                    .map(|name| name.to_string())
                else {
                    return;
                };
                if name != current {
                    ui.borrow().switch_database(profile_id, name);
                }
            });
        }
        panel.append(&dropdown);

        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let heading_label = gtk::Label::new(Some(if profile.engine == DatabaseEngine::Redis {
            "KEYSPACE"
        } else {
            "SCHEMA"
        }));
        heading_label.add_css_class("panel-label");
        heading_label.set_xalign(0.0);
        let refresh = gtk::Button::with_label("Refresh");
        refresh.add_css_class("link-button");
        let heading_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        heading_spacer.set_hexpand(true);
        heading.append(&heading_label);
        heading.append(&heading_spacer);
        heading.append(&refresh);
        panel.append(&heading);
        {
            let ui = self.this();
            refresh.connect_clicked(move |_| ui.borrow().refresh_schema(profile_id));
        }

        let selected = self
            .active_table()
            .filter(|(table_profile, _, _)| *table_profile == profile_id)
            .map(|(_, schema, table)| (schema, table));
        let tree = gtk::Box::new(gtk::Orientation::Vertical, 1);
        tree.add_css_class("schema-tree");
        for node in self.schemas.get(&profile_id).cloned().unwrap_or_default() {
            tree.append(&self.schema_branch(profile_id, &node, 0, &selected));
        }
        panel.append(&tree);

        panel.upcast()
    }

    fn schema_branch(
        &self,
        profile_id: Uuid,
        node: &SchemaNode,
        depth: usize,
        selected: &Option<(String, String)>,
    ) -> gtk::Widget {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 1);
        let is_table = node.table.is_some() && node.schema.is_some();
        let is_selected = is_table
            && selected.as_ref().is_some_and(|(schema, table)| {
                Some(schema) == node.schema.as_ref() && Some(table) == node.table.as_ref()
            });

        let row = gtk::Button::new();
        row.add_css_class("schema-row");
        row.set_halign(gtk::Align::Fill);
        row.set_margin_start((depth as i32) * 12);
        if is_selected {
            row.add_css_class("active");
        }
        let caret = gtk::Label::new(Some(if is_table { "▧" } else { "›" }));
        caret.add_css_class("muted");
        let badge = gtk::Label::new(Some(match node.kind.as_str() {
            "table" => "T",
            "key" => "K",
            "view" => "V",
            _ => "S",
        }));
        badge.add_css_class("schema-icon");
        badge.add_css_class(&format!("schema-icon-{}", node.kind));
        let name = gtk::Label::new(Some(&node.name));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.append(&caret);
        content.append(&badge);
        content.append(&name);
        row.set_child(Some(&content));

        let children = gtk::Box::new(gtk::Orientation::Vertical, 1);
        children.set_visible(depth < 1);
        if !is_table {
            for child in &node.children {
                children.append(&self.schema_branch(profile_id, child, depth + 1, selected));
            }
        }

        if is_table {
            let schema = node.schema.clone().expect("schema node");
            let table = node.table.clone().expect("table node");
            let ui = self.this();
            row.connect_clicked(move |_| {
                ui.borrow_mut()
                    .open_table(profile_id, schema.clone(), table.clone());
            });
        } else {
            let children_for_click = children.clone();
            let caret_for_click = caret.clone();
            row.connect_clicked(move |_| {
                let visible = !children_for_click.is_visible();
                children_for_click.set_visible(visible);
                caret_for_click.set_label(if visible { "⌄" } else { "›" });
            });
        }

        container.append(&row);
        if !is_table {
            container.append(&children);
        }
        container.upcast()
    }

    // ------------------------------------------------------------------
    // Chrome
    // ------------------------------------------------------------------

    pub fn rebuild_tab_strip(&mut self) {
        while let Some(child) = self.tab_strip.first_child() {
            self.tab_strip.remove(&child);
        }
        let tabs: Vec<(TabId, String, String)> = self
            .tabs
            .iter()
            .map(|tab| {
                let color = self.profile(tab.profile_id).map_or_else(
                    || format::DEFAULT_CONNECTION_COLOR.to_owned(),
                    |profile| format::profile_color(&profile).to_owned(),
                );
                (tab.id, tab.title.clone(), color)
            })
            .collect();
        for (tab_id, title, color) in tabs {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            row.add_css_class("tab");
            if self.active_tab == Some(tab_id) {
                row.add_css_class("active");
                row.add_css_class(&format!("{}-accent", theme::color_class(&color)));
            }
            let title_button = gtk::Button::with_label(&title);
            title_button.add_css_class("tab-title");
            title_button.set_tooltip_text(Some(&title));
            {
                let ui = self.this();
                title_button.connect_clicked(move |_| {
                    ui.borrow_mut().set_active_tab(tab_id);
                });
            }
            let close_button = gtk::Button::with_label("×");
            close_button.add_css_class("tab-close");
            close_button.set_tooltip_text(Some(&format!("Close {title}")));
            {
                let ui = self.this();
                close_button.connect_clicked(move |_| {
                    ui.borrow_mut().close_tab(tab_id);
                });
            }
            row.append(&title_button);
            row.append(&close_button);
            self.tab_strip.append(&row);
        }
        self.new_query_button.set_visible(
            self.active_profile
                .is_some_and(|profile_id| self.workspaces.contains_key(&profile_id)),
        );
    }

    fn update_identity(&mut self) {
        let profile = self
            .active_profile
            .and_then(|profile_id| self.profile(profile_id));
        match profile {
            Some(profile) => {
                self.identity_box.set_visible(true);
                self.breadcrumb.set_visible(false);
                let color_class = theme::color_class(format::profile_color(&profile));
                for class in self
                    .identity_dot
                    .css_classes()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                {
                    if class.starts_with("conn-color-") {
                        self.identity_dot.remove_css_class(&class);
                    }
                }
                self.identity_dot.add_css_class(&color_class);
                self.identity_name.set_label(&profile.name);
                let meta = if profile.username.is_empty() {
                    format!(
                        "{}:{} / {}",
                        profile.host, profile.port, profile.default_database
                    )
                } else {
                    format!(
                        "{}@{}:{} / {}",
                        profile.username, profile.host, profile.port, profile.default_database
                    )
                };
                self.identity_meta.set_label(&meta);
            }
            None => {
                self.identity_box.set_visible(false);
                self.breadcrumb.set_visible(true);
            }
        }
    }

    fn update_welcome(&mut self) {
        let selected = self
            .active_profile
            .and_then(|profile_id| self.profile(profile_id));
        let connected = self
            .active_profile
            .is_some_and(|profile_id| self.workspaces.contains_key(&profile_id));
        let (title, body) = match selected {
            Some(profile) => (
                profile.name.clone(),
                if connected {
                    "Choose a table from the sidebar or open a new query with the plus button above."
                        .to_owned()
                } else {
                    "This connection is selected but not connected. Select it again to connect."
                        .to_owned()
                },
            ),
            None => (
                "No connection selected".to_owned(),
                if self.profiles.is_empty() {
                    "Create a connection from the sidebar to get started.".to_owned()
                } else {
                    "Select a saved connection from the sidebar to browse its data.".to_owned()
                },
            ),
        };
        self.welcome_title.set_label(&title);
        self.welcome_body.set_label(&body);
    }

    pub fn show_error(&mut self, message: &str) {
        self.banner_label.set_label(message);
        self.banner.set_reveal_child(true);
        let generation = self.banner_generation.get() + 1;
        self.banner_generation.set(generation);
        let banner = self.banner.clone();
        let counter = self.banner_generation.clone();
        glib::timeout_add_seconds_local(10, move || {
            if counter.get() == generation {
                banner.set_reveal_child(false);
            }
            glib::ControlFlow::Break
        });
    }

    pub fn show_toast(&mut self, message: &str) {
        self.toast_label.set_label(message);
        self.toast.set_reveal_child(true);
        let generation = self.toast_generation.get() + 1;
        self.toast_generation.set(generation);
        let toast = self.toast.clone();
        let counter = self.toast_generation.clone();
        glib::timeout_add_seconds_local(6, move || {
            if counter.get() == generation {
                toast.set_reveal_child(false);
            }
            glib::ControlFlow::Break
        });
    }

    pub fn set_sidebar_collapsed(&mut self, collapsed: bool) {
        self.sidebar_collapsed = collapsed;
        self.sidebar_body.set_visible(!collapsed);
        self.brand_mark.set_visible(!collapsed);
        self.brand_copy.set_visible(!collapsed);
        self.collapse_button
            .set_label(if collapsed { "›" } else { "‹" });
        self.collapse_button.set_tooltip_text(Some(if collapsed {
            "Expand sidebar"
        } else {
            "Collapse sidebar"
        }));
        self.sidebar.set_width_request(if collapsed {
            COLLAPSED_SIDEBAR_WIDTH
        } else {
            SIDEBAR_WIDTH
        });
        self.shell.set_position(if collapsed {
            COLLAPSED_SIDEBAR_WIDTH
        } else {
            SIDEBAR_WIDTH
        });
    }

    pub fn confirm_delete_profile(&self, profile: ConnectionProfile) {
        let ui = self.this();
        let name = profile.name.clone();
        dialogs::confirm(
            &self.window,
            "Delete connection",
            &format!(
                "Delete connection “{name}”? Saved password and query history for this profile will be removed."
            ),
            "Delete",
            true,
            move || {
                let engine = ui.borrow().engine.clone();
                let this = ui.clone();
                let profile_id = profile.id;
                bridge::spawn(
                    async move {
                        let credentials = engine.credentials;
                        credentials.delete_password(profile_id)?;
                        engine.store.delete_profile(profile_id)?;
                        Ok(())
                    },
                    move |result| {
                        let mut ui = this.borrow_mut();
                        match result {
                            Ok(()) => {
                                ui.workspaces.remove(&profile_id);
                                ui.schemas.remove(&profile_id);
                                ui.close_tabs_for_profile(profile_id);
                                if ui.active_profile == Some(profile_id) {
                                    ui.active_profile = None;
                                }
                                ui.load_profiles();
                                ui.show_toast("Connection deleted.");
                            }
                            Err(error) => ui.show_error(&format::error_message(&error)),
                        }
                    },
                );
            },
        );
    }
}
