//! Delta dispatch Tauri commands
//!
//! Commands for the unified board with draft/live trees and delta dispatch.

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::cli::config::get_agent_command;
use crate::cli::go::WorkerScale;
use crate::core::delta::{
    CreateDraftNodeRequest, DeltaDispatchService, DeltaExporter, DeltaState, DraftNodeTree,
    LiveNodeTree, ProjectRun, SyncResult, TreeDiff, UpdateDraftNodeRequest,
};
use crate::core::draft::create_workspace_provider;
use crate::core::names::get_available_names;
use crate::core::ops::{compute_multi_worker_config, setup_run_workspace, RunSetupConfig};
use crate::core::runner::{create_runner, WorkerSpawnConfig as RunnerSpawnConfig};
use crate::core::state::{SQLiteState, WorkerUpdate};
use crate::core::{config, Files, ProjectStore};

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

/// Reset project tree - delete all draft nodes except the root
#[tauri::command]
pub async fn reset_project_tree(project_id: i64) -> Result<(), String> {
    let state = DeltaState::new(project_id);
    state.reset_tree().map_err(|e| e.to_string())
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
///
/// This command:
/// 1. Dispatches deltas (creates run record, delta submissions, live nodes)
/// 2. Creates/updates the actual run directory with workspace if needed
/// 3. Spawns workers to process the tasks
#[tauri::command]
pub async fn dispatch_deltas(project_id: i64) -> Result<DispatchResponse, String> {
    use std::fs;

    let service = DeltaDispatchService::new(project_id);
    let result = service.dispatch().map_err(|e| e.to_string())?;

    let run_name = result.run_name.clone();
    let run_dir = config::run_dir(&run_name);

    // Get project info for workspace setup
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let project = store.get_project(project_id).map_err(|e| e.to_string())?;

    // Check if run directory needs to be created
    let needs_setup = !run_dir.exists();

    if needs_setup {
        info!(
            "Creating new run directory for delta dispatch: {}",
            run_name
        );

        // Create run directory
        fs::create_dir_all(&run_dir)
            .map_err(|e| format!("Failed to create run directory: {}", e))?;

        // Initialize Files helper and create required directories
        let files = Files::new(&run_dir);
        files
            .init_dirs()
            .map_err(|e| format!("Failed to init dirs: {}", e))?;

        // Generate spec content from delta tasks
        let spec_content = generate_spec_from_deltas(project_id)?;
        fs::write(run_dir.join("spec.md"), &spec_content)
            .map_err(|e| format!("Failed to write spec.md: {}", e))?;

        // Initialize database
        let db_path = run_dir.join("hirsel.db");
        let state =
            SQLiteState::new(db_path).map_err(|e| format!("Failed to create database: {}", e))?;

        // Initialize state
        state
            .init_state(None)
            .map_err(|e| format!("Failed to init state: {}", e))?;

        // Set configuration in state
        let effective_worker_scale = project
            .worker_scale
            .clone()
            .unwrap_or_else(|| "1".to_string());
        state
            .set_worker_scale(&effective_worker_scale)
            .map_err(|e| format!("Failed to set worker scale: {}", e))?;

        if let Some(time_limit) = project.time_limit_minutes {
            state
                .set_time_limit_minutes(Some(time_limit))
                .map_err(|e| format!("Failed to set time limit: {}", e))?;
        }

        state
            .set_human_in_the_loop(project.human_in_the_loop)
            .map_err(|e| format!("Failed to set HITL: {}", e))?;

        // Store spec content
        state
            .set_request(Some(&spec_content))
            .map_err(|e| format!("Failed to set request: {}", e))?;

        // Store project association
        state
            .set_project_id(project_id)
            .map_err(|e| format!("Failed to set project_id: {}", e))?;
        state
            .set_project_name(&project.name)
            .map_err(|e| format!("Failed to set project_name: {}", e))?;

        // Create workspace from project's starting point
        let workspace = create_workspace_provider(None);
        let workspace_info = workspace
            .init(&run_name, &project.starting_point)
            .await
            .map_err(|e| format!("Failed to initialize workspace: {}", e))?;

        // Store workspace path in state
        state
            .set_project_path(workspace_info.path.to_str().unwrap_or("."))
            .map_err(|e| format!("Failed to set project path: {}", e))?;

        // Store starting point
        let sp_json = serde_json::to_string(&project.starting_point)
            .map_err(|e| format!("Failed to serialize starting_point: {}", e))?;
        state
            .set_starting_point(Some(&sp_json))
            .map_err(|e| format!("Failed to set starting_point: {}", e))?;

        // Set branch if available
        if let Some(ref branch) = workspace_info.default_branch {
            state
                .set_branch(Some(branch))
                .map_err(|e| format!("Failed to set branch: {}", e))?;
        }

        // Parse worker scale
        let scale = WorkerScale::parse(&effective_worker_scale)
            .map_err(|e| format!("Invalid worker scale: {}", e))?;

        // Get worker names
        let initial_count = scale.initial_count();
        let worker_names = get_available_names(initial_count, &[]);

        // Determine multi-worker configuration
        let (is_multi_worker, leader) = compute_multi_worker_config(&worker_names, scale.max);

        // Load global config
        let (global_config, _) =
            config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

        // Store docs config
        state
            .set_docs_path(Some(&project.docs_path))
            .map_err(|e| format!("Failed to set docs path: {}", e))?;
        state
            .set_persist_docs_changes(project.persist_docs_changes)
            .map_err(|e| format!("Failed to set persist_docs_changes: {}", e))?;

        // Set up workspace, worker clones, and chats
        let setup_config = RunSetupConfig {
            run_name: run_name.clone(),
            project_path: workspace_info.path.clone(),
            run_dir: run_dir.clone(),
            worker_names: worker_names.clone(),
            additional_chat_workers: Vec::new(),
            is_multi_worker,
            leader_name: leader.clone(),
            docs_path: project.docs_path.clone(),
        };

        let setup_result = setup_run_workspace(&setup_config).map_err(|e| e.to_string())?;

        // Register workers in state
        for (worker_name, work_dir) in &setup_result.worker_dirs {
            state
                .add_worker(worker_name, work_dir.to_str().unwrap_or("."), "local")
                .map_err(|e| format!("Failed to register worker {}: {}", worker_name, e))?;
        }

        // Create scope task for first worker
        let first_worker = &worker_names[0];
        state
            .add_task("scope", "Read spec, create exploration tasks", None, None)
            .map_err(|e| format!("Failed to create scope task: {}", e))?;
        state
            .claim_task("scope", first_worker)
            .map_err(|e| format!("Failed to claim scope task: {}", e))?;

        // Set status to working
        state
            .set_status(crate::core::state::Status::Working)
            .map_err(|e| format!("Failed to set status: {}", e))?;

        state
            .set_started_at(None)
            .map_err(|e| format!("Failed to set started_at: {}", e))?;

        // Get agent command
        let agent_command = get_agent_command();
        let spec_path = run_dir.join("spec.md");

        // Spawn workers
        for (i, (worker_name, work_dir)) in setup_result.worker_dirs.iter().enumerate() {
            // Check if run was paused while spawning
            if state
                .status()
                .map(|s| s == crate::core::state::Status::Paused)
                .unwrap_or(false)
            {
                info!("Run paused, stopping worker spawn");
                break;
            }

            let is_leader = i == 0 && is_multi_worker;

            // Build teammates list (all workers except self)
            let teammates = if is_multi_worker {
                Some(
                    worker_names
                        .iter()
                        .filter(|t| *t != worker_name)
                        .cloned()
                        .collect(),
                )
            } else {
                None
            };

            // Get the runner config - prefer project.runner, then global default_runner, then "local"
            let runner_name = project
                .runner
                .clone()
                .or(global_config.default_runner.clone())
                .unwrap_or_else(|| "local".to_string());
            let runner_config = global_config.get_runner(&runner_name).unwrap_or_default();
            let runner = create_runner(&runner_config);

            // Build spawn config
            let spawn_config = RunnerSpawnConfig {
                run_name: run_name.clone(),
                worker_name: worker_name.clone(),
                work_dir: work_dir.clone(),
                run_dir: run_dir.clone(),
                spec_path: spec_path.clone(),
                agent_command: agent_command.clone(),
                is_leader,
                leader_name: leader.clone(),
                teammates,
                resume_session_id: None,
                env_vars: None,
                coordinator_url: None,
                tailscale_authkey: None,
                credentials: None,
                assigned_task_id: None,
            };

            // Spawn the worker
            match runner.spawn(&spawn_config).await {
                Ok(spawn_result) => {
                    // Update worker with PID
                    if let Some(pid) = spawn_result.pid {
                        let _ = state.update_worker(
                            worker_name,
                            WorkerUpdate {
                                pid: Some(pid as i64),
                                ..Default::default()
                            },
                        );
                    }
                    info!(
                        "Spawned worker {} for delta dispatch (type: {})",
                        worker_name, spawn_result.handle.runner_type
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to spawn worker {}: {}", worker_name, e);
                }
            }
        }
    } else {
        // Run exists - resume workers if needed
        info!("Run directory exists, checking for idle workers to resume");
        resume_idle_workers(&run_name).await?;
    }

    Ok(DispatchResponse {
        run_name: result.run_name,
        batch_id: result.batch_id,
        delta_count: result.delta_count,
        diff_summary: result.diff_summary,
    })
}

/// Generate spec.md content from delta submissions
fn generate_spec_from_deltas(project_id: i64) -> Result<String, String> {
    let state = DeltaState::new(project_id);
    let draft_tree = state.get_draft_tree().map_err(|e| e.to_string())?;

    let mut lines = vec![
        "# Project Scope".to_string(),
        String::new(),
        "## Tasks".to_string(),
        String::new(),
    ];

    fn collect_tasks(nodes: &[DraftNodeTree], lines: &mut Vec<String>, depth: usize) {
        for node in nodes {
            let indent = "  ".repeat(depth);
            lines.push(format!("{}- **{}**: {}", indent, node.name, node.content));
            if !node.children.is_empty() {
                collect_tasks(&node.children, lines, depth + 1);
            }
        }
    }

    collect_tasks(&draft_tree, &mut lines, 0);

    Ok(lines.join("\n"))
}

/// Resume idle workers in an existing run
async fn resume_idle_workers(run_name: &str) -> Result<(), String> {
    let run_dir = config::run_dir(run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Ok(());
    }

    let state = SQLiteState::new(db_path).map_err(|e| e.to_string())?;

    // Check if run is working
    let status = state.status().map_err(|e| e.to_string())?;
    if status != crate::core::state::Status::Working {
        state
            .set_status(crate::core::state::Status::Working)
            .map_err(|e| e.to_string())?;
    }

    // Get workers that need respawning (no PID or dead)
    let workers = state.get_workers().map_err(|e| e.to_string())?;

    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));
    let agent_command = get_agent_command();
    let spec_path = run_dir.join("spec.md");

    // Get project runner if available
    let project_runner = if let Some(project_id) = state.get_project_id().ok().flatten() {
        let store = ProjectStore::open().map_err(|e| e.to_string())?;
        store
            .get_project(project_id)
            .ok()
            .and_then(|p| p.runner.clone())
    } else {
        None
    };

    let worker_names: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();
    let is_multi_worker = workers.len() > 1;
    let leader = worker_names.first().cloned();

    for worker in &workers {
        // Skip if worker has a PID and is alive
        if let Some(pid) = worker.pid {
            if crate::core::workers::is_pid_alive(pid as u32) {
                continue;
            }
        }

        // Skip workers without work_dir
        let Some(ref work_dir_str) = worker.work_dir else {
            continue;
        };
        let work_dir = std::path::PathBuf::from(work_dir_str);
        let is_leader = Some(&worker.name) == leader.as_ref() && is_multi_worker;

        let teammates = if is_multi_worker {
            Some(
                worker_names
                    .iter()
                    .filter(|t| *t != &worker.name)
                    .cloned()
                    .collect(),
            )
        } else {
            None
        };

        // Get the runner config - prefer project.runner, then global default_runner, then "local"
        let runner_name = project_runner
            .clone()
            .or(global_config.default_runner.clone())
            .unwrap_or_else(|| "local".to_string());
        let runner_config = global_config.get_runner(&runner_name).unwrap_or_default();
        let runner = create_runner(&runner_config);

        let spawn_config = RunnerSpawnConfig {
            run_name: run_name.to_string(),
            worker_name: worker.name.clone(),
            work_dir,
            run_dir: run_dir.clone(),
            spec_path: spec_path.clone(),
            agent_command: agent_command.clone(),
            is_leader,
            leader_name: leader.clone(),
            teammates,
            resume_session_id: None,
            env_vars: None,
            coordinator_url: None,
            tailscale_authkey: None,
            credentials: None,
            assigned_task_id: None,
        };

        match runner.spawn(&spawn_config).await {
            Ok(spawn_result) => {
                if let Some(pid) = spawn_result.pid {
                    let _ = state.update_worker(
                        &worker.name,
                        WorkerUpdate {
                            pid: Some(pid as i64),
                            ..Default::default()
                        },
                    );
                }
                info!("Resumed worker {} for delta dispatch", worker.name);
            }
            Err(e) => {
                tracing::error!("Failed to resume worker {}: {}", worker.name, e);
            }
        }
    }

    Ok(())
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
    // Return flat live nodes - frontend builds tree structure using draft hierarchy
    // (project nodes are UI-only and don't exist in live_nodes table)
    let live_nodes = service
        .state()
        .get_live_nodes()
        .map_err(|e| e.to_string())?;
    let live: Vec<LiveNodeTree> = live_nodes.into_iter().map(|n| n.into()).collect();
    let diff = service.get_diff().map_err(|e| e.to_string())?;
    let project_run = service.get_project_run().map_err(|e| e.to_string())?;

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
#[tauri::command]
pub async fn sync_gyp_changes(project_id: i64) -> Result<SyncResult, String> {
    let mut exporter = DeltaExporter::new(project_id);
    exporter
        .sync_file_changes()
        .map_err(|e| format!("Sync failed: {}", e))
}
