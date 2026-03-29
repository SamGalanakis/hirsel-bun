//! Runtime setup operations - workspace creation, worker registration, and configuration
//!
//! These operations are shared between CLI and GUI for setting up runtimes.

use std::path::PathBuf;

use crate::backend::git::{create_worker_clone, create_workspace, GitError};
use crate::backend::state::SQLiteState;

use super::OpsError;

// =============================================================================
// Configuration Types
// =============================================================================

/// Configuration for setting up a runtime workspace and its workers.
#[derive(Debug, Clone)]
pub struct RunSetupConfig {
    /// Name of the runtime
    pub runtime_name: String,
    /// Path to the project repository
    pub project_path: PathBuf,
    /// Path to the runtime directory (~/.hirsel/runtimes/<runtime_name>)
    pub runtime_dir: PathBuf,
    /// Names of workers to create clones for (local workers)
    pub worker_names: Vec<String>,
    /// Whether this is a multi-worker runtime (affects workspace layout)
    pub is_multi_worker: bool,
    /// Name of the leader worker (first worker in multi-worker mode)
    pub leader_name: Option<String>,
}

/// Result of setting up a runtime workspace.
#[derive(Debug, Clone)]
pub struct RunSetupResult {
    /// Path to the main workspace directory (staging area)
    pub workspace_dir: PathBuf,
    /// Worker directories: (worker_name, worker_dir_path)
    pub worker_dirs: Vec<(String, PathBuf)>,
}

// =============================================================================
// Error Conversion
// =============================================================================

impl From<GitError> for OpsError {
    fn from(e: GitError) -> Self {
        OpsError::Git(e.to_string())
    }
}

/// Determine multi-worker configuration from worker count and scale
///
/// Returns (is_multi_worker, leader_name) tuple.
/// - is_multi_worker is true if there are multiple workers or autoscale allows it
/// - leader_name is the first worker's name if multi-worker mode is enabled
///
/// # Arguments
///
/// * `worker_names` - List of worker names
/// * `max_scale` - Maximum number of workers (for autoscaling)
///
/// # Example
///
/// ```ignore
/// let (is_multi, leader) = compute_multi_worker_config(&["alpha", "beta"], 4);
/// assert!(is_multi);
/// assert_eq!(leader, Some("alpha".to_string()));
/// ```
pub fn compute_multi_worker_config(
    worker_names: &[String],
    max_scale: u32,
) -> (bool, Option<String>) {
    let is_multi_worker = worker_names.len() > 1 || max_scale > 1;
    let leader = if is_multi_worker {
        worker_names.first().cloned()
    } else {
        None
    };
    (is_multi_worker, leader)
}

// =============================================================================
// Workspace Setup
// =============================================================================

/// Set up workspace and worker clones for a run
///
/// This operation:
/// 1. Creates the main workspace (staging) from the project
/// 2. Creates worker clone directories (in multi-worker mode) or uses workspace directly
/// 3. Returns the created worker workspaces
///
/// # Arguments
///
/// * `config` - Configuration specifying run details and worker names
///
/// # Returns
///
/// * `Ok(RunSetupResult)` - Paths to created directories
/// * `Err(OpsError)` - If any operation failed
pub fn setup_run_workspace(config: &RunSetupConfig) -> Result<RunSetupResult, OpsError> {
    // Get runs directory (parent of runtime_dir)
    let runtimes_dir = config
        .runtime_dir
        .parent()
        .ok_or_else(|| OpsError::InvalidState("Invalid runtime_dir path".to_string()))?;

    // Create workspace with staging branch
    let workspace_dir = create_workspace(&config.runtime_name, &config.project_path, runtimes_dir)?;

    // Create worker clones/worktrees
    let mut worker_dirs: Vec<(String, PathBuf)> = Vec::new();

    for worker_name in &config.worker_names {
        let worker_dir = if config.is_multi_worker {
            create_worker_clone(
                &config.runtime_name,
                &config.project_path,
                worker_name,
                Some(&workspace_dir),
                runtimes_dir,
            )?
        } else {
            // Single worker uses workspace directly
            workspace_dir.clone()
        };

        worker_dirs.push((worker_name.clone(), worker_dir));
    }

    Ok(RunSetupResult {
        workspace_dir,
        worker_dirs,
    })
}

// =============================================================================
// Worker Registration
// =============================================================================

/// Register workers in the database
///
/// Adds worker entries to the state database with their work directories.
///
/// # Arguments
///
/// * `state` - The SQLite state to register workers in
/// * `worker_dirs` - List of (worker_name, work_dir) tuples
/// * `location` - Worker location ("local" or "remote")
///
/// # Returns
///
/// * `Ok(())` - If all workers were registered successfully
/// * `Err(OpsError)` - If registration failed
pub async fn register_workers(
    state: &SQLiteState,
    worker_dirs: &[(String, PathBuf)],
    location: &str,
) -> Result<(), OpsError> {
    for (worker_name, work_dir) in worker_dirs {
        state
            .add_worker(
                worker_name,
                work_dir.to_str().unwrap_or("."),
                location,
                None,
            )
            .await?;
    }
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_multi_worker_config_single() {
        let names = vec!["alpha".to_string()];
        let (is_multi, leader) = compute_multi_worker_config(&names, 1);
        assert!(!is_multi);
        assert!(leader.is_none());
    }

    #[test]
    fn test_compute_multi_worker_config_multiple() {
        let names = vec!["alpha".to_string(), "beta".to_string()];
        let (is_multi, leader) = compute_multi_worker_config(&names, 2);
        assert!(is_multi);
        assert_eq!(leader, Some("alpha".to_string()));
    }

    #[test]
    fn test_compute_multi_worker_config_autoscale() {
        // Single worker but max_scale > 1 means we might scale up
        let names = vec!["alpha".to_string()];
        let (is_multi, leader) = compute_multi_worker_config(&names, 4);
        assert!(is_multi);
        assert_eq!(leader, Some("alpha".to_string()));
    }

    #[test]
    fn test_compute_multi_worker_config_empty() {
        let names: Vec<String> = vec![];
        let (is_multi, leader) = compute_multi_worker_config(&names, 1);
        assert!(!is_multi);
        assert!(leader.is_none());
    }
}
