//! Core operations module - shared logic between CLI and GUI
//!
//! This module provides unified implementations of operations that are
//! used by both the CLI and GUI, ensuring consistent behavior and reducing
//! code duplication.
//!
//! # Design Principles
//!
//! - Operations return structured results, callers format for their context
//! - Configuration structs allow caller-specific options (e.g., delete_gyp_chat)
//! - No prompting or user interaction - that's the caller's responsibility
//! - Operations are idempotent where possible

pub mod docs;
pub mod project;
pub mod run;
pub mod setup;
pub mod spawn;
pub mod types;

pub use docs::{deliver_docs, setup_docs, DocsDeliveryConfig, DocsSetupConfig};
pub use project::*;
pub use run::*;
pub use setup::*;
pub use spawn::*;
pub use types::*;

use thiserror::Error;

/// Errors that can occur during core operations
#[derive(Debug, Error)]
pub enum OpsError {
    #[error("Run '{0}' not found")]
    RunNotFound(String),

    #[error("Run '{0}' already exists")]
    RunAlreadyExists(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("File system error: {0}")]
    FileSystem(#[from] std::io::Error),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

impl From<sqlx::Error> for OpsError {
    fn from(e: sqlx::Error) -> Self {
        OpsError::Database(e.to_string())
    }
}

impl From<git2::Error> for OpsError {
    fn from(e: git2::Error) -> Self {
        OpsError::Git(e.to_string())
    }
}

impl From<crate::core::state::StateError> for OpsError {
    fn from(e: crate::core::state::StateError) -> Self {
        OpsError::Database(e.to_string())
    }
}

/// Convert OpsError to a string suitable for Tauri commands
impl From<OpsError> for String {
    fn from(e: OpsError) -> Self {
        e.to_string()
    }
}
