//! Delta dispatch Tauri commands
//!
//! Commands for the unified board with draft/live trees and delta dispatch.

use serde::{Deserialize, Serialize};

use crate::core::delta::{
    CreateDraftNodeRequest, DeltaDispatchService, DeltaState, DraftNodeTree, LiveNodeTree,
    ProjectRun, TreeDiff, UpdateDraftNodeRequest,
};

// =============================================================================
// Tree Operations
// =============================================================================

/// Get the draft tree for a project
#[tauri::command]
pub async fn get_draft_tree(project_id: i64) -> Result<Vec<DraftNodeTree>, String> {
    let state = DeltaState::new(project_id);
    state.get_draft_tree().map_err(|e| e.to_string())
}

/// Get the live tree for a project
#[tauri::command]
pub async fn get_live_tree(project_id: i64) -> Result<Vec<LiveNodeTree>, String> {
    let state = DeltaState::new(project_id);
    state.get_live_tree().map_err(|e| e.to_string())
}

/// Create a new draft node
#[tauri::command]
pub async fn create_draft_node(
    project_id: i64,
    request: CreateDraftNodeRequest,
) -> Result<crate::core::delta::DraftNode, String> {
    let state = DeltaState::new(project_id);
    state.create_draft_node(&request).map_err(|e| e.to_string())
}

/// Update a draft node
#[tauri::command]
pub async fn update_draft_node(
    project_id: i64,
    node_id: String,
    request: UpdateDraftNodeRequest,
) -> Result<crate::core::delta::DraftNode, String> {
    let state = DeltaState::new(project_id);
    state
        .update_draft_node(&node_id, &request)
        .map_err(|e| e.to_string())
}

/// Delete a draft node
#[tauri::command]
pub async fn delete_draft_node(project_id: i64, node_id: String) -> Result<(), String> {
    let state = DeltaState::new(project_id);
    state.delete_draft_node(&node_id).map_err(|e| e.to_string())
}

/// Move a draft node to a new parent/position
#[tauri::command]
pub async fn move_draft_node(
    project_id: i64,
    node_id: String,
    new_parent_id: Option<String>,
    new_position: i32,
) -> Result<(), String> {
    let state = DeltaState::new(project_id);
    state
        .move_draft_node(&node_id, new_parent_id.as_deref(), new_position)
        .map_err(|e| e.to_string())
}

// =============================================================================
// Diff Operations
// =============================================================================

/// Compute the diff between draft and live trees
#[tauri::command]
pub async fn compute_tree_diff(project_id: i64) -> Result<TreeDiff, String> {
    let service = DeltaDispatchService::new(project_id);
    service.get_diff().map_err(|e| e.to_string())
}

/// Get a human-readable diff summary
#[tauri::command]
pub async fn get_diff_summary(project_id: i64) -> Result<String, String> {
    let service = DeltaDispatchService::new(project_id);
    service.get_diff_summary().map_err(|e| e.to_string())
}

// =============================================================================
// Dispatch Operations
// =============================================================================

/// Dispatch result for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchResponse {
    pub run_name: String,
    pub batch_id: i64,
    pub delta_count: usize,
    pub diff_summary: String,
}

/// Dispatch deltas - create delta tasks and start/resume run
#[tauri::command]
pub async fn dispatch_deltas(project_id: i64) -> Result<DispatchResponse, String> {
    let service = DeltaDispatchService::new(project_id);
    let result = service.dispatch().map_err(|e| e.to_string())?;

    Ok(DispatchResponse {
        run_name: result.run_name,
        batch_id: result.batch_id,
        delta_count: result.delta_count,
        diff_summary: result.diff_summary,
    })
}

/// Preview dispatch without executing
#[tauri::command]
pub async fn preview_delta_dispatch(
    project_id: i64,
) -> Result<DeltaDispatchPreviewResponse, String> {
    let service = DeltaDispatchService::new(project_id);
    let preview = service.preview().map_err(|e| e.to_string())?;

    Ok(DeltaDispatchPreviewResponse {
        diff: preview.diff,
        task_count: preview.tasks.len(),
        has_existing_run: preview.has_existing_run,
    })
}

/// Preview response for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaDispatchPreviewResponse {
    pub diff: TreeDiff,
    pub task_count: usize,
    pub has_existing_run: bool,
}

// =============================================================================
// Run Operations
// =============================================================================

/// Get the persistent run for a project
#[tauri::command]
pub async fn get_project_run(project_id: i64) -> Result<Option<ProjectRun>, String> {
    let service = DeltaDispatchService::new(project_id);
    service.get_project_run().map_err(|e| e.to_string())
}

/// Complete a live node (mark as done/failed)
#[tauri::command]
pub async fn complete_live_node(
    project_id: i64,
    node_id: String,
    success: bool,
    commit_sha: Option<String>,
) -> Result<(), String> {
    let service = DeltaDispatchService::new(project_id);
    service
        .complete_live_node(&node_id, success, commit_sha.as_deref())
        .map_err(|e| e.to_string())
}

/// Complete a revert operation (delete the live node)
#[tauri::command]
pub async fn complete_revert(project_id: i64, node_id: String) -> Result<(), String> {
    let service = DeltaDispatchService::new(project_id);
    service.complete_revert(&node_id).map_err(|e| e.to_string())
}

// =============================================================================
// Both Trees Response (for UI)
// =============================================================================

/// Response containing both draft and live trees
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DualTreeResponse {
    pub draft: Vec<DraftNodeTree>,
    pub live: Vec<LiveNodeTree>,
    pub diff: TreeDiff,
    pub project_run: Option<ProjectRun>,
}

/// Get both trees in one call (more efficient for UI)
#[tauri::command]
pub async fn get_dual_trees(project_id: i64) -> Result<DualTreeResponse, String> {
    let service = DeltaDispatchService::new(project_id);

    let draft = service.get_draft_tree().map_err(|e| e.to_string())?;
    let live = service.get_live_tree().map_err(|e| e.to_string())?;
    let diff = service.get_diff().map_err(|e| e.to_string())?;
    let project_run = service.get_project_run().map_err(|e| e.to_string())?;

    Ok(DualTreeResponse {
        draft,
        live,
        diff,
        project_run,
    })
}
