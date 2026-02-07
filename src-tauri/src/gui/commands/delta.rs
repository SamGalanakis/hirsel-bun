//! Board tree Tauri commands
//!
//! Commands for the unified board tree with feature/task/check nodes and dispatch.
//! All commands are scoped to a specific route within a project.

use serde::Serialize;

use super::ResultExt;
use crate::core::config;
use crate::core::delta::{
    bump_generation, BoardNode, BoardNodeTree, CreateBoardNodeRequest, DeltaDispatchService,
    DeltaExporter, DeltaState, ProjectRun, SyncResult, UpdateBoardNodeRequest,
};
use crate::core::orchestrator::{
    create_local_orchestrator, DaemonOrchestrator, Orchestrator, StartRunRequest,
};
use crate::core::project::ProjectStore;
use crate::core::state::SQLiteState;

// =============================================================================
// Response Types
// =============================================================================

/// Response containing the board tree, project run, and generation counter
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardTreeResponse {
    tree: Vec<BoardNodeTree>,
    project_run: Option<ProjectRun>,
    generation: i64,
}

/// Dispatch result for frontend
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchResponse {
    pub run_name: String,
    pub node_count: usize,
    pub feature_count: usize,
    pub plan_task_count: usize,
    pub version_number: i32,
    pub version_id: i64,
}

// =============================================================================
// Tree Operations
// =============================================================================

/// Get the board tree for a project route
#[tracing::instrument]
#[tauri::command]
pub async fn get_board_tree(project_id: i64, route_id: i64) -> Result<BoardTreeResponse, String> {
    let dispatch = DeltaDispatchService::new(project_id, route_id);
    let state = DeltaState::with_route(project_id, route_id);

    let tree = dispatch.get_tree().await.str_err()?;
    let project_run = dispatch.get_project_run().await.str_err()?;
    let generation = state.tree_generation().await.str_err()?;

    Ok(BoardTreeResponse {
        tree,
        project_run,
        generation,
    })
}

/// Create a new board node
#[tracing::instrument]
#[tauri::command]
pub async fn create_board_node(
    project_id: i64,
    route_id: i64,
    request: CreateBoardNodeRequest,
) -> Result<BoardNode, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.create_node(&request).await.str_err()
}

/// Update a board node
#[tracing::instrument]
#[tauri::command]
pub async fn update_board_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
    request: UpdateBoardNodeRequest,
) -> Result<BoardNode, String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.update_node(&node_id, &request).await.str_err()
}

/// Delete a board node
#[tracing::instrument]
#[tauri::command]
pub async fn delete_board_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.delete_node(&node_id).await.str_err()
}

/// Move a board node to a new parent/position
#[tracing::instrument]
#[tauri::command]
pub async fn move_board_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
    new_parent_id: Option<String>,
    new_position: i32,
) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state
        .move_node(&node_id, new_parent_id.as_deref(), new_position)
        .await
        .str_err()
}

/// Reset project tree - delete all board nodes except the root
#[tracing::instrument]
#[tauri::command]
pub async fn reset_project_tree(project_id: i64, route_id: i64) -> Result<(), String> {
    let state = DeltaState::with_route(project_id, route_id);
    state.reset_tree().await.str_err()
}

// =============================================================================
// Dispatch Operations
// =============================================================================

/// Dispatch board features - set features to pending, create plan tasks, start run
///
/// This command:
/// 1. Finds draft feature nodes and sets them to pending
/// 2. Creates __plan tasks as children of each feature
/// 3. Gets or creates the persistent project run
/// 4. Creates a board version
/// 5. Ensures the daemon is running for orchestration
#[tracing::instrument]
#[tauri::command]
pub async fn dispatch_board(project_id: i64, route_id: i64) -> Result<DispatchResponse, String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    let result = service.dispatch().await.str_err()?;

    // Bootstrap per-run infrastructure so the daemon can manage the run
    let run_db_path = config::run_dir(&result.run_name).join("hirsel.db");
    if !run_db_path.exists() {
        // First dispatch: create per-run DB, workspace, and spawn first worker
        let store = ProjectStore::open().await.str_err()?;
        let project = store.get_project(project_id).await.str_err()?;

        let orchestrator = create_local_orchestrator().str_err()?;
        orchestrator
            .start_run(StartRunRequest {
                name: result.run_name.clone(),
                project_id,
                route_id: Some(route_id),
                spec: String::new(),
                starting_point: None,
                eval: None,
                worker_scale: project.worker_scale.as_deref().and_then(|s| s.parse().ok()),
                time_limit_minutes: project.time_limit_minutes,
                human_in_the_loop: Some(project.human_in_the_loop),
                runner: project.runner.clone(),
                worker_runners: None,
                tailscale_oauth: None,
            })
            .await
            .str_err()?;
    } else {
        // Re-dispatch: signal daemon to spawn workers for new tasks
        let state = SQLiteState::new(&result.run_name).await.str_err()?;
        state.request_scaling_check().await.str_err()?;
    }

    // Ensure daemon is running for lifecycle management
    let _orchestrator =
        DaemonOrchestrator::connect_or_start().context("Failed to connect to daemon")?;

    bump_generation("runs_gen").await.ok();

    Ok(DispatchResponse {
        run_name: result.run_name,
        node_count: result.node_count,
        feature_count: result.feature_count,
        plan_task_count: result.plan_task_count,
        version_number: result.version_number,
        version_id: result.version_id,
    })
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

/// Complete a board node (mark as done/failed)
#[tracing::instrument]
#[tauri::command]
pub async fn complete_board_node(
    project_id: i64,
    route_id: i64,
    node_id: String,
    success: bool,
    commit_sha: Option<String>,
) -> Result<(), String> {
    let service = DeltaDispatchService::new(project_id, route_id);
    service
        .complete_node(&node_id, success, commit_sha.as_deref())
        .await
        .str_err()
}

// =============================================================================
// Gyp Sync Operations
// =============================================================================

/// Sync changes from Gyp content files back to the database
///
/// This should be called periodically while Gyp is active to pick up
/// changes made by the agent to the board content files.
#[tracing::instrument]
#[tauri::command]
pub async fn sync_gyp_changes(project_id: i64, route_id: i64) -> Result<SyncResult, String> {
    let mut exporter = DeltaExporter::new(project_id, route_id);
    exporter
        .sync_file_changes()
        .map_err(|e| format!("Sync failed: {}", e))
}

/// Combined sync + get board tree in one IPC call.
///
/// Syncs Gyp file changes, then returns the board tree and project run.
/// Eliminates the need for two sequential IPC round-trips per poll cycle.
#[tracing::instrument]
#[tauri::command]
pub async fn sync_and_get_trees(
    project_id: i64,
    route_id: i64,
) -> Result<BoardTreeResponse, String> {
    // 1. Sync Gyp file changes
    let mut exporter = DeltaExporter::new(project_id, route_id);
    let _ = exporter
        .sync_file_changes()
        .map_err(|e| format!("Sync failed: {}", e));

    // 2. Fetch board tree
    let dispatch = DeltaDispatchService::new(project_id, route_id);
    let state = DeltaState::with_route(project_id, route_id);

    let tree = dispatch.get_tree().await.str_err()?;
    let project_run = dispatch.get_project_run().await.str_err()?;
    let generation = state.tree_generation().await.str_err()?;

    Ok(BoardTreeResponse {
        tree,
        project_run,
        generation,
    })
}

/// Generation-aware sync + get tree -- skips full fetch if nothing changed.
///
/// Returns None if the tree generation matches last_generation (no changes),
/// or Some(response) if trees were modified since last check.
#[tracing::instrument]
#[tauri::command]
pub async fn sync_and_get_tree_if_changed(
    project_id: i64,
    route_id: i64,
    last_generation: i64,
) -> Result<Option<BoardTreeResponse>, String> {
    // Always sync Gyp file changes (may bump generation if content differs)
    let mut exporter = DeltaExporter::new(project_id, route_id);
    let _ = exporter.sync_file_changes();

    let state = DeltaState::with_route(project_id, route_id);
    let current = state.tree_generation().await.str_err()?;
    if current == last_generation {
        return Ok(None);
    }

    // Generation changed -- fetch full tree
    let dispatch = DeltaDispatchService::new(project_id, route_id);
    let tree = dispatch.get_tree().await.str_err()?;
    let project_run = dispatch.get_project_run().await.str_err()?;

    Ok(Some(BoardTreeResponse {
        tree,
        project_run,
        generation: current,
    }))
}
