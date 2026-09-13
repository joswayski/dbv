use keyring::Entry;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "io.github.joswayski.dbm";

#[derive(Debug, Clone, Copy, Default)]
pub struct CredentialStore;

impl CredentialStore {
    pub fn save_password(&self, profile_id: Uuid, password: &str) -> AppResult<()> {
        let entry = entry_for(profile_id)?;
        entry.set_password(password).map_err(AppError::from)
    }

    pub fn get_password(&self, profile_id: Uuid) -> AppResult<Option<String>> {
        let entry = entry_for(profile_id)?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AppError::from(error)),
        }
    }

    pub fn delete_password(&self, profile_id: Uuid) -> AppResult<()> {
        let entry = entry_for(profile_id)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AppError::from(error)),
        }
    }
}

fn entry_for(profile_id: Uuid) -> AppResult<Entry> {
    Entry::new(SERVICE, &format!("profile-{profile_id}")).map_err(AppError::from)
}
