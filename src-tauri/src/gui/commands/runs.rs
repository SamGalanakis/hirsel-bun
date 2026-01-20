//! Run-related commands
//!
//! Commands for managing runs: listing, viewing details, pausing, resuming, deleting, and delivering.

use super::types::{RunDetail, RunSummary};
use crate::core::orchestrator::create_orchestrator;

/// Ensure the daemon is running for lifecycle management
fn ensure_daemon_running() {
    // Attempt to start daemon in background - non-blocking
    // The daemon handles eval triggering, time limits, and compaction
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

    // Note: Lifecycle management (eval triggering, compaction) is now handled
    // by the daemon's polling loop, not by the GUI polling

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
/// Removes the entire runs directory
#[tauri::command]
pub async fn delete_all_runs() -> Result<(), String> {
    let runs_dir = crate::core::config::runs_dir();
    if runs_dir.exists() {
        std::fs::remove_dir_all(&runs_dir).map_err(|e| format!("Failed to delete runs: {}", e))?;
        // Recreate empty runs directory
        std::fs::create_dir_all(&runs_dir)
            .map_err(|e| format!("Failed to recreate runs dir: {}", e))?;
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
