//! Delivery commands
//!
//! Commands for delivering run changes to git branches and PRs.
//! Also includes board delivery commands for the board dispatch system.

use super::ResultExt;
use crate::core::config::Config;
use crate::core::delivery::{
    delivery_branch_name, pr_body, pr_title, DeliveryOrchestrator, DeliveryState, PushResult,
};
use crate::core::delta::{
    BoardDeliveryStatus, BoardVersion, Delivery, DeliveryAttempt, DeltaState,
};
use crate::core::forge::{MergeResult, PrInfo};
use crate::core::route::RouteStore;
use crate::core::{hirsel_dir, SQLiteState};
use serde::Serialize;

/// Look up the route's git remote URL from its default starting point.
async fn route_remote_url(project_id: i64, route_id: i64) -> Option<String> {
    let store = RouteStore::new(project_id).await.ok()?;
    let route = store.get_route(route_id).await.ok()?;
    let default_repo = if let Some(default_id) = route.default_repo_id {
        route.repos.iter().find(|r| r.id == default_id)
    } else {
        route.repos.first()
    }?;

    default_repo.starting_point.git_url().map(|s| s.to_string())
}

/// Get the delivery state for a run
#[tracing::instrument]
#[tauri::command]
pub async fn get_delivery_state(
    run_name: String,
    target_branch: String,
) -> Result<DeliveryState, String> {
    // Get branch_off_commit from run state
    let branch_off_commit = match SQLiteState::new(&run_name).await {
        Ok(state) => state.get_branch_off_commit().await.ok().flatten(),
        Err(_) => None,
    };

    let orchestrator = DeliveryOrchestrator::from_run_name(&run_name, None).str_err()?;

    orchestrator
        .get_delivery_state(&target_branch, branch_off_commit.as_deref())
        .await
        .str_err()
}

/// Check merge state for a run
#[tracing::instrument]
#[tauri::command]
pub async fn check_merge_state(run_name: String, target_branch: String) -> Result<String, String> {
    let orchestrator = DeliveryOrchestrator::from_run_name(&run_name, None).str_err()?;

    let state = orchestrator.check_merge_state(&target_branch).str_err()?;

    Ok(state.as_str().to_string())
}

/// Get conflicting files for a merge
#[tracing::instrument]
#[tauri::command]
pub async fn get_conflicting_files(
    run_name: String,
    target_branch: String,
) -> Result<Vec<String>, String> {
    let orchestrator = DeliveryOrchestrator::from_run_name(&run_name, None).str_err()?;

    orchestrator.get_conflicting_files(&target_branch).str_err()
}

/// Check staleness (commits on target since branch-off)
#[tracing::instrument]
#[tauri::command]
pub async fn check_staleness(
    run_name: String,
    target_branch: String,
    branch_off_commit: String,
) -> Result<u32, String> {
    let orchestrator = DeliveryOrchestrator::from_run_name(&run_name, None).str_err()?;

    orchestrator
        .check_staleness(&target_branch, &branch_off_commit)
        .str_err()
}

/// Tier 1: Push branch to remote
#[tracing::instrument]
#[tauri::command]
pub async fn push_run_branch(run_name: String) -> Result<PushResult, String> {
    let orchestrator = DeliveryOrchestrator::from_run_name(&run_name, None).str_err()?;

    orchestrator.push_branch(None, None).str_err()
}

/// Tier 2: Create a pull request
#[tracing::instrument]
#[tauri::command]
pub async fn create_run_pr(
    run_name: String,
    target_branch: String,
    title: String,
    body: String,
) -> Result<PrInfo, String> {
    let orchestrator = DeliveryOrchestrator::from_run_name(&run_name, None).str_err()?;

    orchestrator
        .create_pr(&target_branch, &title, &body)
        .await
        .str_err()
}

/// Tier 3: Auto-merge (push, create PR, merge) with AI-assisted conflict resolution
#[tracing::instrument]
#[tauri::command]
pub async fn auto_merge_run(
    run_name: String,
    target_branch: String,
    title: String,
    body: String,
) -> Result<MergeResult, String> {
    let config = Config::load().map(|(c, _)| c).unwrap_or_default();

    let orchestrator = DeliveryOrchestrator::from_run_name(&run_name, None).str_err()?;

    orchestrator
        .auto_merge_with_resolution(&target_branch, &title, &body, config, None)
        .await
        .str_err()
}

/// Generate PR title from run
#[tracing::instrument]
#[tauri::command]
pub async fn generate_pr_title(
    run_name: String,
    summary: Option<String>,
) -> Result<String, String> {
    Ok(pr_title(&run_name, summary.as_deref()))
}

/// Generate PR body from run
#[tracing::instrument]
#[tauri::command]
pub async fn generate_pr_body(
    run_name: String,
    task_ids: Vec<String>,
    eval_ids: Vec<String>,
) -> Result<String, String> {
    Ok(pr_body(&run_name, &task_ids, &eval_ids))
}

/// Generate delivery branch name
#[tracing::instrument]
#[tauri::command]
pub async fn get_delivery_branch_name(run_name: String) -> Result<String, String> {
    Ok(delivery_branch_name(&run_name))
}

// =============================================================================
// Delivery Validation (Board Pre-flight Check)
// =============================================================================

/// Result of validating a delivery target branch
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryValidation {
    pub has_remote: bool,
    pub has_forge: bool,
    pub target_exists_on_remote: bool,
    pub merge_state: String,
    pub conflicting_files: Vec<String>,
    pub available_actions: Vec<String>,
    pub remote_branches: Vec<String>,
    pub remote_url: Option<String>,
    pub is_local: bool,
    pub needs_init: bool,
    pub error: Option<String>,
}

/// Resolve the effective remote URL: prefer explicit `remote_url`, then
/// caller should fall back to project_remote_url.
fn resolve_remote(remote_url: Option<&str>) -> Option<String> {
    if let Some(url) = remote_url {
        if !url.is_empty() {
            return Some(crate::core::system::normalise_local_remote(url));
        }
    }
    None
}

use crate::core::system::is_local_remote;

/// Validate a delivery target branch for a project's workspace
#[tracing::instrument]
#[tauri::command]
pub async fn validate_delivery_target(
    project_id: i64,
    route_id: i64,
    target_branch: String,
    remote_url: Option<String>,
) -> Result<DeliveryValidation, String> {
    let state = DeltaState::with_route(project_id, route_id);

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

    // Resolve the remote: explicit param → project setting
    let explicit_remote = resolve_remote(remote_url.as_deref());
    let project_fallback = route_remote_url(project_id, route_id).await;
    let effective_remote = explicit_remote.or(project_fallback);

    let is_local = effective_remote
        .as_deref()
        .map(is_local_remote)
        .unwrap_or(false);

    // For local paths, check the target directory exists
    if is_local {
        let local_check_error = effective_remote.as_deref().and_then(|url| {
            let path_str = url.strip_prefix("file://").unwrap_or(url);
            let path = std::path::Path::new(path_str);
            if !path.exists() {
                return Some(format!("Path does not exist: {}", path_str));
            }
            let is_git = path.join("HEAD").exists() || path.join(".git").exists();
            if !is_git {
                return Some("Directory is not a git repository".to_string());
            }
            None
        });

        if let Some(err) = local_check_error {
            return Ok(DeliveryValidation {
                has_remote: false,
                has_forge: false,
                target_exists_on_remote: false,
                merge_state: "unknown".to_string(),
                conflicting_files: vec![],
                available_actions: vec!["push".to_string()],
                remote_branches: vec![],
                remote_url: effective_remote,
                is_local: true,
                needs_init: true,
                error: Some(err),
            });
        }
    }

    let orchestrator =
        DeliveryOrchestrator::from_work_dir(&work_dir, effective_remote.as_deref()).str_err()?;

    let has_remote = orchestrator.has_remote();
    let has_forge = orchestrator.has_forge();
    let resolved_url = orchestrator.git_ops().and_then(|ops| ops.remote_url().ok());

    let mut target_exists_on_remote = false;
    let mut merge_state = "unknown".to_string();
    let mut conflicting_files = Vec::new();
    let remote_branches = orchestrator.list_remote_branches().unwrap_or_default();

    if has_remote {
        target_exists_on_remote = orchestrator
            .target_exists_on_remote(&target_branch)
            .unwrap_or(false);

        if target_exists_on_remote {
            if let Ok(state) = orchestrator.check_merge_state(&target_branch) {
                merge_state = state.as_str().to_string();
                if state == crate::core::state::MergeState::Conflicts {
                    conflicting_files = orchestrator
                        .get_conflicting_files(&target_branch)
                        .unwrap_or_default();
                }
            }
        }
    }

    let mut available_actions = Vec::new();
    if has_remote {
        available_actions.push("push".to_string());
        if has_forge && !is_local {
            available_actions.push("pr".to_string());
        }
        if has_forge && !is_local {
            available_actions.push("merge".to_string());
        }
    }

    Ok(DeliveryValidation {
        has_remote,
        has_forge: has_forge && !is_local,
        target_exists_on_remote,
        merge_state,
        conflicting_files,
        available_actions,
        remote_branches,
        remote_url: resolved_url.or(effective_remote),
        is_local,
        needs_init: false,
        error: None,
    })
}

// =============================================================================
// Board Delivery Commands
// =============================================================================

/// Get all board versions for a project
#[tracing::instrument]
#[tauri::command]
pub async fn get_board_versions(
    project_id: i64,
    route_id: i64,
) -> Result<Vec<BoardVersion>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_board_versions().await.str_err()
}

/// Get the latest board version for a project
#[tracing::instrument]
#[tauri::command]
pub async fn get_latest_board_version(
    project_id: i64,
    route_id: i64,
) -> Result<Option<BoardVersion>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_latest_version().await.str_err()
}

/// Get the current (non-terminal) delivery for a project
#[tracing::instrument]
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
#[tracing::instrument]
#[tauri::command]
pub async fn start_board_delivery(
    project_id: i64,
    route_id: i64,
    version_id: i64,
    target_branch: String,
    _resolve_conflicts: bool,
    remote_url: Option<String>,
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

    // Resolve the remote: explicit param → project setting
    let explicit_remote = resolve_remote(remote_url.as_deref());
    let project_fallback = route_remote_url(project_id, route_id).await;
    let effective_remote = explicit_remote.or(project_fallback);

    let is_local = effective_remote
        .as_deref()
        .map(is_local_remote)
        .unwrap_or(false);

    // For local paths that need init, init the bare repo
    if is_local {
        if let Some(ref url) = effective_remote {
            let path_str = url.strip_prefix("file://").unwrap_or(url);
            let path = std::path::Path::new(path_str);
            if !path.exists() || (!path.join("HEAD").exists() && !path.join(".git").exists()) {
                use crate::core::delivery::GitOperations;
                GitOperations::init_repo(path).str_err()?;
            }
        }
    }

    let orchestrator =
        DeliveryOrchestrator::from_work_dir(&work_dir, effective_remote.as_deref()).str_err()?;

    // For local repos, push directly to target branch.
    // For remote repos, push to a delivery branch (hirsel/<run>).
    let delivery_branch = if is_local {
        None // push HEAD:<target_branch> directly
    } else {
        Some(delivery_branch_name(&project_run.run_name))
    };

    let push_target = if is_local {
        Some(target_branch.as_str())
    } else {
        None
    };

    let push_result = orchestrator
        .push_branch(push_target, delivery_branch.as_deref())
        .str_err()?;

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
#[tracing::instrument]
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
#[tracing::instrument]
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
#[tracing::instrument]
#[tauri::command]
pub async fn complete_board_delivery(
    project_id: i64,
    route_id: i64,
    delivery_id: i64,
    action: String,          // "push", "pr", "merge"
    summary: Option<String>, // Optional summary for PR body
    remote_url: Option<String>,
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

    let explicit_remote = resolve_remote(remote_url.as_deref());
    let project_fallback = route_remote_url(project_id, route_id).await;
    let effective_remote = explicit_remote.or(project_fallback);

    let orchestrator =
        DeliveryOrchestrator::from_work_dir(&work_dir, effective_remote.as_deref()).str_err()?;

    match action.as_str() {
        "push" => {
            let push_result = orchestrator.push_branch(None, None).str_err()?;
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
#[tracing::instrument]
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
