use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::keyring_store::CredentialStore;
use crate::models::{ConnectionProfile, ProfileSummary, SaveProfileInput};
use crate::session::DbSession;
use crate::storage::LocalStore;

pub struct AppState {
    pub store: LocalStore,
    pub credentials: CredentialStore,
    pub sessions: Mutex<HashMap<Uuid, Arc<DbSession>>>,
}

impl AppState {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            store: LocalStore::new()?,
            credentials: CredentialStore,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub async fn session(&self, profile_id: Uuid) -> AppResult<Arc<DbSession>> {
        self.sessions
            .lock()
            .await
            .get(&profile_id)
            .cloned()
            .ok_or(AppError::NotConnected)
    }

    pub fn profile(&self, profile_id: Uuid) -> AppResult<ConnectionProfile> {
        self.store
            .get_profile(profile_id)?
            .ok_or(AppError::ProfileNotFound)
    }

    pub fn profile_summaries(&self) -> AppResult<Vec<ProfileSummary>> {
        Ok(self
            .store
            .list_profiles()?
            .into_iter()
            .map(|profile| ProfileSummary { profile })
            .collect())
    }

    pub async fn connect(&self, profile: ConnectionProfile) -> AppResult<Arc<DbSession>> {
        let password = self.credentials.get_password(profile.id)?;
        let session = Arc::new(DbSession::connect(profile.clone(), password).await?);
        let previous = {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(profile.id, session.clone())
        };
        if let Some(previous) = previous {
            previous.close().await;
        }
        Ok(session)
    }

    pub async fn disconnect(&self, profile_id: Uuid) {
        let previous = self.sessions.lock().await.remove(&profile_id);
        if let Some(previous) = previous {
            previous.close().await;
        }
    }

    pub fn save_profile(&self, input: SaveProfileInput) -> AppResult<ConnectionProfile> {
        self.store.save_profile(&input)
    }
}
