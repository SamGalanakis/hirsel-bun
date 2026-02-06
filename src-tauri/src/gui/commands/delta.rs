//! Delta dispatch Tauri commands
//!
//! Commands for the unified board with draft/live trees and delta dispatch.
//! All commands are scoped to a specific route within a project.

use serde::{Deserialize, Serialize};

use super::ResultExt;
use crate::core::delta::{
    bump_generation, CreateDraftNodeRequest, DeltaDispatchService, DeltaExporter, DeltaRunner,
    DeltaState, DraftNodeTree, LiveNodeTree, ProjectRun, SyncResult, TreeDiff,
    UpdateDraftNodeRequest,
};
use crate::core::orchestrator::DaemonOrchestrator;

// =============================================================================
// Tree Operations
// =============================================================================

/// Get the draft tree for a project route
#[tracing::instrument]
#[tauri::command]
pub async fn get_draft_tree(project_id: i64, route_id: i64) -> Result<Vec<DraftNodeTree>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_draft_tree().await.str_err()
}

/// Get the live tree for a project route
#[tracing::instrument]
#[tauri::command]
pub async fn get_live_tree(project_id: i64, route_id: i64) -> Result<Vec<LiveNodeTree>, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.get_live_tree().await.str_err()
}

/// Create a new draft node
#[tracing::instrument]
#[tauri::command]
pub async fn create_draft_node(
    project_id: i64,
    route_id: i64,
    request: CreateDraftNodeRequest,
) -> Result<crate::core::delta::DraftNode, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.create_draft_node(&request).await.str_err()
}

/// Update a draft node
#[tracing::instrument]
#[tauri::command]
pub async fn update_draft_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
    request: UpdateDraftNodeRequest,
) -> Result<crate::core::delta::DraftNode, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.update_draft_node(&node_id, &request).await.str_err()
}

/// Delete a draft node
#[tracing::instrument]
#[tauri::command]
pub async fn delete_draft_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.delete_draft_node(&node_id).await.str_err()
}

/// Move a draft node to a new parent/position
#[tracing::instrument]
#[tauri::command]
pub async fn move_draft_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
    new_parent_id: Option<String>,
    new_position: i32,
) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state
        .move_draft_node(&node_id, new_parent_id.as_deref(), new_position)
        .await
        .str_err()
}

/// Reset project tree - delete all draft nodes except the root
#[tracing::instrument]
#[tauri::command]
pub async fn reset_project_tree(project_id: i64, route_id: i64) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.reset_tree().await.str_err()
}

// =============================================================================
// Diff Operations
// =============================================================================

/// Compute the diff between draft and live trees
#[tracing::instrument]
#[tauri::command]
pub async fn compute_tree_diff(project_id: i64, route_id: i64) -> Result<TreeDiff, String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    service.get_diff().await.str_err()
}

/// Get a human-readable diff summary
#[tracing::instrument]
#[tauri::command]
pub async fn get_diff_summary(project_id: i64, route_id: i64) -> Result<String, String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    service.get_diff_summary().await.str_err()
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
///
/// This command:
/// 1. Dispatches deltas (creates run record, delta submissions, live nodes)
/// 2. Uses DeltaRunner to process pending submissions via orchestrator
///    - Creates the run directory and workspace if needed
///    - Adds all delta tasks to the run database
///    - Spawns workers to process the tasks
#[tracing::instrument]
#[tauri::command]
pub async fn dispatch_deltas(project_id: i64, route_id: i64) -> Result<DispatchResponse, String> {
    // 1. Dispatch creates global DB records (delta_submissions, live_nodes)
    let service = DeltaDispatchService::new(project_id, route_id);
    let result = service.dispatch().await.str_err()?;

    // 2. Use DeltaRunner to process pending submissions via orchestrator
    let runner = DeltaRunner::new(project_id, route_id);
    let orchestrator = DaemonOrchestrator::connect_or_start()
        .map_err(|e| format!("Failed to connect to daemon: {}", e))?;

    runner
        .process_pending(&orchestrator)
        .await
        .map_err(|e| format!("Failed to process delta dispatch: {}", e))?;

    bump_generation("runs_gen").await.ok();

    Ok(DispatchResponse {
        run_name: result.run_name,
        batch_id: result.batch_id,
        delta_count: result.delta_count,
        diff_summary: result.diff_summary,
    })
}

/// Preview dispatch without executing
#[tracing::instrument]
#[tauri::command]
pub async fn preview_delta_dispatch(
    project_id: i64,
    route_id: i64,
) -> Result<DeltaDispatchPreviewResponse, String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    let preview = service.preview().await.str_err()?;

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

/// Get the persistent run for a project route
#[tracing::instrument]
#[tauri::command]
pub async fn get_project_run(project_id: i64, route_id: i64) -> Result<Option<ProjectRun>, String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    service.get_project_run().await.str_err()
}

/// Complete a live node (mark as done/failed)
#[tracing::instrument]
#[tauri::command]
pub async fn complete_live_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
    success: bool,
    commit_sha: Option<String>,
) -> Result<(), String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    service
        .complete_live_node(&node_id, success, commit_sha.as_deref())
        .await
        .str_err()
}

/// Complete a revert operation (delete the live node)
#[tracing::instrument]
#[tauri::command]
pub async fn complete_revert(
    project_id: i64,
    route_id: i64,
    node_id: String,
) -> Result<(), String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    service.complete_revert(&node_id).await.str_err()
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
#[tracing::instrument]
#[tauri::command]
pub async fn get_dual_trees(project_id: i64, route_id: i64) -> Result<DualTreeResponse, String> {
    let service = DeltaDispatchService::new(project_id, route_id);

    let draft = service.get_draft_tree().await.str_err()?;
    // Return flat live nodes - frontend builds tree structure using draft hierarchy
    // (project nodes are UI-only and don't exist in live_nodes table)
    let live_nodes = service.state().get_live_nodes().await.str_err()?;
    let live: Vec<LiveNodeTree> = live_nodes.into_iter().map(|n| n.into()).collect();
    let diff = service.get_diff().await.str_err()?;
    let project_run = service.get_project_run().await.str_err()?;

    Ok(DualTreeResponse {
        draft,
        live,
        diff,
        project_run,
    })
}
// =============================================================================
// Gyp Sync Operations
// =============================================================================

/// Sync changes from Gyp JSON files back to the database
///
/// This should be called periodically while Gyp is active to pick up
/// changes made by the agent to the board JSON files.
#[tracing::instrument]
#[tauri::command]
pub async fn sync_gyp_changes(project_id: i64, route_id: i64) -> Result<SyncResult, String> {
    let mut exporter = DeltaExporter::new(project_id, route_id);
    exporter
        .sync_file_changes()
        .map_err(|e| format!("Sync failed: {}", e))
}

/// Combined sync + get_dual_trees in one IPC call.
///
/// Syncs Gyp file changes, then returns both trees, diff, and project run.
/// Eliminates the need for two sequential IPC round-trips per poll cycle.
#[tracing::instrument]
#[tauri::command]
pub async fn sync_and_get_trees(
    project_id: i64,
    route_id: i64,
) -> Result<DualTreeResponse, String> {
    // 1. Sync Gyp file changes
    let mut exporter = DeltaExporter::new(project_id, route_id);
    let _ = exporter
        .sync_file_changes()
        .map_err(|e| format!("Sync failed: {}", e));

    // 2. Fetch both trees
    let service = DeltaDispatchService::new(project_id, route_id);
    let draft = service.get_draft_tree().await.str_err()?;
    let live_nodes = service.state().get_live_nodes().await.str_err()?;
    let live: Vec<LiveNodeTree> = live_nodes.into_iter().map(|n| n.into()).collect();
    let diff = service.get_diff().await.str_err()?;
    let project_run = service.get_project_run().await.str_err()?;

    Ok(DualTreeResponse {
        draft,
        live,
        diff,
        project_run,
    })
}

/// Generation-aware sync + get trees — skips full fetch if nothing changed.
///
/// Returns None if the tree generation matches last_generation (no changes),
/// or Some((response, new_generation)) if trees were modified since last check.
#[tracing::instrument]
#[tauri::command]
pub async fn sync_and_get_trees_if_changed(
    project_id: i64,
    route_id: i64,
    last_generation: i64,
) -> Result<Option<(DualTreeResponse, i64)>, String> {
    // Always sync Gyp file changes (may bump generation if content differs)
    let mut exporter = DeltaExporter::new(project_id, route_id);
    let _ = exporter.sync_file_changes();

    let state = DeltaState::with_route(project_id, route_id);
    let current = state.tree_generation().await.str_err()?;
    if current == last_generation {
        return Ok(None);
    }

    // Generation changed — fetch full trees
    let service = DeltaDispatchService::new(project_id, route_id);
    let draft = service.get_draft_tree().await.str_err()?;
    let live_nodes = service.state().get_live_nodes().await.str_err()?;
    let live: Vec<LiveNodeTree> = live_nodes.into_iter().map(|n| n.into()).collect();
    let diff = service.get_diff().await.str_err()?;
    let project_run = service.get_project_run().await.str_err()?;

    Ok(Some((
        DualTreeResponse {
            draft,
            live,
            diff,
            project_run,
        },
        current,
    )))
}
