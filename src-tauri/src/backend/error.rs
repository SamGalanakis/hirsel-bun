//! Unified error hierarchy for Hirsel.
//!
//! This module consolidates error types across the codebase into a single
//! `HirselError` enum with `ErrorKind` categorization.
//!
//! Design goals:
//! - Single error type for most of the codebase
//! - Automatic conversion from existing error types via `From` impls
//! - Consistent error categorization via `ErrorKind`
//! - HTTP status code mapping for API responses

use std::fmt;
use thiserror::Error;

// =============================================================================
// ErrorKind - Categorization
// =============================================================================

/// High-level error categories for classification and HTTP mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Resource not found (404)
    NotFound,
    /// Resource already exists (409)
    AlreadyExists,
    /// Operation not valid for current state (400)
    InvalidState,
    /// Invalid user input or parameters (400)
    InvalidInput,
    /// Database/state error (500)
    State,
    /// File system error (500)
    Io,
    /// Git operation error (500)
    Git,
    /// Network/HTTP error (502)
    Network,
    /// Authentication/authorization error (401/403)
    Auth,
    /// Process/subprocess error (500)
    Process,
    /// Serialization/deserialization error (400/500)
    Serialization,
    /// Operation timed out (408/504)
    Timeout,
    /// Internal/unexpected error (500)
    Internal,
}

impl ErrorKind {
    /// Get the HTTP status code for this error kind
    pub fn http_status(&self) -> u16 {
        match self {
            ErrorKind::NotFound => 404,
            ErrorKind::AlreadyExists => 409,
            ErrorKind::InvalidState | ErrorKind::InvalidInput => 400,
            ErrorKind::Auth => 401,
            ErrorKind::Timeout => 408,
            ErrorKind::Network => 502,
            ErrorKind::State
            | ErrorKind::Io
            | ErrorKind::Git
            | ErrorKind::Process
            | ErrorKind::Serialization
            | ErrorKind::Internal => 500,
        }
    }

    /// Get a short label for this error kind
    pub fn label(&self) -> &'static str {
        match self {
            ErrorKind::NotFound => "Not Found",
            ErrorKind::AlreadyExists => "Already Exists",
            ErrorKind::InvalidState => "Invalid State",
            ErrorKind::InvalidInput => "Invalid Input",
            ErrorKind::State => "Database Error",
            ErrorKind::Io => "File Error",
            ErrorKind::Git => "Git Error",
            ErrorKind::Network => "Network Error",
            ErrorKind::Auth => "Auth Error",
            ErrorKind::Process => "Process Error",
            ErrorKind::Serialization => "Serialization Error",
            ErrorKind::Timeout => "Timeout",
            ErrorKind::Internal => "Internal Error",
        }
    }

    /// Whether this is a user error (vs system error)
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            ErrorKind::NotFound
                | ErrorKind::AlreadyExists
                | ErrorKind::InvalidState
                | ErrorKind::InvalidInput
                | ErrorKind::Auth
        )
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

// =============================================================================
// HirselError - Unified Error Type
// =============================================================================

/// Unified error type for Hirsel operations.
///
/// This enum consolidates error types from across the codebase:
/// - State errors (database, sqlite)
/// - Config errors
/// - Ops errors
/// - Lifecycle errors
/// - Runner errors
/// - Orchestrator errors
/// - IO errors
/// - Git errors
/// - Network errors
#[derive(Debug, Error)]
pub enum HirselError {
    // =========================================================================
    // NotFound variants
    // =========================================================================
    #[error("Run '{0}' not found")]
    RunNotFound(String),

    #[error("Worker '{0}' not found")]
    WorkerNotFound(String),

    #[error("Task '{0}' not found")]
    TaskNotFound(String),

    #[error("Thread '{0}' not found")]
    ThreadNotFound(String),

    #[error("Eval '{0}' not found")]
    EvalNotFound(String),

    #[error("Config key '{0}' not found")]
    ConfigKeyNotFound(String),

    #[error("{0}")]
    NotFound(String),

    // =========================================================================
    // AlreadyExists variants
    // =========================================================================
    #[error("Run '{0}' already exists")]
    RunAlreadyExists(String),

    #[error("Task '{0}' already exists")]
    TaskAlreadyExists(String),

    #[error("Worker '{0}' already exists")]
    WorkerAlreadyExists(String),

    #[error("{0}")]
    AlreadyExists(String),

    // =========================================================================
    // InvalidState variants
    // =========================================================================
    #[error("Invalid state transition: {from} -> {to}")]
    InvalidTransition { from: String, to: String },

    #[error("Run is not active")]
    RunNotActive,

    #[error("Worker is not in expected state: {expected}, got {actual}")]
    WorkerNotInState { expected: String, actual: String },

    #[error("{0}")]
    InvalidState(String),

    // =========================================================================
    // InvalidInput variants
    // =========================================================================
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    // =========================================================================
    // State/Database variants
    // =========================================================================
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("State error: {0}")]
    State(String),

    // =========================================================================
    // IO variants
    // =========================================================================
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("File not found: {0}")]
    FileNotFound(String),

    // =========================================================================
    // Git variants
    // =========================================================================
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("Git operation failed: {0}")]
    GitOp(String),

    // =========================================================================
    // Network variants
    // =========================================================================
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Connection failed: {0}")]
    Connection(String),

    // =========================================================================
    // Auth variants
    // =========================================================================
    #[error("Authentication required")]
    AuthRequired,

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    // =========================================================================
    // Process variants
    // =========================================================================
    #[error("Process error: {0}")]
    Process(String),

    #[error("Process exited with code {code}: {message}")]
    ProcessExited { code: i32, message: String },

    // =========================================================================
    // Serialization variants
    // =========================================================================
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    // =========================================================================
    // Timeout variants
    // =========================================================================
    #[error("Operation timed out: {0}")]
    Timeout(String),

    // =========================================================================
    // Internal variants
    // =========================================================================
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl HirselError {
    /// Get the error kind for this error
    pub fn kind(&self) -> ErrorKind {
        match self {
            // NotFound
            HirselError::RunNotFound(_)
            | HirselError::WorkerNotFound(_)
            | HirselError::TaskNotFound(_)
            | HirselError::ThreadNotFound(_)
            | HirselError::EvalNotFound(_)
            | HirselError::ConfigKeyNotFound(_)
            | HirselError::NotFound(_)
            | HirselError::FileNotFound(_) => ErrorKind::NotFound,

            // AlreadyExists
            HirselError::RunAlreadyExists(_)
            | HirselError::TaskAlreadyExists(_)
            | HirselError::WorkerAlreadyExists(_)
            | HirselError::AlreadyExists(_) => ErrorKind::AlreadyExists,

            // InvalidState
            HirselError::InvalidTransition { .. }
            | HirselError::RunNotActive
            | HirselError::WorkerNotInState { .. }
            | HirselError::InvalidState(_) => ErrorKind::InvalidState,

            // InvalidInput
            HirselError::InvalidInput(_) | HirselError::MissingField(_) => ErrorKind::InvalidInput,

            // State
            HirselError::Database(_) | HirselError::State(_) => ErrorKind::State,

            // IO
            HirselError::Io(_) => ErrorKind::Io,

            // Git
            HirselError::Git(_) | HirselError::GitOp(_) => ErrorKind::Git,

            // Network
            HirselError::Http(_) | HirselError::Request(_) | HirselError::Connection(_) => {
                ErrorKind::Network
            }

            // Auth
            HirselError::AuthRequired | HirselError::PermissionDenied(_) => ErrorKind::Auth,

            // Process
            HirselError::Process(_) | HirselError::ProcessExited { .. } => ErrorKind::Process,

            // Serialization
            HirselError::Json(_) | HirselError::Toml(_) => ErrorKind::Serialization,

            // Timeout
            HirselError::Timeout(_) => ErrorKind::Timeout,

            // Internal
            HirselError::Internal(_) | HirselError::Other(_) => ErrorKind::Internal,
        }
    }

    /// Get the HTTP status code for this error
    pub fn http_status(&self) -> u16 {
        self.kind().http_status()
    }

    /// Whether this is a user error (vs system error)
    pub fn is_user_error(&self) -> bool {
        self.kind().is_user_error()
    }
}

// =============================================================================
// From impls for existing error types
// =============================================================================

impl From<crate::backend::state::StateError> for HirselError {
    fn from(err: crate::backend::state::StateError) -> Self {
        match err {
            crate::backend::state::StateError::Database(e) => HirselError::Database(e),
            crate::backend::state::StateError::NotFound(msg) => {
                // Try to parse the message to determine specific type
                if msg.contains("Task") {
                    let task_id = msg
                        .strip_prefix("Task '")
                        .and_then(|s| s.strip_suffix("'"))
                        .unwrap_or(&msg);
                    HirselError::TaskNotFound(task_id.to_string())
                } else if msg.contains("Worker") {
                    let worker_name = msg
                        .strip_prefix("Worker '")
                        .and_then(|s| s.strip_suffix("'"))
                        .unwrap_or(&msg);
                    HirselError::WorkerNotFound(worker_name.to_string())
                } else {
                    HirselError::NotFound(msg)
                }
            }
            crate::backend::state::StateError::InvalidState(msg) => HirselError::InvalidState(msg),
            crate::backend::state::StateError::AlreadyExists(msg) => {
                HirselError::AlreadyExists(msg)
            }
            crate::backend::state::StateError::Blocked(msg) => HirselError::InvalidState(msg),
            crate::backend::state::StateError::InvalidTransition(from, to) => {
                HirselError::InvalidTransition {
                    from: from.to_string(),
                    to: to.to_string(),
                }
            }
        }
    }
}

impl From<crate::backend::config::ConfigError> for HirselError {
    fn from(err: crate::backend::config::ConfigError) -> Self {
        match err {
            crate::backend::config::ConfigError::NoRunSelected => {
                HirselError::InvalidState("No run selected".to_string())
            }
            crate::backend::config::ConfigError::InvalidToml { path, message } => {
                HirselError::InvalidInput(format!(
                    "Invalid TOML in {}: {}",
                    path.display(),
                    message
                ))
            }
            crate::backend::config::ConfigError::PermissionDenied { path } => {
                HirselError::PermissionDenied(path.display().to_string())
            }
            crate::backend::config::ConfigError::ReadError { path, message } => {
                HirselError::FileNotFound(format!("{}: {}", path.display(), message))
            }
            crate::backend::config::ConfigError::EmptyRunName => {
                HirselError::InvalidInput("Run name cannot be empty".to_string())
            }
            crate::backend::config::ConfigError::RunNameHasPathSeparators => {
                HirselError::InvalidInput("Run name cannot contain path separators".to_string())
            }
            crate::backend::config::ConfigError::ValidationError(msg) => {
                HirselError::InvalidInput(msg)
            }
            crate::backend::config::ConfigError::Store(e) => {
                HirselError::State(format!("Config store error: {}", e))
            }
        }
    }
}

impl From<crate::backend::lifecycle::LifecycleError> for HirselError {
    fn from(err: crate::backend::lifecycle::LifecycleError) -> Self {
        match err {
            crate::backend::lifecycle::LifecycleError::State(msg) => HirselError::State(msg),
            crate::backend::lifecycle::LifecycleError::InvalidTransition { from, to } => {
                HirselError::InvalidTransition { from, to }
            }
            crate::backend::lifecycle::LifecycleError::Worker(msg) => HirselError::Process(msg),
            crate::backend::lifecycle::LifecycleError::Io(e) => HirselError::Io(e),
            crate::backend::lifecycle::LifecycleError::RunNotActive => HirselError::RunNotActive,
            crate::backend::lifecycle::LifecycleError::Config(msg) => {
                HirselError::InvalidInput(msg)
            }
        }
    }
}

impl From<crate::backend::orchestrator::OrchestratorError> for HirselError {
    fn from(err: crate::backend::orchestrator::OrchestratorError) -> Self {
        HirselError::Internal(err.to_string())
    }
}

impl From<crate::backend::git::GitError> for HirselError {
    fn from(err: crate::backend::git::GitError) -> Self {
        match err {
            crate::backend::git::GitError::Git2(e) => HirselError::Git(e),
            crate::backend::git::GitError::Io(e) => HirselError::Io(e),
            crate::backend::git::GitError::NotARepository(path) => {
                HirselError::GitOp(format!("Not a git repository: {}", path.display()))
            }
            crate::backend::git::GitError::BranchNotFound(branch) => {
                HirselError::GitOp(format!("Branch not found: {}", branch))
            }
            crate::backend::git::GitError::MergeConflict(files) => {
                HirselError::GitOp(format!("Merge conflict in files: {:?}", files))
            }
            crate::backend::git::GitError::PushFailed(msg) => {
                HirselError::GitOp(format!("Push failed: {}", msg))
            }
            crate::backend::git::GitError::Other(msg) => HirselError::GitOp(msg),
        }
    }
}

impl From<crate::backend::storage::StorageError> for HirselError {
    fn from(err: crate::backend::storage::StorageError) -> Self {
        HirselError::Io(std::io::Error::other(err.to_string()))
    }
}

impl From<crate::backend::runner::RunnerError> for HirselError {
    fn from(err: crate::backend::runner::RunnerError) -> Self {
        match err {
            crate::backend::runner::RunnerError::SpawnFailed(msg) => HirselError::Process(msg),
            crate::backend::runner::RunnerError::StopFailed(msg) => HirselError::Process(msg),
            crate::backend::runner::RunnerError::WorkerNotFound(name) => {
                HirselError::WorkerNotFound(name)
            }
            crate::backend::runner::RunnerError::SetupFailed(msg) => HirselError::Process(msg),
            crate::backend::runner::RunnerError::Io(e) => HirselError::Io(e),
            crate::backend::runner::RunnerError::Config(msg) => HirselError::InvalidInput(msg),
            crate::backend::runner::RunnerError::State(msg) => HirselError::State(msg),
            crate::backend::runner::RunnerError::RunPaused => {
                HirselError::InvalidState("Run is paused".to_string())
            }
            crate::backend::runner::RunnerError::Timeout(msg) => HirselError::Timeout(msg),
        }
    }
}

impl From<crate::backend::state_access::StateAccessError> for HirselError {
    fn from(err: crate::backend::state_access::StateAccessError) -> Self {
        match err {
            crate::backend::state_access::StateAccessError::Database(msg) => {
                HirselError::State(msg)
            }
            crate::backend::state_access::StateAccessError::Http(msg) => HirselError::Http(msg),
            crate::backend::state_access::StateAccessError::Connection(msg) => {
                HirselError::Connection(msg)
            }
            crate::backend::state_access::StateAccessError::NotFound(msg) => {
                HirselError::NotFound(msg)
            }
            crate::backend::state_access::StateAccessError::InvalidOperation(msg) => {
                HirselError::InvalidState(msg)
            }
        }
    }
}

// =============================================================================
// Result type alias
// =============================================================================

/// Result type using HirselError
pub type HirselResult<T> = Result<T, HirselError>;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_kind_http_status() {
        assert_eq!(ErrorKind::NotFound.http_status(), 404);
        assert_eq!(ErrorKind::AlreadyExists.http_status(), 409);
        assert_eq!(ErrorKind::InvalidInput.http_status(), 400);
        assert_eq!(ErrorKind::Internal.http_status(), 500);
    }

    #[test]
    fn test_error_kind_is_user_error() {
        assert!(ErrorKind::NotFound.is_user_error());
        assert!(ErrorKind::InvalidInput.is_user_error());
        assert!(!ErrorKind::Internal.is_user_error());
        assert!(!ErrorKind::State.is_user_error());
    }

    #[test]
    fn test_hirsel_error_kind() {
        let err = HirselError::RunNotFound("test".to_string());
        assert_eq!(err.kind(), ErrorKind::NotFound);

        let err = HirselError::InvalidTransition {
            from: "a".to_string(),
            to: "b".to_string(),
        };
        assert_eq!(err.kind(), ErrorKind::InvalidState);

        let err = HirselError::Internal("test".to_string());
        assert_eq!(err.kind(), ErrorKind::Internal);
    }
}
