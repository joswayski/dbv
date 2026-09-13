//! A virtualized data grid built on `GtkColumnView`.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use serde_json::Value;

use dbm_workbench::format::display_value;

/// One grid row. `None` marks a SQL NULL so the cell can render it muted.
#[derive(Clone)]
pub struct GridRow {
    pub values: Vec<Option<String>>,
    pub source: Vec<Value>,
    pub editable: Vec<bool>,
    pub changed: Vec<bool>,
    pub deleted: bool,
    pub staged: bool,
}

pub enum GridEvent {
    Edited {
        row: Vec<Value>,
        column: usize,
        text: String,
    },
    Preview {
        row: Vec<Value>,
        column: usize,
        anchor: gtk::Widget,
    },
    PreviewLeft,
    SelectionChanged,
    DeleteSelection,
}

type EventHandler = Rc<RefCell<Option<Rc<dyn Fn(GridEvent)>>>>;

struct Editor {
    entry: gtk::Entry,
    stack: gtk::Stack,
    row: Vec<Value>,
    column: usize,
}

fn finish_edit(editor: &Rc<RefCell<Option<Editor>>>, events: &EventHandler, commit: bool) {
    let Some(editor) = editor.borrow_mut().take() else {
        return;
    };
    editor.stack.set_visible_child_name("value");
    if commit {
        let callback = events.borrow().clone();
        if let Some(callback) = callback {
            callback(GridEvent::Edited {
                row: editor.row,
                column: editor.column,
                text: editor.entry.text().to_string(),
            });
        }
    }
}

// Selection and hover signals can fire while the table is rebinding its model.
fn emit_later(events: &EventHandler, event: GridEvent) {
    let callback = events.borrow().clone();
    if let Some(callback) = callback {
        glib::idle_add_local_once(move || callback(event));
    }
}

#[derive(Clone)]
pub struct DataGrid {
    pub root: gtk::Overlay,
    pub view: gtk::ColumnView,
    store: gio::ListStore,
    loading: gtk::Revealer,
    selection: gtk::MultiSelection,
    events: EventHandler,
    editor: Rc<RefCell<Option<Editor>>>,
}

impl Default for DataGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl DataGrid {
    pub fn new() -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        let view = gtk::ColumnView::new(Some(selection.clone()));
        let events: EventHandler = Rc::new(RefCell::new(None));
        let editor = Rc::new(RefCell::new(None));
        {
            let events = events.clone();
            selection.connect_selection_changed(move |_, _, _| {
                emit_later(&events, GridEvent::SelectionChanged)
            });
        }
        let keys = gtk::EventControllerKey::new();
        {
            let events = events.clone();
            let editor = editor.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                if key == gtk::gdk::Key::Delete && editor.borrow().is_none() {
                    emit_later(&events, GridEvent::DeleteSelection);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        }
        view.add_controller(keys);
        view.add_css_class("data-grid");
        view.set_show_column_separators(false);
        view.set_show_row_separators(false);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();

        let pill = gtk::Label::new(Some("Loading…"));
        pill.add_css_class("grid-loading-pill");
        pill.set_halign(gtk::Align::Center);
        pill.set_valign(gtk::Align::Center);
        let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shade.add_css_class("grid-loading");
        pill.set_vexpand(true);
        shade.append(&pill);
        let loading = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .transition_duration(180)
            .child(&shade)
            .can_target(false)
            .build();
        let root = gtk::Overlay::builder()
            .child(&scroll)
            .hexpand(true)
            .vexpand(true)
            .overflow(gtk::Overflow::Hidden)
            .build();
        root.add_css_class("grid-frame");
        root.add_overlay(&loading);

        Self {
            root,
            view,
            store,
            loading,
            selection,
            events,
            editor,
        }
    }

    pub fn connect_event(&self, callback: impl Fn(GridEvent) + 'static) {
        *self.events.borrow_mut() = Some(Rc::new(callback));
    }

    pub fn commit_edit(&self) {
        finish_edit(&self.editor, &self.events, true);
    }

    pub fn is_editing(&self) -> bool {
        self.editor.borrow().is_some()
    }

    pub fn selected_rows(&self) -> Vec<usize> {
        (0..self.store.n_items())
            .filter(|index| self.selection.is_selected(*index))
            .map(|index| index as usize)
            .collect()
    }

    pub fn clear_selection(&self) {
        self.selection.unselect_all();
    }

    pub fn set_loading(&self, loading: bool) {
        self.loading.set_reveal_child(loading);
        self.view.set_sensitive(!loading);
    }

    /// Rebuilds the columns. Titles carry the data type on a second line, and
    /// widths mirror the React table's defaults.
    pub fn set_columns(&self, columns: &[(String, String, i32)]) {
        while let Some(column) = self.view.columns().item(0) {
            self.view
                .remove_column(&column.downcast::<gtk::ColumnViewColumn>().unwrap());
        }
        for (index, (name, data_type, width)) in columns.iter().enumerate() {
            let factory = gtk::SignalListItemFactory::new();
            let events = self.events.clone();
            let editor = self.editor.clone();
            factory.connect_setup(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.add_css_class("grid-cell");
                let entry = gtk::Entry::new();
                entry.add_css_class("cell-editor");
                entry.set_has_frame(false);
                let stack = gtk::Stack::new();
                stack.set_hhomogeneous(false);
                stack.set_vhomogeneous(false);
                stack.add_named(&label, Some("value"));
                stack.add_named(&entry, Some("editor"));
                stack.set_visible_child_name("value");
                item.set_child(Some(&stack));
                let click = gtk::GestureClick::new();
                click.set_button(1);
                {
                    let item = item.downgrade();
                    let stack = stack.clone();
                    let entry = entry.clone();
                    let editor = editor.clone();
                    let events = events.clone();
                    click.connect_pressed(move |click, count, _, _| {
                        if count != 2 {
                            return;
                        }
                        let Some(item) = item.upgrade() else {
                            return;
                        };
                        let Some(object) = item.item().and_downcast::<glib::BoxedAnyObject>()
                        else {
                            return;
                        };
                        let row = object.borrow::<GridRow>().clone();
                        if !row.editable.get(index).copied().unwrap_or(false) || row.deleted {
                            return;
                        }
                        finish_edit(&editor, &events, true);
                        entry.set_text(
                            row.values
                                .get(index)
                                .and_then(Option::as_deref)
                                .unwrap_or(""),
                        );
                        *editor.borrow_mut() = Some(Editor {
                            entry: entry.clone(),
                            stack: stack.clone(),
                            row: row.source,
                            column: index,
                        });
                        stack.set_visible_child_name("editor");
                        entry.grab_focus();
                        entry.select_region(0, -1);
                        click.set_state(gtk::EventSequenceState::Claimed);
                    });
                }
                stack.add_controller(click);
                {
                    let editor = editor.clone();
                    let events = events.clone();
                    entry.connect_activate(move |_| finish_edit(&editor, &events, true));
                }
                let focus = gtk::EventControllerFocus::new();
                {
                    let editor = editor.clone();
                    let events = events.clone();
                    focus.connect_leave(move |_| finish_edit(&editor, &events, true));
                }
                entry.add_controller(focus);
                let keys = gtk::EventControllerKey::new();
                {
                    let editor = editor.clone();
                    let events = events.clone();
                    keys.connect_key_pressed(move |_, key, _, _| {
                        if key == gtk::gdk::Key::Escape {
                            finish_edit(&editor, &events, false);
                            return glib::Propagation::Stop;
                        }
                        glib::Propagation::Proceed
                    });
                }
                entry.add_controller(keys);
                let hover = gtk::EventControllerMotion::new();
                {
                    let item = item.downgrade();
                    let events = events.clone();
                    hover.connect_enter(move |_, _, _| {
                        let Some(item) = item.upgrade() else {
                            return;
                        };
                        let Some(object) = item.item().and_downcast::<glib::BoxedAnyObject>()
                        else {
                            return;
                        };
                        let row = object.borrow::<GridRow>();
                        if let Some(anchor) = item.child() {
                            emit_later(
                                &events,
                                GridEvent::Preview {
                                    row: row.source.clone(),
                                    column: index,
                                    anchor,
                                },
                            );
                        }
                    });
                }
                {
                    let events = events.clone();
                    hover.connect_leave(move |_| emit_later(&events, GridEvent::PreviewLeft));
                }
                stack.add_controller(hover);
            });
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let stack = item.child().and_downcast::<gtk::Stack>().expect("stack");
                let label = stack
                    .child_by_name("value")
                    .and_downcast::<gtk::Label>()
                    .expect("label");
                let Some(row) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let row = row.borrow::<GridRow>();
                // Style the ColumnView cell itself so the tint fills the row.
                if let Some(cell) = stack.parent() {
                    for (class, active) in [
                        (
                            "changed-cell",
                            row.changed.get(index).copied().unwrap_or(false),
                        ),
                        ("deleted-cell", row.deleted),
                        ("staged-cell", row.staged),
                    ] {
                        if active {
                            cell.add_css_class(class);
                        } else {
                            cell.remove_css_class(class);
                        }
                    }
                }
                label.set_attributes(
                    if row.deleted {
                        let attributes = gtk::pango::AttrList::new();
                        attributes.insert(gtk::pango::AttrInt::new_strikethrough(true));
                        Some(attributes)
                    } else {
                        None
                    }
                    .as_ref(),
                );
                match row.values.get(index).and_then(Option::as_deref) {
                    Some(value) => {
                        label.set_label(value);
                        label.remove_css_class("null-value");
                    }
                    None => {
                        label.set_label("NULL");
                        label.add_css_class("null-value");
                    }
                }
            });
            let title = if data_type.is_empty() {
                name.clone()
            } else {
                format!("{name}\n{data_type}")
            };
            let column = gtk::ColumnViewColumn::new(Some(&title), Some(factory));
            column.set_fixed_width(*width);
            column.set_resizable(true);
            column.set_expand(true);
            self.view.append_column(&column);
        }
    }

    pub fn set_rows(&self, rows: &[Vec<Value>]) {
        self.set_table_rows(
            rows.iter()
                .map(|row| GridRow {
                    values: row
                        .iter()
                        .map(|value| (!value.is_null()).then(|| display_value(value)))
                        .collect(),
                    source: row.clone(),
                    editable: vec![],
                    changed: vec![],
                    deleted: false,
                    staged: false,
                })
                .collect(),
        );
    }

    pub fn set_table_rows(&self, rows: Vec<GridRow>) {
        let selected = self.selected_rows();
        let objects: Vec<_> = rows.into_iter().map(glib::BoxedAnyObject::new).collect();
        self.store.splice(0, self.store.n_items(), &objects);
        for index in selected {
            self.selection.select_item(index as u32, false);
        }
    }

    pub fn clear(&self) {
        self.store.remove_all();
    }
}

/// Mirrors `defaultColumnWidth` in the React table.
pub fn default_column_width(data_type: &str) -> i32 {
    let data_type = data_type.to_lowercase();
    if data_type.contains("json") || data_type.contains("array") {
        320
    } else if data_type.contains("text")
        || data_type.contains("character")
        || data_type.contains("timestamp")
    {
        220
    } else {
        160
    }
}
