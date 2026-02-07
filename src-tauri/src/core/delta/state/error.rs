//! Error types for board state operations

/// Error type for board state operations
#[derive(Debug, thiserror::Error)]
pub enum DeltaStateError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Board node not found: {0}")]
    NodeNotFound(String),
    #[error("Project run not found for project: {0}")]
    ProjectRunNotFound(i64),
    #[error("Parent node not found: {0}")]
    ParentNodeNotFound(String),
}

pub type DeltaStateResult<T> = Result<T, DeltaStateError>;
