//! A virtualized data grid built on `GtkColumnView`.

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
}

#[derive(Clone)]
pub struct DataGrid {
    pub root: gtk::ScrolledWindow,
    pub view: gtk::ColumnView,
    store: gio::ListStore,
}

impl Default for DataGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl DataGrid {
    pub fn new() -> Self {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::NoSelection::new(Some(store.clone()));
        let view = gtk::ColumnView::new(Some(selection));
        view.add_css_class("data-grid");
        view.set_show_column_separators(true);
        view.set_show_row_separators(true);

        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();

        Self { root, view, store }
    }

    /// Rebuilds the columns. Widths mirror the React table's defaults.
    pub fn set_columns(&self, columns: &[(String, i32)]) {
        while let Some(column) = self.view.columns().item(0) {
            self.view
                .remove_column(&column.downcast::<gtk::ColumnViewColumn>().unwrap());
        }
        for (index, (name, width)) in columns.iter().enumerate() {
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(|_, item| {
                let label = gtk::Label::new(None);
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.add_css_class("grid-cell");
                item.downcast_ref::<gtk::ListItem>()
                    .expect("list item")
                    .set_child(Some(&label));
            });
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let label = item.child().and_downcast::<gtk::Label>().expect("label");
                let Some(row) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let row = row.borrow::<GridRow>();
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
            let column = gtk::ColumnViewColumn::new(Some(name), Some(factory));
            column.set_fixed_width(*width);
            column.set_resizable(true);
            column.set_expand(false);
            self.view.append_column(&column);
        }
    }

    pub fn set_rows(&self, rows: &[Vec<Value>]) {
        self.store.remove_all();
        for row in rows {
            let values = row
                .iter()
                .map(|value| (value != &Value::Null).then(|| display_value(value)))
                .collect();
            self.store
                .append(&glib::BoxedAnyObject::new(GridRow { values }));
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
