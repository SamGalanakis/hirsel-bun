//! Delivery commands
//!
//! Commands for delivering run changes to git branches and PRs.
//! Also includes board delivery commands for the delta dispatch system.

use super::get_run_work_dir;
use super::ResultExt;
use crate::core::config::Config;
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

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    orchestrator
        .get_delivery_state(&target_branch, branch_off_commit.as_deref())
        .await
        .str_err()
}

/// Check merge state for a run
#[tauri::command]
pub async fn check_merge_state(run_name: String, target_branch: String) -> Result<String, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    let state = orchestrator.check_merge_state(&target_branch).str_err()?;

    Ok(state.as_str().to_string())
}

/// Get conflicting files for a merge
#[tauri::command]
pub async fn get_conflicting_files(
    run_name: String,
    target_branch: String,
) -> Result<Vec<String>, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    orchestrator.get_conflicting_files(&target_branch).str_err()
}

/// Check staleness (commits on target since branch-off)
#[tauri::command]
pub async fn check_staleness(
    run_name: String,
    target_branch: String,
    branch_off_commit: String,
) -> Result<u32, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    orchestrator
        .check_staleness(&target_branch, &branch_off_commit)
        .str_err()
}

/// Tier 1: Push branch to remote
#[tauri::command]
pub async fn push_run_branch(run_name: String) -> Result<PushResult, String> {
    let work_dir = get_run_work_dir(&run_name)?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    orchestrator.push_branch(None).str_err()
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

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    orchestrator
        .create_pr(&target_branch, &title, &body)
        .await
        .str_err()
}

/// Tier 3: Auto-merge (push, create PR, merge) with AI-assisted conflict resolution
#[tauri::command]
pub async fn auto_merge_run(
    run_name: String,
    target_branch: String,
    title: String,
    body: String,
) -> Result<MergeResult, String> {
    let work_dir = get_run_work_dir(&run_name)?;
    let config = Config::load().map(|(c, _)| c).unwrap_or_default();

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    orchestrator
        .auto_merge_with_resolution(&target_branch, &title, &body, config, None)
        .await
        .str_err()
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
    state.get_board_versions().await.str_err()
}

/// Get the latest board version for a project
#[tauri::command]
pub async fn get_latest_board_version(
    project_id: i64,
    route_id: i64,
) -> Result<Option<BoardVersion>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_latest_version().await.str_err()
}

/// Get the current (non-terminal) delivery for a project
#[tauri::command]
pub async fn get_current_board_delivery(
    project_id: i64,
    route_id: i64,
) -> Result<Option<Delivery>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_current_delivery().await.str_err()
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
        .str_err()?;

    // Mark as in progress
    state
        .update_delivery_status(delivery.id, BoardDeliveryStatus::InProgress)
        .await
        .str_err()?;

    // Create an attempt record
    state.add_delivery_attempt(delivery.id).await.str_err()?;

    // Get the project run to find the work directory
    let project_run = state
        .get_project_run()
        .await
        .str_err()?
        .ok_or("No project run found")?;

    let run_path = hirsel_dir().join("runs").join(&project_run.run_name);
    let work_dir = run_path.join("work").join("staging");

    if !work_dir.exists() {
        return Err(format!("Work directory not found: {}", work_dir.display()));
    }

    // Create orchestrator and push
    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    let push_result = orchestrator.push_branch(None).str_err()?;

    // Update delivery with branch info
    state
        .update_delivery_info(
            delivery.id,
            Some(&push_result.branch),
            push_result.url.as_deref(),
            None,
        )
        .await
        .str_err()?;

    state
        .update_delivery_status(delivery.id, BoardDeliveryStatus::Pushed)
        .await
        .str_err()?;

    // Return the updated delivery
    state.get_delivery(delivery.id).await.str_err()
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
        .str_err()?;

    // Add new attempt
    state.add_delivery_attempt(delivery_id).await.str_err()
}

/// Get delivery attempts for a delivery
#[tauri::command]
pub async fn get_delivery_attempts(
    project_id: i64,
    route_id: i64,
    delivery_id: i64,
) -> Result<Vec<DeliveryAttempt>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_delivery_attempts(delivery_id).await.str_err()
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
        .str_err()?
        .ok_or("No project run found")?;

    let run_path = hirsel_dir().join("runs").join(&project_run.run_name);
    let work_dir = run_path.join("work").join("staging");

    if !work_dir.exists() {
        return Err(format!("Work directory not found: {}", work_dir.display()));
    }

    let delivery = state.get_delivery(delivery_id).await.str_err()?;

    let orchestrator = DeliveryOrchestrator::from_work_dir(&work_dir).str_err()?;

    match action.as_str() {
        "push" => {
            let push_result = orchestrator.push_branch(None).str_err()?;
            state
                .update_delivery_info(
                    delivery_id,
                    Some(&push_result.branch),
                    push_result.url.as_deref(),
                    None,
                )
                .await
                .str_err()?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::Pushed)
                .await
                .str_err()?;
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
                .str_err()?;
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
                .str_err()?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::PrOpen)
                .await
                .str_err()?;
        }
        "merge" => {
            let config = Config::load().map(|(c, _)| c).unwrap_or_default();
            let title = format!("Board v{} delivery", delivery.version_id);
            let body = format!(
                "Automated delivery from Hirsel board version {}",
                delivery.version_id
            );
            let _merge = orchestrator
                .auto_merge_with_resolution(&delivery.target_branch, &title, &body, config, None)
                .await
                .str_err()?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::Merged)
                .await
                .str_err()?;
        }
        _ => return Err(format!("Unknown action: {}", action)),
    }

    state.get_delivery(delivery_id).await.str_err()
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
        .str_err()
}
