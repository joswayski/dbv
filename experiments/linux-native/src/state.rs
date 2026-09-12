//! Native workbench state: workspaces and tabs.

use dbm_engine::models::{ConnectionProfile, DatabaseRef};
use gtk4 as gtk;
use uuid::Uuid;

pub type TabId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabKind {
    Table { schema: String, table: String },
    Query { database: String },
}

pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub kind: TabKind,
    pub profile_id: Uuid,
    pub widget: gtk::Widget,
}

#[derive(Clone)]
pub struct Workspace {
    pub profile: ConnectionProfile,
    pub databases: Vec<DatabaseRef>,
}

impl Workspace {
    pub fn database_names(&self) -> Vec<String> {
        self.databases
            .iter()
            .map(|database| database.name.clone())
            .collect()
    }
}
