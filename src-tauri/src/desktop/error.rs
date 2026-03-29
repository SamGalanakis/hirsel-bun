//! Unified error handling for Tauri GUI commands.
//!
//! This module provides a structured error type that:
//! - Converts all internal errors to user-friendly messages
//! - Includes error codes for frontend categorization
//! - Supports toast notification display

use serde::Serialize;
use std::fmt;

/// Error codes for frontend categorization.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Run not found
    RunNotFound,
    /// Task not found
    TaskNotFound,
    /// Worker not found
    WorkerNotFound,
    /// Thread not found
    ThreadNotFound,
    /// Invalid input from user
    InvalidInput,
    /// Database/state error
    StateError,
    /// Git operation failed
    GitError,
    /// File system error
    IoError,
    /// Configuration error
    ConfigError,
    /// Operation not allowed in current state
    InvalidState,
    /// Permission denied
    PermissionDenied,
    /// Network/remote error
    NetworkError,
    /// Process/subprocess error
    ProcessError,
    /// General server error
    InternalError,
}

impl ErrorCode {
    /// Get a short label for this error code
    pub fn label(&self) -> &'static str {
        match self {
            ErrorCode::RunNotFound => "Not Found",
            ErrorCode::TaskNotFound => "Not Found",
            ErrorCode::WorkerNotFound => "Not Found",
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

    /// Whether this is a user error (vs system error)
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            ErrorCode::InvalidInput
                | ErrorCode::RunNotFound
                | ErrorCode::TaskNotFound
                | ErrorCode::WorkerNotFound
                | ErrorCode::ThreadNotFound
                | ErrorCode::InvalidState
        )
    }
}

/// Structured error for Tauri commands.
///
/// This error type is serialized to JSON for the frontend,
/// providing both a user-friendly message and an error code
/// for categorization.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiError {
    /// Error code for categorization
    pub code: ErrorCode,
    /// User-friendly error message
    pub message: String,
    /// Optional technical details (for debugging)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl GuiError {
    /// Create a new GUI error
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    /// Add technical details
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Create a "run not found" error
    pub fn run_not_found(name: &str) -> Self {
        Self::new(ErrorCode::RunNotFound, format!("Run '{}' not found", name))
    }

    /// Create a "task not found" error
    pub fn task_not_found(id: &str) -> Self {
        Self::new(ErrorCode::TaskNotFound, format!("Task '{}' not found", id))
    }

    /// Create a "worker not found" error
    pub fn worker_not_found(name: &str) -> Self {
        Self::new(
            ErrorCode::WorkerNotFound,
            format!("Worker '{}' not found", name),
        )
    }

    /// Create an "invalid input" error
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    /// Create an "invalid state" error
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidState, message)
    }

    /// Create a "state error" from a database error
    pub fn state_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::StateError, message)
    }

    /// Create an "internal error"
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

// Convert from various internal error types

impl From<crate::backend::state::StateError> for GuiError {
    fn from(err: crate::backend::state::StateError) -> Self {
        Self::new(ErrorCode::StateError, "Database operation failed").with_details(err.to_string())
    }
}

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

// Implement IntoResponse for Tauri
// Tauri commands can return Result<T, GuiError> directly since GuiError is Serialize

/// Result type for GUI commands
pub type GuiResult<T> = Result<T, GuiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_serialization() {
        let err = GuiError::run_not_found("my-run");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("run_not_found"));
        assert!(json.contains("Run 'my-run' not found"));
    }

    #[test]
    fn test_error_with_details() {
        let err = GuiError::state_error("Query failed").with_details("SQL syntax error");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("SQL syntax error"));
    }

    #[test]
    fn test_error_code_labels() {
        assert_eq!(ErrorCode::RunNotFound.label(), "Not Found");
        assert_eq!(ErrorCode::InvalidInput.label(), "Invalid Input");
        assert_eq!(ErrorCode::InternalError.label(), "Internal Error");
    }

    #[test]
    fn test_user_error_classification() {
        assert!(ErrorCode::InvalidInput.is_user_error());
        assert!(ErrorCode::RunNotFound.is_user_error());
        assert!(!ErrorCode::InternalError.is_user_error());
        assert!(!ErrorCode::StateError.is_user_error());
    }
}
