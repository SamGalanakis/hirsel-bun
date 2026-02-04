//! Workspace location abstraction for delivery operations
//!
//! Handles different workspace scenarios:
//! - Local: Files are on the local filesystem
//! - Coordinator: Files are held by a remote coordinator

use std::path::PathBuf;

use thiserror::Error;

use crate::core::config::{hirsel_dir, Config, OrchestratorMode};
use crate::core::delta::DeltaState;

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
        run_name: String,
        coordinator_url: Option<String>,
    },
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
    run_name: &str,
    mode: OrchestratorMode,
) -> WorkspaceResult<WorkspaceLocation> {
    match mode {
        OrchestratorMode::Local => {
            // For local mode, the workspace is in the run's work directory
            let run_path = hirsel_dir().join("runs").join(run_name);
            let work_dir = run_path.join("work").join("staging");

            if !work_dir.exists() {
                return Err(WorkspaceError::WorkDirNotFound(
                    work_dir.display().to_string(),
                ));
            }

            Ok(WorkspaceLocation::Local(work_dir))
        }
        OrchestratorMode::Remote => {
            // For remote mode, operations go through the coordinator
            let (config, _) = Config::load().map_err(|e| WorkspaceError::Config(e.to_string()))?;

            let profile = config
                .profiles
                .get(&config.default_profile)
                .ok_or_else(|| WorkspaceError::Config("No default profile".to_string()))?;

            Ok(WorkspaceLocation::Coordinator {
                run_name: run_name.to_string(),
                coordinator_url: profile.url.clone(),
            })
        }
    }
}

/// Resolve workspace for a project using its active run
pub async fn resolve_workspace_for_project(project_id: i64) -> WorkspaceResult<WorkspaceLocation> {
    // Get the active project run
    let state = DeltaState::new(project_id);
    let project_run = state
        .get_project_run()
        .await
        .map_err(|e| WorkspaceError::State(e.to_string()))?
        .ok_or(WorkspaceError::NoActiveRun(project_id))?;

    // Determine the mode from config
    let (config, _) = Config::load().map_err(|e| WorkspaceError::Config(e.to_string()))?;
    let profile = config
        .profiles
        .get(&config.default_profile)
        .ok_or_else(|| WorkspaceError::Config("No default profile".to_string()))?;

    resolve_workspace(project_id, &project_run.run_name, profile.mode.clone()).await
}
