use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseEngine {
    #[default]
    Postgres,
    Mysql,
    Redis,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    Disabled,
    #[default]
    Preferred,
    Required,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub private_key_path: Option<String>,
    pub use_agent: bool,
    pub password_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub name: String,
    pub color: Option<String>,
    #[serde(default)]
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub default_database: String,
    pub tls_mode: TlsMode,
    pub ca_cert_path: Option<String>,
    pub ssh: Option<SshConfig>,
    pub read_only: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectionProfile {
    pub fn validate(&self) -> AppResult<()> {
        if self.name.is_empty() || self.host.is_empty() {
            return Err(AppError::InvalidInput(
                "name, host, and username are required".into(),
            ));
        }
        if self.engine != DatabaseEngine::Redis && self.username.is_empty() {
            return Err(AppError::InvalidInput(
                "name, host, and username are required".into(),
            ));
        }
        if self.engine == DatabaseEngine::Redis {
            parse_redis_database(&self.default_database)?;
        } else if self.default_database.is_empty() {
            return Err(AppError::InvalidInput("database is required".into()));
        }
        if self.port == 0 {
            return Err(AppError::InvalidInput(
                "port must be between 1 and 65535".into(),
            ));
        }
        Ok(())
    }
}

pub fn parse_redis_database(value: &str) -> AppResult<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(
            "Redis database index is required".into(),
        ));
    }
    let db = trimmed.parse::<i64>().map_err(|_| {
        AppError::InvalidInput("Redis database must be a non-negative integer".into())
    })?;
    if db < 0 {
        return Err(AppError::InvalidInput(
            "Redis database must be a non-negative integer".into(),
        ));
    }
    Ok(db)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProfileInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub color: Option<String>,
    #[serde(default)]
    pub engine: DatabaseEngine,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub default_database: String,
    #[serde(default)]
    pub tls_mode: TlsMode,
    pub ca_cert_path: Option<String>,
    pub ssh: Option<SshConfig>,
    #[serde(default)]
    pub read_only: bool,
    pub password: Option<String>,
}

impl SaveProfileInput {
    /// Builds a validated profile from this input, preserving `created_at` when
    /// editing an existing profile. Frontends share this so test-connection,
    /// save, and import paths validate identically.
    pub fn to_profile(&self, existing: Option<&ConnectionProfile>) -> AppResult<ConnectionProfile> {
        let now = Utc::now();
        let profile = ConnectionProfile {
            id: self.id.unwrap_or_else(Uuid::new_v4),
            name: self.name.trim().to_owned(),
            color: self.color.clone(),
            engine: self.engine,
            host: self.host.trim().to_owned(),
            port: self.port,
            username: self.username.trim().to_owned(),
            default_database: self.default_database.trim().to_owned(),
            tls_mode: self.tls_mode.clone(),
            ca_cert_path: self.ca_cert_path.clone(),
            ssh: self.ssh.clone(),
            read_only: self.read_only,
            created_at: existing.map_or(now, |profile| profile.created_at),
            updated_at: now,
        };
        profile.validate()?;
        Ok(profile)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub profile: ConnectionProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRef {
    pub name: String,
    pub is_template: bool,
    pub is_connectable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub profile: ConnectionProfile,
    pub databases: Vec<DatabaseRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaNode {
    pub name: String,
    pub kind: String,
    pub schema: Option<String>,
    pub table: Option<String>,
    pub children: Vec<SchemaNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub ordinal: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableMetadata {
    pub schema: String,
    pub table: String,
    pub columns: Vec<TableColumn>,
    pub primary_key: Vec<String>,
    pub has_xmin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    EndsWith,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    In,
    NotIn,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterCondition {
    pub column: String,
    pub operator: FilterOperator,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderSpec {
    pub column: String,
    pub descending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePageRequest {
    pub profile_id: Uuid,
    pub schema: String,
    pub table: String,
    pub offset: u32,
    pub limit: u32,
    pub filters: Vec<FilterCondition>,
    pub order_by: Option<OrderSpec>,
    pub include_total: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    pub metadata: TableMetadata,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total_rows: Option<u64>,
    pub offset: u32,
    pub limit: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowMutation {
    pub original: Vec<Value>,
    pub changes: Vec<Value>,
    pub primary_key: Vec<Value>,
    pub xmin: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationBatch {
    pub profile_id: Uuid,
    pub schema: String,
    pub table: String,
    pub mutations: Vec<RowMutation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub applied: usize,
    pub conflicts: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub profile_id: Uuid,
    pub sql: String,
    pub max_rows: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryColumn {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: usize,
    pub affected_rows: Option<u64>,
    pub duration_ms: u128,
    pub truncated: bool,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryEntry {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub database: String,
    pub sql: String,
    pub executed_at: DateTime<Utc>,
    pub duration_ms: u128,
    pub success: bool,
}
