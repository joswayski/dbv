//! Paginated table browsing: filters, ordering, pagination, CSV copy/export.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dbm_engine::error::AppError;
use dbm_engine::models::{
    FilterCondition, FilterOperator, MutationBatch, OrderSpec, TablePage, TablePageRequest,
};
use dbm_engine::state::AppState;
use gtk4 as gtk;
use gtk4::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use crate::bridge;
use crate::ui::app::Ui;
use crate::ui::dialogs;
use crate::ui::grid::{default_column_width, DataGrid, GridEvent, GridRow};
use dbm_workbench::format;
use dbm_workbench::pending_edits::PendingEdits;

const FILTER_OPERATORS: [(FilterOperator, &str); 13] = [
    (FilterOperator::Equals, "Equals"),
    (FilterOperator::NotEquals, "Does not equal"),
    (FilterOperator::Contains, "Contains"),
    (FilterOperator::StartsWith, "Starts with"),
    (FilterOperator::EndsWith, "Ends with"),
    (FilterOperator::GreaterThan, "Greater than"),
    (FilterOperator::GreaterThanOrEqual, "Greater than or equal"),
    (FilterOperator::LessThan, "Less than"),
    (FilterOperator::LessThanOrEqual, "Less than or equal"),
    (FilterOperator::In, "In list"),
    (FilterOperator::NotIn, "Not in list"),
    (FilterOperator::IsNull, "Is null"),
    (FilterOperator::IsNotNull, "Is not null"),
];

#[derive(Clone)]
struct FilterDraft {
    column: String,
    operator: FilterOperator,
    value: String,
}

struct TableViewState {
    profile_id: Uuid,
    schema: String,
    table: String,
    page_index: u32,
    limit: u32,
    filters: Vec<FilterCondition>,
    order_by: Option<OrderSpec>,
    drafts: Vec<FilterDraft>,
    filters_initialized: bool,
    columns: Vec<(String, String)>,
    grid: DataGrid,
    status: gtk::Label,
    page_label: gtk::Label,
    previous_button: gtk::Button,
    next_button: gtk::Button,
    refresh_button: gtk::Button,
    copy_button: gtk::Button,
    export_button: gtk::Button,
    limit_input: gtk::SpinButton,
    filter_list: gtk::Box,
    order_dropdown: gtk::DropDown,
    order_descending: gtk::ToggleButton,
    page: Option<TablePage>,
    read_only: bool,
    pending: PendingEdits,
    loading: bool,
    saving: bool,
    pending_bar: gtk::Revealer,
    pending_label: gtk::Label,
    save_button: gtk::Button,
    discard_button: gtk::Button,
    delete_button: gtk::Button,
    copy_selected_button: gtk::Button,
    selection_label: gtk::Label,
    filter_panel: gtk::Box,
    preview: Option<gtk::Popover>,
    preview_generation: u64,
}

pub fn control_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("control-label");
    label.set_xalign(0.0);
    label
}

pub fn build(
    ui: Rc<RefCell<Ui>>,
    engine: Arc<AppState>,
    profile_id: Uuid,
    schema: String,
    table: String,
    read_only: bool,
) -> gtk::Widget {
    let grid = DataGrid::new();

    let title = gtk::Label::new(Some(&format!("{schema}.{table}")));
    title.add_css_class("title");
    title.set_xalign(0.0);
    let eyebrow = gtk::Label::new(Some("TABLE VIEWER"));
    eyebrow.add_css_class("eyebrow");
    eyebrow.set_xalign(0.0);
    let title_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    title_box.append(&eyebrow);
    title_box.append(&title);

    let status = gtk::Label::new(Some("Loading…"));
    status.add_css_class("muted");
    status.set_xalign(0.0);
    status.set_hexpand(true);

    let refresh_button = gtk::Button::with_label("Refresh");
    refresh_button.add_css_class("secondary-button");
    let copy_button = gtk::Button::with_label("Copy CSV");
    copy_button.add_css_class("secondary-button");
    let export_button = gtk::Button::with_label("Export CSV");
    export_button.add_css_class("secondary-button");
    let filter_list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let add_filter = gtk::Button::with_label("+ Add filter");
    add_filter.add_css_class("link-button");
    add_filter.set_halign(gtk::Align::Start);
    let apply_filters = gtk::Button::with_label("Apply filters");
    apply_filters.add_css_class("primary-button");
    apply_filters.add_css_class("apply-filters-button");
    let clear_filters = gtk::Button::with_label("Clear");
    clear_filters.add_css_class("secondary-button");

    let order_dropdown = gtk::DropDown::from_strings(&["Default order"]);
    order_dropdown.set_tooltip_text(Some("Order rows by a column"));
    order_dropdown.add_css_class("sort-column-select");
    let order_descending = gtk::ToggleButton::with_label("↓");
    order_descending.add_css_class("secondary-button");
    order_descending.set_tooltip_text(Some("Toggle descending order"));
    let limit_input = gtk::SpinButton::with_range(1.0, 10_000.0, 50.0);
    limit_input.set_value(f64::from(format::MAX_PREVIEW_ROWS));
    limit_input.set_tooltip_text(Some("Rows fetched per page"));
    let limit_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    limit_box.append(&control_label("Preview limit"));
    limit_box.append(&limit_input);
    let sort_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    sort_box.append(&control_label("Sort by"));
    sort_box.append(&order_dropdown);
    let direction_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    direction_box.append(&control_label("Direction"));
    direction_box.append(&order_descending);
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.add_css_class("table-query-controls");
    controls.append(&limit_box);
    controls.append(&sort_box);
    controls.append(&direction_box);
    let filter_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    filter_spacer.set_hexpand(true);
    controls.append(&filter_spacer);
    clear_filters.set_valign(gtk::Align::End);
    apply_filters.set_valign(gtk::Align::End);
    controls.append(&clear_filters);
    controls.append(&apply_filters);
    let filter_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    filter_header.add_css_class("filter-panel-header");
    let filter_title = gtk::Label::new(Some("Filters"));
    filter_title.add_css_class("filter-title");
    let filter_join = gtk::Label::new(Some("All filters must match"));
    filter_join.add_css_class("filter-join");
    filter_header.append(&filter_title);
    filter_header.append(&filter_join);
    filter_header.append(&add_filter);
    let filter_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    filter_panel.add_css_class("filter-panel");
    filter_panel.append(&filter_header);
    filter_panel.append(&filter_list);
    filter_panel.append(&controls);

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.add_css_class("view-toolbar");
    toolbar.append(&title_box);
    let toolbar_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toolbar_spacer.set_hexpand(true);
    toolbar.append(&toolbar_spacer);
    toolbar.append(&copy_button);
    toolbar.append(&export_button);
    toolbar.append(&refresh_button);

    let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    // Selection actions must not move rows between the clicks of a double-click.
    status_row.set_size_request(-1, 36);
    status_row.set_margin_bottom(8);
    status_row.append(&status);
    let selection_label = gtk::Label::new(None);
    selection_label.add_css_class("muted");
    let copy_selected_button = gtk::Button::with_label("Copy selected");
    copy_selected_button.add_css_class("secondary-button");
    copy_selected_button.set_visible(false);
    let delete_button = gtk::Button::with_label("Delete selected");
    delete_button.add_css_class("danger-button");
    delete_button.set_visible(false);
    status_row.append(&selection_label);
    status_row.append(&copy_selected_button);
    status_row.append(&delete_button);

    let pending_label = gtk::Label::new(None);
    pending_label.set_xalign(0.0);
    pending_label.set_hexpand(true);
    let save_button = gtk::Button::with_label("Save changes");
    save_button.add_css_class("primary-button");
    let discard_button = gtk::Button::with_label("Discard all");
    discard_button.add_css_class("secondary-button");
    let pending_content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    pending_content.add_css_class("pending-changes");
    pending_content.append(&pending_label);
    pending_content.append(&discard_button);
    pending_content.append(&save_button);
    let pending_bar = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(120)
        .child(&pending_content)
        .build();

    let previous_button = gtk::Button::with_label("← Previous");
    previous_button.add_css_class("secondary-button");
    previous_button.set_visible(false);
    let next_button = gtk::Button::with_label("Next →");
    next_button.add_css_class("secondary-button");
    next_button.set_visible(false);
    let page_label = gtk::Label::new(Some("Page 1"));
    page_label.add_css_class("muted");
    let pagination = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    pagination.add_css_class("pagination");
    pagination.set_halign(gtk::Align::Center);
    pagination.append(&previous_button);
    pagination.append(&page_label);
    pagination.append(&next_button);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("workbench-view");
    root.append(&toolbar);
    root.append(&filter_panel);
    root.append(&status_row);
    root.append(&pending_bar);
    root.append(&grid.root);
    root.append(&pagination);

    let state = Rc::new(RefCell::new(TableViewState {
        profile_id,
        schema,
        table,
        page_index: 0,
        limit: format::MAX_PREVIEW_ROWS,
        filters: Vec::new(),
        order_by: None,
        drafts: Vec::new(),
        filters_initialized: false,
        columns: Vec::new(),
        grid,
        status: status.clone(),
        page_label,
        previous_button: previous_button.clone(),
        next_button: next_button.clone(),
        refresh_button: refresh_button.clone(),
        copy_button: copy_button.clone(),
        export_button: export_button.clone(),
        limit_input: limit_input.clone(),
        filter_list: filter_list.clone(),
        order_dropdown: order_dropdown.clone(),
        order_descending: order_descending.clone(),
        page: None,
        read_only,
        pending: PendingEdits::default(),
        loading: false,
        saving: false,
        pending_bar,
        pending_label,
        save_button: save_button.clone(),
        discard_button: discard_button.clone(),
        delete_button: delete_button.clone(),
        copy_selected_button: copy_selected_button.clone(),
        selection_label,
        filter_panel,
        preview: None,
        preview_generation: 0,
    }));

    {
        let weak = Rc::downgrade(&state);
        let ui = ui.clone();
        state.borrow().grid.connect_event(move |event| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            match event {
                GridEvent::Edited { row, column, text } => {
                    let mut current = state.borrow_mut();
                    if current.read_only || current.saving {
                        return;
                    }
                    let Some(page) = current.page.as_ref() else {
                        return;
                    };
                    let metadata = page.metadata.clone();
                    if let Err(error) = current.pending.set_cell(&metadata, &row, column, &text) {
                        drop(current);
                        ui.borrow_mut().show_error(&error);
                        return;
                    }
                    render_drafts(&mut current);
                }
                GridEvent::SelectionChanged => update_actions(&state.borrow()),
                GridEvent::PreviewLeft => defer_close_preview(&state),
                GridEvent::DeleteSelection => toggle_delete(&state, &ui),
                GridEvent::Preview {
                    row,
                    column,
                    anchor,
                } => show_preview(&state, row, column, anchor),
            }
        });
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        delete_button.connect_clicked(move |_| toggle_delete(&state, &ui));
    }
    {
        let state = state.clone();
        discard_button.connect_clicked(move |_| {
            let grid = state.borrow().grid.clone();
            grid.commit_edit();
            let mut state = state.borrow_mut();
            if state.saving {
                return;
            }
            state.pending.clear();
            render_drafts(&mut state);
        });
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        save_button
            .connect_clicked(move |_| save_changes(state.clone(), ui.clone(), engine.clone()));
    }
    // Refresh
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        refresh_button.connect_clicked(move |_| {
            let grid = state.borrow().grid.clone();
            grid.commit_edit();
            if !state.borrow().pending.is_empty() {
                return;
            }
            load(state.clone(), ui.clone(), engine.clone());
        });
    }
    // Pagination
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        previous_button.connect_clicked(move |_| {
            {
                let mut state = state.borrow_mut();
                state.page_index = state.page_index.saturating_sub(1);
            }
            load(state.clone(), ui.clone(), engine.clone());
        });
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        next_button.connect_clicked(move |_| {
            {
                let mut state = state.borrow_mut();
                state.page_index += 1;
            }
            load(state.clone(), ui.clone(), engine.clone());
        });
    }
    // Ordering
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        order_dropdown.connect_selected_notify(move |_| {
            update_order(state.clone(), ui.clone(), engine.clone());
        });
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        order_descending.connect_toggled(move |_| {
            update_order(state.clone(), ui.clone(), engine.clone());
        });
    }
    {
        let state = state.clone();
        add_filter.connect_clicked(move |_| {
            let column = {
                let state = state.borrow();
                state
                    .columns
                    .first()
                    .map(|(name, _)| name.clone())
                    .unwrap_or_default()
            };
            if column.is_empty() {
                return;
            }
            state.borrow_mut().drafts.push(FilterDraft {
                column,
                operator: FilterOperator::Contains,
                value: String::new(),
            });
            rebuild_filter_rows(&state);
        });
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        apply_filters.connect_clicked(move |_| {
            let filters = {
                let state = state.borrow();
                state
                    .drafts
                    .iter()
                    .filter(|draft| {
                        draft_needs_value(&draft.operator) && !draft.value.trim().is_empty()
                            || !draft_needs_value(&draft.operator)
                    })
                    .map(|draft| FilterCondition {
                        column: draft.column.clone(),
                        operator: draft.operator.clone(),
                        value: if draft_needs_value(&draft.operator) {
                            Some(draft.value.trim().to_owned())
                        } else {
                            None
                        },
                    })
                    .collect::<Vec<_>>()
            };
            {
                let mut state = state.borrow_mut();
                state.filters = filters;
                state.page_index = 0;
                state.limit = state.limit_input.value() as u32;
            }
            load(state.clone(), ui.clone(), engine.clone());
        });
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        clear_filters.connect_clicked(move |_| {
            {
                let mut state = state.borrow_mut();
                state.filters.clear();
                state.drafts.clear();
                state.page_index = 0;
            }
            rebuild_filter_rows(&state);
            load(state.clone(), ui.clone(), engine.clone());
        });
    }
    // Copy and export
    for (button, selected_only) in [(copy_button, false), (copy_selected_button, true)] {
        let state = state.clone();
        button.connect_clicked(move |_| copy_rows(&state, selected_only));
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        export_button
            .connect_clicked(move |_| export_csv(state.clone(), ui.clone(), engine.clone()));
    }

    load(state, ui, engine);
    root.upcast()
}

fn close_preview(state: &mut TableViewState) {
    state.preview_generation += 1;
    if let Some(preview) = state.preview.take() {
        preview.popdown();
        preview.unparent();
    }
}

fn defer_close_preview(state: &Rc<RefCell<TableViewState>>) {
    let generation = {
        let mut state = state.borrow_mut();
        state.preview_generation += 1;
        state.preview_generation
    };
    let weak = Rc::downgrade(state);
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(180), move || {
        let Some(state) = weak.upgrade() else {
            return;
        };
        let mut state = state.borrow_mut();
        if state.preview_generation == generation {
            close_preview(&mut state);
        }
    });
}

fn render_drafts(state: &mut TableViewState) {
    close_preview(state);
    let Some(page) = &state.page else {
        return;
    };
    let editable: Vec<_> = page
        .metadata
        .columns
        .iter()
        .map(|column| {
            !state.read_only
                && !page.metadata.primary_key.is_empty()
                && !page.metadata.primary_key.contains(&column.name)
        })
        .collect();
    state.grid.set_table_rows(
        page.rows
            .iter()
            .map(|row| {
                let pending = state.pending.get(&page.metadata, row);
                let values = state.pending.values(&page.metadata, row);
                GridRow {
                    values: values
                        .iter()
                        .map(|value| (!value.is_null()).then(|| format::display_value(value)))
                        .collect(),
                    source: row.clone(),
                    editable: editable.clone(),
                    changed: pending.map_or_else(Vec::new, |pending| {
                        pending
                            .original
                            .iter()
                            .zip(&pending.changes)
                            .map(|(old, new)| old != new)
                            .collect()
                    }),
                    deleted: pending.is_some_and(|pending| pending.deleted),
                    staged: pending.is_some(),
                }
            })
            .collect(),
    );
    update_actions(state);
}

fn update_actions(state: &TableViewState) {
    let idle = !state.loading && !state.saving;
    let has_pending = !state.pending.is_empty();
    state.pending_bar.set_reveal_child(has_pending);
    state.pending_label.set_label(&format!(
        "{} pending · {} edited · {} deleted — not saved yet",
        state.pending.len(),
        state.pending.len() - state.pending.delete_count(),
        state.pending.delete_count()
    ));
    state.save_button.set_label(if state.saving {
        "Saving…"
    } else {
        "Save changes"
    });
    state
        .save_button
        .set_sensitive(idle && has_pending && !state.read_only);
    state.discard_button.set_sensitive(idle);
    state.refresh_button.set_sensitive(idle && !has_pending);
    state.refresh_button.set_tooltip_text(
        has_pending.then_some("Save or discard pending changes before refreshing"),
    );
    state
        .export_button
        .set_sensitive(idle && !has_pending && state.page.is_some());
    state.export_button.set_tooltip_text(
        has_pending.then_some("Save or discard pending changes before exporting all rows"),
    );
    state.filter_panel.set_sensitive(idle);
    state.previous_button.set_sensitive(idle);
    state.next_button.set_sensitive(idle);
    state
        .copy_button
        .set_sensitive(idle && state.page.is_some());
    let selected = state.grid.selected_rows();
    state.selection_label.set_label(&if selected.is_empty() {
        String::new()
    } else {
        format!("{} selected", selected.len())
    });
    state.copy_selected_button.set_visible(!selected.is_empty());
    state.copy_selected_button.set_sensitive(idle);
    if let Some(page) = &state.page {
        let editable = !state.read_only && !page.metadata.primary_key.is_empty();
        state
            .delete_button
            .set_visible(editable && !selected.is_empty());
        state.delete_button.set_sensitive(idle);
        let all_deleted = !selected.is_empty()
            && selected.iter().all(|index| {
                page.rows
                    .get(*index)
                    .and_then(|row| state.pending.get(&page.metadata, row))
                    .is_some_and(|draft| draft.deleted)
            });
        state.delete_button.set_label(if all_deleted {
            "Undo delete"
        } else {
            "Delete selected"
        });
        let copyable = page
            .rows
            .iter()
            .filter(|row| {
                !state
                    .pending
                    .get(&page.metadata, row)
                    .is_some_and(|pending| pending.deleted)
            })
            .count();
        state
            .copy_button
            .set_label(&format!("Copy visible ({copyable})"));
    }
}

fn toggle_delete(state: &Rc<RefCell<TableViewState>>, ui: &Rc<RefCell<Ui>>) {
    let grid = state.borrow().grid.clone();
    grid.commit_edit();
    let mut state = state.borrow_mut();
    if state.read_only || state.loading || state.saving {
        return;
    }
    let Some(page) = &state.page else {
        return;
    };
    let metadata = page.metadata.clone();
    let rows: Vec<_> = state
        .grid
        .selected_rows()
        .iter()
        .filter_map(|index| page.rows.get(*index).cloned())
        .collect();
    if let Err(error) = state.pending.toggle_delete(&metadata, &rows) {
        drop(state);
        ui.borrow_mut().show_error(&error);
        return;
    }
    render_drafts(&mut state);
}

fn copy_rows(state: &Rc<RefCell<TableViewState>>, selected_only: bool) {
    let grid = state.borrow().grid.clone();
    grid.commit_edit();
    let state = state.borrow();
    let Some(page) = &state.page else {
        return;
    };
    let selected = state.grid.selected_rows();
    let rows: Vec<_> = page
        .rows
        .iter()
        .enumerate()
        .filter(|(index, row)| {
            (!selected_only || selected.contains(index))
                && !state
                    .pending
                    .get(&page.metadata, row)
                    .is_some_and(|pending| pending.deleted)
        })
        .map(|(_, row)| state.pending.values(&page.metadata, row))
        .collect();
    if let Some(display) = gtk::gdk::Display::default() {
        let columns = page
            .metadata
            .columns
            .iter()
            .map(|column| column.name.clone())
            .collect::<Vec<_>>();
        display
            .clipboard()
            .set_text(&format::csv_document(&columns, &rows));
    }
}

fn save_changes(state: Rc<RefCell<TableViewState>>, ui: Rc<RefCell<Ui>>, engine: Arc<AppState>) {
    let grid = state.borrow().grid.clone();
    grid.commit_edit();
    let batch = {
        let mut state = state.borrow_mut();
        if state.read_only || state.loading || state.saving || state.pending.is_empty() {
            return;
        }
        state.saving = true;
        close_preview(&mut state);
        update_actions(&state);
        state.grid.set_loading(true);
        MutationBatch {
            profile_id: state.profile_id,
            schema: state.schema.clone(),
            table: state.table.clone(),
            mutations: state.pending.mutations(),
        }
    };
    let reload_engine = engine.clone();
    bridge::spawn(
        async move {
            engine
                .session(batch.profile_id)
                .await?
                .apply_mutations(&batch)
                .await
        },
        move |result| {
            {
                let mut current = state.borrow_mut();
                current.saving = false;
                current.grid.set_loading(false);
                update_actions(&current);
            }
            match result {
                Ok(result) => {
                    state.borrow_mut().pending.clear();
                    state.borrow().grid.clear_selection();
                    load(state, ui.clone(), reload_engine);
                    if result.conflicts.is_empty() {
                        ui.borrow_mut()
                            .show_toast(&format!("{} change(s) saved.", result.applied));
                    } else {
                        ui.borrow_mut().show_error(&format!(
                            "{} row conflict(s); the table was refreshed.",
                            result.conflicts.len()
                        ));
                    }
                }
                Err(error) => ui.borrow_mut().show_error(&format::error_message(&error)),
            }
        },
    );
}

fn show_preview(
    state: &Rc<RefCell<TableViewState>>,
    row: Vec<Value>,
    column: usize,
    anchor: gtk::Widget,
) {
    let mut current = state.borrow_mut();
    close_preview(&mut current);
    if current.loading || current.saving || current.grid.is_editing() || !anchor.is_mapped() {
        return;
    }
    let Some(page) = &current.page else {
        return;
    };
    let Some(pending) = current.pending.get(&page.metadata, &row) else {
        return;
    };
    let Some(field) = page.metadata.columns.get(column) else {
        return;
    };
    if !pending.deleted && pending.original[column] == pending.changes[column] {
        return;
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.add_css_class("change-preview");
    let heading = gtk::Label::new(Some(&format!(
        "{} · {}",
        field.name,
        if pending.deleted {
            "PENDING DELETE"
        } else {
            "PENDING EDIT"
        }
    )));
    heading.add_css_class("eyebrow");
    heading.set_xalign(0.0);
    content.append(&heading);
    if pending.deleted {
        let message = gtk::Label::new(Some("This row will be deleted when you save."));
        message.set_xalign(0.0);
        content.append(&message);
    } else {
        for (title, value, class) in [
            ("Before", &pending.original[column], "before-value"),
            ("After", &pending.changes[column], "after-value"),
        ] {
            content.append(&control_label(title));
            let value = gtk::Label::new(Some(&format::display_value(value)));
            value.add_css_class(class);
            value.set_xalign(0.0);
            value.set_wrap(true);
            value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            value.set_max_width_chars(48);
            content.append(&value);
        }
    }
    let undo = gtk::Button::with_label(if pending.deleted {
        "Undo delete"
    } else {
        "Discard edit"
    });
    undo.add_css_class("secondary-button");
    undo.set_halign(gtk::Align::Start);
    let weak = Rc::downgrade(state);
    undo.connect_clicked(move |_| {
        let Some(state) = weak.upgrade() else {
            return;
        };
        let mut state = state.borrow_mut();
        if state.saving {
            return;
        }
        if let Some(page) = &state.page {
            let metadata = page.metadata.clone();
            state.pending.discard_row(&metadata, &row);
            render_drafts(&mut state);
        }
    });
    content.append(&undo);
    let preview = gtk::Popover::builder()
        .child(&content)
        .autohide(false)
        .has_arrow(false)
        .position(gtk::PositionType::Bottom)
        .build();
    preview.add_css_class("change-popover");
    // Keep the popover's shadow/input surface below the hovered cell; otherwise
    // it can cause a leave event as soon as the preview opens on X11.
    preview.set_offset(0, 24);
    let hover = gtk::EventControllerMotion::new();
    {
        let weak = Rc::downgrade(state);
        hover.connect_enter(move |_, _, _| {
            let weak = weak.clone();
            gtk::glib::idle_add_local_once(move || {
                if let Some(state) = weak.upgrade() {
                    state.borrow_mut().preview_generation += 1;
                }
            });
        });
    }
    {
        let weak = Rc::downgrade(state);
        hover.connect_leave(move |_| {
            let weak = weak.clone();
            gtk::glib::idle_add_local_once(move || {
                if let Some(state) = weak.upgrade() {
                    defer_close_preview(&state);
                }
            });
        });
    }
    preview.add_controller(hover);
    preview.set_parent(&anchor);
    preview.popup();
    current.preview = Some(preview);
}

fn draft_needs_value(operator: &FilterOperator) -> bool {
    !matches!(operator, FilterOperator::IsNull | FilterOperator::IsNotNull)
}

fn rebuild_filter_rows(state: &Rc<RefCell<TableViewState>>) {
    let mut borrowed = state.borrow_mut();
    rebuild_filter_rows_locked(&mut borrowed, state);
}

fn rebuild_filter_rows_locked(state: &mut TableViewState, this: &Rc<RefCell<TableViewState>>) {
    let drafts = state.drafts.clone();
    let columns = state.columns.clone();
    while let Some(child) = state.filter_list.first_child() {
        state.filter_list.remove(&child);
    }
    if drafts.is_empty() {
        let empty = gtk::Label::new(Some("No filters applied."));
        empty.add_css_class("muted");
        empty.set_xalign(0.0);
        state.filter_list.append(&empty);
        return;
    }
    for (index, draft) in drafts.iter().enumerate() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let column_names: Vec<&str> = columns.iter().map(|(name, _)| name.as_str()).collect();
        let column_dropdown = gtk::DropDown::from_strings(&column_names);
        column_dropdown.add_css_class("filter-column");
        if let Some(position) = columns.iter().position(|(name, _)| name == &draft.column) {
            column_dropdown.set_selected(position as u32);
        }
        column_dropdown.set_tooltip_text(Some("Column"));
        let operator_labels: Vec<&str> = FILTER_OPERATORS.iter().map(|(_, label)| *label).collect();
        let operator_dropdown = gtk::DropDown::from_strings(&operator_labels);
        operator_dropdown.add_css_class("filter-operator");
        if let Some(position) = FILTER_OPERATORS
            .iter()
            .position(|(operator, _)| operator == &draft.operator)
        {
            operator_dropdown.set_selected(position as u32);
        }
        operator_dropdown.set_tooltip_text(Some("Operator"));
        let value_entry = gtk::Entry::new();
        value_entry.set_text(&draft.value);
        value_entry.set_hexpand(true);
        value_entry.set_sensitive(draft_needs_value(&draft.operator));
        value_entry.set_placeholder_text(Some(
            if matches!(draft.operator, FilterOperator::In | FilterOperator::NotIn) {
                "a, b, c"
            } else {
                "value"
            },
        ));
        let remove = gtk::Button::with_label("×");
        remove.add_css_class("icon-button");
        remove.set_tooltip_text(Some("Remove filter"));

        {
            let state = this.clone();
            column_dropdown.connect_selected_notify(move |dropdown| {
                if let Some(name) = dropdown
                    .model()
                    .and_downcast::<gtk::StringList>()
                    .and_then(|model| model.string(dropdown.selected()))
                    .map(|name| name.to_string())
                {
                    state.borrow_mut().drafts[index].column = name;
                }
            });
        }
        {
            let state = this.clone();
            let value_entry = value_entry.clone();
            operator_dropdown.connect_selected_notify(move |dropdown| {
                let selected = dropdown.selected();
                let Some((operator, _)) = FILTER_OPERATORS.get(selected as usize) else {
                    return;
                };
                let operator = operator.clone();
                let needs_value = draft_needs_value(&operator);
                {
                    let mut state = state.borrow_mut();
                    if let Some(draft) = state.drafts.get_mut(index) {
                        draft.operator = operator;
                    }
                }
                value_entry.set_sensitive(needs_value);
            });
        }
        {
            let state = this.clone();
            value_entry.connect_changed(move |entry| {
                let mut state = state.borrow_mut();
                if let Some(draft) = state.drafts.get_mut(index) {
                    draft.value = entry.text().to_string();
                }
            });
        }
        {
            let state = this.clone();
            remove.connect_clicked(move |_| {
                {
                    let mut state = state.borrow_mut();
                    if index < state.drafts.len() {
                        state.drafts.remove(index);
                    }
                }
                rebuild_filter_rows(&state);
            });
        }

        row.append(&column_dropdown);
        row.append(&operator_dropdown);
        row.append(&value_entry);
        row.append(&remove);
        state.filter_list.append(&row);
    }
}

fn update_order(state: Rc<RefCell<TableViewState>>, ui: Rc<RefCell<Ui>>, engine: Arc<AppState>) {
    // The dropdown emits `notify::selected` while a page load is still
    // rebuilding its model, so skip those emissions instead of panicking.
    let Ok(current) = state.try_borrow() else {
        return;
    };
    let index = current.order_dropdown.selected();
    let order = if index == 0 {
        None
    } else {
        current
            .columns
            .get(index as usize - 1)
            .map(|(name, _)| OrderSpec {
                column: name.clone(),
                descending: current.order_descending.is_active(),
            })
    };
    if order == current.order_by {
        return;
    }
    drop(current);
    {
        let mut state = state.borrow_mut();
        state.order_by = order;
        state.page_index = 0;
    }
    load(state, ui, engine);
}

fn load(state: Rc<RefCell<TableViewState>>, ui: Rc<RefCell<Ui>>, engine: Arc<AppState>) {
    let grid = state.borrow().grid.clone();
    grid.commit_edit();
    {
        let mut state = state.borrow_mut();
        if state.loading || state.saving {
            return;
        }
        state.loading = true;
        close_preview(&mut state);
        state.grid.clear_selection();
        update_actions(&state);
    }
    let (profile_id, schema, table, offset, limit, filters, order_by) = {
        let state = state.borrow();
        (
            state.profile_id,
            state.schema.clone(),
            state.table.clone(),
            state.page_index * state.limit,
            state.limit,
            state.filters.clone(),
            state.order_by.clone(),
        )
    };
    state.borrow().status.set_label("Loading…");
    state.borrow().refresh_button.set_sensitive(false);
    state.borrow().grid.set_loading(true);
    bridge::spawn(
        async move {
            let session = engine.session(profile_id).await?;
            session
                .table_page(&TablePageRequest {
                    profile_id,
                    schema,
                    table,
                    offset,
                    limit,
                    filters,
                    order_by,
                    include_total: Some(true),
                })
                .await
        },
        move |result| {
            let this = state.clone();
            let mut state = this.borrow_mut();
            state.loading = false;
            state.grid.set_loading(false);
            update_actions(&state);
            match result {
                Ok(page) => apply_page(&mut state, &this, page),
                Err(error) => {
                    state.status.set_label("Load failed.");
                    drop(state);
                    ui.borrow_mut().show_error(&format::error_message(&error));
                }
            }
        },
    );
}

fn apply_page(state: &mut TableViewState, this: &Rc<RefCell<TableViewState>>, page: TablePage) {
    let columns: Vec<(String, String, i32)> = page
        .metadata
        .columns
        .iter()
        .map(|column| {
            (
                column.name.clone(),
                column.data_type.clone(),
                default_column_width(&column.data_type),
            )
        })
        .collect();
    let columns_changed = state.columns
        != columns
            .iter()
            .map(|(name, data_type, _)| (name.clone(), data_type.clone()))
            .collect::<Vec<_>>();
    state.columns = page
        .metadata
        .columns
        .iter()
        .map(|column| (column.name.clone(), column.data_type.clone()))
        .collect();

    if !state.filters_initialized {
        if let Some((name, _)) = state.columns.first() {
            state.drafts = vec![FilterDraft {
                column: name.clone(),
                operator: FilterOperator::Contains,
                value: String::new(),
            }];
        }
        state.filters_initialized = true;
    }

    let order_names: Vec<&str> = std::iter::once("Default order")
        .chain(state.columns.iter().map(|(name, _)| name.as_str()))
        .collect();
    state
        .order_dropdown
        .set_model(Some(&gtk::StringList::new(&order_names)));
    if let Some(order) = &state.order_by {
        if let Some(index) = state
            .columns
            .iter()
            .position(|(name, _)| name == &order.column)
        {
            state.order_dropdown.set_selected(index as u32 + 1);
        }
    }
    state.order_descending.set_active(
        state
            .order_by
            .as_ref()
            .is_some_and(|order| order.descending),
    );

    if columns_changed {
        state.grid.set_columns(&columns);
    }

    let total = page.total_rows.map_or_else(
        || format!("{} rows on this page", page.rows.len()),
        |total| format!("{total} rows"),
    );
    state.export_button.set_label(&page.total_rows.map_or_else(
        || "Export all".into(),
        |total| format!("Export all ({total})"),
    ));
    state.status.set_label(&format!(
        "{total} · {} columns · showing rows {}–{}",
        page.metadata.columns.len(),
        if page.rows.is_empty() {
            0
        } else {
            page.offset + 1
        },
        page.offset + page.rows.len() as u32
    ));
    state
        .page_label
        .set_label(&format!("Page {}", state.page_index + 1));
    state.previous_button.set_visible(state.page_index > 0);
    state.next_button.set_visible(page.has_more);
    state.grid.view.set_tooltip_text(if state.read_only {
        Some("Read-only connection")
    } else if page.metadata.primary_key.is_empty() {
        Some("This table has no primary key; rows cannot be edited or deleted")
    } else {
        None
    });
    state.page = Some(page);
    render_drafts(state);
    rebuild_filter_rows_locked(state, this);
}

fn export_csv(state: Rc<RefCell<TableViewState>>, ui: Rc<RefCell<Ui>>, engine: Arc<AppState>) {
    let grid = state.borrow().grid.clone();
    grid.commit_edit();
    if !state.borrow().pending.is_empty() {
        ui.borrow_mut()
            .show_error("Save or discard pending changes before exporting all rows.");
        return;
    }
    let total = state
        .borrow()
        .page
        .as_ref()
        .and_then(|page| page.total_rows);
    if let Some(total) = total.filter(|total| *total > format::LARGE_EXPORT_WARNING_ROWS) {
        let window = ui.borrow().window.clone();
        let confirm_state = state.clone();
        let confirm_ui = ui.clone();
        let confirm_engine = engine.clone();
        dialogs::confirm(
            &window,
            "Export large table",
            &format!("This export contains {total} rows and may take a while. Continue?"),
            "Export",
            false,
            move || choose_export_path(confirm_state, confirm_ui, confirm_engine),
        );
        return;
    }
    choose_export_path(state, ui, engine);
}

fn choose_export_path(
    state: Rc<RefCell<TableViewState>>,
    ui: Rc<RefCell<Ui>>,
    engine: Arc<AppState>,
) {
    let (schema, table, filters, order_by) = {
        let state = state.borrow();
        (
            state.schema.clone(),
            state.table.clone(),
            state.filters.clone(),
            state.order_by.clone(),
        )
    };
    let dialog = gtk::FileChooserNative::builder()
        .title("Export CSV")
        .action(gtk::FileChooserAction::Save)
        .accept_label("Export")
        .cancel_label("Cancel")
        .build();
    dialog.set_current_name(&format!(
        "{}.csv",
        format::safe_file_name(&format!("{schema}.{table}"))
    ));
    let profile_id = state.borrow().profile_id;
    {
        let state = state.clone();
        let ui = ui.clone();
        dialog.connect_response(move |dialog, response| {
            let path = if response == gtk::ResponseType::Accept {
                dialog.file().and_then(|file| file.path())
            } else {
                None
            };
            dialog.destroy();
            let Some(path) = path else {
                return;
            };
            export_to_path(
                state.clone(),
                ui.clone(),
                engine.clone(),
                profile_id,
                schema.clone(),
                table.clone(),
                filters.clone(),
                order_by.clone(),
                path,
            );
        });
    }
    dialog.show();
}

#[allow(clippy::too_many_arguments)]
fn export_to_path(
    state: Rc<RefCell<TableViewState>>,
    ui: Rc<RefCell<Ui>>,
    engine: Arc<AppState>,
    profile_id: Uuid,
    schema: String,
    table: String,
    filters: Vec<FilterCondition>,
    order_by: Option<OrderSpec>,
    path: std::path::PathBuf,
) {
    if !state.borrow().pending.is_empty() {
        ui.borrow_mut()
            .show_error("Save or discard pending changes before exporting all rows.");
        return;
    }
    state.borrow().status.set_label("Exporting…");
    bridge::spawn(
        async move {
            let session = engine.session(profile_id).await?;
            let mut offset = 0_u32;
            let mut document = String::new();
            let mut rows_written = 0_u64;
            loop {
                let page = session
                    .table_page(&TablePageRequest {
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
                            .map(|column| Value::String(column.name.clone()))
                            .collect::<Vec<_>>(),
                    ));
                }
                for row in &page.rows {
                    document.push('\n');
                    document.push_str(&format::csv_line(&row[..page.metadata.columns.len()]));
                }
                rows_written += page.rows.len() as u64;
                if !page.has_more {
                    break;
                }
                offset += format::EXPORT_PAGE_SIZE;
            }
            std::fs::write(&path, document)
                .map_err(|error| AppError::Storage(error.to_string()))?;
            Ok(rows_written)
        },
        move |result| {
            let state = state.borrow();
            match result {
                Ok(rows) => {
                    state.status.set_label(&format!("Exported {rows} rows."));
                    drop(state);
                    ui.borrow_mut()
                        .show_toast(&format!("Exported {rows} rows."));
                }
                Err(error) => {
                    state.status.set_label("Export failed.");
                    drop(state);
                    ui.borrow_mut().show_error(&format::error_message(&error));
                }
            }
        },
    );
}
