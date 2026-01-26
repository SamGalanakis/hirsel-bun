//! Delivery commands
//!
//! Commands for delivering run changes to git branches and PRs.

use crate::core::delivery::{DeliveryService, DeliveryState, PushResult};
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
