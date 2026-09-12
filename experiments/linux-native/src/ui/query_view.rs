//! The SQL / Redis command workbench: editor, history, and results.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use dbm_engine::models::{DatabaseEngine, QueryHistoryEntry, QueryResponse};
use dbm_engine::state::AppState;
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use uuid::Uuid;

use crate::bridge;
use crate::ui::app::Ui;
use crate::ui::dialogs;
use crate::ui::grid::{default_column_width, DataGrid};
use dbm_workbench::format;
use dbm_workbench::sql_target;
use dbm_workbench::table_select;

struct QueryState {
    profile_id: Uuid,
    database: String,
    engine: DatabaseEngine,
    running: bool,
    executed_sql: Option<String>,
    editor: gtk::TextView,
    grid: DataGrid,
    meta: gtk::Label,
    run_button: gtk::Button,
    refresh_button: gtk::Button,
    inline_error: gtk::Revealer,
    inline_error_label: gtk::Label,
    history_list: gtk::ListBox,
    history_sql: Rc<RefCell<Vec<String>>>,
}

pub fn build(
    ui: Rc<RefCell<Ui>>,
    engine: Arc<AppState>,
    profile_id: Uuid,
    database: String,
    engine_kind: DatabaseEngine,
    initial_sql: String,
) -> gtk::Widget {
    let editor = gtk::TextView::new();
    editor.add_css_class("editor");
    editor.set_monospace(true);
    editor.set_wrap_mode(gtk::WrapMode::None);
    editor.buffer().set_text(&initial_sql);

    let editor_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&editor)
        .vexpand(true)
        .hexpand(true)
        .build();

    let history_list = gtk::ListBox::new();
    history_list.add_css_class("history-list");
    history_list.set_selection_mode(gtk::SelectionMode::None);
    let history_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&history_list)
        .width_request(230)
        .build();
    let history_panel = gtk::Box::new(gtk::Orientation::Vertical, 4);
    history_panel.add_css_class("history-panel");
    let history_title = gtk::Label::new(Some("HISTORY"));
    history_title.add_css_class("panel-label");
    history_title.set_xalign(0.0);
    history_title.set_margin_start(8);
    history_title.set_margin_top(6);
    history_panel.append(&history_title);
    history_panel.append(&history_scroll);

    let split = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    split.append(&editor_scroll);
    split.append(&history_panel);
    split.set_vexpand(true);

    let meta = gtk::Label::new(Some("Results will appear here."));
    meta.add_css_class("result-meta");
    meta.set_xalign(0.0);
    meta.set_hexpand(true);

    let grid = DataGrid::new();
    grid.root.set_vexpand(true);

    let inline_error_label = gtk::Label::new(None);
    inline_error_label.set_xalign(0.0);
    inline_error_label.set_wrap(true);
    inline_error_label.set_hexpand(true);
    let inline_error_dismiss = gtk::Button::with_label("×");
    inline_error_dismiss.add_css_class("icon-button");
    let inline_error_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    inline_error_content.add_css_class("banner");
    inline_error_content.add_css_class("error");
    inline_error_content.append(&inline_error_label);
    inline_error_content.append(&inline_error_dismiss);
    let inline_error = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .child(&inline_error_content)
        .build();
    {
        let inline_error = inline_error.clone();
        inline_error_dismiss.connect_clicked(move |_| inline_error.set_reveal_child(false));
    }

    let title = gtk::Label::new(Some(if engine_kind == DatabaseEngine::Redis {
        "REDIS WORKBENCH"
    } else {
        "SQL WORKBENCH"
    }));
    title.add_css_class("eyebrow");
    title.set_xalign(0.0);

    let refresh_button = gtk::Button::with_label("Refresh");
    refresh_button.add_css_class("secondary-button");
    refresh_button.set_sensitive(false);
    refresh_button.set_tooltip_text(Some("Re-run the last executed statement for fresh results"));
    let run_button = gtk::Button::with_label(if engine_kind == DatabaseEngine::Redis {
        "Run command"
    } else {
        "Run statement"
    });
    run_button.add_css_class("primary-button");
    run_button.set_tooltip_text(Some(
        "Run the statement under the cursor or the selection (Ctrl+Enter)",
    ));

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.add_css_class("view-toolbar");
    toolbar.append(&title);
    let toolbar_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toolbar_spacer.set_hexpand(true);
    toolbar.append(&toolbar_spacer);
    toolbar.append(&refresh_button);
    toolbar.append(&run_button);

    let hint = gtk::Label::new(Some(if engine_kind == DatabaseEngine::Redis {
        "The command under the cursor or the selection will run · Ctrl+Enter · results capped at 10,000 rows"
    } else {
        "The statement under the cursor or the selected SQL will run · Ctrl+Enter · results capped at 10,000 rows"
    }));
    hint.add_css_class("muted");
    hint.set_xalign(0.0);
    hint.set_margin_start(14);
    hint.set_margin_top(4);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&toolbar);
    root.append(&split);
    root.append(&hint);
    root.append(&inline_error);
    root.append(&meta);
    root.append(&grid.root);

    let state = Rc::new(RefCell::new(QueryState {
        profile_id,
        database,
        engine: engine_kind,
        running: false,
        executed_sql: None,
        editor: editor.clone(),
        grid,
        meta: meta.clone(),
        run_button: run_button.clone(),
        refresh_button: refresh_button.clone(),
        inline_error: inline_error.clone(),
        inline_error_label,
        history_list: history_list.clone(),
        history_sql: Rc::new(RefCell::new(Vec::new())),
    }));

    // Ctrl+Enter inside the editor.
    {
        let controller = gtk::EventControllerKey::new();
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        controller.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gdk::Key::Return
                && (modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                    || modifiers.contains(gdk::ModifierType::SUPER_MASK))
            {
                run(state.clone(), ui.clone(), engine.clone());
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        editor.add_controller(controller);
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        run_button.connect_clicked(move |_| run(state.clone(), ui.clone(), engine.clone()));
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        refresh_button.connect_clicked(move |_| {
            let sql = state.borrow().executed_sql.clone();
            if let Some(sql) = sql {
                let target = resolve_target(&state, &ui, &sql);
                execute(state.clone(), ui.clone(), engine.clone(), sql, target);
            }
        });
    }
    {
        let state = state.clone();
        history_list.connect_row_activated(move |_, row| {
            let sql = row.widget_name();
            state.borrow().editor.buffer().set_text(sql.as_str());
        });
    }

    refresh_history(state.clone(), engine);
    root.upcast()
}

fn selected_or_current_sql(state: &QueryState) -> String {
    let buffer = state.editor.buffer();
    let text = buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string();
    let (from, to) = match buffer.selection_bounds() {
        Some((start, end)) => (start.offset() as usize, end.offset() as usize),
        None => {
            let cursor = buffer.cursor_position() as usize;
            (cursor, cursor)
        }
    };
    let target = if state.engine == DatabaseEngine::Redis {
        sql_target::line_execution_target(&text, from, to)
    } else {
        sql_target::sql_execution_target(&text, from, to)
    };
    target.map_or_else(String::new, |target| target.sql)
}

/// The table a full-table select names, when this profile's schema tree has
/// exactly one match. Redis commands never resolve to a table.
fn resolve_target(
    state: &Rc<RefCell<QueryState>>,
    ui: &Rc<RefCell<Ui>>,
    sql: &str,
) -> Option<(String, String)> {
    let state = state.borrow();
    if state.engine == DatabaseEngine::Redis {
        return None;
    }
    let tree = ui
        .borrow()
        .schemas
        .get(&state.profile_id)
        .cloned()
        .unwrap_or_default();
    table_select::resolve_full_table_select(sql, &tree)
}

fn run(state: Rc<RefCell<QueryState>>, ui: Rc<RefCell<Ui>>, engine: Arc<AppState>) {
    let (sql, running, engine_kind) = {
        let state = state.borrow();
        (selected_or_current_sql(&state), state.running, state.engine)
    };
    if running || sql.trim().is_empty() {
        return;
    }
    let target_table = resolve_target(&state, &ui, &sql);
    if format::requires_confirmation(&sql, engine_kind) {
        let confirm_state = state.clone();
        let confirm_engine = engine.clone();
        let confirm_ui = ui.clone();
        let confirm_target = target_table.clone();
        let window = ui.borrow().window.clone();
        dialogs::confirm(
            &window,
            "Run destructive statement",
            "This query may change or remove many rows. Run it anyway?",
            "Run",
            true,
            move || {
                execute(
                    confirm_state,
                    confirm_ui,
                    confirm_engine,
                    sql,
                    confirm_target,
                )
            },
        );
        return;
    }
    execute(state, ui, engine, sql, target_table);
}

fn execute(
    state: Rc<RefCell<QueryState>>,
    ui: Rc<RefCell<Ui>>,
    engine: Arc<AppState>,
    sql: String,
    target_table: Option<(String, String)>,
) {
    let (profile_id, database) = {
        let state = state.borrow();
        (state.profile_id, state.database.clone())
    };
    {
        let mut state = state.borrow_mut();
        state.running = true;
        state.run_button.set_sensitive(false);
        state.refresh_button.set_sensitive(false);
        state.meta.set_label("Running…");
        state.inline_error.set_reveal_child(false);
    }
    let started = Instant::now();
    let sql_for_callback = sql.clone();
    let engine_for_history = engine.clone();
    let ui_for_callback = ui.clone();
    bridge::spawn(
        async move {
            let session = engine.session(profile_id).await?;
            let response = session.run_query(&sql, Some(format::QUERY_ROW_LIMIT)).await;
            let duration_ms = response.as_ref().map_or_else(
                |_| started.elapsed().as_millis(),
                |response| response.duration_ms,
            );
            let entry = QueryHistoryEntry {
                id: Uuid::new_v4(),
                profile_id,
                database,
                sql: sql.clone(),
                executed_at: Utc::now(),
                duration_ms,
                success: response.is_ok(),
            };
            engine.store.add_history(&entry)?;
            response
        },
        move |result| {
            let mut succeeded = false;
            {
                let mut state = state.borrow_mut();
                state.running = false;
                state.run_button.set_sensitive(true);
                match result {
                    Ok(response) => {
                        succeeded = true;
                        state.executed_sql = Some(sql_for_callback.clone());
                        state.refresh_button.set_sensitive(true);
                        apply_response(&mut state, &response);
                    }
                    Err(error) => {
                        let message = format::error_message(&error);
                        state.meta.set_label("Statement failed.");
                        state.inline_error_label.set_label(&message);
                        state.inline_error.set_reveal_child(true);
                        state.grid.clear();
                    }
                }
            }
            // `SELECT * FROM table` opens the full table view, matching the
            // Tauri workbench, so the result is browsable (and filterable).
            if let (true, Some((schema, table))) = (succeeded, target_table) {
                ui_for_callback
                    .borrow_mut()
                    .open_table(profile_id, schema, table);
            }
            refresh_history(state.clone(), engine_for_history.clone());
        },
    );
}

fn apply_response(state: &mut QueryState, response: &QueryResponse) {
    let columns: Vec<(String, i32)> = response
        .columns
        .iter()
        .map(|column| (column.name.clone(), default_column_width(&column.data_type)))
        .collect();
    state.grid.set_columns(&columns);
    state.grid.set_rows(&response.rows);
    let mut meta = format!("{} rows", response.row_count);
    if let Some(affected) = response.affected_rows {
        meta.push_str(&format!(" · {affected} affected"));
    }
    meta.push_str(&format!(" · {} ms", response.duration_ms));
    if response.truncated {
        meta.push_str(" · truncated");
    }
    state.meta.set_label(&meta);
}

fn refresh_history(state: Rc<RefCell<QueryState>>, engine: Arc<AppState>) {
    let (profile_id, database) = {
        let state = state.borrow();
        (state.profile_id, state.database.clone())
    };
    bridge::spawn(
        async move {
            Ok(engine
                .store
                .list_history(profile_id, &database, 100)?
                .into_iter()
                .collect::<Vec<_>>())
        },
        move |result| {
            let Ok(history) = result else {
                return;
            };
            let state = state.borrow();
            while let Some(child) = state.history_list.first_child() {
                state.history_list.remove(&child);
            }
            if history.is_empty() {
                let empty = gtk::Label::new(Some("Run a query to start history."));
                empty.add_css_class("muted");
                empty.set_xalign(0.0);
                empty.set_margin_start(8);
                state.history_list.append(&empty);
                return;
            }
            state.history_sql.borrow_mut().clear();
            for entry in history {
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_markup(&format!(
                    "{} <span foreground=\"#7c8ea6\">{}</span>",
                    glib::markup_escape_text(&history_label(&entry)),
                    glib::markup_escape_text(&format_time(&entry))
                ));
                let row = gtk::ListBoxRow::new();
                row.add_css_class("history-item");
                row.set_child(Some(&label));
                row.set_tooltip_text(Some(&entry.sql));
                state.history_sql.borrow_mut().push(entry.sql.clone());
                state.history_list.append(&row);
            }
        },
    );
}

fn history_label(entry: &QueryHistoryEntry) -> String {
    let compact = entry.sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let prefix: String = compact.chars().take(70).collect();
    let truncated = compact.chars().count() > 70;
    format!(
        "{} {}{}",
        if entry.success { "✓" } else { "!" },
        prefix,
        if truncated { "…" } else { "" }
    )
}

fn format_time(entry: &QueryHistoryEntry) -> String {
    entry.executed_at.format("%H:%M:%S").to_string()
}
