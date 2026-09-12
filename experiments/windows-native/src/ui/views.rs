//! Custom-painted views: sidebar, chrome, table browser, and query workbench.

use dbm_engine::models::{DatabaseEngine, SchemaNode};
use dbm_workbench::format;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use super::{column_width, Action, FieldId, Layout, TabKind, Ui, ViewId};
use crate::render::{height, rect, width, Renderer, TextAlign};
use crate::theme::{self, Font};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Danger,
    Link,
}

impl Ui {
    // ------------------------------------------------------------------
    // Sidebar
    // ------------------------------------------------------------------

    pub fn paint_sidebar(&mut self, r: &mut Renderer, layout: &Layout) {
        let area = layout.sidebar;
        let _ = r.fill_rect(area, theme::SIDEBAR_BG, 1.0);
        let _ = r.vline(area.right, area.top, area.bottom, theme::BORDER);

        // Brand row
        let brand = rect(area.left, area.top, area.right, area.top + 54.0);
        let _ = r.fill_round_rect(
            rect(
                brand.left + 14.0,
                brand.top + 12.0,
                brand.left + 48.0,
                brand.top + 44.0,
            ),
            7.0,
            theme::PANEL_RAISED,
            1.0,
        );
        let _ = r.draw_text(
            "DB",
            rect(
                brand.left + 14.0,
                brand.top + 12.0,
                brand.left + 48.0,
                brand.top + 44.0,
            ),
            theme::ACCENT,
            Font::UiBold,
            TextAlign::Center,
            false,
        );
        let _ = r.draw_text(
            "DBM",
            rect(
                brand.left + 56.0,
                brand.top + 12.0,
                brand.right - 40.0,
                brand.top + 30.0,
            ),
            theme::TEXT,
            Font::UiBold,
            TextAlign::Leading,
            false,
        );
        let _ = r.draw_text(
            "database manager",
            rect(
                brand.left + 56.0,
                brand.top + 26.0,
                brand.right - 40.0,
                brand.top + 44.0,
            ),
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
            false,
        );
        let _ = r.hline(brand.left, brand.right, brand.bottom, theme::BORDER);

        // Connections header
        let mut y = brand.bottom + 10.0;
        let _ = r.draw_text(
            "CONNECTIONS",
            rect(area.left + 14.0, y, area.right - 140.0, y + 18.0),
            theme::MUTED,
            Font::Eyebrow,
            TextAlign::Leading,
            false,
        );
        let new_button = rect(area.right - 126.0, y - 3.0, area.right - 10.0, y + 23.0);
        self.button(
            r,
            new_button,
            "New connection",
            ButtonKind::Primary,
            Action::NewConnection,
            true,
        );
        y += 30.0;

        let list_area = rect(area.left, y, area.right, area.bottom - 42.0);
        self.scroll_region(ViewId::Sidebar, list_area);
        let scroll = self.scroll_offset(ViewId::Sidebar);
        let _ = r.push_clip(list_area);
        r.translate(0.0, -scroll);
        let mut cursor_y = y + 4.0;
        let profiles: Vec<_> = self.profiles.clone();
        for profile in &profiles {
            cursor_y = self.paint_connection_group(r, layout, profile, cursor_y);
        }
        r.reset_transform();
        r.pop_clip();

        // Footer
        let footer = rect(area.left, area.bottom - 42.0, area.right, area.bottom);
        let _ = r.hline(footer.left, footer.right, footer.top, theme::BORDER);
        let chip = rect(
            footer.left + 14.0,
            footer.top + 12.0,
            footer.left + 96.0,
            footer.top + 30.0,
        );
        let _ = r.fill_round_rect(chip, 4.0, theme::SUCCESS, 0.12);
        let _ = r.stroke_round_rect(chip, 4.0, theme::SUCCESS, 1.0);
        let _ = r.draw_text(
            "LOCAL ONLY",
            chip,
            theme::SUCCESS,
            Font::Eyebrow,
            TextAlign::Center,
            false,
        );
    }

    fn paint_connection_group(
        &mut self,
        r: &mut Renderer,
        layout: &Layout,
        profile: &dbm_engine::models::ConnectionProfile,
        y: f32,
    ) -> f32 {
        let area = layout.sidebar;
        let profile_id = profile.id;
        let connected = self.workspaces.contains_key(&profile_id);
        let active = self.active_profile == Some(profile_id);
        let expanded = active && connected && self.expanded.contains(&profile_id);
        let color = theme::parse_hex(format::profile_color(profile));
        let row = rect(area.left + 8.0, y, area.right - 8.0, y + 44.0);

        if active {
            let _ = r.fill_round_rect(row, 7.0, theme::PANEL_RAISED, 1.0);
            let _ = r.fill_rect(
                rect(row.left, row.top + 6.0, row.left + 3.0, row.bottom - 6.0),
                color,
                1.0,
            );
        } else if self.hovered(&Action::SelectProfile(profile_id)) {
            let _ = r.fill_round_rect(row, 7.0, theme::PANEL_HOVER, 1.0);
        }

        let dot = rect(
            row.left + 12.0,
            row.top + 17.0,
            row.left + 22.0,
            row.top + 27.0,
        );
        let _ = r.fill_round_rect(dot, 5.0, color, 1.0);

        let name_area = rect(
            row.left + 30.0,
            row.top + 6.0,
            row.right - 76.0,
            row.top + 24.0,
        );
        let _ = r.draw_text_ellipsis(
            &profile.name,
            name_area,
            theme::TEXT,
            Font::UiBold,
            TextAlign::Leading,
        );
        let meta = if profile.username.is_empty() {
            format!(
                "{} · {}",
                format::engine_label(profile.engine),
                profile.host
            )
        } else {
            format!(
                "{} · {}@{}",
                format::engine_label(profile.engine),
                profile.username,
                profile.host
            )
        };
        let meta_area = rect(
            row.left + 30.0,
            row.top + 22.0,
            row.right - 76.0,
            row.top + 38.0,
        );
        let _ = r.draw_text_ellipsis(
            &meta,
            meta_area,
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
        );
        self.region(row, Action::SelectProfile(profile_id));

        if connected {
            let toggle = rect(
                row.right - 56.0,
                row.top + 12.0,
                row.right - 34.0,
                row.top + 32.0,
            );
            if self.hovered(&Action::ToggleProfile(profile_id)) {
                let _ = r.fill_round_rect(toggle, 5.0, theme::PANEL_HOVER, 1.0);
            }
            let _ = r.draw_text(
                if expanded { "⌃" } else { "⌄" },
                toggle,
                theme::MUTED,
                Font::Small,
                TextAlign::Center,
                false,
            );
            self.region(toggle, Action::ToggleProfile(profile_id));
        }

        let menu = rect(
            row.right - 32.0,
            row.top + 12.0,
            row.right - 10.0,
            row.top + 32.0,
        );
        if self.hovered(&Action::ToggleMenu(profile_id)) {
            let _ = r.fill_round_rect(menu, 5.0, theme::PANEL_HOVER, 1.0);
        }
        let _ = r.draw_text("⋯", menu, theme::MUTED, Font::Ui, TextAlign::Center, false);
        self.region(menu, Action::ToggleMenu(profile_id));

        let mut next_y = row.bottom + 4.0;

        if self.open_menu == Some(profile_id) {
            let menu_rect = rect(
                row.right - 170.0,
                row.bottom - 2.0,
                row.right - 10.0,
                row.bottom + 56.0,
            );
            let _ = r.fill_round_rect(menu_rect, 6.0, theme::PANEL_RAISED, 1.0);
            let _ = r.stroke_round_rect(menu_rect, 6.0, theme::BORDER_STRONG, 1.0);
            let edit = rect(
                menu_rect.left + 6.0,
                menu_rect.top + 6.0,
                menu_rect.right - 6.0,
                menu_rect.top + 28.0,
            );
            let _ = r.draw_text(
                "Edit connection",
                edit,
                if self.hovered(&Action::EditProfile(profile_id)) {
                    theme::ACCENT
                } else {
                    theme::TEXT
                },
                Font::Small,
                TextAlign::Leading,
                false,
            );
            self.region(edit, Action::EditProfile(profile_id));
            if connected {
                let disconnect = rect(
                    menu_rect.left + 6.0,
                    menu_rect.top + 28.0,
                    menu_rect.right - 6.0,
                    menu_rect.top + 50.0,
                );
                let _ = r.draw_text(
                    "Disconnect",
                    disconnect,
                    if self.hovered(&Action::Disconnect(profile_id)) {
                        theme::DANGER
                    } else {
                        theme::TEXT
                    },
                    Font::Small,
                    TextAlign::Leading,
                    false,
                );
                self.region(disconnect, Action::Disconnect(profile_id));
            }
            next_y = menu_rect.bottom + 6.0;
        }

        if expanded {
            next_y = self.paint_workspace_panel(r, layout, profile, next_y);
        }
        next_y + 6.0
    }

    fn paint_workspace_panel(
        &mut self,
        r: &mut Renderer,
        layout: &Layout,
        profile: &dbm_engine::models::ConnectionProfile,
        y: f32,
    ) -> f32 {
        let area = layout.sidebar;
        let profile_id = profile.id;
        let Some(workspace) = self.workspaces.get(&profile_id).cloned() else {
            return y;
        };
        let panel = rect(area.left + 8.0, y, area.right - 8.0, y + 300.0);
        let _ = r.fill_round_rect(panel, 7.0, theme::BG, 0.35);

        let mut cursor = y + 10.0;
        let label = if profile.engine == DatabaseEngine::Redis {
            "DATABASE INDEX"
        } else {
            "DATABASE"
        };
        let _ = r.draw_text(
            label,
            rect(panel.left + 10.0, cursor, panel.right - 10.0, cursor + 16.0),
            theme::MUTED,
            Font::Eyebrow,
            TextAlign::Leading,
            false,
        );
        cursor += 18.0;

        // Database selector
        let select = rect(panel.left + 10.0, cursor, panel.right - 10.0, cursor + 28.0);
        let _ = r.fill_round_rect(select, 6.0, theme::PANEL, 1.0);
        let _ = r.stroke_round_rect(select, 6.0, theme::BORDER, 1.0);
        let _ = r.draw_text_ellipsis(
            &workspace.profile.default_database,
            rect(
                select.left + 8.0,
                select.top,
                select.right - 24.0,
                select.bottom,
            ),
            theme::TEXT,
            Font::Ui,
            TextAlign::Leading,
        );
        let _ = r.draw_text(
            "▾",
            rect(
                select.right - 22.0,
                select.top,
                select.right - 6.0,
                select.bottom,
            ),
            theme::MUTED,
            Font::Small,
            TextAlign::Center,
            false,
        );
        self.region(select, Action::ToggleDatabases(profile_id));
        if self.open_databases == Some(profile_id) {
            let names = workspace.database_names();
            let height = (names.len() as f32 * 24.0 + 8.0).min(240.0);
            let popup = rect(
                select.left,
                select.bottom + 2.0,
                select.right,
                select.bottom + 2.0 + height,
            );
            let _ = r.fill_round_rect(popup, 6.0, theme::PANEL_RAISED, 1.0);
            let _ = r.stroke_round_rect(popup, 6.0, theme::BORDER_STRONG, 1.0);
            let mut item_y = popup.top + 4.0;
            for name in &names {
                let item = rect(popup.left + 4.0, item_y, popup.right - 4.0, item_y + 24.0);
                let selected = name == &workspace.profile.default_database;
                if selected {
                    let _ = r.fill_round_rect(item, 4.0, theme::ACCENT, 0.15);
                } else if self.hovered(&Action::SelectDatabase(profile_id, name.clone())) {
                    let _ = r.fill_round_rect(item, 4.0, theme::PANEL_HOVER, 1.0);
                }
                let _ = r.draw_text(
                    name,
                    rect(item.left + 6.0, item.top, item.right - 6.0, item.bottom),
                    theme::TEXT,
                    Font::Ui,
                    TextAlign::Leading,
                    false,
                );
                self.region(item, Action::SelectDatabase(profile_id, name.clone()));
                item_y += 24.0;
            }
        }
        cursor = select.bottom + 10.0;

        // Schema heading
        let _ = r.draw_text(
            if profile.engine == DatabaseEngine::Redis {
                "KEYSPACE"
            } else {
                "SCHEMA"
            },
            rect(panel.left + 10.0, cursor, panel.right - 70.0, cursor + 16.0),
            theme::MUTED,
            Font::Eyebrow,
            TextAlign::Leading,
            false,
        );
        let refresh = rect(
            panel.right - 66.0,
            cursor - 4.0,
            panel.right - 10.0,
            cursor + 18.0,
        );
        let _ = r.draw_text(
            "Refresh",
            refresh,
            theme::ACCENT,
            Font::Small,
            TextAlign::Center,
            false,
        );
        self.region(refresh, Action::RefreshSchema(profile_id));
        cursor += 22.0;

        let selected = self
            .active_table()
            .filter(|(table_profile, _, _)| *table_profile == profile_id)
            .map(|(_, schema, table)| (schema, table));
        let tree = self.schemas.get(&profile_id).cloned().unwrap_or_default();
        let tree_area = rect(panel.left, cursor, panel.right, panel.bottom - 8.0);
        self.scroll_region(ViewId::Schema(profile_id), tree_area);
        let tree_scroll = self.scroll_offset(ViewId::Schema(profile_id));
        let _ = r.push_clip(tree_area);
        r.translate(0.0, -tree_scroll);
        let mut node_y = cursor + 2.0;
        for node in &tree {
            node_y = self.paint_schema_node(r, profile_id, node, 0, node_y, &selected);
        }
        r.reset_transform();
        r.pop_clip();

        (panel.bottom + 6.0).max(cursor + 8.0)
    }

    fn paint_schema_node(
        &mut self,
        r: &mut Renderer,
        profile_id: uuid::Uuid,
        node: &SchemaNode,
        depth: usize,
        y: f32,
        selected: &Option<(String, String)>,
    ) -> f32 {
        let area = self.sidebar_tree_rect();
        let is_table = node.table.is_some() && node.schema.is_some();
        let key = format!("{}:{}:{}:{}", profile_id, depth, node.kind, node.name);
        let row = rect(
            area.left + 10.0 + depth as f32 * 12.0,
            y,
            area.right - 12.0,
            y + 22.0,
        );
        let is_selected = is_table
            && selected.as_ref().is_some_and(|(schema, table)| {
                Some(schema) == node.schema.as_ref() && Some(table) == node.table.as_ref()
            });
        let collapsed = self.collapsed_nodes.contains(&key);

        let action = if is_table {
            Action::OpenTable {
                profile: profile_id,
                schema: node.schema.clone().unwrap_or_default(),
                table: node.table.clone().unwrap_or_default(),
            }
        } else {
            Action::ToggleNode(key.clone())
        };
        if is_selected {
            let _ = r.fill_round_rect(row, 4.0, theme::ACCENT, 0.15);
        } else if self.hovered(&action) {
            let _ = r.fill_round_rect(row, 4.0, theme::PANEL_HOVER, 1.0);
        }

        let caret = if is_table {
            "▧".to_owned()
        } else if collapsed {
            "›".to_owned()
        } else {
            "⌄".to_owned()
        };
        let _ = r.draw_text(
            &caret,
            rect(row.left + 4.0, row.top, row.left + 18.0, row.bottom),
            theme::MUTED,
            Font::Small,
            TextAlign::Center,
            false,
        );
        let badge_color = match node.kind.as_str() {
            "table" => theme::ACCENT,
            "key" => 0xa78bfa,
            "view" => theme::SUCCESS,
            _ => theme::MUTED,
        };
        let _ = r.draw_text(
            match node.kind.as_str() {
                "table" => "T",
                "key" => "K",
                "view" => "V",
                _ => "S",
            },
            rect(row.left + 18.0, row.top, row.left + 32.0, row.bottom),
            badge_color,
            Font::Eyebrow,
            TextAlign::Center,
            false,
        );
        let _ = r.draw_text_ellipsis(
            &node.name,
            rect(row.left + 36.0, row.top, row.right, row.bottom),
            theme::TEXT,
            Font::Ui,
            TextAlign::Leading,
        );
        self.region(row, action);

        let mut next_y = row.bottom + 2.0;
        if !is_table && !collapsed {
            for child in &node.children {
                next_y = self.paint_schema_node(r, profile_id, child, depth + 1, next_y, selected);
            }
        }
        next_y
    }

    fn sidebar_tree_rect(&self) -> D2D_RECT_F {
        // The schema tree is clipped to the sidebar; a generous width keeps
        // indentation math simple because rows are clipped anyway.
        rect(0.0, 0.0, theme::SIDEBAR_WIDTH, 4000.0)
    }

    // ------------------------------------------------------------------
    // Chrome
    // ------------------------------------------------------------------

    pub fn paint_topbar(&mut self, r: &mut Renderer, layout: &Layout) {
        let area = layout.topbar;
        let _ = r.fill_rect(area, theme::BG, 1.0);
        let _ = r.hline(area.left, area.right, area.bottom, theme::BORDER);
        let profile = self
            .active_profile
            .and_then(|profile_id| self.profile(profile_id));
        match profile {
            Some(profile) => {
                let color = theme::parse_hex(format::profile_color(&profile));
                let dot = rect(
                    area.left + 18.0,
                    area.top + 18.0,
                    area.left + 30.0,
                    area.top + 30.0,
                );
                let _ = r.fill_round_rect(dot, 6.0, color, 1.0);
                let _ = r.draw_text(
                    &profile.name,
                    rect(
                        area.left + 38.0,
                        area.top + 6.0,
                        area.right - 20.0,
                        area.top + 26.0,
                    ),
                    theme::TEXT,
                    Font::UiBold,
                    TextAlign::Leading,
                    false,
                );
                let meta = if profile.username.is_empty() {
                    format!(
                        "{}:{} / {}",
                        profile.host, profile.port, profile.default_database
                    )
                } else {
                    format!(
                        "{}@{}:{} / {}",
                        profile.username, profile.host, profile.port, profile.default_database
                    )
                };
                let _ = r.draw_text(
                    &meta,
                    rect(
                        area.left + 38.0,
                        area.top + 24.0,
                        area.right - 20.0,
                        area.top + 42.0,
                    ),
                    theme::MUTED,
                    Font::Small,
                    TextAlign::Leading,
                    false,
                );
            }
            None => {
                let _ = r.draw_text(
                    "No active connection",
                    rect(area.left + 18.0, area.top, area.right, area.bottom),
                    theme::MUTED,
                    Font::Ui,
                    TextAlign::Leading,
                    false,
                );
            }
        }
    }

    pub fn paint_tab_strip(&mut self, r: &mut Renderer, layout: &Layout) {
        let area = layout.tabs;
        let _ = r.fill_rect(area, theme::SIDEBAR_BG, 1.0);
        let _ = r.hline(area.left, area.right, area.bottom, theme::BORDER);
        let _ = r.push_clip(area);
        let mut x = area.left;
        let tabs: Vec<(u64, String, String)> = self
            .tabs
            .iter()
            .map(|tab| {
                let color = self.profile(tab.profile_id).map_or_else(
                    || format::DEFAULT_CONNECTION_COLOR.to_owned(),
                    |profile| format::profile_color(&profile).to_owned(),
                );
                (tab.id, tab.title.clone(), color)
            })
            .collect();
        for (tab_id, title, color) in tabs {
            let width = (r.text_width(&title, Font::Ui).unwrap_or(80.0) + 46.0).clamp(90.0, 260.0);
            let tab_rect = rect(x, area.top, x + width, area.bottom);
            let active = self.active_tab == Some(tab_id);
            if active {
                let _ = r.fill_rect(tab_rect, theme::PANEL_RAISED, 1.0);
                let accent = theme::parse_hex(&color);
                let _ = r.fill_rect(
                    rect(
                        tab_rect.left,
                        tab_rect.bottom - 2.0,
                        tab_rect.right,
                        tab_rect.bottom,
                    ),
                    accent,
                    1.0,
                );
            } else if self.hovered(&Action::SelectTab(tab_id)) {
                let _ = r.fill_rect(tab_rect, theme::PANEL_RAISED, 0.5);
            }
            let _ = r.vline(tab_rect.right, tab_rect.top, tab_rect.bottom, theme::BORDER);
            let _ = r.draw_text_ellipsis(
                &title,
                rect(
                    tab_rect.left + 12.0,
                    tab_rect.top,
                    tab_rect.right - 30.0,
                    tab_rect.bottom,
                ),
                if active { theme::TEXT } else { theme::MUTED },
                Font::Ui,
                TextAlign::Leading,
            );
            self.region(
                rect(
                    tab_rect.left,
                    tab_rect.top,
                    tab_rect.right - 24.0,
                    tab_rect.bottom,
                ),
                Action::SelectTab(tab_id),
            );
            let close = rect(
                tab_rect.right - 24.0,
                tab_rect.top + 8.0,
                tab_rect.right - 6.0,
                tab_rect.bottom - 8.0,
            );
            let _ = r.draw_text(
                "×",
                close,
                if self.hovered(&Action::CloseTab(tab_id)) {
                    theme::DANGER
                } else {
                    theme::MUTED
                },
                Font::Ui,
                TextAlign::Center,
                false,
            );
            self.region(close, Action::CloseTab(tab_id));
            x += width;
        }
        let has_workspace = self
            .active_profile
            .is_some_and(|profile_id| self.workspaces.contains_key(&profile_id));
        if has_workspace {
            let new_tab = rect(x + 4.0, area.top + 6.0, x + 34.0, area.bottom - 6.0);
            let _ = r.draw_text(
                "＋",
                new_tab,
                if self.hovered(&Action::NewQuery) {
                    theme::ACCENT
                } else {
                    theme::MUTED
                },
                Font::Ui,
                TextAlign::Center,
                false,
            );
            self.region(new_tab, Action::NewQuery);
        }
        r.pop_clip();
    }

    pub fn paint_content(&mut self, r: &mut Renderer, layout: &Layout) {
        let area = layout.content;
        let _ = r.fill_rect(area, theme::BG, 1.0);
        let Some(tab) = self
            .active_tab
            .and_then(|tab_id| self.tabs.iter().find(|tab| tab.id == tab_id).cloned())
        else {
            self.paint_welcome(r, area);
            return;
        };
        match tab.kind {
            TabKind::Table { .. } => self.paint_table(r, area, tab.id),
            TabKind::Query { .. } => self.paint_query(r, area, tab.id),
        }
    }

    fn paint_welcome(&mut self, r: &mut Renderer, area: D2D_RECT_F) {
        let selected = self
            .active_profile
            .and_then(|profile_id| self.profile(profile_id));
        let connected = self
            .active_profile
            .is_some_and(|profile_id| self.workspaces.contains_key(&profile_id));
        let (title, body) = match selected {
            Some(profile) => (
                profile.name,
                if connected {
                    "Choose a table from the sidebar or open a new query with the plus button above."
                } else {
                    "This connection is selected but not connected. Select it again to connect."
                },
            ),
            None => (
                "No connection selected".to_owned(),
                if self.profiles.is_empty() {
                    "Create a connection from the sidebar to get started."
                } else {
                    "Select a saved connection from the sidebar to browse its data."
                },
            ),
        };
        let middle = (area.top + area.bottom) / 2.0;
        let _ = r.draw_text(
            &title,
            rect(area.left, middle - 34.0, area.right, middle - 6.0),
            theme::TEXT,
            Font::Title,
            TextAlign::Center,
            false,
        );
        let _ = r.draw_text(
            body,
            rect(area.left + 40.0, middle, area.right - 40.0, middle + 30.0),
            theme::MUTED,
            Font::Ui,
            TextAlign::Center,
            false,
        );
    }

    // ------------------------------------------------------------------
    // Overlays
    // ------------------------------------------------------------------

    pub fn paint_overlays(&mut self, r: &mut Renderer, layout: &Layout) {
        if let Some((message, shown_at)) = self.error.clone() {
            if self.time - shown_at > 10.0 {
                self.error = None;
            } else {
                let banner = rect(
                    layout.sidebar.right,
                    layout.topbar.bottom,
                    layout.width,
                    layout.topbar.bottom + 32.0,
                );
                let _ = r.fill_rect(banner, theme::DANGER, 0.12);
                let _ = r.hline(banner.left, banner.right, banner.bottom, theme::DANGER);
                let _ = r.draw_text_ellipsis(
                    &message,
                    rect(
                        banner.left + 14.0,
                        banner.top,
                        banner.right - 30.0,
                        banner.bottom,
                    ),
                    theme::DANGER,
                    Font::Ui,
                    TextAlign::Leading,
                );
                let dismiss = rect(
                    banner.right - 26.0,
                    banner.top + 4.0,
                    banner.right - 8.0,
                    banner.bottom - 4.0,
                );
                let _ = r.draw_text(
                    "×",
                    dismiss,
                    theme::DANGER,
                    Font::Ui,
                    TextAlign::Center,
                    false,
                );
                self.region(dismiss, Action::DismissError);
            }
        }
        if let Some((message, shown_at)) = self.toast.clone() {
            if self.time - shown_at > 6.0 {
                self.toast = None;
            } else {
                let width = r.text_width(&message, Font::Ui).unwrap_or(200.0) + 32.0;
                let toast = rect(
                    layout.width - width - 18.0,
                    layout.height - 60.0,
                    layout.width - 18.0,
                    layout.height - 18.0,
                );
                let _ = r.fill_round_rect(toast, 7.0, theme::PANEL_RAISED, 1.0);
                let _ = r.stroke_round_rect(toast, 7.0, theme::BORDER_STRONG, 1.0);
                let _ = r.draw_text_ellipsis(
                    &message,
                    rect(
                        toast.left + 12.0,
                        toast.top,
                        toast.right - 12.0,
                        toast.bottom,
                    ),
                    theme::TEXT,
                    Font::Ui,
                    TextAlign::Leading,
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Table browser
    // ------------------------------------------------------------------

    fn paint_table(&mut self, r: &mut Renderer, area: D2D_RECT_F, tab_id: u64) {
        let Some(state) = self.tables.get(&tab_id) else {
            return;
        };
        let title = format!("{}.{}", state.schema, state.table);
        let status = state.status.clone();
        let loading = state.loading;
        let order_by = state.order_by.clone();
        let columns: Vec<(String, String)> = state.columns.clone();
        let page = state.page.clone();

        let toolbar = rect(area.left, area.top, area.right, area.top + 54.0);
        let _ = r.fill_rect(toolbar, theme::BG, 1.0);
        let _ = r.draw_text(
            "TABLE VIEWER",
            rect(
                toolbar.left + 14.0,
                toolbar.top + 8.0,
                toolbar.right,
                toolbar.top + 24.0,
            ),
            theme::MUTED,
            Font::Eyebrow,
            TextAlign::Leading,
            false,
        );
        let _ = r.draw_text_ellipsis(
            &title,
            rect(
                toolbar.left + 14.0,
                toolbar.top + 24.0,
                toolbar.right - 420.0,
                toolbar.top + 48.0,
            ),
            theme::TEXT,
            Font::Title,
            TextAlign::Leading,
        );
        let mut x = toolbar.right - 12.0;
        for (label, action) in [
            ("Refresh", Action::RefreshTable(tab_id)),
            ("Export CSV", Action::ExportTableCsv(tab_id)),
            ("Copy CSV", Action::CopyTableCsv(tab_id)),
        ] {
            let width = r.text_width(label, Font::Ui).unwrap_or(70.0) + 24.0;
            let button_rect = rect(x - width, toolbar.top + 13.0, x, toolbar.top + 41.0);
            self.button(
                r,
                button_rect,
                label,
                ButtonKind::Secondary,
                action,
                !loading,
            );
            x -= width + 6.0;
        }
        let _ = r.hline(toolbar.left, toolbar.right, toolbar.bottom, theme::BORDER);

        let status_rect = rect(
            area.left + 14.0,
            toolbar.bottom + 6.0,
            area.right - 14.0,
            toolbar.bottom + 24.0,
        );
        let _ = r.draw_text(
            &status,
            status_rect,
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
            false,
        );

        let grid_top = status_rect.bottom + 6.0;
        let grid_bottom = area.bottom - 40.0;
        let grid = rect(area.left, grid_top, area.right, grid_bottom);
        if columns.is_empty() {
            let _ = r.draw_text(
                if loading {
                    "Loading…"
                } else {
                    "No rows match this view."
                },
                grid,
                theme::MUTED,
                Font::Ui,
                TextAlign::Center,
                false,
            );
        } else {
            let widths: Vec<f32> = columns
                .iter()
                .map(|(_, data_type)| column_width(data_type).max(90.0))
                .collect();
            let total_width: f32 = widths.iter().sum();
            let h_scroll = self.scroll_offset(ViewId::TableX(tab_id));
            let v_scroll = self.scroll_offset(ViewId::Table(tab_id));
            let header_height = 28.0;
            let row_height = 26.0;

            self.scroll_region(
                ViewId::Table(tab_id),
                rect(grid.left, grid.top + header_height, grid.right, grid.bottom),
            );
            self.scroll_region(
                ViewId::TableX(tab_id),
                rect(grid.left, grid.top, grid.right, grid.bottom),
            );

            let _ = r.push_clip(rect(
                grid.left,
                grid.top + header_height,
                grid.right,
                grid.bottom,
            ));
            r.translate(grid.left - h_scroll, grid.top + header_height - v_scroll);

            if let Some(page) = &page {
                let mut row_y = 0.0;
                for row in &page.rows {
                    let mut cell_x = 0.0;
                    // Only the metadata columns are shown: PostgreSQL table
                    // pages carry a trailing `__dbm_xmin` value for mutations.
                    for (column_index, _) in columns.iter().enumerate() {
                        let width = widths.get(column_index).copied().unwrap_or(120.0);
                        let value = row.get(column_index).unwrap_or(&serde_json::Value::Null);
                        let cell = rect(
                            cell_x + 6.0,
                            row_y,
                            cell_x + width - 6.0,
                            row_y + row_height,
                        );
                        let text = format::display_value(value);
                        let color = if value.is_null() {
                            theme::SUBTLE
                        } else {
                            theme::TEXT
                        };
                        let _ =
                            r.draw_text_ellipsis(&text, cell, color, Font::Ui, TextAlign::Leading);
                        cell_x += width;
                    }
                    let _ = r.hline(0.0, total_width, row_y + row_height, theme::BORDER);
                    row_y += row_height;
                }
            }
            r.reset_transform();
            r.pop_clip();

            // Sticky header
            let _ = r.push_clip(rect(
                grid.left,
                grid.top,
                grid.right,
                grid.top + header_height,
            ));
            r.translate(grid.left - h_scroll, grid.top);
            let _ = r.fill_rect(
                rect(0.0, 0.0, total_width, header_height),
                theme::PANEL,
                1.0,
            );
            let mut cell_x = 0.0;
            for (column_index, (name, _)) in columns.iter().enumerate() {
                let width = widths.get(column_index).copied().unwrap_or(120.0);
                let action = Action::SortColumn {
                    tab: tab_id,
                    column: name.clone(),
                };
                let header = rect(cell_x, 0.0, cell_x + width, header_height);
                if self.hovered(&action) {
                    let _ = r.fill_rect(header, theme::PANEL_HOVER, 1.0);
                }
                let marker = match &order_by {
                    Some(order) if order.column == *name => {
                        if order.descending {
                            " ↓"
                        } else {
                            " ↑"
                        }
                    }
                    _ => "",
                };
                let _ = r.draw_text_ellipsis(
                    &format!("{name}{marker}"),
                    rect(
                        header.left + 6.0,
                        header.top,
                        header.right - 6.0,
                        header.bottom,
                    ),
                    theme::MUTED,
                    Font::UiBold,
                    TextAlign::Leading,
                );
                let _ = r.vline(header.right, header.top, header.bottom, theme::BORDER);
                self.region(header, action);
                cell_x += width;
            }
            r.reset_transform();
            r.pop_clip();
            let _ = r.hline(
                grid.left,
                grid.right,
                grid.top + header_height,
                theme::BORDER,
            );

            // Pagination
            let page_index = self.tables.get(&tab_id).map_or(0, |state| state.page_index);
            let has_more = page.as_ref().is_some_and(|page| page.has_more);
            let pager = rect(area.left, grid_bottom, area.right, area.bottom);
            let mut px = pager.left + 14.0;
            if page_index > 0 {
                let previous = rect(px, pager.top + 6.0, px + 96.0, pager.top + 32.0);
                self.button(
                    r,
                    previous,
                    "← Previous",
                    ButtonKind::Secondary,
                    Action::PagePrev(tab_id),
                    !loading,
                );
                px += 104.0;
            }
            let label = format!("Page {}", page_index + 1);
            let width = r.text_width(&label, Font::Small).unwrap_or(60.0);
            let _ = r.draw_text(
                &label,
                rect(px, pager.top, px + width + 10.0, pager.bottom),
                theme::MUTED,
                Font::Small,
                TextAlign::Leading,
                false,
            );
            px += width + 12.0;
            if has_more {
                let next = rect(px, pager.top + 6.0, px + 88.0, pager.top + 32.0);
                self.button(
                    r,
                    next,
                    "Next →",
                    ButtonKind::Secondary,
                    Action::PageNext(tab_id),
                    !loading,
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // Query workbench
    // ------------------------------------------------------------------

    fn paint_query(&mut self, r: &mut Renderer, area: D2D_RECT_F, tab_id: u64) {
        let Some(state) = self.queries.get(&tab_id) else {
            return;
        };
        let engine = state.engine;
        let database = state.database.clone();
        let running = state.running;
        let meta = state.meta.clone();
        let error = state.error.clone();
        let executed = state.executed_sql.clone();
        let history: Vec<(String, String, bool)> = state
            .history
            .iter()
            .map(|entry| {
                (
                    entry.sql.split_whitespace().collect::<Vec<_>>().join(" "),
                    entry.executed_at.format("%H:%M:%S").to_string(),
                    entry.success,
                )
            })
            .collect();
        let response = state.response.clone();
        let title = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map_or_else(|| "Query".to_owned(), |tab| tab.title.clone());

        let toolbar = rect(area.left, area.top, area.right, area.top + 54.0);
        let _ = r.fill_rect(toolbar, theme::BG, 1.0);
        let _ = r.draw_text(
            if engine == DatabaseEngine::Redis {
                "REDIS WORKBENCH"
            } else {
                "SQL WORKBENCH"
            },
            rect(
                toolbar.left + 14.0,
                toolbar.top + 8.0,
                toolbar.right,
                toolbar.top + 24.0,
            ),
            theme::MUTED,
            Font::Eyebrow,
            TextAlign::Leading,
            false,
        );
        let _ = r.draw_text_ellipsis(
            &format!("{title}  ·  {database}"),
            rect(
                toolbar.left + 14.0,
                toolbar.top + 24.0,
                toolbar.right - 300.0,
                toolbar.top + 48.0,
            ),
            theme::TEXT,
            Font::Title,
            TextAlign::Leading,
        );
        let run_label = if engine == DatabaseEngine::Redis {
            "Run command"
        } else {
            "Run statement"
        };
        let run_width = r.text_width(run_label, Font::Ui).unwrap_or(90.0) + 26.0;
        let run = rect(
            toolbar.right - 12.0 - run_width,
            toolbar.top + 13.0,
            toolbar.right - 12.0,
            toolbar.top + 41.0,
        );
        self.button(
            r,
            run,
            run_label,
            ButtonKind::Primary,
            Action::RunQuery(tab_id),
            !running,
        );
        let refresh = rect(
            run.left - 92.0,
            toolbar.top + 13.0,
            run.left - 8.0,
            toolbar.top + 41.0,
        );
        self.button(
            r,
            refresh,
            "Refresh",
            ButtonKind::Secondary,
            Action::RefreshQuery(tab_id),
            executed.is_some() && !running,
        );
        let _ = r.hline(toolbar.left, toolbar.right, toolbar.bottom, theme::BORDER);

        let history_width = 230.0_f32.min(width(area) * 0.3);
        let editor_area = rect(
            area.left,
            toolbar.bottom,
            area.right - history_width,
            area.bottom - 40.0,
        );
        let history_area = rect(
            area.right - history_width,
            toolbar.bottom,
            area.right,
            area.bottom,
        );
        let results_height = (height(area) * 0.35).clamp(140.0, 320.0);
        let editor_view = rect(
            editor_area.left,
            editor_area.top,
            editor_area.right,
            editor_area.bottom - results_height,
        );
        let results_view = rect(
            editor_area.left,
            editor_area.bottom - results_height,
            editor_area.right,
            editor_area.bottom,
        );

        // Editor
        let _ = r.fill_rect(editor_view, theme::EDITOR_BG, 1.0);
        let editor_scroll = self.scroll_offset(ViewId::Editor(tab_id));
        self.scroll_region(ViewId::Editor(tab_id), editor_view);
        let field_id = FieldId::Query(tab_id);
        let focused = self.focused(field_id);
        let (text, caret, selection, field_scroll) = self.fields.get(&field_id).map_or_else(
            || (String::new(), 0, None, 0.0),
            |field| {
                (
                    field.text.clone(),
                    field.caret,
                    field.selection(),
                    field.scroll,
                )
            },
        );
        self.field_rect(field_id, editor_view);
        self.region(editor_view, Action::ClickField { field: field_id });
        let line_height = r.line_height(Font::Mono);
        let first_line = (editor_scroll / line_height).floor().max(0.0) as usize;
        let visible_lines = (height(editor_view) / line_height).ceil() as usize + 1;
        let _ = r.push_clip(editor_view);
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let mut line_index = first_line;
        let mut char_index = 0_usize;
        for line in lines.iter().skip(first_line).take(visible_lines) {
            let line_y = editor_view.top + 8.0 + line_index as f32 * line_height - editor_scroll;
            let line_start = char_index;
            let line_end = line_start + line.chars().count();
            if let Some((selection_start, selection_end)) = selection {
                let from = selection_start.max(line_start).min(line_end);
                let to = selection_end.max(line_start).min(line_end);
                if from < to {
                    let x0 = r
                        .caret_x(line, from - line_start, Font::Mono)
                        .unwrap_or(0.0);
                    let x1 = r.caret_x(line, to - line_start, Font::Mono).unwrap_or(x0);
                    let _ = r.fill_rect(
                        rect(
                            editor_view.left + 10.0 + x0 - field_scroll,
                            line_y,
                            editor_view.left + 10.0 + x1 - field_scroll,
                            line_y + line_height,
                        ),
                        theme::ACCENT,
                        0.22,
                    );
                }
            }
            let _ = r.draw_text(
                line,
                rect(
                    editor_view.left + 10.0 - field_scroll,
                    line_y,
                    editor_view.right - 10.0,
                    line_y + line_height,
                ),
                theme::TEXT,
                Font::Mono,
                TextAlign::Leading,
                true,
            );
            if focused && caret >= line_start && (caret < line_end || line_index == lines.len() - 1)
            {
                let x = r
                    .caret_x(line, caret - line_start, Font::Mono)
                    .unwrap_or(0.0);
                let _ = r.fill_rect(
                    rect(
                        editor_view.left + 10.0 + x - field_scroll,
                        line_y,
                        editor_view.left + 11.0 + x - field_scroll,
                        line_y + line_height,
                    ),
                    theme::ACCENT,
                    1.0,
                );
            }
            char_index = line_end + 1;
            line_index += 1;
        }
        r.pop_clip();
        let hint = if engine == DatabaseEngine::Redis {
            "The command under the cursor or the selection will run · Ctrl+Enter · results capped at 10,000 rows"
        } else {
            "The statement under the cursor or the selected SQL will run · Ctrl+Enter · results capped at 10,000 rows"
        };
        let _ = r.draw_text_ellipsis(
            hint,
            rect(
                editor_view.left + 10.0,
                editor_view.bottom + 4.0,
                editor_view.right - 10.0,
                editor_view.bottom + 22.0,
            ),
            theme::MUTED,
            Font::Small,
            TextAlign::Leading,
        );
        let _ = r.vline(
            editor_area.right,
            editor_area.top,
            editor_area.bottom,
            theme::BORDER,
        );
        let _ = r.hline(
            editor_area.left,
            editor_area.right,
            results_view.top,
            theme::BORDER,
        );

        // History
        let _ = r.fill_rect(history_area, theme::SIDEBAR_BG, 1.0);
        let _ = r.draw_text(
            "HISTORY",
            rect(
                history_area.left + 10.0,
                history_area.top + 6.0,
                history_area.right - 10.0,
                history_area.top + 24.0,
            ),
            theme::MUTED,
            Font::Eyebrow,
            TextAlign::Leading,
            false,
        );
        let list = rect(
            history_area.left,
            history_area.top + 26.0,
            history_area.right,
            history_area.bottom,
        );
        self.scroll_region(ViewId::History(tab_id), list);
        let history_scroll = self.scroll_offset(ViewId::History(tab_id));
        let _ = r.push_clip(list);
        r.translate(0.0, -history_scroll);
        if history.is_empty() {
            let _ = r.draw_text(
                "Run a query to start history.",
                rect(
                    list.left + 10.0,
                    list.top,
                    list.right - 10.0,
                    list.top + 24.0,
                ),
                theme::MUTED,
                Font::Small,
                TextAlign::Leading,
                false,
            );
        }
        let mut item_y = list.top + 2.0;
        for (index, (sql, time, success)) in history.iter().enumerate() {
            let item = rect(list.left + 6.0, item_y, list.right - 6.0, item_y + 40.0);
            let action = Action::HistoryItem { tab: tab_id, index };
            if self.hovered(&action) {
                let _ = r.fill_round_rect(item, 5.0, theme::PANEL_HOVER, 1.0);
            }
            let glyph = if *success { "✓" } else { "!" };
            let _ = r.draw_text(
                glyph,
                rect(item.left + 4.0, item.top, item.left + 20.0, item.bottom),
                if *success {
                    theme::SUCCESS
                } else {
                    theme::DANGER
                },
                Font::Small,
                TextAlign::Center,
                false,
            );
            let label = if sql.chars().count() > 60 {
                format!("{}…", sql.chars().take(60).collect::<String>())
            } else {
                sql.clone()
            };
            let _ = r.draw_text_ellipsis(
                &label,
                rect(
                    item.left + 22.0,
                    item.top + 2.0,
                    item.right - 4.0,
                    item.top + 22.0,
                ),
                theme::TEXT,
                Font::Small,
                TextAlign::Leading,
            );
            let _ = r.draw_text(
                time,
                rect(
                    item.left + 22.0,
                    item.top + 18.0,
                    item.right - 4.0,
                    item.bottom - 2.0,
                ),
                theme::MUTED,
                Font::Small,
                TextAlign::Leading,
                false,
            );
            self.region(item, action);
            item_y += 42.0;
        }
        r.reset_transform();
        r.pop_clip();

        // Results
        let _ = r.fill_rect(results_view, theme::BG, 1.0);
        let meta_rect = rect(
            results_view.left + 14.0,
            results_view.top + 4.0,
            results_view.right - 14.0,
            results_view.top + 22.0,
        );
        let _ = r.draw_text_ellipsis(
            &meta,
            meta_rect,
            if error.is_some() {
                theme::DANGER
            } else {
                theme::MUTED
            },
            Font::Small,
            TextAlign::Leading,
        );
        if let Some(error) = &error {
            let _ = r.draw_text_ellipsis(
                error,
                rect(
                    results_view.left + 14.0,
                    results_view.top + 20.0,
                    results_view.right - 14.0,
                    results_view.top + 38.0,
                ),
                theme::DANGER,
                Font::Small,
                TextAlign::Leading,
            );
        }
        if let Some(response) = &response {
            if !response.columns.is_empty() {
                let grid = rect(
                    results_view.left,
                    results_view.top + 24.0,
                    results_view.right,
                    results_view.bottom,
                );
                let widths: Vec<f32> = response
                    .columns
                    .iter()
                    .map(|column| column_width(&column.data_type).max(90.0))
                    .collect();
                let total_width: f32 = widths.iter().sum();
                let h_scroll = self.scroll_offset(ViewId::ResultsX(tab_id));
                let v_scroll = self.scroll_offset(ViewId::Results(tab_id));
                self.scroll_region(ViewId::Results(tab_id), grid);
                self.scroll_region(ViewId::ResultsX(tab_id), grid);
                let _ = r.push_clip(grid);
                r.translate(grid.left - h_scroll, grid.top - v_scroll);
                let row_height = 24.0;
                let _ = r.fill_rect(rect(0.0, 0.0, total_width, 24.0), theme::PANEL, 1.0);
                let mut cell_x = 0.0;
                for (index, column) in response.columns.iter().enumerate() {
                    let width = widths.get(index).copied().unwrap_or(120.0);
                    let _ = r.draw_text_ellipsis(
                        &column.name,
                        rect(cell_x + 6.0, 0.0, cell_x + width - 6.0, 24.0),
                        theme::MUTED,
                        Font::UiBold,
                        TextAlign::Leading,
                    );
                    let _ = r.vline(cell_x + width, 0.0, height(grid), theme::BORDER);
                    cell_x += width;
                }
                let _ = r.hline(0.0, total_width, 24.0, theme::BORDER);
                let mut row_y = 24.0;
                for row in &response.rows {
                    let mut cell_x = 0.0;
                    for (index, _) in response.columns.iter().enumerate() {
                        let width = widths.get(index).copied().unwrap_or(120.0);
                        let value = row.get(index).unwrap_or(&serde_json::Value::Null);
                        let text = format::display_value(value);
                        let _ = r.draw_text_ellipsis(
                            &text,
                            rect(
                                cell_x + 6.0,
                                row_y,
                                cell_x + width - 6.0,
                                row_y + row_height,
                            ),
                            if value.is_null() {
                                theme::SUBTLE
                            } else {
                                theme::TEXT
                            },
                            Font::Ui,
                            TextAlign::Leading,
                        );
                        cell_x += width;
                    }
                    let _ = r.hline(0.0, total_width, row_y + row_height, theme::BORDER);
                    row_y += row_height;
                }
                r.reset_transform();
                r.pop_clip();
            }
        }
    }

    // ------------------------------------------------------------------
    // Small widgets
    // ------------------------------------------------------------------

    pub fn button(
        &mut self,
        r: &mut Renderer,
        rect: D2D_RECT_F,
        label: &str,
        kind: ButtonKind,
        action: Action,
        enabled: bool,
    ) {
        let hovered = enabled && self.hovered(&action);
        let pressed = enabled && self.pressed(&action);
        let (bg, fg, border) = match kind {
            ButtonKind::Primary => (
                if hovered {
                    theme::ACCENT
                } else {
                    theme::ACCENT_STRONG
                },
                theme::INK_ON_ACCENT,
                None,
            ),
            ButtonKind::Secondary => (
                if hovered {
                    theme::PANEL_HOVER
                } else {
                    theme::PANEL_RAISED
                },
                if hovered { theme::ACCENT } else { theme::TEXT },
                Some(theme::BORDER_STRONG),
            ),
            ButtonKind::Danger => (theme::PANEL_RAISED, theme::DANGER, Some(theme::DANGER)),
            ButtonKind::Link => (theme::BG, theme::ACCENT, None),
        };
        if !enabled {
            let _ = r.fill_round_rect(rect, 6.0, theme::PANEL, 1.0);
            let _ = r.draw_text(
                label,
                rect,
                theme::SUBTLE,
                Font::Ui,
                TextAlign::Center,
                false,
            );
            return;
        }
        if kind == ButtonKind::Link {
            let _ = r.draw_text(label, rect, fg, Font::Small, TextAlign::Center, false);
        } else {
            let _ = r.fill_round_rect(rect, 6.0, bg, if pressed { 0.85 } else { 1.0 });
            if let Some(border) = border {
                let _ = r.stroke_round_rect(rect, 6.0, border, 1.0);
            }
            let _ = r.draw_text(label, rect, fg, Font::Ui, TextAlign::Center, false);
        }
        self.region(rect, action);
    }

    /// Paints a text field and records its rect for click-to-caret handling.
    pub fn text_field(&mut self, r: &mut Renderer, bounds: D2D_RECT_F, field_id: FieldId) {
        let focused = self.focused(field_id);
        let (value, placeholder, password, scroll, selection) =
            self.fields.get(&field_id).map_or_else(
                || (String::new(), String::new(), false, 0.0, None),
                |field| {
                    (
                        field.text.clone(),
                        field.placeholder.clone(),
                        field.password,
                        field.scroll,
                        field.selection(),
                    )
                },
            );
        let _ = r.fill_round_rect(bounds, 6.0, theme::PANEL, 1.0);
        let _ = r.stroke_round_rect(
            bounds,
            6.0,
            if focused {
                theme::ACCENT
            } else {
                theme::BORDER
            },
            1.0,
        );
        let inner = rect(
            bounds.left + 8.0,
            bounds.top,
            bounds.right - 8.0,
            bounds.bottom,
        );
        let display = if password {
            "•".repeat(value.chars().count())
        } else {
            value.clone()
        };
        if display.is_empty() && !focused {
            let _ = r.draw_text_ellipsis(
                &placeholder,
                inner,
                theme::SUBTLE,
                Font::Ui,
                TextAlign::Leading,
            );
        } else {
            let _ = r.push_clip(inner);
            let _ = r.draw_text(
                &display,
                rect(
                    inner.left - scroll,
                    inner.top,
                    inner.right + 400.0,
                    inner.bottom,
                ),
                theme::TEXT,
                Font::Ui,
                TextAlign::Leading,
                true,
            );
            r.pop_clip();
        }
        if focused {
            if let Some((start, end)) = selection {
                let x0 = r.caret_x(&display, start, Font::Ui).unwrap_or(0.0);
                let x1 = r.caret_x(&display, end, Font::Ui).unwrap_or(x0);
                let _ = r.fill_rect(
                    rect(
                        inner.left + x0 - scroll,
                        bounds.top + 4.0,
                        inner.left + x1 - scroll,
                        bounds.bottom - 4.0,
                    ),
                    theme::ACCENT,
                    0.25,
                );
            }
            let caret = self.fields.get(&field_id).map_or(0, |field| field.caret);
            let x = r.caret_x(&display, caret, Font::Ui).unwrap_or(0.0);
            let _ = r.fill_rect(
                rect(
                    inner.left + x - scroll,
                    bounds.top + 4.0,
                    inner.left + x + 1.0 - scroll,
                    bounds.bottom - 4.0,
                ),
                theme::ACCENT,
                1.0,
            );
        }
        self.field_rect(field_id, inner);
        self.region(bounds, Action::ClickField { field: field_id });
    }
}
