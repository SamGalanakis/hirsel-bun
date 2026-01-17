//! Run-related commands
//!
//! Commands for managing runs: listing, viewing details, pausing, resuming, deleting, and delivering.

use chrono::Utc;

use super::helpers::{
    calculate_duration_minutes, convert_status, is_completed_status, parse_elapsed_minutes,
    trigger_compaction_if_needed,
};
use super::types::{RunDetail, RunStatus, RunSummary};
use crate::core::gyp_chat::GypChatStore;
use crate::core::{config, state::SQLiteState};

/// Get list of all runs
/// Optimized to use get_run_summary which fetches all data in 3 queries per run
#[tauri::command]
pub async fn get_runs() -> Result<Vec<RunSummary>, String> {
    let run_names = config::list_runs().map_err(|e| format!("list_runs error: {}", e))?;
    let mut runs = Vec::new();

    for name in run_names {
        let db_path = config::run_dir(&name).join("hirsel.db");
        if !db_path.exists() {
            continue;
        }

        match SQLiteState::new(db_path.clone()) {
            Ok(state) => {
                // Use optimized summary fetch (3 queries instead of ~9)
                let summary = match state.get_run_summary() {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // Convert core Status to GUI RunStatus
                let run_status = convert_status(summary.status);

                // For completed runs, recalculate elapsed as duration (start to completion)
                // The summary returns elapsed from start to now, which is correct for active runs
                let elapsed_minutes = if is_completed_status(&run_status) {
                    let start = summary
                        .started_at
                        .as_deref()
                        .or(summary.created_at.as_deref());
                    if let (Some(start), Some(end)) = (start, summary.updated_at.as_deref()) {
                        calculate_duration_minutes(start, end)
                    } else {
                        summary.elapsed_minutes
                    }
                } else {
                    summary.elapsed_minutes
                };

                let created_at = summary
                    .created_at
                    .unwrap_or_else(|| Utc::now().to_rfc3339());

                runs.push(RunSummary {
                    name,
                    status: run_status,
                    tasks_done: summary.tasks_done,
                    tasks_total: summary.tasks_total,
                    workers_active: summary.workers_active,
                    workers_total: summary.workers_total,
                    elapsed_minutes,
                    time_limit_minutes: summary.time_limit_minutes.map(|m| m as u32),
                    has_unread_messages: summary.unread_count > 0,
                    created_at,
                });
            }
            Err(_) => continue,
        }
    }

    // Sort by created_at descending (newest first)
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(runs)
}

/// Get detailed information about a specific run
#[tauri::command]
pub async fn get_run_detail(run_name: String) -> Result<RunDetail, String> {
    let db_path = config::run_dir(&run_name).join("hirsel.db");
    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    let status = state.status().unwrap_or(crate::core::state::Status::Idle);
    let run_status = convert_status(status);

    let request = state.get_request().ok().flatten();
    let project_path = state.get_project_path().ok().flatten();
    let worker_scale = state.get_worker_scale().ok().flatten();
    let time_limit_minutes = state
        .get_time_limit_minutes()
        .ok()
        .flatten()
        .map(|m| m as u32);
    let started_at = state.get_started_at().ok().flatten();
    let summary = state.get_summary().ok().flatten();
    let created_at = state
        .get_created_at()
        .ok()
        .flatten()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let iteration_count = state.get_iteration_count().unwrap_or(0) as u32;
    let max_iterations = state.get_max_iterations().ok().flatten().map(|m| m as u32);
    let human_in_the_loop = state.get_human_in_the_loop().unwrap_or(true);
    let waiting_reason = state.get_waiting_reason().ok().flatten();
    let unread_count = state.get_unread_count().unwrap_or(0) as u32;

    // Get tasks and workers for counts
    let tasks = state.get_tasks().unwrap_or_default();
    let workers = state.get_workers().unwrap_or_default();

    let tasks_done = tasks
        .iter()
        .filter(|t| t.status == crate::core::state::TaskStatus::Done)
        .count() as u32;
    let tasks_total = tasks.len() as u32;
    let workers_active = workers
        .iter()
        .filter(|w| w.status == crate::core::state::WorkerStatus::Working)
        .count() as u32;
    let workers_total = workers.len() as u32;

    // Calculate elapsed minutes
    let elapsed_minutes = if let Ok(Some(time_info)) = state.get_time_info() {
        time_info.elapsed_minutes
    } else if let Some(ref sa) = started_at {
        parse_elapsed_minutes(sa)
    } else {
        parse_elapsed_minutes(&created_at)
    };

    // Get remote URL if set
    let remote_url = state.get_remote_url().ok().flatten();

    // Get branch if set
    let branch = state.get_branch().ok().flatten();

    // Get learnings count (efficient COUNT query instead of fetching all)
    let learnings_count = state.get_messages_count("learnings").unwrap_or(0) as u32;
    let learnings_processed_at = state.get_learnings_processed_at().ok().flatten();

    // Trigger automatic compaction check (spawns subprocess if needed)
    // Only when run is in an active state
    if matches!(
        run_status,
        RunStatus::Working | RunStatus::Eval | RunStatus::Done
    ) {
        if let Err(e) = trigger_compaction_if_needed(&run_name) {
            tracing::debug!("Compaction check failed: {}", e);
        }
    }

    Ok(RunDetail {
        name: run_name,
        status: run_status,
        request,
        project_path,
        remote_url,
        branch,
        worker_scale,
        time_limit_minutes,
        started_at,
        summary,
        created_at: created_at.clone(),
        updated_at: created_at,
        iteration_count,
        max_iterations,
        human_in_the_loop,
        waiting_reason,
        unread_count,
        tasks_done,
        tasks_total,
        workers_active,
        workers_total,
        elapsed_minutes,
        learnings_count,
        learnings_processed_at,
    })
}

/// Pause a running run
#[tauri::command]
pub async fn pause_run(run_name: String) -> Result<(), String> {
    use crate::core::workers::pause_all_workers;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Check current status
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status == crate::core::state::Status::Paused {
        return Ok(()); // Already paused
    }
    if status != crate::core::state::Status::Working {
        return Err(format!("Cannot pause run in '{}' status", status));
    }

    // Pause all workers (sends SIGTERM)
    let paused =
        pause_all_workers(&state).map_err(|e| format!("Failed to pause workers: {}", e))?;

    // Update status
    state
        .set_status(crate::core::state::Status::Paused)
        .map_err(|e| format!("Failed to update status: {}", e))?;

    tracing::info!(
        "Paused run '{}', stopped {} workers",
        run_name,
        paused.len()
    );
    Ok(())
}

/// Resume a paused run
#[tauri::command]
pub async fn resume_run(run_name: String) -> Result<(), String> {
    use crate::cli::config::get_agent_command;
    use crate::core::workers::{maybe_scale_up, resume_awaiting_workers};

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Check current status
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status == crate::core::state::Status::Working {
        return Ok(()); // Already running
    }
    if status != crate::core::state::Status::Paused && status != crate::core::state::Status::Runaway
    {
        return Err(format!("Cannot resume run in '{}' status", status));
    }

    // Update status first
    state
        .set_status(crate::core::state::Status::Working)
        .map_err(|e| format!("Failed to update status: {}", e))?;

    // Resume existing workers
    let agent_command = get_agent_command();
    let resumed = resume_awaiting_workers(&run_name, &run_dir, &agent_command)
        .map_err(|e| format!("Failed to resume workers: {}", e))?;

    tracing::info!(
        "Resumed run '{}', restarted {} workers",
        run_name,
        resumed.len()
    );

    // Try to scale up if more tasks are available
    loop {
        match maybe_scale_up(&run_name, &run_dir, &agent_command) {
            Ok(Some(new_worker)) => {
                tracing::info!("Scaled up: spawned new worker {}", new_worker);
            }
            Ok(None) => break, // No more scaling needed
            Err(e) => {
                tracing::warn!("Failed to scale up: {}", e);
                break;
            }
        }
    }

    Ok(())
}

/// Delete a run
#[tauri::command]
pub async fn delete_run(run_name: String) -> Result<(), String> {
    use crate::core::workers::kill_all_workers;
    use std::fs;

    let run_dir = config::run_dir(&run_name);

    if !run_dir.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    // Kill any running workers first
    let db_path = run_dir.join("hirsel.db");
    if db_path.exists() {
        if let Ok(state) = SQLiteState::new(db_path) {
            if let Ok(killed) = kill_all_workers(&state) {
                if !killed.is_empty() {
                    tracing::info!("Killed {} worker(s) before deleting run", killed.len());
                }
            }
        }
    }

    // Delete Gyp chat history for this run
    match GypChatStore::open() {
        Ok(store) => {
            if let Err(e) = store.delete_run_messages(&run_name) {
                tracing::warn!(
                    "Failed to delete Gyp chat history for run '{}': {}",
                    run_name,
                    e
                );
            }
        }
        Err(_) => {
            // Ignore errors opening the store
        }
    }

    // Delete the run directory
    fs::remove_dir_all(&run_dir).map_err(|e| format!("Failed to delete run directory: {}", e))?;

    tracing::info!("Deleted run '{}'", run_name);
    Ok(())
}

/// Deliver a run's changes to a branch
///
/// Creates a branch in the target repository with the run's changes.
/// For remote repos, pushes to the remote. For local repos, creates a local branch.
/// Returns the branch name on success.
#[tauri::command]
pub async fn deliver_run(run_name: String, branch_name: Option<String>) -> Result<String, String> {
    use crate::core::git;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Get project path and remote URL
    let project_path_str = state
        .get_project_path()
        .map_err(|e| format!("Failed to get project path: {}", e))?;
    let remote_url = state
        .get_remote_url()
        .map_err(|e| format!("Failed to get remote URL: {}", e))?;
    let saved_branch = state
        .get_branch()
        .map_err(|e| format!("Failed to get branch: {}", e))?;

    let project_path = project_path_str
        .as_ref()
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists())
        .ok_or_else(|| format!("Project path not found for run '{}'", run_name))?;

    // Find work directory (staging dir has the main git repo)
    let work_dir = run_dir.join("work").join("staging");
    let work_dir = if work_dir.exists() {
        work_dir
    } else {
        let fallback = run_dir.join("work");
        if fallback.exists() {
            fallback
        } else {
            return Err(format!("Work directory not found for run '{}'", run_name));
        }
    };

    if !work_dir.join(".git").exists() {
        return Err("No git repository found in work directory".into());
    }

    // Check for unmerged branches
    let unmerged = git::list_unmerged_branches(&work_dir)
        .map_err(|e| format!("Failed to check branches: {}", e))?;
    if !unmerged.is_empty() {
        let branch_list = unmerged.join(", ");
        return Err(format!(
            "Unmerged branches exist: {}. All work must be merged to 'staging' before delivering.",
            branch_list
        ));
    }

    // Determine branch name: provided > saved > default
    let branch = branch_name
        .or(saved_branch)
        .unwrap_or_else(|| format!("hirsel/{}", run_name));

    // Deliver based on whether it's a remote or local repo
    let (success, message) = if let Some(ref url) = remote_url {
        git::push_to_remote(&work_dir, url, &branch)
            .map_err(|e| format!("Failed to push to remote: {}", e))?
    } else {
        // Check if branch already exists in local project repo
        if git::branch_exists(&branch, Some(&project_path))
            .map_err(|e| format!("Failed to check branch: {}", e))?
        {
            return Err(format!(
                "Branch '{}' already exists in project repository",
                branch
            ));
        }

        git::push_staging_as_branch(&work_dir, &project_path, &branch)
            .map_err(|e| format!("Failed to create branch: {}", e))?
    };

    if !success {
        return Err(format!("Delivery failed: {}", message));
    }

    // Update run status to delivered
    state
        .set_status(crate::core::state::Status::Delivered)
        .map_err(|e| format!("Failed to update status: {}", e))?;

    tracing::info!("Delivered run '{}' to branch '{}'", run_name, branch);
    Ok(branch)
}
