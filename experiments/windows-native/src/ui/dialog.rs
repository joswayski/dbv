//! Custom modal windows: the connection editor and DBM confirmations.

use super::views::ButtonKind;
use super::{Action, FieldId, Layout, Modal, Ui};
use crate::render::{rect, Renderer, TextAlign};
use crate::theme::{self, Font};
use dbm_engine::models::DatabaseEngine;
use dbm_workbench::format;

const PANEL_WIDTH: f32 = 660.0;

impl Ui {
    pub fn paint_modal(&mut self, r: &mut Renderer, layout: &Layout) {
        let _ = r.fill_rect(rect(0.0, 0.0, layout.width, layout.height), theme::BG, 0.65);
        let modal = self.modal.take();
        match &modal {
            Some(Modal::Profile(form)) => self.paint_profile_dialog(r, layout, form),
            Some(Modal::Confirm(confirm)) => {
                let title = confirm.title.clone();
                let body = confirm.body.clone();
                let label = confirm.confirm_label.clone();
                let danger = confirm.danger;
                self.paint_confirm(r, layout, &title, &body, &label, danger);
            }
            None => {}
        }
        self.modal = modal;
    }

    fn paint_confirm(
        &mut self,
        r: &mut Renderer,
        layout: &Layout,
        title: &str,
        body: &str,
        confirm_label: &str,
        danger: bool,
    ) {
        let width = 460.0_f32.min(layout.width - 40.0);
        let height = 190.0;
        let panel = rect(
            (layout.width - width) / 2.0,
            (layout.height - height) / 2.0,
            (layout.width + width) / 2.0,
            (layout.height + height) / 2.0,
        );
        let _ = r.fill_round_rect(panel, 10.0, theme::BG, 1.0);
        let _ = r.stroke_round_rect(panel, 10.0, theme::BORDER_STRONG, 1.0);
        let _ = r.draw_text(
            title,
            rect(
                panel.left + 20.0,
                panel.top + 18.0,
                panel.right - 20.0,
                panel.top + 44.0,
            ),
            theme::TEXT,
            Font::Title,
            TextAlign::Leading,
            false,
        );
        let _ = r.draw_text(
            body,
            rect(
                panel.left + 20.0,
                panel.top + 50.0,
                panel.right - 20.0,
                panel.top + 110.0,
            ),
            theme::MUTED,
            Font::Ui,
            TextAlign::Leading,
            false,
        );
        let cancel = rect(
            panel.right - 20.0 - 110.0 - 8.0 - 110.0,
            panel.bottom - 46.0,
            panel.right - 20.0 - 118.0,
            panel.bottom - 18.0,
        );
        self.button(
            r,
            cancel,
            "Cancel",
            ButtonKind::Secondary,
            Action::ConfirmCancel,
            true,
        );
        let accept = rect(
            panel.right - 20.0 - 110.0,
            panel.bottom - 46.0,
            panel.right - 20.0,
            panel.bottom - 18.0,
        );
        self.button(
            r,
            accept,
            confirm_label,
            if danger {
                ButtonKind::Danger
            } else {
                ButtonKind::Primary
            },
            Action::ConfirmAccept,
            true,
        );
    }

    fn paint_profile_dialog(
        &mut self,
        r: &mut Renderer,
        layout: &Layout,
        form: &super::ProfileForm,
    ) {
        let width = PANEL_WIDTH.min(layout.width - 40.0);
        let height = (layout.height - 60.0).min(700.0);
        let panel = rect(
            (layout.width - width) / 2.0,
            (layout.height - height) / 2.0,
            (layout.width + width) / 2.0,
            (layout.height + height) / 2.0,
        );
        let _ = r.fill_round_rect(panel, 10.0, theme::BG, 1.0);
        let _ = r.stroke_round_rect(panel, 10.0, theme::BORDER_STRONG, 1.0);

        let editing = form.existing.is_some();
        let _ = r.draw_text(
            if editing {
                "Edit connection"
            } else {
                "New connection"
            },
            rect(
                panel.left + 20.0,
                panel.top + 16.0,
                panel.right - 20.0,
                panel.top + 42.0,
            ),
            theme::TEXT,
            Font::Title,
            TextAlign::Leading,
            false,
        );

        let label_x = panel.left + 20.0;
        let field_x = panel.left + 190.0;
        let field_right = panel.right - 20.0;
        let mut y = panel.top + 56.0;
        let row = 42.0;

        // Engine picker
        let _ = r.draw_text(
            "Database engine",
            rect(label_x, y, field_x - 10.0, y + 28.0),
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
            false,
        );
        let mut engine_x = field_x;
        for engine in [
            DatabaseEngine::Postgres,
            DatabaseEngine::Mysql,
            DatabaseEngine::Redis,
        ] {
            let label = format::engine_label(engine);
            let width = r.text_width(label, Font::Ui).unwrap_or(80.0) + 24.0;
            let button = rect(engine_x, y, engine_x + width, y + 28.0);
            let selected = form.input.engine == engine;
            let action = Action::ModalEngine(engine);
            let _ = r.fill_round_rect(
                button,
                6.0,
                if selected {
                    theme::ACCENT_STRONG
                } else {
                    theme::PANEL_RAISED
                },
                1.0,
            );
            let _ = r.stroke_round_rect(
                button,
                6.0,
                if selected {
                    theme::ACCENT
                } else {
                    theme::BORDER
                },
                1.0,
            );
            let _ = r.draw_text(
                label,
                button,
                if selected {
                    theme::INK_ON_ACCENT
                } else {
                    theme::TEXT
                },
                Font::Ui,
                TextAlign::Center,
                false,
            );
            self.region(button, action);
            engine_x += width + 6.0;
        }
        y += row;

        // Connection URL
        let _ = r.draw_text(
            "Connection URL",
            rect(label_x, y, field_x - 10.0, y + 28.0),
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
            false,
        );
        let import_width = r.text_width("Import URL", Font::Ui).unwrap_or(80.0) + 24.0;
        let url_field = rect(field_x, y, field_right - import_width - 8.0, y + 28.0);
        self.text_field(r, url_field, FieldId::Url);
        let import = rect(field_right - import_width, y, field_right, y + 28.0);
        self.button(
            r,
            import,
            "Import URL",
            ButtonKind::Secondary,
            Action::ModalImportUrl,
            true,
        );
        y += row;

        for (label, field) in [
            ("Name", FieldId::Name),
            ("Host", FieldId::Host),
            ("Port", FieldId::Port),
            ("Username", FieldId::Username),
            ("Database", FieldId::Database),
            ("Password", FieldId::Password),
            ("CA certificate path (optional)", FieldId::CaPath),
        ] {
            let _ = r.draw_text(
                label,
                rect(label_x, y, field_x - 10.0, y + 28.0),
                theme::MUTED,
                Font::Small,
                TextAlign::Leading,
                false,
            );
            self.text_field(r, rect(field_x, y, field_right, y + 28.0), field);
            y += row;
        }

        // TLS
        let _ = r.draw_text(
            "TLS",
            rect(label_x, y, field_x - 10.0, y + 28.0),
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
            false,
        );
        let mut tls_x = field_x;
        for (index, label) in ["Preferred", "Required", "Disabled"].iter().enumerate() {
            let width = r.text_width(label, Font::Ui).unwrap_or(70.0) + 22.0;
            let button = rect(tls_x, y, tls_x + width, y + 28.0);
            let selected = form.tls_index == index;
            let _ = r.fill_round_rect(
                button,
                6.0,
                if selected {
                    theme::ACCENT_STRONG
                } else {
                    theme::PANEL_RAISED
                },
                1.0,
            );
            let _ = r.draw_text(
                label,
                button,
                if selected {
                    theme::INK_ON_ACCENT
                } else {
                    theme::TEXT
                },
                Font::Ui,
                TextAlign::Center,
                false,
            );
            self.region(button, Action::ModalTls(index));
            tls_x += width + 6.0;
        }
        y += row;

        // Color swatches
        let _ = r.draw_text(
            "Connection color",
            rect(label_x, y, field_x - 10.0, y + 28.0),
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
            false,
        );
        let mut swatch_x = field_x;
        for color in format::CONNECTION_COLORS {
            let swatch = rect(swatch_x, y + 3.0, swatch_x + 22.0, y + 25.0);
            let rgb = theme::parse_hex(color);
            let selected = form.color.eq_ignore_ascii_case(color);
            let _ = r.fill_round_rect(swatch, 11.0, rgb, 1.0);
            let _ = r.stroke_round_rect(
                swatch,
                11.0,
                if selected {
                    theme::TEXT
                } else {
                    theme::PANEL_RAISED
                },
                if selected { 2.0 } else { 1.0 },
            );
            self.region(swatch, Action::ModalColor(color.to_owned()));
            swatch_x += 28.0;
        }
        y += row;

        // Read-only
        let checkbox = rect(field_x, y + 4.0, field_x + 16.0, y + 20.0);
        let _ = r.fill_round_rect(
            checkbox,
            4.0,
            if form.read_only {
                theme::ACCENT_STRONG
            } else {
                theme::PANEL
            },
            1.0,
        );
        let _ = r.stroke_round_rect(checkbox, 4.0, theme::BORDER_STRONG, 1.0);
        if form.read_only {
            let _ = r.draw_text(
                "✓",
                checkbox,
                theme::INK_ON_ACCENT,
                Font::Small,
                TextAlign::Center,
                false,
            );
        }
        self.region(
            rect(field_x - 4.0, y, field_right, y + 28.0),
            Action::ModalReadOnly,
        );
        let _ = r.draw_text(
            "Read-only profile (blocks GUI edits and mutations)",
            rect(field_x + 24.0, y, field_right, y + 28.0),
            theme::TEXT,
            Font::Small,
            TextAlign::Leading,
            false,
        );
        y += row + 4.0;

        if let Some((success, message)) = &form.feedback {
            let color = if *success {
                theme::SUCCESS
            } else {
                theme::DANGER
            };
            let _ = r.draw_text_ellipsis(
                message,
                rect(label_x, y, field_right, y + 22.0),
                color,
                Font::Small,
                TextAlign::Leading,
            );
        }
        let _ = r.draw_text(
            "Passwords are stored in your operating system credential manager and are never written to DBM's profile database.",
            rect(label_x, panel.bottom - 96.0, field_right, panel.bottom - 58.0),
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
            false,
        );

        let actions_y = panel.bottom - 46.0;
        let mut x = panel.right - 20.0;
        let save_width = r.text_width("Save & connect", Font::Ui).unwrap_or(110.0) + 26.0;
        let save = rect(x - save_width, actions_y, x, actions_y + 28.0);
        self.button(
            r,
            save,
            "Save & connect",
            ButtonKind::Primary,
            Action::ModalSave,
            !form.busy,
        );
        x -= save_width + 8.0;
        let test_width = r.text_width("Test connection", Font::Ui).unwrap_or(100.0) + 26.0;
        let test = rect(x - test_width, actions_y, x, actions_y + 28.0);
        self.button(
            r,
            test,
            "Test connection",
            ButtonKind::Secondary,
            Action::ModalTest,
            !form.busy,
        );
        x -= test_width + 8.0;
        let cancel = rect(x - 90.0, actions_y, x, actions_y + 28.0);
        self.button(
            r,
            cancel,
            "Cancel",
            ButtonKind::Secondary,
            Action::ModalCancel,
            true,
        );
        if editing {
            let delete = rect(
                panel.left + 20.0,
                actions_y,
                panel.left + 20.0 + 84.0,
                actions_y + 28.0,
            );
            self.button(
                r,
                delete,
                "Delete",
                ButtonKind::Danger,
                Action::ModalDelete,
                true,
            );
        }
    }
}
