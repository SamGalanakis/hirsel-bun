//! Worker spawning operations - spawn local workers for a run
//!
//! These operations are shared between CLI and GUI for spawning workers.

use std::path::PathBuf;

use crate::core::state::SQLiteState;
use crate::core::workers::{spawn_worker, WorkerError, WorkerSpawnConfig};
use tracing::{info, warn};

// =============================================================================
// Configuration Types
// =============================================================================

/// Configuration for spawning workers
#[derive(Debug, Clone)]
pub struct SpawnWorkersConfig {
    /// Name of the run
    pub run_name: String,
    /// Path to the run directory
    pub run_dir: PathBuf,
    /// Agent command to run (e.g., ["codex"])
    pub agent_command: Vec<String>,
    /// Whether this is a multi-worker run
    pub is_multi_worker: bool,
    /// Name of the leader worker
    pub leader_name: Option<String>,
    /// All worker names (for teammates list)
    pub all_worker_names: Vec<String>,
}

/// Result of spawning workers
#[derive(Debug, Clone)]
pub struct SpawnWorkersResult {
    /// Names of successfully spawned workers
    pub spawned: Vec<String>,
    /// Failed spawns: (worker_name, error_message)
    pub failed: Vec<(String, String)>,
    /// Whether spawning was stopped because the run was paused
    pub paused: bool,
}

// =============================================================================
// Worker Spawning
// =============================================================================

/// Spawn local workers for a run
///
/// Iterates through workers and spawns each one. Handles:
/// - Leader designation (first worker in multi-worker mode)
/// - Teammates list for each worker
/// - Run paused detection (stops spawning if run is paused)
///
/// # Arguments
///
/// * `config` - Configuration specifying run details and agent command
/// * `worker_dirs` - List of (worker_name, work_dir) tuples
/// * `state` - The SQLite state for the run
///
/// # Returns
///
/// * `SpawnWorkersResult` - Details about spawned workers and any failures
pub async fn spawn_local_workers(
    config: &SpawnWorkersConfig,
    worker_dirs: &[(String, PathBuf)],
    state: &SQLiteState,
) -> SpawnWorkersResult {
    let mut result = SpawnWorkersResult {
        spawned: Vec::new(),
        failed: Vec::new(),
        paused: false,
    };

    for (i, (worker_name, work_dir)) in worker_dirs.iter().enumerate() {
        let is_leader = i == 0 && config.is_multi_worker;

        // Build teammates list (all workers except self)
        let teammates = if config.is_multi_worker {
            Some(
                config
                    .all_worker_names
                    .iter()
                    .filter(|t| *t != worker_name)
                    .cloned()
                    .collect(),
            )
        } else {
            None
        };

        let spawn_config = WorkerSpawnConfig {
            run_name: config.run_name.clone(),
            worker_name: worker_name.clone(),
            work_dir: work_dir.clone(),
            run_dir: config.run_dir.clone(),
            agent_command: config.agent_command.clone(),
            is_leader,
            leader_name: config.leader_name.clone(),
            teammates,
            resume_session_id: None,
            env_vars: None,
            credentials: None,
            coordinator_url: None,
            tailscale_authkey: None,
            assigned_task_id: None,
            is_plan_task: false,
        };

        match spawn_worker(spawn_config, state).await {
            Ok(spawn_result) => {
                info!(
                    "Spawned worker {} (PID {})",
                    spawn_result.worker_name, spawn_result.pid
                );
                result.spawned.push(worker_name.clone());
            }
            Err(WorkerError::RunPaused) => {
                // Run was paused - don't spawn more workers
                info!("Run paused, stopping worker spawn at {}", worker_name);
                result.paused = true;
                break;
            }
            Err(e) => {
                // Log error but continue with other workers
                warn!("Failed to spawn worker {}: {}", worker_name, e);
                result.failed.push((worker_name.clone(), e.to_string()));
            }
        }
    }

    result
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_workers_config() {
        let config = SpawnWorkersConfig {
            run_name: "test-run".to_string(),
            run_dir: PathBuf::from("/runs/test-run"),
            agent_command: vec!["codex".to_string()],
            is_multi_worker: true,
            leader_name: Some("alpha".to_string()),
            all_worker_names: vec!["alpha".to_string(), "beta".to_string()],
        };

        assert_eq!(config.run_name, "test-run");
        assert_eq!(config.agent_command, vec!["codex".to_string()]);
        assert!(config.is_multi_worker);
        assert_eq!(config.leader_name, Some("alpha".to_string()));
    }

    #[test]
    fn test_spawn_workers_result_default() {
        let result = SpawnWorkersResult {
            spawned: vec![],
            failed: vec![],
            paused: false,
        };

        assert!(result.spawned.is_empty());
        assert!(result.failed.is_empty());
        assert!(!result.paused);
    }
}
