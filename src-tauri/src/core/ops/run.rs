//! Run operations - create, delete, clone, start runs
//!
//! These operations are shared between CLI and GUI.

use std::fs;
use std::path::PathBuf;

use crate::core::config::{self, Config};
use crate::core::lifecycle::LocalLifecycleManager;
use crate::core::shepherd_chat::ShepherdChatStore;
use crate::core::snapshot::{create_archive_strategy, ArchiveHandle, WorkerStateHandle};
use crate::core::state::SQLiteState;
use crate::core::Files;

use super::types::{CloneRunConfig, CloneRunResult, DeleteRunConfig, DeleteRunResult};
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
    let run_dir = config::run_dir(&config.run_name);

    // Check run exists
    if !run_dir.exists() {
        return Err(OpsError::RunNotFound(config.run_name));
    }

    let mut result = DeleteRunResult {
        run_name: config.run_name.clone(),
        workers_killed: 0,
    };

    // Try to get project path, kill workers, and clean up snapshots
    let project_path = if let Ok(lifecycle) = LocalLifecycleManager::new(
        &config.run_name,
        run_dir.clone(),
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
                            let runner_config = app_config.get_runner_for_worker(&worker.name);
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
                    config.run_name
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
                        config.run_name
                    );
                }
            }
        }
    }

    // Delete Shepherd chat history
    if let Ok(store) = ShepherdChatStore::open().await {
        if store.delete_run_messages(&config.run_name).await.is_ok() {
            tracing::debug!(
                "Deleted ShepherdChat messages for run '{}'",
                config.run_name
            );
        }
    }

    // Remove the run directory
    fs::remove_dir_all(&run_dir)?;

    tracing::info!("Deleted run '{}'", config.run_name);

    Ok(result)
}

/// Clone a run to a new draft
///
/// This operation:
/// 1. Validates source exists and new name doesn't
/// 2. Reads settings from source (project path, worker scale, time limit, etc.)
/// 3. Copies spec.md, eval.md, and optionally assets
/// 4. Initializes new database with Draft status
///
/// # Arguments
///
/// * `config` - Configuration specifying source and destination
///
/// # Returns
///
/// * `Ok(CloneRunResult)` - Details about what was cloned
/// * `Err(OpsError)` - If the operation failed
pub async fn clone_run(config: CloneRunConfig) -> Result<CloneRunResult, OpsError> {
    // Validate new name
    let new_name = config.new_name.trim().to_string();
    if new_name.is_empty() {
        return Err(OpsError::InvalidState(
            "New run name cannot be empty".into(),
        ));
    }

    // Check source exists
    let source_dir = config::run_dir(&config.source_run);
    let source_db_path = source_dir.join("hirsel.db");
    if !source_db_path.exists() {
        return Err(OpsError::RunNotFound(config.source_run));
    }

    // Check new name doesn't exist
    let new_dir = config::run_dir(&new_name);
    if new_dir.exists() {
        return Err(OpsError::RunAlreadyExists(new_name));
    }

    // Open source database to read settings
    let source_state = SQLiteState::new(&config.source_run).await?;

    // Read starting_point from source (if stored)
    let starting_point_json = source_state.get_starting_point().await.ok().flatten();

    // Read settings from source
    // For greenfield/gitrepo starting points, don't copy project_path - the cloned draft
    // will create its own workspace when started. For LocalFolder, keep the external path.
    let project_path = source_state
        .get_project_path()
        .await
        .ok()
        .flatten()
        .and_then(|p| {
            // Check if we have starting_point info
            if let Some(ref sp_json) = starting_point_json {
                if let Ok(sp) = serde_json::from_str::<crate::core::draft::StartingPoint>(sp_json) {
                    match sp {
                        crate::core::draft::StartingPoint::LocalFolder { .. } => {
                            // External folder - keep the reference
                            return Some(p);
                        }
                        _ => {
                            // Greenfield or GitRepo - don't copy (internal workspace)
                            tracing::debug!(
                                "Skipping project_path for clone - starting_point is {:?}",
                                sp
                            );
                            return None;
                        }
                    }
                }
            }

            // Fallback: check if project_path is inside source run directory
            let path = PathBuf::from(&p);
            let source_dir_canonical = source_dir.canonicalize().ok()?;
            let path_canonical = path.canonicalize().ok()?;

            if path_canonical.starts_with(&source_dir_canonical) {
                tracing::debug!(
                    "Skipping project_path '{}' for clone - it's inside source run directory",
                    p
                );
                None
            } else {
                Some(p)
            }
        });
    let worker_scale = source_state
        .get_worker_scale()
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "5".to_string());
    let time_limit_minutes = source_state.get_time_limit_minutes().await.ok().flatten();
    let human_in_the_loop = source_state.get_human_in_the_loop().await.unwrap_or(true);
    let default_runner = source_state.get_default_runner().await.ok().flatten();

    // Read spec.md from source
    let source_spec_path = source_dir.join("spec.md");
    let spec_content = if source_spec_path.exists() {
        fs::read_to_string(&source_spec_path).unwrap_or_default()
    } else {
        String::new()
    };

    // Read eval.md from source (optional)
    let source_eval_path = source_dir.join("eval.md");
    let eval_content = if source_eval_path.exists() {
        fs::read_to_string(&source_eval_path).ok()
    } else {
        None
    };

    // Create new run directory
    fs::create_dir_all(&new_dir)?;

    // Initialize Files helper and create required directories
    let files = Files::new(&new_dir);
    files
        .init_dirs()
        .map_err(|e| OpsError::OperationFailed(format!("Failed to init dirs: {}", e)))?;

    // Write spec.md
    fs::write(new_dir.join("spec.md"), &spec_content)?;

    // Write eval.md if exists
    let has_eval = eval_content.is_some();
    if let Some(eval) = &eval_content {
        fs::write(new_dir.join("eval.md"), eval)?;
    }

    // Copy assets folder if requested and exists
    let mut assets_copied = false;
    if config.copy_assets {
        let source_assets = source_dir.join("assets");
        if source_assets.exists() && source_assets.is_dir() {
            let dest_assets = new_dir.join("assets");
            fs::create_dir_all(&dest_assets)?;
            // Copy all files
            if let Ok(entries) = fs::read_dir(&source_assets) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(filename) = path.file_name() {
                            let _ = fs::copy(&path, dest_assets.join(filename));
                            assets_copied = true;
                        }
                    }
                }
            }
        }
    }

    // Write initial tasks.md
    fs::write(
        new_dir.join("tasks.md"),
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Scope |\n",
    )?;

    // Initialize database
    let new_state = SQLiteState::new(&new_name).await?;

    // Set up new run with Draft status
    new_state.init_state(project_path.as_deref()).await?;
    new_state
        .set_status(crate::core::state::Status::Draft)
        .await?;
    new_state.set_worker_scale(&worker_scale).await?;
    new_state.set_human_in_the_loop(human_in_the_loop).await?;

    if let Some(limit) = time_limit_minutes {
        new_state.set_time_limit_minutes(Some(limit)).await?;
    }
    if let Some(ref runner) = default_runner {
        new_state.set_default_runner(Some(runner)).await?;
    }
    if !spec_content.is_empty() {
        new_state.set_request(Some(&spec_content)).await?;
    }

    // Copy starting_point from source (if exists)
    if let Some(ref sp_json) = starting_point_json {
        new_state.set_starting_point(Some(sp_json)).await?;
    }

    tracing::info!("Cloned '{}' to '{}' (draft)", config.source_run, new_name);

    Ok(CloneRunResult {
        source_run: config.source_run,
        new_name,
        project_path,
        worker_scale,
        time_limit_minutes,
        human_in_the_loop,
        default_runner,
        spec_content,
        has_eval,
        assets_copied,
        run_dir: new_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_run_config() {
        let config = DeleteRunConfig::new("test-run");
        assert_eq!(config.run_name, "test-run");
    }

    #[tokio::test]
    async fn test_delete_nonexistent_run() {
        let config = DeleteRunConfig::new("nonexistent-run-12345");
        let result = delete_run(config).await;
        assert!(matches!(result, Err(OpsError::RunNotFound(_))));
    }

    #[tokio::test]
    async fn test_clone_nonexistent_run() {
        let config = CloneRunConfig::new("nonexistent-run-12345", "new-run");
        let result = clone_run(config).await;
        assert!(matches!(result, Err(OpsError::RunNotFound(_))));
    }

    #[tokio::test]
    async fn test_clone_empty_name() {
        let config = CloneRunConfig::new("source", "  ");
        let result = clone_run(config).await;
        assert!(matches!(result, Err(OpsError::InvalidState(_))));
    }
}
