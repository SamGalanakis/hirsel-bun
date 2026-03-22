//! Run-related commands
//!
//! Commands for managing runs: listing, viewing details, pausing, resuming, deleting, and delivering.

use super::ResultExt;
use crate::core::api_types::{RunDetail, RunSummary};
use crate::core::delta::{
    bump_generation, get_generation, update_project_run_status_by_name, ProjectRunStatus,
};
use crate::core::orchestrator::create_orchestrator;

/// Ensure the daemon is running for lifecycle management
fn ensure_daemon_running() {
    // Attempt to start daemon in background - non-blocking
    // The daemon handles eval triggering and time limits
    if let Err(e) = crate::daemon::DaemonClient::connect_or_start() {
        tracing::debug!("Could not start daemon: {}", e);
    }
}

/// Get list of all runs
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn get_runs() -> Result<Vec<RunSummary>, String> {
    // Ensure daemon is running for lifecycle management
    ensure_daemon_running();

    let orch = create_orchestrator().str_err()?;
    orch.list_runs().await.str_err()
}

/// Get detailed information about a specific run
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn get_run_detail(run_name: String) -> Result<RunDetail, String> {
    let orch = create_orchestrator().str_err()?;
    let detail = orch.get_run(&run_name).await.str_err()?;

    // Note: Lifecycle management (eval triggering) is handled by the daemon

    Ok(detail)
}

/// Pause a running run
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn pause_run(run_name: String) -> Result<(), String> {
    let orch = create_orchestrator().str_err()?;
    orch.pause_run(&run_name).await.str_err()?;
    update_project_run_status_by_name(&run_name, ProjectRunStatus::Paused)
        .await
        .ok();
    bump_generation("runs_gen").await.ok();
    Ok(())
}

/// Resume a paused run
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn resume_run(run_name: String) -> Result<(), String> {
    let orch = create_orchestrator().str_err()?;
    orch.resume_run(&run_name, None).await.str_err()?;
    update_project_run_status_by_name(&run_name, ProjectRunStatus::Working)
        .await
        .ok();
    bump_generation("runs_gen").await.ok();
    Ok(())
}

/// Delete a run
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn delete_run(run_name: String) -> Result<(), String> {
    let orch = create_orchestrator().str_err()?;
    orch.delete_run(&run_name).await.str_err()?;
    bump_generation("runs_gen").await.ok();
    Ok(())
}

/// Delete all runs
/// Iterates through each run and deletes it properly (killing workers, closing connections)
#[tracing::instrument]
#[tauri::command]
pub async fn delete_all_runs() -> Result<(), String> {
    use crate::core::ops::run::delete_run;
    use crate::core::ops::types::DeleteRunConfig;

    let runs_dir = crate::core::config::runs_dir();
    if !runs_dir.exists() {
        return Ok(());
    }

    // Get list of run names
    let entries = std::fs::read_dir(&runs_dir).context("Failed to read runs directory")?;

    let run_names: Vec<String> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.path().is_dir() {
                entry.file_name().to_str().map(String::from)
            } else {
                None
            }
        })
        .collect();

    // Delete each run properly
    let mut errors = Vec::new();
    for run_name in run_names {
        let config = DeleteRunConfig::new(&run_name);
        if let Err(e) = delete_run(config).await {
            errors.push(format!("{}: {}", run_name, e));
        }
    }

    if !errors.is_empty() {
        return Err(format!("Failed to delete some runs: {}", errors.join(", ")));
    }

    Ok(())
}

/// Deliver a run's changes to a branch
///
/// Creates a branch in the target repository with the run's changes.
/// For remote repos, pushes to the remote. For local repos, creates a local branch.
/// Returns the branch name on success.
/// Uses the orchestrator to support both local and remote modes
#[tracing::instrument]
#[tauri::command]
pub async fn deliver_run(run_name: String, branch_name: Option<String>) -> Result<String, String> {
    let orch = create_orchestrator().str_err()?;
    let result = orch.deliver_run(&run_name, branch_name).await.str_err()?;
    bump_generation("runs_gen").await.ok();
    Ok(result)
}

/// Get runs only if the generation has changed since last check.
///
/// Returns None if generation matches (no changes), or Some((runs, new_generation))
/// if there are updates. This avoids the full 1+4N query cycle on ~80% of polls.
#[tracing::instrument]
#[tauri::command]
pub async fn get_runs_if_changed(
    last_generation: i64,
) -> Result<Option<(Vec<RunSummary>, i64)>, String> {
    ensure_daemon_running();
    let current = get_generation("runs_gen").await.str_err()?;
    if current == last_generation {
        return Ok(None);
    }
    let orch = create_orchestrator().str_err()?;
    let runs = orch.list_runs().await.str_err()?;
    Ok(Some((runs, current)))
}
