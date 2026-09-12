//! C ABI bridge between the SwiftUI macOS frontend and the shared DBM engines.
//!
//! The Swift side sends one JSON request per call and receives one JSON
//! response. Calls are blocking on purpose: Swift runs them on a background
//! task, which keeps the ABI small and avoids callback lifetime problems.
//!
//! Request shapes (`op` selects the operation):
//!
//! ```json
//! {"op":"list_profiles"}
//! {"op":"save_profile","input":{...},"password":"…"}
//! {"op":"delete_profile","profileId":"…"}
//! {"op":"test_profile","input":{...}}
//! {"op":"connect","profileId":"…"}
//! {"op":"connect_database","profileId":"…","database":"…"}
//! {"op":"disconnect","profileId":"…"}
//! {"op":"schema_tree","profileId":"…"}
//! {"op":"table_page","request":{...}}
//! {"op":"run_query","profileId":"…","sql":"…","maxRows":10000}
//! {"op":"history","profileId":"…","database":"…","limit":100}
//! {"op":"export_csv","request":{...},"path":"…"}
//! {"op":"import_url","url":"postgresql://…"}
//! ```
//!
//! Every response is `{"ok": <payload>}` or `{"error": "<message>"}`.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Arc, OnceLock};

use dbm_engine::error::{AppError, AppResult};
use dbm_engine::models::{
    ConnectionProfile, QueryHistoryEntry, SaveProfileInput, TablePageRequest,
};
use dbm_engine::session::DbSession;
use dbm_engine::state::AppState;
use dbm_workbench::format;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

static STATE: OnceLock<Arc<AppState>> = OnceLock::new();
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("dbm-engine")
            .build()
            .expect("the DBM engine runtime must start")
    })
}

fn state() -> AppResult<&'static Arc<AppState>> {
    STATE
        .get()
        .ok_or_else(|| AppError::Storage("the bridge is not initialized".to_owned()))
}

fn init_state(state: AppState) -> AppResult<&'static Arc<AppState>> {
    let state = Arc::new(state);
    Ok(STATE.get_or_init(|| state))
}

/// Initializes the engine with the user's local profile database.
fn initialize() -> AppResult<&'static Arc<AppState>> {
    if let Some(state) = STATE.get() {
        return Ok(state);
    }
    init_state(AppState::new()?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveProfileRequest {
    input: SaveProfileInput,
    #[serde(default)]
    password: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunQueryRequest {
    profile_id: Uuid,
    sql: String,
    #[serde(default)]
    max_rows: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportRequest {
    request: TablePageRequest,
    path: String,
}

/// Handles one request. Exposed for tests and used by the C entry point.
pub fn call(state: &Arc<AppState>, request: &str) -> String {
    let request: Value = match serde_json::from_str(request) {
        Ok(value) => value,
        Err(error) => return error_json(&format!("invalid request JSON: {error}")),
    };
    let op = request
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match handle(state, op, &request) {
        Ok(value) => json!({ "ok": value }).to_string(),
        Err(error) => error_json(&format::error_message(&error)),
    }
}

fn handle(state: &Arc<AppState>, op: &str, request: &Value) -> AppResult<Value> {
    match op {
        "list_profiles" => Ok(serde_json::to_value(state.profile_summaries()?)
            .map_err(|error| AppError::Storage(error.to_string()))?),
        "save_profile" => {
            let request: SaveProfileRequest = decode(request)?;
            let profile = state.save_profile(request.input.clone())?;
            match request.password.as_deref() {
                Some("") => state.credentials.delete_password(profile.id)?,
                Some(password) => state.credentials.save_password(profile.id, password)?,
                None => {}
            }
            runtime().block_on(state.disconnect(profile.id));
            Ok(serde_json::to_value(profile)
                .map_err(|error| AppError::Storage(error.to_string()))?)
        }
        "delete_profile" => {
            let profile_id = uuid_field(request, "profileId")?;
            runtime().block_on(state.delete_profile(profile_id))?;
            Ok(Value::Null)
        }
        "test_profile" => {
            let request: SaveProfileRequest = decode(request)?;
            let existing = request.input.id.map(|id| state.profile(id)).transpose()?;
            let profile = request.input.to_profile(existing.as_ref())?;
            let password = request
                .password
                .filter(|password| !password.is_empty())
                .or_else(|| state.credentials.get_password(profile.id).ok().flatten());
            runtime().block_on(async {
                let session = DbSession::connect(profile, password).await?;
                session.close().await;
                Ok(Value::Null)
            })
        }
        "connect" => {
            let profile_id = uuid_field(request, "profileId")?;
            let profile = state.profile(profile_id)?;
            connect(state, profile)
        }
        "connect_database" => {
            let profile_id = uuid_field(request, "profileId")?;
            let database = string_field(request, "database")?;
            if database.trim().is_empty() {
                return Err(AppError::InvalidInput("database is required".to_owned()));
            }
            let mut profile = state.profile(profile_id)?;
            profile.default_database = database.trim().to_owned();
            runtime().block_on(state.disconnect(profile_id));
            connect(state, profile)
        }
        "disconnect" => {
            let profile_id = uuid_field(request, "profileId")?;
            runtime().block_on(state.disconnect(profile_id));
            Ok(Value::Null)
        }
        "schema_tree" => {
            let profile_id = uuid_field(request, "profileId")?;
            runtime().block_on(async {
                let session = state.session(profile_id).await?;
                let tree = session.schema_tree().await?;
                serde_json::to_value(tree).map_err(|error| AppError::Storage(error.to_string()))
            })
        }
        "table_page" => {
            let request: TablePageRequest = decode_field(request, "request")?;
            runtime().block_on(async {
                let session = state.session(request.profile_id).await?;
                let page = session.table_page(&request).await?;
                serde_json::to_value(page).map_err(|error| AppError::Storage(error.to_string()))
            })
        }
        "run_query" => {
            let request: RunQueryRequest = decode(request)?;
            runtime().block_on(async {
                let session = state.session(request.profile_id).await?;
                let response = session.run_query(&request.sql, request.max_rows).await;
                let success = response.is_ok();
                let duration_ms = response.as_ref().map_or(0, |response| response.duration_ms);
                let entry = QueryHistoryEntry {
                    id: Uuid::new_v4(),
                    profile_id: request.profile_id,
                    database: session.profile().default_database.clone(),
                    sql: request.sql,
                    executed_at: chrono::Utc::now(),
                    duration_ms,
                    success,
                };
                state.store.add_history(&entry)?;
                let response = response?;
                serde_json::to_value(response).map_err(|error| AppError::Storage(error.to_string()))
            })
        }
        "history" => {
            let profile_id = uuid_field(request, "profileId")?;
            let database = string_field(request, "database")?;
            let limit = request.get("limit").and_then(Value::as_u64).unwrap_or(100);
            let entries = state.store.list_history(
                profile_id,
                &database,
                u32::try_from(limit).unwrap_or(100).clamp(1, 500),
            )?;
            Ok(serde_json::to_value(entries)
                .map_err(|error| AppError::Storage(error.to_string()))?)
        }
        "export_csv" => {
            let request: ExportRequest = decode(request)?;
            runtime().block_on(async {
                let session = state.session(request.request.profile_id).await?;
                let mut offset = 0_u32;
                let mut document = String::new();
                let mut rows_written = 0_u64;
                loop {
                    let page = session
                        .table_page(&TablePageRequest {
                            offset,
                            limit: format::EXPORT_PAGE_SIZE,
                            include_total: Some(false),
                            ..request.request.clone()
                        })
                        .await?;
                    if offset == 0 {
                        document.push_str(&format::csv_line(
                            &page
                                .columns
                                .iter()
                                .map(|name| Value::String(name.clone()))
                                .collect::<Vec<_>>(),
                        ));
                    }
                    for row in &page.rows {
                        document.push('\n');
                        document.push_str(&format::csv_line(row));
                    }
                    rows_written += page.rows.len() as u64;
                    if !page.has_more {
                        break;
                    }
                    offset += format::EXPORT_PAGE_SIZE;
                }
                std::fs::write(&request.path, document)
                    .map_err(|error| AppError::Storage(error.to_string()))?;
                Ok(json!(rows_written))
            })
        }
        "import_url" => {
            let url = string_field(request, "url")?;
            let imported = dbm_workbench::connection_url::parse_connection_url(&url)
                .map_err(AppError::InvalidInput)?;
            Ok(json!({
                "engine": imported.engine,
                "host": imported.host,
                "port": imported.port,
                "username": imported.username,
                "defaultDatabase": imported.default_database,
                "tlsMode": imported.tls_mode,
                "password": imported.password,
                "suggestedName": imported.suggested_name,
            }))
        }
        other => Err(AppError::Unsupported(format!("unknown op {other:?}"))),
    }
}

fn connect(state: &Arc<AppState>, profile: ConnectionProfile) -> AppResult<Value> {
    runtime().block_on(async {
        let session = state.connect(profile.clone()).await?;
        let databases = session.list_databases().await?;
        let schema = session.schema_tree().await?;
        Ok(json!({
            "profile": profile,
            "databases": databases,
            "schema": schema,
        }))
    })
}

fn decode<T: for<'de> Deserialize<'de>>(value: &Value) -> AppResult<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| AppError::InvalidInput(format!("invalid request: {error}")))
}

fn decode_field<T: for<'de> Deserialize<'de>>(value: &Value, field: &str) -> AppResult<T> {
    let inner = value
        .get(field)
        .ok_or_else(|| AppError::InvalidInput(format!("missing {field}")))?;
    decode(inner)
}

fn uuid_field(value: &Value, field: &str) -> AppResult<Uuid> {
    let text = string_field(value, field)?;
    Uuid::parse_str(&text).map_err(|_| AppError::InvalidInput(format!("invalid {field}")))
}

fn string_field(value: &Value, field: &str) -> AppResult<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| AppError::InvalidInput(format!("missing {field}")))
}

fn error_json(message: &str) -> String {
    json!({ "error": message }).to_string()
}

fn to_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new("{\"error\":\"response contained a NUL byte\"}")
            .expect("static JSON")
            .into_raw(),
    }
}

/// Initializes the bridge and returns the saved profiles as JSON.
#[no_mangle]
pub extern "C" fn dbm_init() -> *mut c_char {
    let response = match initialize().and_then(|state| {
        let summaries = state.profile_summaries()?;
        serde_json::to_value(summaries).map_err(|error| AppError::Storage(error.to_string()))
    }) {
        Ok(profiles) => json!({ "ok": profiles }).to_string(),
        Err(error) => error_json(&format::error_message(&error)),
    };
    to_c_string(response)
}

/// Handles one JSON request and returns a JSON response. The caller owns the
/// returned pointer and must release it with `dbm_free`.
///
/// # Safety
///
/// `request` must be null or a pointer to a NUL-terminated C string that stays
/// valid for the duration of the call. The returned pointer must be freed with
/// `dbm_free` and must not be used after that.
#[no_mangle]
pub unsafe extern "C" fn dbm_call(request: *const c_char) -> *mut c_char {
    if request.is_null() {
        return to_c_string(error_json("missing request"));
    }
    let request = unsafe { CStr::from_ptr(request) };
    let request = match request.to_str() {
        Ok(request) => request,
        Err(_) => return to_c_string(error_json("request was not valid UTF-8")),
    };
    let response = match state() {
        Ok(state) => call(state, request),
        Err(error) => error_json(&format::error_message(&error)),
    };
    to_c_string(response)
}

/// Frees a string returned by `dbm_init` or `dbm_call`.
///
/// # Safety
///
/// `pointer` must be null or a pointer previously returned by `dbm_init` or
/// `dbm_call` that has not already been freed. Passing any other pointer is
/// undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn dbm_free(pointer: *mut c_char) {
    if !pointer.is_null() {
        drop(unsafe { CString::from_raw(pointer) });
    }
}

/// Frees a session owned by the bridge. Present so the Swift side can shut down
/// deterministically; the process exit path also handles it.
#[no_mangle]
pub extern "C" fn dbm_shutdown() {
    if let Some(state) = STATE.get() {
        runtime().block_on(async {
            let sessions: Vec<Uuid> = state.sessions.lock().await.keys().copied().collect();
            for profile_id in sessions {
                state.disconnect(profile_id).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_engine::keyring_store::CredentialStore;
    use dbm_engine::storage::LocalStore;

    fn test_state() -> Arc<AppState> {
        let path = std::env::temp_dir().join(format!("dbm-bridge-{}.sqlite3", Uuid::new_v4()));
        let state = AppState {
            store: LocalStore::from_path(&path).expect("store"),
            credentials: CredentialStore,
            sessions: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        };
        Arc::new(state)
    }

    #[test]
    fn rejects_invalid_requests() {
        let state = test_state();
        let response: Value = serde_json::from_str(&call(&state, "not json")).expect("json");
        assert!(response.get("error").is_some());

        let response: Value =
            serde_json::from_str(&call(&state, "{\"op\":\"nope\"}")).expect("json");
        assert!(response["error"]
            .as_str()
            .is_some_and(|message| message.contains("unknown op")));
    }

    #[test]
    fn profiles_round_trip_through_the_bridge() {
        let state = test_state();
        let save = json!({
            "op": "save_profile",
            "input": {
                "name": "Local",
                "color": "#38bdf8",
                "engine": "postgres",
                "host": "localhost",
                "port": 5432,
                "username": "postgres",
                "defaultDatabase": "postgres",
                "tlsMode": "preferred",
                "caCertPath": null,
                "ssh": null,
                "readOnly": false,
                "password": null
            }
        });
        let response: Value = serde_json::from_str(&call(&state, &save.to_string())).expect("json");
        let profile_id = response["ok"]["id"].as_str().expect("id").to_owned();

        let response: Value =
            serde_json::from_str(&call(&state, "{\"op\":\"list_profiles\"}")).expect("json");
        assert_eq!(response["ok"].as_array().map(Vec::len), Some(1));

        let delete = json!({ "op": "delete_profile", "profileId": profile_id });
        let response: Value =
            serde_json::from_str(&call(&state, &delete.to_string())).expect("json");
        eprintln!("delete response: {response}");
        assert!(response.get("error").is_none(), "delete failed: {response}");

        let response: Value =
            serde_json::from_str(&call(&state, "{\"op\":\"list_profiles\"}")).expect("json");
        assert_eq!(response["ok"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn imports_connection_urls() {
        let state = test_state();
        let request = json!({
            "op": "import_url",
            "url": "postgresql://postgres:secret@db.example:5432/app?sslmode=require"
        });
        let response: Value =
            serde_json::from_str(&call(&state, &request.to_string())).expect("json");
        assert_eq!(response["ok"]["host"], "db.example");
        assert_eq!(response["ok"]["defaultDatabase"], "app");
        assert_eq!(response["ok"]["tlsMode"], "required");
    }
}
