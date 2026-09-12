use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::models::{
    ConnectionProfile, DatabaseEngine, QueryHistoryEntry, SaveProfileInput, SshConfig, TlsMode,
};

#[derive(Debug, Clone)]
pub struct LocalStore {
    path: PathBuf,
}

impl LocalStore {
    pub fn new() -> AppResult<Self> {
        let project_dirs = ProjectDirs::from("io", "github", "dbm").ok_or_else(|| {
            AppError::Storage("could not determine application data directory".into())
        })?;
        let directory = project_dirs.data_local_dir();
        std::fs::create_dir_all(directory).map_err(|error| AppError::Storage(error.to_string()))?;
        let store = Self {
            path: directory.join("dbm.sqlite3"),
        };
        store.migrate()?;
        Ok(store)
    }

    /// Opens a store at an explicit path. Used by tests and tooling that must
    /// not touch the user's profile database.
    pub fn from_path(path: impl AsRef<Path>) -> AppResult<Self> {
        let store = Self {
            path: path.as_ref().to_path_buf(),
        };
        store.migrate()?;
        Ok(store)
    }

    fn open(&self) -> AppResult<Connection> {
        Ok(Connection::open(&self.path)?)
    }

    fn migrate(&self) -> AppResult<()> {
        let connection = self.open()?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS profiles (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL,
                 color TEXT,
                 engine TEXT NOT NULL DEFAULT 'postgres',
                 host TEXT NOT NULL,
                 port INTEGER NOT NULL,
                 username TEXT NOT NULL,
                 default_database TEXT NOT NULL,
                 tls_mode TEXT NOT NULL,
                 ca_cert_path TEXT,
                 ssh_json TEXT,
                 read_only INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS query_history (
                 id TEXT PRIMARY KEY NOT NULL,
                 profile_id TEXT NOT NULL,
                 database_name TEXT NOT NULL,
                 sql TEXT NOT NULL,
                 executed_at TEXT NOT NULL,
                 duration_ms INTEGER NOT NULL,
                 success INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS query_history_profile_time
                 ON query_history(profile_id, executed_at DESC);
             CREATE INDEX IF NOT EXISTS query_history_profile_database_time
                 ON query_history(profile_id, database_name, executed_at DESC);",
        )?;
        ensure_column(
            &connection,
            "profiles",
            "engine",
            "TEXT NOT NULL DEFAULT 'postgres'",
        )?;
        Ok(())
    }

    pub fn list_profiles(&self) -> AppResult<Vec<ConnectionProfile>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, name, color, engine, host, port, username, default_database,
                    tls_mode, ca_cert_path, ssh_json, read_only, created_at, updated_at
             FROM profiles ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], profile_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_profile(&self, id: Uuid) -> AppResult<Option<ConnectionProfile>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, name, color, engine, host, port, username, default_database,
                    tls_mode, ca_cert_path, ssh_json, read_only, created_at, updated_at
             FROM profiles WHERE id = ?1",
        )?;
        statement
            .query_row([id.to_string()], profile_from_row)
            .optional()
            .map_err(AppError::from)
    }

    pub fn save_profile(&self, input: &SaveProfileInput) -> AppResult<ConnectionProfile> {
        let existing = match input.id {
            Some(id) => self.get_profile(id)?,
            None => None,
        };
        let profile = input.to_profile(existing.as_ref())?;

        let ssh_json = profile
            .ssh
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| AppError::Storage(error.to_string()))?;
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO profiles (
                id, name, color, engine, host, port, username, default_database, tls_mode,
                ca_cert_path, ssh_json, read_only, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                color = excluded.color,
                engine = excluded.engine,
                host = excluded.host,
                port = excluded.port,
                username = excluded.username,
                default_database = excluded.default_database,
                tls_mode = excluded.tls_mode,
                ca_cert_path = excluded.ca_cert_path,
                ssh_json = excluded.ssh_json,
                read_only = excluded.read_only,
                updated_at = excluded.updated_at",
            params![
                profile.id.to_string(),
                profile.name,
                profile.color,
                engine_to_string(&profile.engine),
                profile.host,
                i64::from(profile.port),
                profile.username,
                profile.default_database,
                tls_mode_to_string(&profile.tls_mode),
                profile.ca_cert_path,
                ssh_json,
                i64::from(u8::from(profile.read_only)),
                profile.created_at.to_rfc3339(),
                profile.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(profile)
    }

    pub fn delete_profile(&self, id: Uuid) -> AppResult<()> {
        let connection = self.open()?;
        connection.execute("DELETE FROM profiles WHERE id = ?1", [id.to_string()])?;
        connection.execute(
            "DELETE FROM query_history WHERE profile_id = ?1",
            [id.to_string()],
        )?;
        Ok(())
    }

    pub fn add_history(&self, entry: &QueryHistoryEntry) -> AppResult<()> {
        let connection = self.open()?;
        connection.execute(
            "INSERT INTO query_history (
                id, profile_id, database_name, sql, executed_at, duration_ms, success
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.id.to_string(),
                entry.profile_id.to_string(),
                entry.database,
                entry.sql,
                entry.executed_at.to_rfc3339(),
                i64::try_from(entry.duration_ms).unwrap_or(i64::MAX),
                i64::from(u8::from(entry.success)),
            ],
        )?;
        connection.execute(
            "DELETE FROM query_history
             WHERE profile_id = ?1 AND id NOT IN (
                 SELECT id FROM query_history
                 WHERE profile_id = ?1 ORDER BY executed_at DESC LIMIT 500
             )",
            [entry.profile_id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_history(
        &self,
        profile_id: Uuid,
        database: &str,
        limit: u32,
    ) -> AppResult<Vec<QueryHistoryEntry>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, profile_id, database_name, sql, executed_at, duration_ms, success
             FROM query_history WHERE profile_id = ?1 AND database_name = ?2
             ORDER BY executed_at DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![profile_id.to_string(), database, i64::from(limit)],
            query_history_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionProfile> {
    let engine = match row.get::<_, String>(3)?.as_str() {
        "mysql" => DatabaseEngine::Mysql,
        "redis" => DatabaseEngine::Redis,
        _ => DatabaseEngine::Postgres,
    };
    let tls_mode = match row.get::<_, String>(8)?.as_str() {
        "disabled" => TlsMode::Disabled,
        "required" => TlsMode::Required,
        _ => TlsMode::Preferred,
    };
    let ssh = row
        .get::<_, Option<String>>(10)?
        .map(|json| serde_json::from_str::<SshConfig>(&json))
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                10,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(ConnectionProfile {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        name: row.get(1)?,
        color: row.get(2)?,
        engine,
        host: row.get(4)?,
        port: u16::try_from(row.get::<_, i64>(5)?).unwrap_or_else(|_| default_port(&engine)),
        username: row.get(6)?,
        default_database: row.get(7)?,
        tls_mode,
        ca_cert_path: row.get(9)?,
        ssh,
        read_only: row.get::<_, i64>(11)? != 0,
        created_at: parse_datetime(row.get::<_, String>(12)?)?,
        updated_at: parse_datetime(row.get::<_, String>(13)?)?,
    })
}

fn query_history_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueryHistoryEntry> {
    Ok(QueryHistoryEntry {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        profile_id: parse_uuid(row.get::<_, String>(1)?)?,
        database: row.get(2)?,
        sql: row.get(3)?,
        executed_at: parse_datetime(row.get::<_, String>(4)?)?,
        duration_ms: u128::try_from(row.get::<_, i64>(5)?).unwrap_or_default(),
        success: row.get::<_, i64>(6)? != 0,
    })
}

fn parse_uuid(value: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn parse_datetime(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn tls_mode_to_string(mode: &TlsMode) -> &'static str {
    match mode {
        TlsMode::Disabled => "disabled",
        TlsMode::Preferred => "preferred",
        TlsMode::Required => "required",
    }
}

fn engine_to_string(engine: &DatabaseEngine) -> &'static str {
    match engine {
        DatabaseEngine::Postgres => "postgres",
        DatabaseEngine::Mysql => "mysql",
        DatabaseEngine::Redis => "redis",
    }
}

fn default_port(engine: &DatabaseEngine) -> u16 {
    match engine {
        DatabaseEngine::Postgres => 5432,
        DatabaseEngine::Mysql => 3306,
        DatabaseEngine::Redis => 6379,
    }
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> AppResult<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == column);
    if !exists {
        connection.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_round_trip() {
        let path = std::env::temp_dir().join(format!("dbm-test-{}.sqlite3", Uuid::new_v4()));
        let store = LocalStore::from_path(&path).expect("store");
        let input = SaveProfileInput {
            id: None,
            name: "Local".into(),
            color: Some("#22c55e".into()),
            engine: DatabaseEngine::Postgres,
            host: "localhost".into(),
            port: 5432,
            username: "postgres".into(),
            default_database: "postgres".into(),
            tls_mode: TlsMode::Disabled,
            ca_cert_path: None,
            ssh: None,
            read_only: false,
            password: None,
        };
        let profile = store.save_profile(&input).expect("save");
        let profiles = store.list_profiles().expect("list");
        assert_eq!(profiles, vec![profile]);
        std::fs::remove_file(path).expect("remove temp db");
    }

    #[test]
    fn mysql_profiles_round_trip_and_legacy_rows_default_to_postgres() {
        let path = std::env::temp_dir().join(format!("dbm-test-{}.sqlite3", Uuid::new_v4()));
        let store = LocalStore::from_path(&path).expect("store");
        let mysql = store
            .save_profile(&SaveProfileInput {
                id: None,
                name: "Railway MySQL".into(),
                color: None,
                engine: DatabaseEngine::Mysql,
                host: "caboose.proxy.rlwy.net".into(),
                port: 3306,
                username: "root".into(),
                default_database: "railway".into(),
                tls_mode: TlsMode::Required,
                ca_cert_path: None,
                ssh: None,
                read_only: true,
                password: None,
            })
            .expect("save mysql");
        assert_eq!(mysql.engine, DatabaseEngine::Mysql);
        assert_eq!(
            store
                .get_profile(mysql.id)
                .expect("get")
                .expect("found")
                .engine,
            DatabaseEngine::Mysql
        );

        let connection = rusqlite::Connection::open(&path).expect("open");
        connection
            .execute(
                "INSERT INTO profiles (
                    id, name, color, host, port, username, default_database, tls_mode,
                    ca_cert_path, ssh_json, read_only, created_at, updated_at
                 ) VALUES (?1, 'Legacy', NULL, 'localhost', 5432, 'postgres', 'postgres',
                           'preferred', NULL, NULL, 0, ?2, ?2)",
                params![Uuid::new_v4().to_string(), Utc::now().to_rfc3339()],
            )
            .expect("insert legacy without engine");
        drop(connection);

        let profiles = store.list_profiles().expect("list after legacy insert");
        assert!(
            profiles
                .iter()
                .any(|profile| profile.name == "Legacy"
                    && profile.engine == DatabaseEngine::Postgres)
        );
        std::fs::remove_file(path).expect("remove temp db");
    }

    #[test]
    fn redis_profiles_round_trip_and_allow_empty_username() {
        let path = std::env::temp_dir().join(format!("dbm-test-{}.sqlite3", Uuid::new_v4()));
        let store = LocalStore::from_path(&path).expect("store");
        let redis = store
            .save_profile(&SaveProfileInput {
                id: None,
                name: "Local Redis".into(),
                color: None,
                engine: DatabaseEngine::Redis,
                host: "localhost".into(),
                port: 6379,
                username: String::new(),
                default_database: "0".into(),
                tls_mode: TlsMode::Disabled,
                ca_cert_path: None,
                ssh: None,
                read_only: false,
                password: None,
            })
            .expect("save redis");
        assert_eq!(redis.engine, DatabaseEngine::Redis);
        assert_eq!(redis.port, 6379);
        assert_eq!(redis.username, "");
        assert_eq!(
            store
                .get_profile(redis.id)
                .expect("get")
                .expect("found")
                .engine,
            DatabaseEngine::Redis
        );
        std::fs::remove_file(path).expect("remove temp db");
    }

    #[test]
    fn query_history_is_scoped_to_database() {
        let path = std::env::temp_dir().join(format!("dbm-test-{}.sqlite3", Uuid::new_v4()));
        let store = LocalStore::from_path(&path).expect("store");
        let profile_id = Uuid::new_v4();
        let postgres_entry = QueryHistoryEntry {
            id: Uuid::new_v4(),
            profile_id,
            database: "postgres".into(),
            sql: "SELECT 1".into(),
            executed_at: Utc::now(),
            duration_ms: 1,
            success: true,
        };
        let analytics_entry = QueryHistoryEntry {
            id: Uuid::new_v4(),
            profile_id,
            database: "analytics".into(),
            sql: "SELECT 2".into(),
            executed_at: Utc::now(),
            duration_ms: 2,
            success: true,
        };
        store
            .add_history(&postgres_entry)
            .expect("postgres history");
        store
            .add_history(&analytics_entry)
            .expect("analytics history");

        let postgres_history = store
            .list_history(profile_id, "postgres", 100)
            .expect("list postgres");
        assert_eq!(postgres_history.len(), 1);
        assert_eq!(postgres_history[0].id, postgres_entry.id);
        let analytics_history = store
            .list_history(profile_id, "analytics", 100)
            .expect("list analytics");
        assert_eq!(analytics_history.len(), 1);
        assert_eq!(analytics_history[0].id, analytics_entry.id);
        std::fs::remove_file(path).expect("remove temp db");
    }
}
