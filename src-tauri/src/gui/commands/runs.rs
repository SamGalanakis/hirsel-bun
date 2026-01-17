//! Run-related commands
//!
//! Commands for managing runs: listing, viewing details, pausing, resuming, deleting, and delivering.

use super::helpers::trigger_compaction_if_needed;
use super::types::{RunDetail, RunStatus, RunSummary};
use crate::core::orchestrator::create_orchestrator;

/// Get list of all runs
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_runs() -> Result<Vec<RunSummary>, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    orch.list_runs().await.map_err(|e| e.to_string())
}

/// Get detailed information about a specific run
/// Uses the orchestrator to support both local and remote modes
#[tauri::command]
pub async fn get_run_detail(run_name: String) -> Result<RunDetail, String> {
    let orch = create_orchestrator(None).map_err(|e| e.to_string())?;
    let detail = orch.get_run(&run_name).await.map_err(|e| e.to_string())?;

    // Trigger automatic compaction check for local runs (spawns subprocess if needed)
    // Only when run is in an active state
    if matches!(
        detail.status,
        RunStatus::Working | RunStatus::Eval | RunStatus::Done
    ) {
        if let Err(e) = trigger_compaction_if_needed(&run_name) {
            tracing::debug!("Compaction check failed: {}", e);
        }
    }

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
