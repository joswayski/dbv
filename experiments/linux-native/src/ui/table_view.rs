//! Paginated table browsing: filters, ordering, pagination, CSV copy/export.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dbm_engine::error::AppError;
use dbm_engine::models::{FilterCondition, FilterOperator, OrderSpec, TablePage, TablePageRequest};
use dbm_engine::state::AppState;
use gtk4 as gtk;
use gtk4::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use crate::bridge;
use crate::format;
use crate::ui::app::Ui;
use crate::ui::dialogs;
use crate::ui::grid::{default_column_width, DataGrid};

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
    filter_popover: gtk::Popover,
    filter_list: gtk::Box,
    order_dropdown: gtk::DropDown,
    order_descending: gtk::ToggleButton,
    page: Option<TablePage>,
}

pub fn build(
    ui: Rc<RefCell<Ui>>,
    engine: Arc<AppState>,
    profile_id: Uuid,
    schema: String,
    table: String,
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
    let filter_button = gtk::Button::with_label("Filters");
    filter_button.add_css_class("secondary-button");

    let filter_popover = gtk::Popover::new();
    filter_popover.set_parent(&filter_button);
    filter_popover.set_has_arrow(true);
    filter_popover.add_css_class("filter-popover");

    let filter_list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let add_filter = gtk::Button::with_label("Add filter");
    add_filter.add_css_class("link-button");
    add_filter.set_halign(gtk::Align::Start);
    let apply_filters = gtk::Button::with_label("Apply");
    apply_filters.add_css_class("primary-button");
    let clear_filters = gtk::Button::with_label("Clear");
    clear_filters.add_css_class("secondary-button");
    let filter_actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    filter_actions.set_halign(gtk::Align::End);
    filter_actions.append(&clear_filters);
    filter_actions.append(&apply_filters);
    let filter_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    filter_content.set_width_request(420);
    filter_content.append(&filter_list);
    filter_content.append(&add_filter);
    filter_content.append(&filter_actions);
    filter_popover.set_child(Some(&filter_content));

    let order_dropdown = gtk::DropDown::from_strings(&["Default order"]);
    order_dropdown.set_tooltip_text(Some("Order rows by a column"));
    let order_descending = gtk::ToggleButton::with_label("↓");
    order_descending.add_css_class("secondary-button");
    order_descending.set_tooltip_text(Some("Toggle descending order"));
    let order_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    order_box.append(&gtk::Label::new(Some("Order")));
    order_box.append(&order_dropdown);
    order_box.append(&order_descending);

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.add_css_class("view-toolbar");
    toolbar.append(&title_box);
    let toolbar_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toolbar_spacer.set_hexpand(true);
    toolbar.append(&toolbar_spacer);
    toolbar.append(&order_box);
    toolbar.append(&filter_button);
    toolbar.append(&copy_button);
    toolbar.append(&export_button);
    toolbar.append(&refresh_button);

    let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    status_row.set_margin_start(14);
    status_row.set_margin_end(14);
    status_row.set_margin_top(6);
    status_row.append(&status);

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
    root.append(&toolbar);
    root.append(&status_row);
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
        filter_popover: filter_popover.clone(),
        filter_list: filter_list.clone(),
        order_dropdown: order_dropdown.clone(),
        order_descending: order_descending.clone(),
        page: None,
    }));

    // Refresh
    {
        let state = state.clone();
        let ui = ui.clone();
        let engine = engine.clone();
        refresh_button.connect_clicked(move |_| load(state.clone(), ui.clone(), engine.clone()));
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
    // Filter popover
    {
        let popover = filter_popover.clone();
        filter_button.connect_clicked(move |_| popover.popup());
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
            }
            state.borrow().filter_popover.popdown();
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
            state.borrow().filter_popover.popdown();
            load(state.clone(), ui.clone(), engine.clone());
        });
    }
    // Copy and export
    {
        let state = state.clone();
        copy_button.connect_clicked(move |_| {
            let state = state.borrow();
            let Some(page) = &state.page else {
                return;
            };
            let document = format::csv_document(&page.columns, &page.rows);
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&document);
            }
        });
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
        if let Some(position) = columns.iter().position(|(name, _)| name == &draft.column) {
            column_dropdown.set_selected(position as u32);
        }
        column_dropdown.set_tooltip_text(Some("Column"));
        let operator_labels: Vec<&str> = FILTER_OPERATORS.iter().map(|(_, label)| *label).collect();
        let operator_dropdown = gtk::DropDown::from_strings(&operator_labels);
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
            state.refresh_button.set_sensitive(true);
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
    let columns: Vec<(String, i32)> = page
        .metadata
        .columns
        .iter()
        .map(|column| (column.name.clone(), default_column_width(&column.data_type)))
        .collect();
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

    state.grid.set_columns(&columns);
    state.grid.set_rows(&page.rows);

    let total = page.total_rows.map_or_else(
        || format!("{} rows on this page", page.rows.len()),
        |total| format!("{total} rows"),
    );
    state.status.set_label(&format!(
        "{total} · showing rows {}–{}",
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
    state.page = Some(page);
    rebuild_filter_rows_locked(state, this);
}

fn export_csv(state: Rc<RefCell<TableViewState>>, ui: Rc<RefCell<Ui>>, engine: Arc<AppState>) {
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
                            .columns
                            .iter()
                            .map(|name| Value::String(name.clone()))
                            .collect::<Vec<_>>(),
                    ));
                }
                for row in &page.rows {
                    document.push('\n');
                    document.push_str(&format::csv_line(row));
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
