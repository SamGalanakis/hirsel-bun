//! Unified error handling for Tauri GUI commands.

use serde::Serialize;
use std::fmt;

/// Error codes for frontend categorization.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ThreadNotFound,
    InvalidInput,
    StateError,
    GitError,
    IoError,
    ConfigError,
    InvalidState,
    PermissionDenied,
    NetworkError,
    ProcessError,
    InternalError,
}

impl ErrorCode {
    pub fn label(&self) -> &'static str {
        match self {
            ErrorCode::ThreadNotFound => "Not Found",
            ErrorCode::InvalidInput => "Invalid Input",
            ErrorCode::StateError => "Database Error",
            ErrorCode::GitError => "Git Error",
            ErrorCode::IoError => "File Error",
            ErrorCode::ConfigError => "Config Error",
            ErrorCode::InvalidState => "Invalid State",
            ErrorCode::PermissionDenied => "Permission Denied",
            ErrorCode::NetworkError => "Network Error",
            ErrorCode::ProcessError => "Process Error",
            ErrorCode::InternalError => "Internal Error",
        }
    }

    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            ErrorCode::ThreadNotFound | ErrorCode::InvalidInput | ErrorCode::InvalidState
        )
    }
}

/// Structured error for Tauri commands.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl GuiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn thread_not_found(id: &str) -> Self {
        Self::new(
            ErrorCode::ThreadNotFound,
            format!("Thread '{}' not found", id),
        )
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidState, message)
    }

    pub fn state_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::StateError, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, message)
    }
}

impl fmt::Display for GuiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GuiError {}

impl From<std::io::Error> for GuiError {
    fn from(err: std::io::Error) -> Self {
        Self::new(ErrorCode::IoError, "File operation failed").with_details(err.to_string())
    }
}

impl From<crate::backend::git::GitError> for GuiError {
    fn from(err: crate::backend::git::GitError) -> Self {
        Self::new(ErrorCode::GitError, "Git operation failed").with_details(err.to_string())
    }
}

impl From<crate::backend::config::ConfigError> for GuiError {
    fn from(err: crate::backend::config::ConfigError) -> Self {
        Self::new(ErrorCode::ConfigError, "Configuration error").with_details(err.to_string())
    }
}

pub type GuiResult<T> = Result<T, GuiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_serialization() {
        let err = GuiError::thread_not_found("t-1");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("thread_not_found"));
        assert!(json.contains("Thread 't-1' not found"));
    }

    #[test]
    fn test_error_with_details() {
        let err = GuiError::state_error("Query failed").with_details("SQL syntax error");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("SQL syntax error"));
    }

    #[test]
    fn test_error_code_labels() {
        assert_eq!(ErrorCode::ThreadNotFound.label(), "Not Found");
        assert_eq!(ErrorCode::InvalidInput.label(), "Invalid Input");
        assert_eq!(ErrorCode::InternalError.label(), "Internal Error");
    }

    #[test]
    fn test_user_error_classification() {
        assert!(ErrorCode::InvalidInput.is_user_error());
        assert!(ErrorCode::ThreadNotFound.is_user_error());
        assert!(!ErrorCode::InternalError.is_user_error());
        assert!(!ErrorCode::StateError.is_user_error());
    }
}
