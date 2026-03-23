//! Runtime deletion operations shared by backend/admin surfaces.

use std::fs;
use std::path::PathBuf;

use crate::core::config::{self, Config};
use crate::core::lifecycle::LocalLifecycleManager;
use crate::core::shepherd_chat::ShepherdChatStore;
use crate::core::snapshot::{create_archive_strategy, ArchiveHandle, WorkerStateHandle};

use super::types::{DeleteRunConfig, DeleteRunResult};
use super::OpsError;

/// Delete a run and all associated resources
///
/// This operation:
/// 1. Kills any running workers
/// 2. Optionally deletes ShepherdChat messages (GUI)
/// 3. Optionally removes the hirsel_work remote from the project (CLI)
/// 4. Removes the run directory
///
/// # Arguments
///
/// * `config` - Configuration specifying what to delete and cleanup options
///
/// # Returns
///
/// * `Ok(DeleteRunResult)` - Details about what was deleted
/// * `Err(OpsError)` - If the operation failed
pub async fn delete_run(config: DeleteRunConfig) -> Result<DeleteRunResult, OpsError> {
    let runtime_dir = config::runtime_dir(&config.runtime_name);

    // Check run exists
    if !runtime_dir.exists() {
        return Err(OpsError::RunNotFound(config.runtime_name));
    }

    let mut result = DeleteRunResult {
        runtime_name: config.runtime_name.clone(),
        workers_killed: 0,
    };

    // Try to get project path, kill workers, and clean up snapshots
    let project_path = if let Ok(lifecycle) = LocalLifecycleManager::new(
        &config.runtime_name,
        runtime_dir.clone(),
        vec![], // Agent command not needed for kill
    )
    .await
    {
        // Clean up any snapshots before killing workers
        if let Ok(workers) = lifecycle.state().get_workers().await {
            let (app_config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

            for worker in workers {
                if let Some(ref state_handle_json) = worker.state_handle {
                    if let Ok(state_handle) =
                        serde_json::from_str::<WorkerStateHandle>(state_handle_json)
                    {
                        // Delete archived work directory if present
                        if let Some(ref work_dir_snapshot) = state_handle.work_dir {
                            let runner_config = app_config.sandbox_config();
                            if let Ok(strategy) =
                                create_archive_strategy(&runner_config, &app_config.storage).await
                            {
                                let handle = ArchiveHandle {
                                    strategy_type: work_dir_snapshot.strategy_type.clone(),
                                    storage_id: work_dir_snapshot.storage_id.clone(),
                                    size_bytes: work_dir_snapshot.size_bytes,
                                };
                                if let Err(e) = strategy.delete(&handle).await {
                                    tracing::warn!(
                                        "Failed to delete archive {} for worker {}: {}",
                                        work_dir_snapshot.storage_id,
                                        worker.name,
                                        e
                                    );
                                } else {
                                    tracing::debug!(
                                        "Deleted archive {} for worker {}",
                                        work_dir_snapshot.storage_id,
                                        worker.name
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Kill any running worker processes using lifecycle manager
        if let Ok(killed) = lifecycle.kill_all_workers().await {
            result.workers_killed = killed.len();
            if !killed.is_empty() {
                tracing::info!(
                    "Killed {} worker(s) before deleting run '{}'",
                    killed.len(),
                    config.runtime_name
                );
            }
        }

        // Get project path for remote cleanup
        lifecycle.state().get_project_path().await.ok().flatten()
    } else {
        None
    };

    // Remove hirsel_work remote from project repo
    if let Some(project_path_str) = &project_path {
        let project_path = PathBuf::from(project_path_str);
        if project_path.exists() {
            if let Ok(repo) = git2::Repository::open(&project_path) {
                if repo.remote_delete("hirsel_work").is_ok() {
                    tracing::debug!(
                        "Removed hirsel_work remote from project for run '{}'",
                        config.runtime_name
                    );
                }
            }
        }
    }

    // Delete Shepherd chat history
    if let Ok(store) = ShepherdChatStore::open().await {
        if store
            .delete_run_messages(&config.runtime_name)
            .await
            .is_ok()
        {
            tracing::debug!(
                "Deleted ShepherdChat messages for run '{}'",
                config.runtime_name
            );
        }
    }

    // Remove the run directory
    fs::remove_dir_all(&runtime_dir)?;

    tracing::info!("Deleted run '{}'", config.runtime_name);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_run_config() {
        let config = DeleteRunConfig::new("test-run");
        assert_eq!(config.runtime_name, "test-run");
    }

    #[tokio::test]
    async fn test_delete_nonexistent_run() {
        let config = DeleteRunConfig::new("nonexistent-run-12345");
        let result = delete_run(config).await;
        assert!(matches!(result, Err(OpsError::RunNotFound(_))));
    }
}
