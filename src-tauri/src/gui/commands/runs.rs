//! Run-related commands
//!
//! Commands for managing runs: listing, viewing details, pausing, resuming, deleting, and delivering.

use crate::core::api_types::{RunDetail, RunSummary};
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
#[tauri::command]
pub async fn get_runs() -> Result<Vec<RunSummary>, String> {
    // Ensure daemon is running for lifecycle management
    ensure_daemon_running();

    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.list_runs().await.map_err(|e| e.to_string())
}

/// Get detailed information about a specific run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_run_detail(run_name: String) -> Result<RunDetail, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    let detail = orch.get_run(&run_name).await.map_err(|e| e.to_string())?;

    // Note: Lifecycle management (eval triggering) is handled by the daemon

    Ok(detail)
}

/// Pause a running run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn pause_run(run_name: String) -> Result<(), String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.pause_run(&run_name).await.map_err(|e| e.to_string())
}

/// Resume a paused run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn resume_run(run_name: String) -> Result<(), String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.resume_run(&run_name, None)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn delete_run(run_name: String) -> Result<(), String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.delete_run(&run_name).await.map_err(|e| e.to_string())
}

/// Delete all runs
/// Iterates through each run and deletes it properly (killing workers, closing connections)
#[tauri::command]
pub async fn delete_all_runs() -> Result<(), String> {
    use crate::core::ops::run::delete_run;
    use crate::core::ops::types::DeleteRunConfig;

    let runs_dir = crate::core::config::runs_dir();
    if !runs_dir.exists() {
        return Ok(());
    }

    // Get list of run names
    let entries = std::fs::read_dir(&runs_dir)
        .map_err(|e| format!("Failed to read runs directory: {}", e))?;

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
#[tauri::command]
pub async fn deliver_run(run_name: String, branch_name: Option<String>) -> Result<String, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.deliver_run(&run_name, branch_name)
        .await
        .map_err(|e| e.to_string())
}
