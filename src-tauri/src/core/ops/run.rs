//! Run operations - create, delete, clone, start runs
//!
//! These operations are shared between CLI and GUI.

use std::fs;
use std::path::PathBuf;

use crate::core::config;
use crate::core::gyp_chat::GypChatStore;
use crate::core::state::SQLiteState;
use crate::core::workers::kill_all_workers;
use crate::core::Files;

use super::types::{CloneRunConfig, CloneRunResult, DeleteRunConfig, DeleteRunResult};
use super::OpsError;

/// Delete a run and all associated resources
///
/// This operation:
/// 1. Kills any running workers
/// 2. Optionally deletes GypChat messages (GUI)
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
pub fn delete_run(config: DeleteRunConfig) -> Result<DeleteRunResult, OpsError> {
    let run_dir = config::run_dir(&config.run_name);

    // Check run exists
    if !run_dir.exists() {
        return Err(OpsError::RunNotFound(config.run_name));
    }

    let mut result = DeleteRunResult {
        run_name: config.run_name.clone(),
        workers_killed: 0,
        gyp_chat_deleted: false,
        project_remote_removed: false,
    };

    // Try to get project path and kill workers
    let files = Files::new(&run_dir);
    let project_path = if let Ok(state) = SQLiteState::new(files.db_path()) {
        // Kill any running worker processes (forcefully)
        if let Ok(killed) = kill_all_workers(&state) {
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
        state.get_project_path().ok().flatten()
    } else {
        None
    };

    // Remove hirsel_work remote from project repo if requested
    if config.remove_project_remote {
        if let Some(project_path_str) = &project_path {
            let project_path = PathBuf::from(project_path_str);
            if project_path.exists() {
                if let Ok(repo) = git2::Repository::open(&project_path) {
                    if repo.remote_delete("hirsel_work").is_ok() {
                        result.project_remote_removed = true;
                        tracing::debug!(
                            "Removed hirsel_work remote from project for run '{}'",
                            config.run_name
                        );
                    }
                }
            }
        }
    }

    // Delete Gyp chat history if requested
    if config.delete_gyp_chat {
        if let Ok(store) = GypChatStore::open() {
            if store.delete_run_messages(&config.run_name).is_ok() {
                result.gyp_chat_deleted = true;
                tracing::debug!("Deleted GypChat messages for run '{}'", config.run_name);
            }
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
pub fn clone_run(config: CloneRunConfig) -> Result<CloneRunResult, OpsError> {
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
    let source_state = SQLiteState::new(source_db_path)?;

    // Read settings from source
    let project_path = source_state.get_project_path().ok().flatten();
    let worker_scale = source_state
        .get_worker_scale()
        .ok()
        .flatten()
        .unwrap_or_else(|| "1".to_string());
    let time_limit_minutes = source_state.get_time_limit_minutes().ok().flatten();
    let human_in_the_loop = source_state.get_human_in_the_loop().unwrap_or(true);
    let max_iterations = source_state.get_max_iterations().ok().flatten();

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
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Read spec, create exploration tasks |\n",
    )?;

    // Initialize database
    let new_state = SQLiteState::new(new_dir.join("hirsel.db"))?;

    // Set up new run with Draft status
    new_state.init_state(project_path.as_deref())?;
    new_state.set_status(crate::core::state::Status::Draft)?;
    new_state.set_worker_scale(&worker_scale)?;
    new_state.set_human_in_the_loop(human_in_the_loop)?;

    if let Some(limit) = time_limit_minutes {
        new_state.set_time_limit_minutes(Some(limit))?;
    }
    if let Some(max_iter) = max_iterations {
        new_state.set_max_iterations(Some(max_iter))?;
    }
    if !spec_content.is_empty() {
        new_state.set_request(Some(&spec_content))?;
    }

    // Add initial scope task
    let _ = new_state.add_task("scope", "Read spec, create exploration tasks", None, None);

    tracing::info!("Cloned '{}' to '{}' (draft)", config.source_run, new_name);

    Ok(CloneRunResult {
        source_run: config.source_run,
        new_name,
        project_path,
        worker_scale,
        time_limit_minutes,
        human_in_the_loop,
        max_iterations,
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
    fn test_delete_run_config_for_gui() {
        let config = DeleteRunConfig::for_gui("test-run");
        assert_eq!(config.run_name, "test-run");
        assert!(config.delete_gyp_chat);
        assert!(!config.remove_project_remote);
    }

    #[test]
    fn test_delete_run_config_for_cli() {
        let config = DeleteRunConfig::for_cli("test-run");
        assert_eq!(config.run_name, "test-run");
        assert!(!config.delete_gyp_chat);
        assert!(config.remove_project_remote);
    }

    #[test]
    fn test_delete_nonexistent_run() {
        let config = DeleteRunConfig::for_cli("nonexistent-run-12345");
        let result = delete_run(config);
        assert!(matches!(result, Err(OpsError::RunNotFound(_))));
    }

    #[test]
    fn test_clone_nonexistent_run() {
        let config = CloneRunConfig::new("nonexistent-run-12345", "new-run");
        let result = clone_run(config);
        assert!(matches!(result, Err(OpsError::RunNotFound(_))));
    }

    #[test]
    fn test_clone_empty_name() {
        let config = CloneRunConfig::new("source", "  ");
        let result = clone_run(config);
        assert!(matches!(result, Err(OpsError::InvalidState(_))));
    }
}
