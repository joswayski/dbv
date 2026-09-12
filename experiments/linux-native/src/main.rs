//! DBM's Linux-native frontend: GTK4 presentation around the shared
//! `dbm-engine` crate.
//!
//! This is an in-development experiment, not a replacement for the shipping
//! Tauri app. See `docs/native-platforms.md` for status and parity notes.

mod bridge;
mod state;
mod theme;
mod ui;

use std::sync::Arc;

use dbm_engine::state::AppState;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;

const APP_ID: &str = "io.github.joswayski.dbm.native";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| theme::install());
    app.connect_activate(|app| {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("DBM")
            .default_width(1400)
            .default_height(900)
            .build();
        let engine = match AppState::new() {
            Ok(engine) => Arc::new(engine),
            Err(error) => {
                eprintln!("DBM could not open its local profile database: {error}");
                std::process::exit(1);
            }
        };
        let workbench = ui::app::Ui::new(engine, window.clone());
        // The workbench owns the window and every view; keep it for the life of
        // the process instead of dropping it when `activate` returns.
        std::mem::forget(workbench);
        window.present();
    });
    app.run()
}
