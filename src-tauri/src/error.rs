// SPDX-License-Identifier: GPL-3.0-or-later
//! Structured error type returned to the webview. Commands never panic on user
//! input; they return `AppResult<T>` and the frontend renders the message.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("filesystem error: {0}")]
    Io(String),
    #[error("no vault is currently open")]
    NoVault,
    #[error("path is outside the active vault")]
    OutsideVault,
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

// Serialize as a plain string so the webview receives a structured, readable
// error instead of an opaque IPC failure.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
