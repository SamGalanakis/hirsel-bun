//! Delivery commands
//!
//! Commands for delivering run changes to git branches and PRs.
//! Also includes board delivery commands for the delta dispatch system.

use crate::core::delivery::{DeliveryService, DeliveryState, PushResult};
use crate::core::delta::{
    BoardDeliveryStatus, BoardVersion, Delivery, DeliveryAttempt, DeltaState,
};
use crate::core::github::{MergeInfo, PrInfo};
use crate::core::{hirsel_dir, SQLiteState};

/// Get the delivery state for a run
#[tauri::command]
pub async fn get_delivery_state(
    run_name: String,
    target_branch: String,
) -> Result<DeliveryState, String> {
    let run_path = hirsel_dir().join("runs").join(&run_name);
    if !run_path.exists() {
        return Err(format!("Run not found: {}", run_name));
    }

    let work_dir = run_path.join("work");
    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }

    // Get branch_off_commit from run state
    let state_path = run_path.join("state.db");
    let branch_off_commit = if state_path.exists() {
        let state = SQLiteState::new(state_path).map_err(|e| e.to_string())?;
        state.get_branch_off_commit().ok().flatten()
    } else {
        None
    };

    let service = DeliveryService::new(&work_dir);
    service
        .get_delivery_state_async(&target_branch, branch_off_commit.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Check merge state for a run
#[tauri::command]
pub async fn check_merge_state(run_name: String, target_branch: String) -> Result<String, String> {
    let run_path = hirsel_dir().join("runs").join(&run_name);
    let work_dir = run_path.join("work");

    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }

    let service = DeliveryService::new(&work_dir);
    let state = service
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
    let run_path = hirsel_dir().join("runs").join(&run_name);
    let work_dir = run_path.join("work");

    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }

    let service = DeliveryService::new(&work_dir);
    service
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
    let run_path = hirsel_dir().join("runs").join(&run_name);
    let work_dir = run_path.join("work");

    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }

    let service = DeliveryService::new(&work_dir);
    service
        .check_staleness(&target_branch, &branch_off_commit)
        .map_err(|e| e.to_string())
}

/// Tier 1: Push branch to remote
#[tauri::command]
pub async fn push_run_branch(run_name: String) -> Result<PushResult, String> {
    let run_path = hirsel_dir().join("runs").join(&run_name);
    let work_dir = run_path.join("work");

    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }

    let service = DeliveryService::new(&work_dir);
    service.push_branch(None).map_err(|e| e.to_string())
}

/// Tier 2: Create a pull request
#[tauri::command]
pub async fn create_run_pr(
    run_name: String,
    target_branch: String,
    title: String,
    body: String,
) -> Result<PrInfo, String> {
    let run_path = hirsel_dir().join("runs").join(&run_name);
    let work_dir = run_path.join("work");

    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }

    let service = DeliveryService::new(&work_dir);
    service
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
) -> Result<MergeInfo, String> {
    let run_path = hirsel_dir().join("runs").join(&run_name);
    let work_dir = run_path.join("work");

    if !work_dir.exists() {
        return Err(format!("Run work directory not found: {}", run_name));
    }

    let service = DeliveryService::new(&work_dir);
    service
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
    Ok(DeliveryService::pr_title(&run_name, summary.as_deref()))
}

/// Generate PR body from run
#[tauri::command]
pub async fn generate_pr_body(
    run_name: String,
    task_ids: Vec<String>,
    eval_ids: Vec<String>,
) -> Result<String, String> {
    Ok(DeliveryService::pr_body(&run_name, &task_ids, &eval_ids))
}

/// Generate delivery branch name
#[tauri::command]
pub async fn delivery_branch_name(run_name: String) -> Result<String, String> {
    Ok(DeliveryService::delivery_branch_name(&run_name))
}

// =============================================================================
// Board Delivery Commands (Delta Dispatch System)
// =============================================================================

/// Get all board versions for a project
#[tauri::command]
pub async fn get_board_versions(project_id: i64) -> Result<Vec<BoardVersion>, String> {
    let state = DeltaState::new(project_id);
    state.get_board_versions().map_err(|e| e.to_string())
}

/// Get the latest board version for a project
#[tauri::command]
pub async fn get_latest_board_version(project_id: i64) -> Result<Option<BoardVersion>, String> {
    let state = DeltaState::new(project_id);
    state.get_latest_version().map_err(|e| e.to_string())
}

/// Get the current (non-terminal) delivery for a project
#[tauri::command]
pub async fn get_current_board_delivery(project_id: i64) -> Result<Option<Delivery>, String> {
    let state = DeltaState::new(project_id);
    state.get_current_delivery().map_err(|e| e.to_string())
}

/// Start a delivery for a board version
#[tauri::command]
pub async fn start_board_delivery(
    project_id: i64,
    version_id: i64,
    target_branch: String,
    _resolve_conflicts: bool, // Reserved for future use
) -> Result<Delivery, String> {
    let state = DeltaState::new(project_id);

    // Create the delivery record
    let delivery = state
        .create_delivery(version_id, &target_branch)
        .map_err(|e| e.to_string())?;

    // Mark as in progress
    state
        .update_delivery_status(delivery.id, BoardDeliveryStatus::InProgress)
        .map_err(|e| e.to_string())?;

    // Create an attempt record
    state
        .add_delivery_attempt(delivery.id)
        .map_err(|e| e.to_string())?;

    // Return the updated delivery
    state.get_delivery(delivery.id).map_err(|e| e.to_string())
}

/// Get delivery status
#[tauri::command]
pub async fn get_board_delivery_status(delivery_id: i64) -> Result<Delivery, String> {
    // We need to query without knowing the project_id
    // Use a helper that queries directly
    let db = rusqlite::Connection::open(crate::core::config::global_db_path())
        .map_err(|e| e.to_string())?;

    let mut stmt = db
        .prepare(
            "SELECT id, project_id, version_id, status, target_branch, delivery_branch,
                    pr_url, pr_number, started_at, completed_at, failure_reason
             FROM deliveries
             WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;

    stmt.query_row([delivery_id], |row| {
        Ok(Delivery {
            id: row.get("id")?,
            project_id: row.get("project_id")?,
            version_id: row.get("version_id")?,
            status: BoardDeliveryStatus::from_str(
                &row.get::<_, String>("status").unwrap_or_default(),
            ),
            target_branch: row.get("target_branch")?,
            delivery_branch: row.get("delivery_branch")?,
            pr_url: row.get("pr_url")?,
            pr_number: row.get("pr_number")?,
            started_at: row.get("started_at")?,
            completed_at: row.get("completed_at")?,
            failure_reason: row.get("failure_reason")?,
        })
    })
    .map_err(|e| e.to_string())
}

/// Retry a failed delivery
#[tauri::command]
pub async fn retry_board_delivery(delivery_id: i64) -> Result<DeliveryAttempt, String> {
    // Get the delivery to find project_id
    let db = rusqlite::Connection::open(crate::core::config::global_db_path())
        .map_err(|e| e.to_string())?;

    let project_id: i64 = db
        .query_row(
            "SELECT project_id FROM deliveries WHERE id = ?1",
            [delivery_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let state = DeltaState::new(project_id);

    // Reset status to in_progress
    state
        .update_delivery_status(delivery_id, BoardDeliveryStatus::InProgress)
        .map_err(|e| e.to_string())?;

    // Add new attempt
    state
        .add_delivery_attempt(delivery_id)
        .map_err(|e| e.to_string())
}

/// Get delivery attempts for a delivery
#[tauri::command]
pub async fn get_delivery_attempts(delivery_id: i64) -> Result<Vec<DeliveryAttempt>, String> {
    // Get the delivery to find project_id
    let db = rusqlite::Connection::open(crate::core::config::global_db_path())
        .map_err(|e| e.to_string())?;

    let project_id: i64 = db
        .query_row(
            "SELECT project_id FROM deliveries WHERE id = ?1",
            [delivery_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let state = DeltaState::new(project_id);
    state
        .get_delivery_attempts(delivery_id)
        .map_err(|e| e.to_string())
}

/// Complete a board delivery (push, PR, merge)
#[tauri::command]
pub async fn complete_board_delivery(
    project_id: i64,
    delivery_id: i64,
    action: String, // "push", "pr", "merge"
) -> Result<Delivery, String> {
    let state = DeltaState::new(project_id);

    // Get the project run to find the work directory
    let project_run = state
        .get_project_run()
        .map_err(|e| e.to_string())?
        .ok_or("No project run found")?;

    let run_path = hirsel_dir().join("runs").join(&project_run.run_name);
    let work_dir = run_path.join("work").join("staging");

    if !work_dir.exists() {
        return Err(format!("Work directory not found: {}", work_dir.display()));
    }

    let delivery = state.get_delivery(delivery_id).map_err(|e| e.to_string())?;
    let delivery_service = DeliveryService::new(&work_dir);

    match action.as_str() {
        "push" => {
            let push_result = delivery_service
                .push_branch(None)
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_info(
                    delivery_id,
                    Some(&push_result.branch),
                    push_result.url.as_deref(),
                    None,
                )
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::Pushed)
                .map_err(|e| e.to_string())?;
        }
        "pr" => {
            let title = format!("Board v{} delivery", delivery.version_id);
            let body = format!(
                "Automated delivery from Hirsel board version {}",
                delivery.version_id
            );
            let pr = delivery_service
                .create_pr(&delivery.target_branch, &title, &body)
                .await
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_info(
                    delivery_id,
                    state
                        .get_delivery(delivery_id)
                        .ok()
                        .and_then(|d| d.delivery_branch)
                        .as_deref(),
                    Some(&pr.url),
                    Some(pr.number as i64),
                )
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::PrOpen)
                .map_err(|e| e.to_string())?;
        }
        "merge" => {
            let title = format!("Board v{} delivery", delivery.version_id);
            let body = format!(
                "Automated delivery from Hirsel board version {}",
                delivery.version_id
            );
            let _merge = delivery_service
                .auto_merge(&delivery.target_branch, &title, &body)
                .await
                .map_err(|e| e.to_string())?;
            state
                .update_delivery_status(delivery_id, BoardDeliveryStatus::Merged)
                .map_err(|e| e.to_string())?;
        }
        _ => return Err(format!("Unknown action: {}", action)),
    }

    state.get_delivery(delivery_id).map_err(|e| e.to_string())
}

/// Abandon a delivery
#[tauri::command]
pub async fn abandon_board_delivery(project_id: i64, delivery_id: i64) -> Result<(), String> {
    let state = DeltaState::new(project_id);
    state
        .update_delivery_status(delivery_id, BoardDeliveryStatus::Abandoned)
        .map_err(|e| e.to_string())
}
