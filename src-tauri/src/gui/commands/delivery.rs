//! Delivery commands
//!
//! Commands for delivering run changes to git branches and PRs.
//! Also includes board delivery commands for the delta dispatch system.

use super::get_run_work_dir;
use crate::core::delivery::{
    delivery_branch_name, pr_body, pr_title, DeliveryOrchestrator, DeliveryState, PushResult,
};
use crate::core::delta::{
    BoardDeliveryStatus, BoardVersion, Delivery, DeliveryAttempt, DeltaState,
};
use crate::core::forge::{MergeResult, PrInfo};
use crate::core::{hirsel_dir, SQLiteState};

/// Get the delivery state for a run
#[tauri::command]
pub async fn get_delivery_state(
    run_name: String,
    target_branch: String,
) -> Result<DeliveryState, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    // Get branch_off_commit from run state
    let branch_off_commit = match SQLiteState::new(&run_name).await {
        Ok(state) => state.get_branch_off_commit().await.ok().flatten(),
        Err(_) => None,
    };

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    orchestrator
        .get_delivery_state(&target_branch, branch_off_commit.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Check merge state for a run
#[tauri::command]
pub async fn check_merge_state(run_name: String, target_branch: String) -> Result<String, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    let state = orchestrator
        .check_merge_state(&target_branch)
        .map_err(|e| e.to_string())?;

    Ok(state.as_str().to_string())
}

/// Get conflicting files for a merge
#[tauri::command]
pub async fn get_conflicting_files(
    run_name: String,
    target_branch: String,
) -> Result<Vec<String>, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    orchestrator
        .get_conflicting_files(&target_branch)
        .map_err(|e| e.to_string())
}

/// Check staleness (commits on target since branch-off)
#[tauri::command]
pub async fn check_staleness(
    run_name: String,
    target_branch: String,
    branch_off_commit: String,
) -> Result<u32, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    orchestrator
        .check_staleness(&target_branch, &branch_off_commit)
        .map_err(|e| e.to_string())
}

/// Tier 1: Push branch to remote
#[tauri::command]
pub async fn push_run_branch(run_name: String) -> Result<PushResult, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    orchestrator.push_branch(None).map_err(|e| e.to_string())
}

/// Tier 2: Create a pull request
#[tauri::command]
pub async fn create_run_pr(
    run_name: String,
    target_branch: String,
    title: String,
    body: String,
) -> Result<PrInfo, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    orchestrator
        .create_pr(&target_branch, &title, &body)
        .await
        .map_err(|e| e.to_string())
}

/// Tier 3: Auto-merge (push, create PR, merge)
#[tauri::command]
pub async fn auto_merge_run(
    run_name: String,
    target_branch: String,
    title: String,
    body: String,
) -> Result<MergeResult, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    orchestrator
        .auto_merge(&target_branch, &title, &body)
        .await
        .map_err(|e| e.to_string())
}

/// Generate PR title from run
#[tauri::command]
pub async fn generate_pr_title(
    run_name: String,
    summary: Option<String>,
) -> Result<String, String> {
    Ok(pr_title(&run_name, summary.as_deref()))
}

/// Generate PR body from run
#[tauri::command]
pub async fn generate_pr_body(
    run_name: String,
    task_ids: Vec<String>,
    eval_ids: Vec<String>,
) -> Result<String, String> {
    Ok(pr_body(&run_name, &task_ids, &eval_ids))
}

/// Generate delivery branch name
#[tauri::command]
pub async fn get_delivery_branch_name(run_name: String) -> Result<String, String> {
    Ok(delivery_branch_name(&run_name))
}

// =============================================================================
// Board Delivery Commands (Delta Dispatch System)
// =============================================================================

/// Get all board versions for a project
#[tauri::command]
pub async fn get_board_versions(
    project_id: i64,
    route_id: i64,
) -> Result<Vec<BoardVersion>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_board_versions().await.map_err(|e| e.to_string())
}

/// Get the latest board version for a project
#[tauri::command]
pub async fn get_latest_board_version(
    project_id: i64,
    route_id: i64,
) -> Result<Option<BoardVersion>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_latest_version().await.map_err(|e| e.to_string())
}

/// Get the current (non-terminal) delivery for a project
#[tauri::command]
pub async fn get_current_board_delivery(
    project_id: i64,
    route_id: i64,
) -> Result<Option<Delivery>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state
        .get_current_delivery()
        .await
        .map_err(|e| e.to_string())
}

/// Start a delivery for a board version
///
/// Creates the delivery record, marks it in progress, and pushes the branch.
#[tauri::command]
pub async fn start_board_delivery(
    project_id: i64,
    route_id: i64,
    version_id: i64,
    target_branch: String,
    _resolve_conflicts: bool, // Reserved for future use
) -> Result<Delivery, String> {
    let state = DeltaState::with_route(project_id, route_id);

    // Create the delivery record
    let delivery = state
        .create_delivery(version_id, &target_branch)
        .await
        .map_err(|e| e.to_string())?;

    // Mark as in progress
    state
        .update_delivery_status(delivery.id, BoardDeliveryStatus::InProgress)
        .await
        .map_err(|e| e.to_string())?;

    // Create an attempt record
    state
        .add_delivery_attempt(delivery.id)
        .await
        .map_err(|e| e.to_string())?;

    // Get the project run to find the work directory
    let project_run = state
        .get_project_run()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No project run found")?;

    let run_path = hirsel_dir().join("runs").join(&project_run.run_name);
    let work_dir = run_path.join("work").join("staging");

    if !work_dir.exists() {
        return Err(format!("Work directory not found: {}", work_dir.display()));
    }

    // Create orchestrator and push
    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    let push_result = orchestrator.push_branch(None).map_err(|e| e.to_string())?;

    // Update delivery with branch info
    state
        .update_delivery_info(
            delivery.id,
            Some(&push_result.branch),
            push_result.url.as_deref(),
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    state
        .update_delivery_status(delivery.id, BoardDeliveryStatus::Pushed)
        .await
        .map_err(|e| e.to_string())?;

    // Return the updated delivery
    state
        .get_delivery(delivery.id)
        .await
        .map_err(|e| e.to_string())
}

/// Retry a failed delivery
#[tauri::command]
pub async fn retry_board_delivery(
    project_id: i64,
    route_id: i64,
    delivery_id: i64,
) -> Result<DeliveryAttempt, String> {
    let state = DeltaState::with_route(project_id, route_id);

    // Reset status to in_progress
    state
        .update_delivery_status(delivery_id, BoardDeliveryStatus::InProgress)
        .await
        .map_err(|e| e.to_string())?;

    // Add new attempt
    state
        .add_delivery_attempt(delivery_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get delivery attempts for a delivery
#[tauri::command]
pub async fn get_delivery_attempts(
    project_id: i64,
    route_id: i64,
    delivery_id: i64,
) -> Result<Vec<DeliveryAttempt>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state
        .get_delivery_attempts(delivery_id)
        .await
        .map_err(|e| e.to_string())
}

/// Complete a board delivery (push, PR, merge)
#[tauri::command]
pub async fn complete_board_delivery(
    project_id: i64,
    route_id: i64,
    delivery_id: i64,
    action: String,          // "push", "pr", "merge"
    summary: Option<String>, // Optional summary for PR body
) -> Result<Delivery, String> {
    let state = DeltaState::with_route(project_id, route_id);

    // Get the project run to find the work directory
    let project_run = state
        .get_project_run()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No project run found")?;

    let run_path = hirsel_dir().join("runs").join(&project_run.run_name);
    let work_dir = run_path.join("work").join("staging");

    if !work_dir.exists() {
        return Err(format!("Work directory not found: {}", work_dir.display()));
    }

    let delivery = state
        .get_delivery(delivery_id)
        .await
        .map_err(|e| e.to_string())?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).map_err(|e| e.to_string())?;

    match action.as_str() {
        "push" => {
            let push_result = orchestrator.push_branch(None).map_err(|e| e.to_string())?;
            state
                .update_delivery_info(
                    delivery_id,
                    Some(&push_result.branch),
                    push_result.url.as_deref(),
                    None,
                )
                .await
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::Pushed)
                .await
                .map_err(|e| e.to_string())?;
        }
        "pr" => {
            let title = format!("Board v{} delivery", delivery.version_id);
            let body = summary.unwrap_or_else(|| {
                format!(
                    "Automated delivery from Hirsel board version {}",
                    delivery.version_id
                )
            });
            let pr = orchestrator
                .create_pr(&delivery.target_branch, &title, &body)
                .await
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_info(
                    delivery_id,
                    state
                        .get_delivery(delivery_id)
                        .await
                        .ok()
                        .and_then(|d| d.delivery_branch)
                        .as_deref(),
                    Some(&pr.url),
                    Some(pr.number as i64),
                )
                .await
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::PrOpen)
                .await
                .map_err(|e| e.to_string())?;
        }
        "merge" => {
            let title = format!("Board v{} delivery", delivery.version_id);
            let body = format!(
                "Automated delivery from Hirsel board version {}",
                delivery.version_id
            );
            let _merge = orchestrator
                .auto_merge(&delivery.target_branch, &title, &body)
                .await
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::Merged)
                .await
                .map_err(|e| e.to_string())?;
        }
        _ => return Err(format!("Unknown action: {}", action)),
    }

    state
        .get_delivery(delivery_id)
        .await
        .map_err(|e| e.to_string())
}

/// Abandon a delivery
#[tauri::command]
pub async fn abandon_board_delivery(
    project_id: i64,
    route_id: i64,
    delivery_id: i64,
) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state
        .update_delivery_status(delivery_id, BoardDeliveryStatus::Abandoned)
        .await
        .map_err(|e| e.to_string())
}
