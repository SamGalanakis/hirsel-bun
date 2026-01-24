//! Run setup operations - workspace creation, worker registration, and configuration
//!
//! These operations are shared between CLI and GUI for setting up runs.

use std::path::PathBuf;

use crate::core::chats::{
    create_default_group_chat, create_learnings_thread, create_worker_chat, ChatError,
};
use crate::core::git::{create_worker_clone, create_workspace, GitError};
use crate::core::state::SQLiteState;
use crate::core::Files;

use super::OpsError;

// =============================================================================
// Configuration Types
// =============================================================================

/// Configuration for setting up a run's workspace and workers
#[derive(Debug, Clone)]
pub struct RunSetupConfig {
    /// Name of the run
    pub run_name: String,
    /// Path to the project repository
    pub project_path: PathBuf,
    /// Path to the run directory (~/.hirsel/runs/<run_name>)
    pub run_dir: PathBuf,
    /// Names of workers to create clones for (local workers)
    pub worker_names: Vec<String>,
    /// Additional workers that need chats but not clones (e.g., remote workers)
    /// These workers are included in group chat and learnings thread, and get individual chats
    pub additional_chat_workers: Vec<String>,
    /// Whether this is a multi-worker run (affects workspace layout)
    pub is_multi_worker: bool,
    /// Name of the leader worker (first worker in multi-worker mode)
    pub leader_name: Option<String>,
}

/// Result of setting up a run's workspace
#[derive(Debug, Clone)]
pub struct RunSetupResult {
    /// Path to the main workspace directory (staging area)
    pub workspace_dir: PathBuf,
    /// Worker directories: (worker_name, worker_dir_path)
    pub worker_dirs: Vec<(String, PathBuf)>,
    /// Path to the chats directory
    pub chats_dir: PathBuf,
}

// =============================================================================
// Error Conversion
// =============================================================================

impl From<GitError> for OpsError {
    fn from(e: GitError) -> Self {
        OpsError::Git(e.to_string())
    }
}

impl From<ChatError> for OpsError {
    fn from(e: ChatError) -> Self {
        OpsError::OperationFailed(format!("Chat error: {}", e))
    }
}

// =============================================================================
// Multi-Worker Configuration
// =============================================================================

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

/// Set up workspace, worker clones, and chats for a run
///
/// This operation:
/// 1. Creates the main workspace (staging) from the project
/// 2. Creates worker clone directories (in multi-worker mode) or uses workspace directly
/// 3. Creates chat files (user chat, group chat if multi-worker, learnings thread, worker chats)
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
    // Get runs directory (parent of run_dir)
    let runs_dir = config
        .run_dir
        .parent()
        .ok_or_else(|| OpsError::InvalidState("Invalid run_dir path".to_string()))?;

    // Create workspace with staging branch
    let workspace_dir = create_workspace(&config.run_name, &config.project_path, runs_dir)?;

    // Create worker clones/worktrees
    let mut worker_dirs: Vec<(String, PathBuf)> = Vec::new();

    for worker_name in &config.worker_names {
        let worker_dir = if config.is_multi_worker {
            create_worker_clone(
                &config.run_name,
                &config.project_path,
                worker_name,
                Some(&workspace_dir),
                runs_dir,
            )?
        } else {
            // Single worker uses workspace directly
            workspace_dir.clone()
        };

        worker_dirs.push((worker_name.clone(), worker_dir));
    }

    // Create chats
    let files = Files::new(&config.run_dir);
    let chats_dir = files.chats_dir();

    // Combine all workers for chat creation (local + additional)
    let mut all_workers: Vec<String> = config.worker_names.clone();
    all_workers.extend(config.additional_chat_workers.clone());

    // Create group chat if multi-worker
    if config.is_multi_worker {
        create_default_group_chat(&chats_dir, &all_workers, config.leader_name.as_deref())?;
    }

    // Create learnings thread
    create_learnings_thread(&chats_dir, &all_workers)?;

    // Create individual worker chats for all workers
    for worker_name in &all_workers {
        create_worker_chat(&chats_dir, worker_name)?;
    }

    Ok(RunSetupResult {
        workspace_dir,
        worker_dirs,
        chats_dir,
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
pub fn register_workers(
    state: &SQLiteState,
    worker_dirs: &[(String, PathBuf)],
    location: &str,
) -> Result<(), OpsError> {
    for (worker_name, work_dir) in worker_dirs {
        state.add_worker(worker_name, work_dir.to_str().unwrap_or("."), location)?;
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
