//! The connection editor: create, test, save, and delete saved profiles.

use std::cell::RefCell;
use std::rc::Rc;

use dbm_engine::models::{ConnectionProfile, DatabaseEngine, SaveProfileInput, TlsMode};
use dbm_engine::session::DbSession;
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::prelude::*;
use uuid::Uuid;

use crate::bridge;
use crate::connection_url;
use crate::format;
use crate::ui::app::Ui;

struct FormState {
    input: SaveProfileInput,
    existing: Option<ConnectionProfile>,
    window: gtk::Window,
    url_entry: gtk::Entry,
    name_entry: gtk::Entry,
    host_entry: gtk::Entry,
    port_spin: gtk::SpinButton,
    username_entry: gtk::Entry,
    username_label: gtk::Label,
    database_entry: gtk::Entry,
    database_label: gtk::Label,
    password_entry: gtk::PasswordEntry,
    tls_dropdown: gtk::DropDown,
    ca_entry: gtk::Entry,
    read_only_switch: gtk::Switch,
    engine_buttons: Vec<(DatabaseEngine, gtk::ToggleButton)>,
    feedback: gtk::Label,
    test_button: gtk::Button,
    save_button: gtk::Button,
}

const TLS_LABELS: [&str; 3] = ["Preferred", "Required", "Disabled"];
const TLS_MODES: [TlsMode; 3] = [TlsMode::Preferred, TlsMode::Required, TlsMode::Disabled];

pub fn open(ui: &Rc<RefCell<Ui>>, profile: Option<ConnectionProfile>) {
    let input = format::default_profile_input(profile.as_ref());
    let window = gtk::Window::builder()
        .title(if profile.is_some() {
            "Edit connection"
        } else {
            "New connection"
        })
        .transient_for(&ui.borrow().window)
        .modal(true)
        .default_width(560)
        .default_height(760)
        .build();
    window.add_css_class("dialog");

    let heading = gtk::Label::new(Some(if profile.is_some() {
        "Edit connection"
    } else {
        "New connection"
    }));
    heading.add_css_class("dialog-title");
    heading.set_xalign(0.0);

    // Engine picker
    let engine_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let mut engine_buttons = Vec::new();
    let mut first_engine_button: Option<gtk::ToggleButton> = None;
    for engine in [
        DatabaseEngine::Postgres,
        DatabaseEngine::Mysql,
        DatabaseEngine::Redis,
    ] {
        let button = gtk::ToggleButton::with_label(format::engine_label(engine));
        button.add_css_class("secondary-button");
        if let Some(first) = &first_engine_button {
            button.set_group(Some(first));
        } else {
            first_engine_button = Some(button.clone());
        }
        button.set_active(engine == input.engine);
        engine_row.append(&button);
        engine_buttons.push((engine, button));
    }

    // URL import
    let url_entry = gtk::Entry::new();
    url_entry.set_placeholder_text(Some(format::preset(input.engine).url_placeholder));
    url_entry.set_hexpand(true);
    let import_button = gtk::Button::with_label("Import URL");
    import_button.add_css_class("secondary-button");
    let url_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    url_row.append(&url_entry);
    url_row.append(&import_button);

    let name_entry = gtk::Entry::new();
    name_entry.set_text(&input.name);
    let host_entry = gtk::Entry::new();
    host_entry.set_text(&input.host);
    let port_spin = gtk::SpinButton::with_range(1.0, 65_535.0, 1.0);
    port_spin.set_value(f64::from(input.port));
    let username_entry = gtk::Entry::new();
    username_entry.set_text(&input.username);
    let username_label = gtk::Label::new(None);
    let database_entry = gtk::Entry::new();
    database_entry.set_text(&input.default_database);
    let database_label = gtk::Label::new(None);
    let password_entry = gtk::PasswordEntry::new();
    password_entry.set_show_peek_icon(true);
    password_entry.set_placeholder_text(Some(if profile.is_some() {
        "Leave blank to keep saved password"
    } else {
        "Stored in OS credential store"
    }));
    let tls_dropdown = gtk::DropDown::from_strings(&TLS_LABELS);
    tls_dropdown.set_selected(
        TLS_MODES
            .iter()
            .position(|mode| mode == &input.tls_mode)
            .unwrap_or(0) as u32,
    );
    let ca_entry = gtk::Entry::new();
    ca_entry.set_text(input.ca_cert_path.as_deref().unwrap_or(""));
    ca_entry.set_placeholder_text(Some("/path/to/root-ca.pem"));
    let read_only_switch = gtk::Switch::new();
    read_only_switch.set_active(input.read_only);
    read_only_switch.set_valign(gtk::Align::Center);

    // Color picker
    let color_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let mut color_buttons = Vec::new();
    let mut first_color_button: Option<gtk::ToggleButton> = None;
    for color in format::CONNECTION_COLORS {
        let button = gtk::ToggleButton::new();
        button.add_css_class("swatch");
        button.add_css_class(&crate::theme::color_class(color));
        button.set_tooltip_text(Some(&format!("Use connection color {color}")));
        if let Some(first) = &first_color_button {
            button.set_group(Some(first));
        } else {
            first_color_button = Some(button.clone());
        }
        button.set_active(input.color.as_deref() == Some(color));
        color_row.append(&button);
        color_buttons.push((color.to_owned(), button));
    }
    let custom_color = gtk::ColorButton::new();
    custom_color.set_rgba(&hex_to_rgba(
        input
            .color
            .as_deref()
            .unwrap_or(format::DEFAULT_CONNECTION_COLOR),
    ));
    custom_color.add_css_class("swatch");
    let custom_label = gtk::Label::new(Some("Custom"));
    custom_label.add_css_class("dialog-note");
    let custom_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    custom_box.append(&custom_color);
    custom_box.append(&custom_label);
    color_row.append(&custom_box);

    let feedback = gtk::Label::new(None);
    feedback.set_xalign(0.0);
    feedback.set_wrap(true);
    feedback.set_visible(false);

    let test_button = gtk::Button::with_label("Test connection");
    test_button.add_css_class("secondary-button");
    let save_button = gtk::Button::with_label("Save & connect");
    save_button.add_css_class("primary-button");
    let cancel_button = gtk::Button::with_label("Cancel");
    cancel_button.add_css_class("secondary-button");
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_halign(gtk::Align::End);
    actions.append(&cancel_button);
    actions.append(&test_button);
    actions.append(&save_button);

    let delete_button = profile.as_ref().map(|_| {
        let button = gtk::Button::with_label("Delete");
        button.add_css_class("danger-button");
        button
    });
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if let Some(button) = &delete_button {
        footer.append(button);
    }
    let footer_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer_spacer.set_hexpand(true);
    footer.append(&footer_spacer);
    footer.append(&actions);

    let note = gtk::Label::new(Some(
        "Passwords are stored in your operating system credential manager and are never written to DBM's profile database.",
    ));
    note.add_css_class("dialog-note");
    note.set_wrap(true);
    note.set_xalign(0.0);

    let form = gtk::Grid::new();
    form.set_row_spacing(8);
    form.set_column_spacing(10);
    let mut row = 0;
    let mut add_field = |label: &str, widget: &gtk::Widget| {
        let label_widget = gtk::Label::new(Some(label));
        label_widget.add_css_class("form-label");
        label_widget.set_xalign(0.0);
        label_widget.set_valign(gtk::Align::Center);
        form.attach(&label_widget, 0, row, 1, 1);
        form.attach(widget, 1, row, 1, 1);
        row += 1;
    };
    add_field("Database engine", engine_row.upcast_ref());
    add_field("Connection URL", url_row.upcast_ref());
    add_field("Name", name_entry.upcast_ref());
    add_field("Connection color", color_row.upcast_ref());
    add_field("Host", host_entry.upcast_ref());
    add_field("Port", port_spin.upcast_ref());
    add_field("Username", username_entry.upcast_ref());
    add_field("Database", database_entry.upcast_ref());
    add_field("Password", password_entry.upcast_ref());
    add_field("TLS", tls_dropdown.upcast_ref());
    add_field("CA certificate path (optional)", ca_entry.upcast_ref());
    let read_only_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    read_only_row.append(&read_only_switch);
    read_only_row.append(&gtk::Label::new(Some(
        "Read-only profile (blocks GUI edits and mutations)",
    )));
    add_field("", read_only_row.upcast_ref());

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(16);
    content.set_margin_start(18);
    content.set_margin_end(18);
    content.append(&heading);
    content.append(&form);
    content.append(&feedback);
    content.append(&note);
    content.append(&footer);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&content)
        .build();
    window.set_child(Some(&scrolled));

    let state = Rc::new(RefCell::new(FormState {
        input,
        existing: profile.clone(),
        window: window.clone(),
        url_entry: url_entry.clone(),
        name_entry: name_entry.clone(),
        host_entry,
        port_spin,
        username_entry: username_entry.clone(),
        username_label,
        database_entry: database_entry.clone(),
        database_label,
        password_entry,
        tls_dropdown,
        ca_entry,
        read_only_switch,
        engine_buttons: engine_buttons.clone(),
        feedback: feedback.clone(),
        test_button: test_button.clone(),
        save_button: save_button.clone(),
    }));

    // Engine picker
    for (engine, button) in &engine_buttons {
        let state = state.clone();
        let engine = *engine;
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            if let Ok(mut form) = state.try_borrow_mut() {
                set_engine(&mut form, engine);
            }
        });
    }
    // Color swatches
    for (color, button) in &color_buttons {
        let state = state.clone();
        let color = color.clone();
        button.connect_toggled(move |button| {
            if !button.is_active() {
                return;
            }
            if let Ok(mut form) = state.try_borrow_mut() {
                form.input.color = Some(color.clone());
            }
        });
    }
    {
        let state = state.clone();
        custom_color.connect_color_set(move |button| {
            if let Ok(mut form) = state.try_borrow_mut() {
                form.input.color = Some(rgba_to_hex(&button.rgba()));
            }
        });
    }
    // URL import
    {
        let state = state.clone();
        import_button.connect_clicked(move |_| {
            let value = state.borrow().url_entry.text().to_string();
            import_url(&state, &value);
        });
    }
    // Test / save / delete / cancel
    {
        let state = state.clone();
        let ui = ui.clone();
        test_button.connect_clicked(move |_| test_connection(&state, &ui));
    }
    {
        let state = state.clone();
        let ui = ui.clone();
        save_button.connect_clicked(move |_| save_and_connect(&state, &ui));
    }
    if let Some(button) = &delete_button {
        let state = state.clone();
        let ui = ui.clone();
        button.connect_clicked(move |_| {
            let Some(profile) = state.borrow().existing.clone() else {
                return;
            };
            state.borrow().window.close();
            ui.borrow().confirm_delete_profile(profile);
        });
    }
    {
        let window = window.clone();
        cancel_button.connect_clicked(move |_| window.close());
    }

    set_engine_labels(&mut state.borrow_mut());
    window.present();
}

fn set_engine(form: &mut FormState, engine: DatabaseEngine) {
    format::apply_engine_defaults(&mut form.input, engine);
    form.port_spin.set_value(f64::from(form.input.port));
    form.name_entry.set_text(&form.input.name);
    form.username_entry.set_text(&form.input.username);
    form.database_entry.set_text(&form.input.default_database);
    form.url_entry
        .set_placeholder_text(Some(format::preset(engine).url_placeholder));
    set_engine_labels(form);
}

fn set_engine_labels(form: &mut FormState) {
    let engine = form.input.engine;
    form.username_label
        .set_label(if engine == DatabaseEngine::Redis {
            "Username (ACL, optional)"
        } else {
            "Username"
        });
    form.database_label
        .set_label(if engine == DatabaseEngine::Redis {
            "Database index"
        } else {
            "Database"
        });
}

fn import_url(state: &Rc<RefCell<FormState>>, value: &str) {
    let imported = match connection_url::parse_connection_url(value) {
        Ok(imported) => imported,
        Err(message) => {
            set_feedback(state, false, &message);
            return;
        }
    };
    {
        let mut form = state.borrow_mut();
        form.input.engine = imported.engine;
        form.input.host = imported.host.clone();
        form.input.port = imported.port;
        form.input.username = imported.username.clone();
        form.input.default_database = imported.default_database.clone();
        form.input.tls_mode = imported.tls_mode.clone();
        if let Some(password) = &imported.password {
            form.input.password = Some(password.clone());
            form.password_entry.set_text(password);
        }
        form.host_entry.set_text(&imported.host);
        form.port_spin.set_value(f64::from(imported.port));
        form.username_entry.set_text(&imported.username);
        form.database_entry.set_text(&imported.default_database);
        form.tls_dropdown.set_selected(
            TLS_MODES
                .iter()
                .position(|mode| mode == &imported.tls_mode)
                .unwrap_or(0) as u32,
        );
        for (engine, button) in &form.engine_buttons {
            button.set_active(*engine == imported.engine);
        }
        set_engine_labels(&mut form);
    }
    set_feedback(
        state,
        true,
        "Connection URL imported. Review the details, then save and connect.",
    );
}

fn read_form(form: &FormState) -> SaveProfileInput {
    let mut input = form.input.clone();
    input.name = form.name_entry.text().to_string();
    input.host = form.host_entry.text().to_string();
    input.port = form.port_spin.value().round() as u16;
    input.username = form.username_entry.text().to_string();
    input.default_database = form.database_entry.text().to_string();
    input.tls_mode = TLS_MODES
        .get(form.tls_dropdown.selected() as usize)
        .cloned()
        .unwrap_or_default();
    input.ca_cert_path = {
        let text = form.ca_entry.text().to_string();
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    };
    input.read_only = form.read_only_switch.is_active();
    input.password = {
        let text = form.password_entry.text().to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    };
    input
}

fn test_connection(state: &Rc<RefCell<FormState>>, ui: &Rc<RefCell<Ui>>) {
    let state = state.clone();
    let ui = ui.clone();
    let (input, existing) = {
        let form = state.borrow();
        (read_form(&form), form.existing.clone())
    };
    let profile = match input.to_profile(existing.as_ref()) {
        Ok(profile) => profile,
        Err(error) => {
            set_feedback(&state, false, &format::error_message(&error));
            return;
        }
    };
    set_busy(&state, true, "Testing connection…");
    let engine = ui.borrow().engine.clone();
    bridge::spawn(
        async move {
            let password = resolve_password(&engine, &input, profile.id)?;
            let session = DbSession::connect(profile, password).await?;
            session.close().await;
            Ok(())
        },
        move |result| {
            set_busy(&state, false, "");
            match result {
                Ok(()) => set_feedback(&state, true, "Connection successful."),
                Err(error) => set_feedback(&state, false, &format::error_message(&error)),
            }
        },
    );
}

fn save_and_connect(state: &Rc<RefCell<FormState>>, ui: &Rc<RefCell<Ui>>) {
    let state = state.clone();
    let ui = ui.clone();
    let (input, existing) = {
        let form = state.borrow();
        (read_form(&form), form.existing.clone())
    };
    let profile = match input.to_profile(existing.as_ref()) {
        Ok(profile) => profile,
        Err(error) => {
            set_feedback(&state, false, &format::error_message(&error));
            return;
        }
    };
    set_busy(&state, true, "Testing connection before saving…");
    let engine = ui.borrow().engine.clone();
    bridge::spawn(
        async move {
            let password = resolve_password(&engine, &input, profile.id)?;
            let session = DbSession::connect(profile, password).await?;
            session.close().await;
            let saved = engine.store.save_profile(&input)?;
            if let Some(password) = input.password.as_deref().filter(|value| !value.is_empty()) {
                engine.credentials.save_password(saved.id, password)?;
            }
            engine.disconnect(saved.id).await;
            Ok(saved)
        },
        move |result| {
            set_busy(&state, false, "");
            match result {
                Ok(saved) => {
                    state.borrow().window.close();
                    ui.borrow().load_profiles();
                    ui.borrow().connect_profile(saved.id);
                }
                Err(error) => set_feedback(&state, false, &format::error_message(&error)),
            }
        },
    );
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

fn set_busy(state: &Rc<RefCell<FormState>>, busy: bool, message: &str) {
    let form = state.borrow();
    form.test_button.set_sensitive(!busy);
    form.save_button.set_sensitive(!busy);
    form.save_button
        .set_label(if busy { "Working…" } else { "Save & connect" });
    if !message.is_empty() {
        drop(form);
        set_feedback(state, true, message);
    }
}

fn set_feedback(state: &Rc<RefCell<FormState>>, success: bool, message: &str) {
    let form = state.borrow();
    form.feedback.set_label(message);
    form.feedback.remove_css_class("error");
    form.feedback.remove_css_class("success");
    form.feedback.add_css_class("feedback");
    form.feedback
        .add_css_class(if success { "success" } else { "error" });
    form.feedback.set_visible(true);
}

fn rgba_to_hex(rgba: &gdk::RGBA) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgba.red()),
        channel(rgba.green()),
        channel(rgba.blue())
    )
}

fn hex_to_rgba(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(hex).unwrap_or_else(|_| gdk::RGBA::new(0.22, 0.74, 0.97, 1.0))
}
