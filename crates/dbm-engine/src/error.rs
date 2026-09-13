use std::error::Error as StdError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("profile not found")]
    ProfileNotFound,
    #[error("workspace is not connected")]
    NotConnected,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("credential error: {0}")]
    Credential(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
}

impl AppError {
    pub fn database(error: &(dyn StdError + 'static)) -> Self {
        let mut messages = Vec::new();
        let mut current = Some(error);
        while let Some(source) = current {
            let message = source.to_string();
            if !message.is_empty() && messages.last() != Some(&message) {
                messages.push(message);
            }
            current = source.source();
        }
        Self::Database(messages.join(": "))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<tokio_postgres::Error> for AppError {
    fn from(error: tokio_postgres::Error) -> Self {
        if let Some(database_error) = error.as_db_error() {
            let mut message = format!(
                "{} (SQLSTATE {})",
                database_error.message(),
                database_error.code().code()
            );
            if let Some(detail) = database_error.detail() {
                message.push_str(&format!("\nDetail: {detail}"));
            }
            if let Some(hint) = database_error.hint() {
                message.push_str(&format!("\nHint: {hint}"));
            }
            Self::Database(message)
        } else {
            Self::database(&error)
        }
    }
}

impl From<mysql_async::Error> for AppError {
    fn from(error: mysql_async::Error) -> Self {
        if let mysql_async::Error::Server(server) = &error {
            return Self::Database(format!("{} (SQLSTATE {})", server.message, server.state));
        }
        Self::database(&error)
    }
}

impl From<keyring::Error> for AppError {
    fn from(error: keyring::Error) -> Self {
        Self::Credential(error.to_string())
    }
}

impl From<redis_rs::RedisError> for AppError {
    fn from(error: redis_rs::RedisError) -> Self {
        Self::database(&error)
    }
}

pub type AppResult<T> = Result<T, AppError>;
