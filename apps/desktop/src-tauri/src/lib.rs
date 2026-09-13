#![deny(unsafe_code)]

mod updates;

use chrono::Utc;
use dbm_engine::error::AppResult;
use dbm_engine::models::{
    ConnectionProfile, MutationBatch, ProfileSummary, QueryHistoryEntry, QueryRequest,
    SaveProfileInput, TablePageRequest, WorkspaceInfo,
};
use dbm_engine::state::AppState;
use dbm_engine::{models, session};
use uuid::Uuid;

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn profile_for_test(state: &AppState, input: &SaveProfileInput) -> AppResult<ConnectionProfile> {
    let existing = input.id.map(|id| state.profile(id)).transpose()?;
    input.to_profile(existing.as_ref())
}

#[tauri::command]
async fn list_profiles(state: tauri::State<'_, AppState>) -> Result<Vec<ProfileSummary>, String> {
    state.profile_summaries().map_err(command_error)
}

#[tauri::command]
async fn save_profile(
    state: tauri::State<'_, AppState>,
    input: SaveProfileInput,
) -> Result<models::ConnectionProfile, String> {
    let profile = state.save_profile(input.clone()).map_err(command_error)?;
    if let Some(password) = input.password.as_deref() {
        if password.is_empty() {
            state
                .credentials
                .delete_password(profile.id)
                .map_err(command_error)?;
        } else {
            state
                .credentials
                .save_password(profile.id, password)
                .map_err(command_error)?;
        }
    }
    state.disconnect(profile.id).await;
    Ok(profile)
}

#[tauri::command]
async fn delete_profile(state: tauri::State<'_, AppState>, profile_id: Uuid) -> Result<(), String> {
    state
        .delete_profile(profile_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
async fn test_profile(
    state: tauri::State<'_, AppState>,
    input: SaveProfileInput,
) -> Result<(), String> {
    let profile = profile_for_test(&state, &input).map_err(command_error)?;
    let password = input
        .password
        .filter(|password| !password.is_empty())
        .or_else(|| {
            input
                .id
                .and_then(|id| state.credentials.get_password(id).ok().flatten())
        });
    let session = session::DbSession::connect(profile, password)
        .await
        .map_err(command_error)?;
    session.close().await;
    Ok(())
}

#[tauri::command]
async fn connect_profile(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<WorkspaceInfo, String> {
    let profile = state.profile(profile_id).map_err(command_error)?;
    let session = state
        .connect(profile.clone())
        .await
        .map_err(command_error)?;
    let databases = session.list_databases().await.map_err(command_error)?;
    Ok(WorkspaceInfo { profile, databases })
}

#[tauri::command]
async fn connect_database(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
    database: String,
) -> Result<WorkspaceInfo, String> {
    let database = database.trim();
    if database.is_empty() {
        return Err("database is required".into());
    }
    let mut profile = state.profile(profile_id).map_err(command_error)?;
    profile.default_database = database.to_owned();
    state.disconnect(profile_id).await;
    let session = state
        .connect(profile.clone())
        .await
        .map_err(command_error)?;
    let databases = session.list_databases().await.map_err(command_error)?;
    Ok(WorkspaceInfo { profile, databases })
}

#[tauri::command]
async fn disconnect_workspace(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<(), String> {
    state.disconnect(profile_id).await;
    Ok(())
}

#[tauri::command]
async fn list_databases(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<Vec<models::DatabaseRef>, String> {
    state
        .session(profile_id)
        .await
        .map_err(command_error)?
        .list_databases()
        .await
        .map_err(command_error)
}

#[tauri::command]
async fn load_schema_tree(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<Vec<models::SchemaNode>, String> {
    state
        .session(profile_id)
        .await
        .map_err(command_error)?
        .schema_tree()
        .await
        .map_err(command_error)
}

#[tauri::command]
async fn load_table_page(
    state: tauri::State<'_, AppState>,
    request: TablePageRequest,
) -> Result<models::TablePage, String> {
    state
        .session(request.profile_id)
        .await
        .map_err(command_error)?
        .table_page(&request)
        .await
        .map_err(command_error)
}

#[tauri::command]
async fn run_query(
    state: tauri::State<'_, AppState>,
    request: QueryRequest,
) -> Result<models::QueryResponse, String> {
    let session = state
        .session(request.profile_id)
        .await
        .map_err(command_error)?;
    let response = session.run_query(&request.sql, request.max_rows).await;
    let success = response.is_ok();
    let response = response.map_err(command_error);
    let (duration_ms, database) = response
        .as_ref()
        .map(|result| {
            (
                result.duration_ms,
                session.profile().default_database.clone(),
            )
        })
        .unwrap_or((0, session.profile().default_database.clone()));
    let entry = QueryHistoryEntry {
        id: Uuid::new_v4(),
        profile_id: request.profile_id,
        database,
        sql: request.sql,
        executed_at: Utc::now(),
        duration_ms,
        success,
    };
    state.store.add_history(&entry).map_err(command_error)?;
    response
}

#[tauri::command]
async fn cancel_query() -> Result<(), String> {
    Err("query cancellation is not available for this connection".into())
}

#[tauri::command]
async fn list_query_history(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
    database: String,
    limit: Option<u32>,
) -> Result<Vec<QueryHistoryEntry>, String> {
    state
        .store
        .list_history(profile_id, &database, limit.unwrap_or(100).clamp(1, 500))
        .map_err(command_error)
}

#[tauri::command]
async fn apply_table_mutations(
    state: tauri::State<'_, AppState>,
    batch: MutationBatch,
) -> Result<models::MutationResult, String> {
    state
        .session(batch.profile_id)
        .await
        .map_err(command_error)?
        .apply_mutations(&batch)
        .await
        .map_err(command_error)
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("dbm_desktop=info")),
        )
        .with_target(false)
        .compact()
        .init();

    let state = AppState::new().expect("DBM local storage must initialize");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .manage(updates::UpdateCoordinator::default())
        .setup(|app| {
            updates::initialize(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_profiles,
            save_profile,
            delete_profile,
            test_profile,
            connect_profile,
            connect_database,
            disconnect_workspace,
            list_databases,
            load_schema_tree,
            load_table_page,
            run_query,
            cancel_query,
            list_query_history,
            apply_table_mutations,
            updates::get_update_status,
            updates::check_for_updates,
            updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DBM");
}
