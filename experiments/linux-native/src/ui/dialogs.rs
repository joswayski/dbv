//! Custom confirmations, matching DBM's dark workbench instead of stock dialogs.

use std::cell::RefCell;

use gtk4 as gtk;
use gtk4::prelude::*;

pub fn confirm(
    parent: &impl IsA<gtk::Window>,
    title: &str,
    body: &str,
    confirm_label: &str,
    danger: bool,
    on_confirm: impl FnOnce() + 'static,
) {
    let window = gtk::Window::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .default_width(430)
        .build();
    window.add_css_class("dialog");

    let title_label = gtk::Label::new(Some(title));
    title_label.add_css_class("dialog-title");
    title_label.set_xalign(0.0);
    let body_label = gtk::Label::new(Some(body));
    body_label.set_xalign(0.0);
    body_label.set_wrap(true);
    body_label.add_css_class("dialog-note");

    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("secondary-button");
    let confirm = gtk::Button::with_label(confirm_label);
    confirm.add_css_class(if danger {
        "danger-button"
    } else {
        "primary-button"
    });
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_halign(gtk::Align::End);
    actions.append(&cancel);
    actions.append(&confirm);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.set_margin_top(18);
    content.set_margin_bottom(16);
    content.set_margin_start(18);
    content.set_margin_end(18);
    content.append(&title_label);
    content.append(&body_label);
    content.append(&actions);
    window.set_child(Some(&content));

    let on_confirm = RefCell::new(Some(Box::new(on_confirm) as Box<dyn FnOnce()>));
    {
        let window = window.clone();
        confirm.connect_clicked(move |_| {
            window.close();
            if let Some(callback) = on_confirm.borrow_mut().take() {
                callback();
            }
        });
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }
    window.present();
}
