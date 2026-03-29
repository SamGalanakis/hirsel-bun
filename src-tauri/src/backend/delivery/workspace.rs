//! Workspace location abstraction for delivery operations
//!
//! Handles different workspace scenarios:
//! - Local: Files are on the local filesystem
//! - Coordinator: Files are held by a remote coordinator

use std::path::PathBuf;

use thiserror::Error;

use crate::backend::config::{hirsel_dir, Config};
use crate::backend::delta::DeltaState;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("Project not found: {0}")]
    ProjectNotFound(i64),
    #[error("No active run for project: {0}")]
    NoActiveRun(i64),
    #[error("Work directory not found: {0}")]
    WorkDirNotFound(String),
    #[error("Config error: {0}")]
    Config(String),
    #[error("State error: {0}")]
    State(String),
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

/// Location of the workspace for delivery operations
#[derive(Debug, Clone)]
pub enum WorkspaceLocation {
    /// Files are on the local filesystem
    Local(PathBuf),
    /// Files are held by a remote coordinator
    Coordinator {
        runtime_name: String,
        coordinator_url: Option<String>,
    },
}

/// Resolve the local work directory for a run.
///
/// Prefers the staging worktree when present, then falls back to the root
/// work directory for older or single-worktree runs.
pub fn resolve_run_work_dir(runtime_name: &str) -> WorkspaceResult<PathBuf> {
    let run_path = hirsel_dir().join("runtimes").join(runtime_name);
    if !run_path.exists() {
        return Err(WorkspaceError::WorkDirNotFound(format!(
            "Run not found: {}",
            runtime_name
        )));
    }

    let staging_dir = run_path.join("work").join("staging");
    if staging_dir.exists() {
        return Ok(staging_dir);
    }

    let work_dir = run_path.join("work");
    if work_dir.exists() {
        return Ok(work_dir);
    }

    Err(WorkspaceError::WorkDirNotFound(format!(
        "Run work directory not found: {}",
        runtime_name
    )))
}

impl WorkspaceLocation {
    /// Get the local path if this is a Local workspace
    pub fn local_path(&self) -> Option<&PathBuf> {
        match self {
            WorkspaceLocation::Local(path) => Some(path),
            WorkspaceLocation::Coordinator { .. } => None,
        }
    }

    /// Check if this is a local workspace
    pub fn is_local(&self) -> bool {
        matches!(self, WorkspaceLocation::Local(_))
    }
}

/// Resolve the workspace location for a project's active run
pub async fn resolve_workspace(
    _project_id: i64,
    runtime_name: &str,
) -> WorkspaceResult<WorkspaceLocation> {
    let (config, _) = Config::load().map_err(|e| WorkspaceError::Config(e.to_string()))?;

    if let Some(url) = config.backend.url {
        Ok(WorkspaceLocation::Coordinator {
            runtime_name: runtime_name.to_string(),
            coordinator_url: Some(url),
        })
    } else {
        Ok(WorkspaceLocation::Local(resolve_run_work_dir(
            runtime_name,
        )?))
    }
}

/// Resolve workspace for a project using its active run
pub async fn resolve_workspace_for_project(
    project_id: i64,
    route_id: i64,
) -> WorkspaceResult<WorkspaceLocation> {
    // Get the active project run
    let state = DeltaState::with_route(project_id, route_id);
    let project_run = state
        .get_route_runtime()
        .await
        .map_err(|e| WorkspaceError::State(e.to_string()))?
        .ok_or(WorkspaceError::NoActiveRun(project_id))?;

    resolve_workspace(project_id, &project_run.runtime_name).await
}
