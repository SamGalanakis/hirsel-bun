//! Error types for delta state operations

/// Error type for delta state operations
#[derive(Debug, thiserror::Error)]
pub enum DeltaStateError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Draft node not found: {0}")]
    DraftNodeNotFound(String),
    #[error("Live node not found: {0}")]
    LiveNodeNotFound(String),
    #[error("Project run not found for project: {0}")]
    ProjectRunNotFound(i64),
    #[error("Parent node not found: {0}")]
    ParentNodeNotFound(String),
    #[error("Eval nodes must validate at least one task")]
    EvalValidatesEmpty,
}

pub type DeltaStateResult<T> = Result<T, DeltaStateError>;
