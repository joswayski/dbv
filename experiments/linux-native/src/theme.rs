//! DBM's dark workbench styling for GTK4, plus per-connection color classes.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk4 as gtk;

use dbm_workbench::format::DEFAULT_CONNECTION_COLOR;

mod satoshi {
    include!(concat!(env!("OUT_DIR"), "/satoshi.rs"));
}

use satoshi::SATOSHI;

thread_local! {
    static COLOR_CLASSES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

pub fn install() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(APP_CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
    }
}

/// Registers the embedded Satoshi font for this process only.
///
/// Pango in GTK4 has no "add a font file" API before 1.56, so DBM writes a
/// private fontconfig configuration that includes the font directory plus the
/// system configuration, and points `FONTCONFIG_FILE` at it before GTK starts.
/// Falls back to the system font when the font was not fetched
/// (`npm run fonts`).
pub fn install_font() {
    let Some(bytes) = SATOSHI else {
        return;
    };
    let Some(directories) = directories::ProjectDirs::from("io", "github", "dbm") else {
        return;
    };
    let font_dir = directories.cache_dir().join("fonts");
    if std::fs::create_dir_all(&font_dir).is_err() {
        return;
    }
    let font_path = font_dir.join("Satoshi-Variable.ttf");
    let outdated = std::fs::read(&font_path).map_or(true, |existing| existing != bytes);
    if outdated && std::fs::write(&font_path, bytes).is_err() {
        return;
    }
    let config = font_dir.join("fonts.conf");
    let xml = format!(
        "<?xml version=\"1.0\"?>\n\
         <!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n\
         <fontconfig>\n\
         \x20 <dir>{}</dir>\n\
         \x20 <cachedir>{}</cachedir>\n\
         \x20 <include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>\n\
         </fontconfig>\n",
        font_dir.display(),
        font_dir.join("cache").display()
    );
    if std::fs::write(&config, xml).is_err() {
        return;
    }
    // Set before GTK (and any fontconfig user) initializes.
    std::env::set_var("FONTCONFIG_FILE", &config);
}

/// Registers a CSS class that paints `color`, returning its name. Profile
/// colors are user data, so they cannot live in the static stylesheet and are
/// normalized before being interpolated into CSS.
pub fn color_class(color: &str) -> String {
    let color = normalize_color(color);
    COLOR_CLASSES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(class) = cache.get(&color) {
            return class.clone();
        }
        let class = format!("conn-color-{}", slug(&color));
        let provider = gtk::CssProvider::new();
        provider.load_from_data(&format!(
            ".{class} {{ background-color: {color}; }}\n\
             .dot.{class} {{ box-shadow: 0 0 10px alpha({color}, 0.7); }}\n\
             .{class}-accent {{ box-shadow: inset 3px 0 {color}; }}\n\
             .tab.active.{class}-accent {{ background-color: mix(#0f1722, {color}, 0.16); box-shadow: inset 0 -3px {color}, inset 0 1px alpha({color}, 0.34); }}\n\
             .tab.active.{class}-accent:hover {{ background-color: mix(#0f1722, {color}, 0.20); }}\n\
             .tab.active.{class}-accent .tab-title {{ text-shadow: 0 0 12px alpha({color}, 0.42); }}\n\
             .{class}-text {{ color: {color}; }}"
        ));
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        cache.insert(color.clone(), class.clone());
        class
    })
}

pub fn normalize_color(color: &str) -> String {
    let trimmed = color.trim();
    if trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return trimmed.to_lowercase();
    }
    DEFAULT_CONNECTION_COLOR.to_owned()
}

fn slug(color: &str) -> String {
    color.trim_start_matches('#').to_lowercase()
}

const APP_CSS: &str = r#"
window, .dialog { background-color: #0b1017; color: #dbe5f2; font-family: "Satoshi Variable", "Satoshi", "Inter", sans-serif; font-size: 13px; }

.sidebar { background-color: #0f1722; border-right: 1px solid #253447; }
.brand-row { min-height: 39px; padding: 14px 16px; border-bottom: 1px solid #253447; }
.brand-mark { background-image: linear-gradient(145deg, #38bdf8, #6366f1); color: #06111b; border-radius: 9px; min-width: 34px; min-height: 34px; font-weight: 900; }
.brand-title { font-weight: 700; font-size: 14px; }
.brand-subtitle { color: #7c8ea6; font-size: 10px; }
.section-title { color: #7c8ea6; font-size: 10px; font-weight: 700; letter-spacing: 0.08em; }
.sidebar-footer { padding: 10px 14px; border-top: 1px solid #253447; }
.connection-group { margin: 3px 8px; border: 1px solid transparent; border-radius: 8px; }
.connection-group.active { border-color: #344963; background-color: rgba(23, 34, 49, 0.58); }

button { background-image: none; box-shadow: none; text-shadow: none; transition: background-color 120ms ease-out, color 120ms ease-out, box-shadow 120ms ease-out; }
button:focus-visible { outline: 2px solid #38bdf8; outline-offset: -2px; }
button.icon-button { background: transparent; border: none; color: #7c8ea6; padding: 1px 5px; min-height: 24px; min-width: 24px; border-radius: 5px; }
button.icon-button:hover { background-color: #1b2a3d; color: #dbe5f2; }
button.new-connection { background-color: #0ea5e9; color: #03121d; border: none; border-radius: 5px; padding: 4px 9px; font-size: 11px; font-weight: 700; }
button.new-connection:hover { background-color: #38bdf8; }

button.connection-row { background: transparent; border: none; border-radius: 7px; padding: 6px 8px; }
button.connection-row:hover { background-color: #1b2a3d; }
button.connection-row.active { background-color: #172231; }
.connection-name { font-weight: 700; font-size: 13px; }
.connection-meta { color: #7c8ea6; font-size: 11px; }
.dot { border-radius: 999px; min-width: 8px; min-height: 8px; }
.empty-note { color: #7c8ea6; font-size: 12px; padding: 12px; }

.workspace-panel { background-color: rgba(7, 13, 20, 0.28); border-top: 1px solid #253447; padding: 8px; }
.panel-label { color: #7c8ea6; font-size: 10px; font-weight: 700; letter-spacing: 0.08em; }
button.schema-row { background: transparent; border: none; border-radius: 5px; padding: 3px 6px; font-size: 12px; color: #c3d1e0; }
button.schema-row:hover { background-color: #1b2a3d; }
button.schema-row.active { background-color: rgba(56, 189, 248, 0.14); color: #dbe5f2; font-weight: 700; }
.schema-icon { font-size: 9px; font-weight: 800; padding: 1px 4px; border-radius: 3px; background-color: rgba(56, 189, 248, 0.15); color: #38bdf8; }
.schema-icon-key { background-color: rgba(167, 139, 250, 0.15); color: #a78bfa; }
.schema-icon-view { background-color: rgba(74, 222, 128, 0.14); color: #4ade80; }

.topbar { min-height: 67px; border-bottom: 1px solid #253447; padding: 0 22px; }
.identity-name { font-weight: 700; font-size: 13px; }
.identity-meta { color: #7c8ea6; font-size: 11px; }
.breadcrumb { color: #7c8ea6; font-size: 12px; }

.tab-strip { background-color: #0f1722; border-bottom: 1px solid #253447; }
.tab { background: transparent; border-right: 1px solid #253447; min-height: 38px; color: #8091a8; transition: background-color 120ms ease-out, box-shadow 120ms ease-out; }
.tab:hover { background-color: rgba(128, 145, 168, 0.06); }
.tab.active { color: #f6faff; }
.tab.active .tab-title { font-weight: 750; }
button.tab-title { background: transparent; border: none; color: inherit; padding: 0 12px; font-size: 11px; }
button.tab-title:hover { color: #ffffff; }
button.tab-close { background: transparent; border: none; color: #7c8ea6; padding: 0 3px; min-height: 20px; min-width: 20px; border-radius: 4px; }
button.tab-close:hover { color: #f87171; background-color: rgba(248, 113, 113, 0.1); }
button.tab-new { background: transparent; border: none; color: #7c8ea6; padding: 4px 9px; font-size: 15px; }
button.tab-new:hover { color: #38bdf8; background-color: rgba(56, 189, 248, 0.08); }

.toolbar { padding: 9px 14px; border-bottom: 1px solid #253447; }
.eyebrow { color: #38bdf8; font-size: 9px; font-weight: 800; letter-spacing: 0.12em; }
.title { font-size: 18px; font-weight: 650; }
.muted { color: #7c8ea6; font-size: 11px; }

button.primary-button { background-color: #0ea5e9; color: #03121d; border: 1px solid #0ea5e9; border-radius: 6px; padding: 5px 11px; font-weight: 700; font-size: 12px; }
button.primary-button:hover { background-color: #38bdf8; border-color: #38bdf8; }
button.primary-button:disabled { background-color: #1b2a3d; border-color: #253447; color: #64748b; }
button.secondary-button { background-color: #172231; color: #dbe5f2; border: 1px solid #344963; border-radius: 6px; padding: 5px 11px; font-size: 12px; }
button.secondary-button:hover { border-color: #38bdf8; color: #38bdf8; }
button.secondary-button:disabled { color: #64748b; border-color: #253447; }
button.danger-button { background-color: rgba(248, 113, 113, 0.08); color: #f87171; border: 1px solid rgba(248, 113, 113, 0.35); border-radius: 6px; padding: 5px 11px; font-size: 12px; }
button.danger-button:hover { background-color: rgba(248, 113, 113, 0.16); }
button.link-button { background: transparent; border: none; color: #38bdf8; font-size: 11px; padding: 2px 4px; }
button.link-button:hover { background-color: rgba(56, 189, 248, 0.08); border-radius: 4px; }

entry { background-color: #111924; color: #dbe5f2; border: 1px solid #253447; border-radius: 6px; padding: 4px 8px; min-height: 26px; }
entry:focus { border-color: #38bdf8; box-shadow: 0 0 0 2px rgba(56, 189, 248, 0.11); }
spinbutton { background-color: #111924; color: #dbe5f2; border: 1px solid #253447; border-radius: 6px; min-height: 26px; }
spinbutton:focus-within { border-color: #38bdf8; }
dropdown > button { background-color: #111924; color: #dbe5f2; border: 1px solid #253447; border-radius: 6px; min-height: 26px; padding: 2px 8px; }
dropdown > button:hover { border-color: #38bdf8; }
switch { background-color: #253447; }
switch:checked { background-color: #0ea5e9; }
checkbutton { color: #dbe5f2; font-size: 12px; }
popover > contents { background-color: #172231; border: 1px solid #344963; border-radius: 7px; padding: 6px; }

.workbench-view { padding: 18px 22px 24px; }
.view-toolbar { margin-bottom: 16px; }
/* Data grid: mirrors the React table's chrome. */
.grid-frame { background-color: #0e1620; border: 1px solid #253447; border-radius: 7px; }
.data-grid, .data-grid > listview { background-color: #0e1620; }
.data-grid > header { background-color: #172332; border-bottom: 1px solid #344963; min-height: 47px; }
.data-grid > header > button { background-color: #172332; color: #aebfd2; font-size: 10px; font-weight: 700; padding: 6px 10px; border: none; border-radius: 0; box-shadow: none; transition: background-color 90ms ease-out, color 90ms ease-out; }
.data-grid > header > button:hover { background-color: rgba(56, 189, 248, 0.07); color: #dbe5f2; }
.data-grid > listview > row { background-color: transparent; transition: background-color 90ms ease-out; }
.data-grid > listview > row:hover { background-color: rgba(56, 189, 248, 0.045); }
.data-grid > listview > row:selected { background-color: rgba(56, 189, 248, 0.09); }
.data-grid > listview > row > cell { min-height: 35px; padding: 0 10px; border-bottom: 1px solid rgba(37, 52, 71, 0.55); color: #c5d2df; font-size: 11px; transition: background-color 90ms ease-out, color 90ms ease-out; }
.data-grid > listview > row > cell.staged-cell { background-color: rgba(251, 191, 36, 0.035); }
.data-grid > listview > row > cell.changed-cell { background-color: rgba(251, 191, 36, 0.14); color: #fde68a; box-shadow: inset 0 -1px rgba(251, 191, 36, 0.38); }
.data-grid > listview > row > cell.deleted-cell { background-color: rgba(248, 113, 113, 0.12); color: #fca5a5; box-shadow: none; }
.data-grid > listview > row > cell.deleted-cell .null-value { color: #e09a9a; }
entry.cell-editor { background-color: #101a27; color: #dbe5f2; border: 1px solid #38bdf8; border-radius: 3px; min-height: 25px; padding: 1px 4px; font-size: 11px; }
.pending-changes { background-color: rgba(251, 191, 36, 0.065); border: 1px solid rgba(251, 191, 36, 0.28); color: #fcd34d; border-radius: 6px; padding: 8px 10px; margin-bottom: 10px; font-size: 11px; }
.change-popover > contents { background-color: #111c29; border: 1px solid #3b4859; padding: 12px; box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5); }
.change-preview { min-width: 260px; font-size: 11px; }
.before-value { color: #fca5a5; background-color: rgba(248, 113, 113, 0.07); padding: 6px 8px; border-radius: 3px; }
.after-value { color: #86efac; background-color: rgba(74, 222, 128, 0.07); padding: 6px 8px; border-radius: 3px; }
.grid-header { color: #aebfd2; font-size: 10px; font-weight: 700; padding: 5px 8px; }
.grid-cell { font-size: 11px; padding: 0; }
.null-value { color: #64748b; font-style: italic; }
.skeleton-row { min-height: 34px; border-bottom: 1px solid rgba(37, 52, 71, 0.55); background-image: linear-gradient(90deg, rgba(96, 117, 141, 0.08), rgba(96, 117, 141, 0.16), rgba(96, 117, 141, 0.08)); background-size: 220% 100%; animation: shimmer 1.5s ease-in-out infinite; }
@keyframes shimmer { 0% { background-position: 100% 0; } 100% { background-position: -100% 0; } }
.cell-input { font-size: 12px; padding: 1px 4px; min-height: 20px; }
.pagination { padding: 8px 14px; border-top: 1px solid #253447; }
.empty-state { color: #7c8ea6; font-size: 12px; padding: 24px; }
.grid-loading { background-color: rgba(8, 14, 21, 0.28); }
.grid-loading-pill { color: #9bdcf8; background-color: rgba(15, 23, 34, 0.94); border: 1px solid rgba(56, 189, 248, 0.25); border-radius: 999px; padding: 7px 10px; box-shadow: 0 7px 22px rgba(0, 0, 0, 0.28); font-size: 10px; }

.editor { font-family: "JetBrains Mono", "Fira Code", "DejaVu Sans Mono", monospace; font-size: 13px; background-color: #0b121a; color: #dbe5f2; padding: 8px; }
.editor text { background-color: #0b121a; color: #dbe5f2; }
.editor-frame, .history-panel { background-color: #0b121a; border: 1px solid #253447; border-radius: 7px; }
.history-list { background: transparent; }
.history-item { background: transparent; border: none; border-radius: 5px; padding: 5px 7px; font-size: 11px; color: #c3d1e0; }
.history-item:hover { background-color: #1b2a3d; }
.result-meta { color: #8fa2b8; font-size: 11px; padding: 10px 0; }

.banner { padding: 7px 14px; font-size: 12px; }
.banner.error { background-color: rgba(248, 113, 113, 0.12); color: #fca5a5; border-bottom: 1px solid rgba(248, 113, 113, 0.35); }
.banner.success { background-color: rgba(74, 222, 128, 0.1); color: #86efac; border-bottom: 1px solid rgba(74, 222, 128, 0.3); }
.toast { background-color: #172231; border: 1px solid #344963; border-radius: 7px; padding: 9px 12px; box-shadow: 0 14px 40px rgba(0, 0, 0, 0.48); font-size: 12px; }

.chip { border-radius: 4px; padding: 1px 5px; font-size: 9px; font-weight: 800; letter-spacing: 0.08em; }
.chip.local { color: #4ade80; border: 1px solid rgba(74, 222, 128, 0.3); }
.chip.warning { color: #fbbf24; border: 1px solid rgba(251, 191, 36, 0.35); }
.chip.readonly { color: #8fa2b8; border: 1px solid #253447; }

.filter-popover { padding: 8px; }
/* Filter panel: mirrors the React table's inline panel. */
.filter-panel { margin-bottom: 12px; padding: 10px; border: 1px solid #253447; border-radius: 7px; background-color: rgba(17, 25, 36, 0.55); }
.filter-panel-header { min-height: 25px; margin-bottom: 4px; }
.filter-title { color: #aebfd2; font-size: 10px; font-weight: 700; letter-spacing: 0.08em; }
.filter-join { color: #60758d; font-size: 9px; }
.control-label { color: #7c8ea6; font-size: 9px; font-weight: 650; }
.table-query-controls { margin-top: 10px; padding-top: 10px; border-top: 1px solid rgba(37, 52, 71, 0.7); }
.apply-filters-button { min-width: 112px; box-shadow: 0 0 0 2px rgba(14, 165, 233, 0.1), 0 6px 18px rgba(14, 165, 233, 0.12); }
.filter-row-entry { min-height: 26px; }
.filter-column { min-width: 165px; }
.filter-operator { min-width: 185px; }
.pagination { padding: 8px 0 12px; }
.dialog-title { font-size: 16px; font-weight: 700; }
.dialog-note { color: #7c8ea6; font-size: 11px; }
.form-label { color: #a9b8c9; font-size: 11px; font-weight: 600; }
.feedback { font-size: 11px; padding: 6px 8px; border-radius: 5px; }
.feedback.error { color: #fca5a5; background-color: rgba(248, 113, 113, 0.1); }
.feedback.success { color: #86efac; background-color: rgba(74, 222, 128, 0.1); }
.feedback.info { color: #a9b8c9; background-color: rgba(148, 163, 184, 0.1); }
button.swatch { min-width: 22px; min-height: 22px; border-radius: 999px; border: 2px solid transparent; padding: 0; }
button.swatch:checked { border-color: #dbe5f2; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::*;

    #[test]
    #[ignore = "requires a display: xvfb-run cargo test -- --ignored"]
    fn rendered_workbench_surfaces() {
        gtk::init().expect("GTK display");
        let provider = gtk::CssProvider::new();
        let errors = std::rc::Rc::new(RefCell::new(Vec::new()));
        let captured = errors.clone();
        provider.connect_parsing_error(move |_, _, error| {
            captured.borrow_mut().push(error.to_string());
        });
        provider.load_from_data(APP_CSS);
        assert!(errors.borrow().is_empty(), "{:?}", errors.borrow());
        install();
        gtk::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(false);

        // A GtkBox, not a button: this catches the old button.tab selector.
        // Use purple rather than the global cyan accent to test profile identity.
        let tab = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab.add_css_class("tab");
        tab.add_css_class("active");
        tab.add_css_class(&format!("{}-accent", color_class("#a78bfa")));
        let grid = crate::ui::grid::DataGrid::new();
        grid.set_columns(&[
            ("id".into(), "integer".into(), 160),
            ("name".into(), "text".into(), 220),
        ]);
        grid.set_rows(&[vec![serde_json::json!(7), serde_json::Value::Null]]);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&tab);
        content.append(&grid.root);
        let window = gtk::Window::builder()
            .default_width(800)
            .default_height(300)
            .child(&content)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        for _ in 0..30 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let snapshot = gtk::Snapshot::new();
        window.snapshot_child(&content, &snapshot);
        let node = snapshot.to_node().expect("painted content");
        let texture = window.renderer().unwrap().render_texture(&node, None);
        let stride = texture.width() as usize * 4;
        let mut pixels = vec![0; stride * texture.height() as usize];
        texture.download(&mut pixels, stride);
        let pixel = |x: usize, y: usize| {
            let offset = y * stride + x * 4;
            [pixels[offset + 2], pixels[offset + 1], pixels[offset]] // native BGRA → RGB
        };
        // 16% purple over #0f1722, not the flat raised surface or cyan accent.
        for (actual, expected) in pixel(40, 15).into_iter().zip([39_u8, 42, 69]) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "active tab tint: {:?}",
                pixel(40, 15)
            );
        }
        // Inner listview used to paint Adwaita gray; rightmost columns must fill too.
        assert_eq!(pixel(40, 200), [14, 22, 32]);
        assert_eq!(pixel(760, 200), [14, 22, 32]);
        assert_eq!(pixel(760, 60), [23, 35, 50], "header fills available width");
        assert!(!grid.view.shows_column_separators());

        // Draft decorations must tint the real cells, not a label-sized patch,
        // and deleting an edited row must override its amber background.
        let draft = crate::ui::grid::GridRow {
            values: vec![Some("7".into()), Some("Edited".into())],
            source: vec![],
            editable: vec![false, true],
            changed: vec![false, true],
            staged: true,
            deleted: false,
        };
        let mut deleted = draft.clone();
        deleted.deleted = true;
        grid.set_table_rows(vec![draft, deleted]);
        for _ in 0..30 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        fn cells(widget: &gtk::Widget, result: &mut Vec<gtk::Widget>) {
            if widget.has_css_class("staged-cell") {
                result.push(widget.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                cells(&widget, result);
                child = widget.next_sibling();
            }
        }
        let snapshot = gtk::Snapshot::new();
        window.snapshot_child(&content, &snapshot);
        let texture = window
            .renderer()
            .unwrap()
            .render_texture(snapshot.to_node().unwrap(), None);
        let stride = texture.width() as usize * 4;
        let mut pixels = vec![0; stride * texture.height() as usize];
        texture.download(&mut pixels, stride);
        let mut staged = Vec::new();
        cells(grid.view.upcast_ref(), &mut staged);
        assert_eq!(staged.len(), 4);
        for cell in staged {
            let bounds = cell.compute_bounds(&content).unwrap();
            let offset = (bounds.y() as usize + 4) * stride + (bounds.x() as usize + 4) * 4;
            let actual = [pixels[offset + 2], pixels[offset + 1], pixels[offset]];
            let expected = if cell.has_css_class("deleted-cell") {
                [42_u8, 33, 42]
            } else if cell.has_css_class("changed-cell") {
                [47, 46, 33]
            } else {
                [22, 28, 32]
            };
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "draft cell tint: {actual} != {expected}"
                );
            }
        }
        window.close();
    }
}
