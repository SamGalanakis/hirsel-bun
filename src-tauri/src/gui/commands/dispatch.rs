//! Dispatch commands
//!
//! Commands for dispatching runs from board tasks.

use super::runs::get_run_detail;
use crate::cli::config::get_agent_command;
use crate::cli::go::WorkerScale;
use crate::core::api_types::RunDetail;
use crate::core::board::{BoardSnapshot, DispatchPreview, TaskRun};
use crate::core::dispatch::{DispatchConfig, DispatchInfo, DispatchService};
use crate::core::draft::create_workspace_provider;
use crate::core::names::get_available_names;
use crate::core::ops::{compute_multi_worker_config, setup_run_workspace, RunSetupConfig};
use crate::core::runner::{create_runner, WorkerSpawnConfig as RunnerSpawnConfig};
use crate::core::state::{SQLiteState, WorkerUpdate};
use crate::core::{config, Files, ProjectStore};

/// Preview what will be dispatched from a task
#[tauri::command]
pub async fn preview_dispatch(project_id: i64, task_id: String) -> Result<DispatchPreview, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let service = DispatchService::new(project_id);
    service
        .preview_dispatch(&task_id)
        .map_err(|e| e.to_string())
}

/// Prepare a dispatch (generates spec/eval content) without creating the run
#[tauri::command]
pub async fn prepare_dispatch(
    project_id: i64,
    task_id: String,
    run_name: Option<String>,
    target_branch: Option<String>,
    worker_scale: Option<String>,
    time_limit_minutes: Option<i64>,
) -> Result<DispatchInfo, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let config = DispatchConfig {
        run_name,
        target_branch,
        worker_scale,
        time_limit_minutes,
    };

    let service = DispatchService::new(project_id);
    service
        .prepare_dispatch(&task_id, &config)
        .map_err(|e| e.to_string())
}

/// Record that a run was dispatched from a task
#[tauri::command]
pub async fn record_dispatch(
    project_id: i64,
    task_id: String,
    run_name: String,
) -> Result<(), String> {
    let service = DispatchService::new(project_id);
    service
        .record_dispatch(&task_id, &run_name)
        .map_err(|e| e.to_string())
}

/// Get all runs dispatched from a specific task
#[tauri::command]
pub async fn get_task_runs(project_id: i64, task_id: String) -> Result<Vec<TaskRun>, String> {
    let service = DispatchService::new(project_id);
    service.get_task_runs(&task_id).map_err(|e| e.to_string())
}

/// Get all task runs for a project
#[tauri::command]
pub async fn get_all_task_runs(project_id: i64) -> Result<Vec<TaskRun>, String> {
    let service = DispatchService::new(project_id);
    service.get_all_task_runs().map_err(|e| e.to_string())
}

/// Create a board snapshot for a dispatch
#[tauri::command]
pub async fn create_dispatch_snapshot(
    project_id: i64,
    task_ids: Vec<String>,
) -> Result<BoardSnapshot, String> {
    let service = DispatchService::new(project_id);
    service
        .create_snapshot(&task_ids)
        .map_err(|e| e.to_string())
}

/// Get the dispatch scope for multiple root tasks
#[tauri::command]
pub async fn get_multi_dispatch_scope(
    project_id: i64,
    root_task_ids: Vec<String>,
) -> Result<crate::core::dispatch::DispatchScope, String> {
    let service = DispatchService::new(project_id);
    service
        .get_multi_dispatch_scope(&root_task_ids)
        .map_err(|e| e.to_string())
}

/// Prepare a multi-root dispatch
#[tauri::command]
pub async fn prepare_multi_dispatch(
    project_id: i64,
    root_task_ids: Vec<String>,
    run_name: Option<String>,
    target_branch: Option<String>,
    worker_scale: Option<String>,
    time_limit_minutes: Option<i64>,
) -> Result<DispatchInfo, String> {
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let _ = store.get_project(project_id).map_err(|e| e.to_string())?;

    let config = DispatchConfig {
        run_name,
        target_branch,
        worker_scale,
        time_limit_minutes,
    };

    let service = DispatchService::new(project_id);
    service
        .prepare_multi_dispatch(&root_task_ids, &config)
        .map_err(|e| e.to_string())
}

/// Dispatch and start a run from board tasks
///
/// This command:
/// 1. Prepares the dispatch (generates spec/eval content from board tasks)
/// 2. Creates run directory and database
/// 3. Creates workspace from project's starting point
/// 4. Creates tasks from board scope (work tasks + eval tasks + scope task)
/// 5. Spawns workers
/// 6. Records dispatch associations for each root task
#[tauri::command]
pub async fn dispatch_board_run(
    project_id: i64,
    root_task_ids: Vec<String>,
    run_name: Option<String>,
    worker_scale: Option<String>,
    time_limit_minutes: Option<i64>,
) -> Result<RunDetail, String> {
    use std::fs;

    // Get project from store
    let store = ProjectStore::open().map_err(|e| e.to_string())?;
    let project = store.get_project(project_id).map_err(|e| e.to_string())?;

    // Prepare dispatch config
    let dispatch_config = DispatchConfig {
        run_name,
        target_branch: project.target_branch.clone(),
        worker_scale: worker_scale
            .clone()
            .or_else(|| project.worker_scale.clone()),
        time_limit_minutes: time_limit_minutes.or(project.time_limit_minutes),
    };

    // Prepare the dispatch (generates spec/eval content, run name)
    let service = DispatchService::new(project_id);
    let dispatch_info = service
        .prepare_multi_dispatch(&root_task_ids, &dispatch_config)
        .map_err(|e| e.to_string())?;

    let run_name = dispatch_info.run_name.clone();
    let run_dir = config::run_dir(&run_name);

    // Ensure run directory doesn't already exist
    if run_dir.exists() {
        return Err(format!("Run '{}' already exists", run_name));
    }

    // Create run directory
    fs::create_dir_all(&run_dir).map_err(|e| format!("Failed to create run directory: {}", e))?;

    // Initialize Files helper and create required directories
    let files = Files::new(&run_dir);
    files
        .init_dirs()
        .map_err(|e| format!("Failed to init dirs: {}", e))?;

    // Write spec.md (tasks.md content for board dispatches)
    fs::write(run_dir.join("spec.md"), &dispatch_info.spec_content)
        .map_err(|e| format!("Failed to write spec.md: {}", e))?;

    // Write eval.md if present
    if let Some(ref eval_content) = dispatch_info.eval_content {
        fs::write(run_dir.join("eval.md"), eval_content)
            .map_err(|e| format!("Failed to write eval.md: {}", e))?;
    }

    // Initialize database
    let db_path = run_dir.join("hirsel.db");
    let state =
        SQLiteState::new(db_path).map_err(|e| format!("Failed to create database: {}", e))?;

    // Initialize state (workspace path will be set after workspace creation)
    state
        .init_state(None)
        .map_err(|e| format!("Failed to init state: {}", e))?;

    // Set configuration in state
    let effective_worker_scale = dispatch_config
        .worker_scale
        .clone()
        .unwrap_or_else(|| "1".to_string());
    state
        .set_worker_scale(&effective_worker_scale)
        .map_err(|e| format!("Failed to set worker scale: {}", e))?;

    if let Some(time_limit) = dispatch_config.time_limit_minutes {
        state
            .set_time_limit_minutes(Some(time_limit))
            .map_err(|e| format!("Failed to set time limit: {}", e))?;
    }

    state
        .set_human_in_the_loop(project.human_in_the_loop)
        .map_err(|e| format!("Failed to set HITL: {}", e))?;

    // Store spec content for display
    state
        .set_request(Some(&dispatch_info.spec_content))
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

    // Store starting point in state
    let sp_json = serde_json::to_string(&project.starting_point)
        .map_err(|e| format!("Failed to serialize starting_point: {}", e))?;
    state
        .set_starting_point(Some(&sp_json))
        .map_err(|e| format!("Failed to set starting_point: {}", e))?;

    // Set branch if available from workspace init
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

    // Load global config for docs settings
    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Store docs config in run state
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

    // Create tasks from board scope (this creates scope + work + eval tasks)
    // Note: create_scoped_run_tasks claims scope for "worker-1", we'll fix it below
    service
        .create_scoped_run_tasks(&state, &root_task_ids)
        .map_err(|e| format!("Failed to create run tasks: {}", e))?;

    // Fix scope task claim to use actual first worker name
    // (create_scoped_run_tasks hardcodes "worker-1" but we use generated names)
    let first_worker = &worker_names[0];
    let _ = state.unclaim_task("scope", "worker-1"); // Clear the "worker-1" claim
    let _ = state.claim_task("scope", first_worker); // Claim with actual worker name

    // Set status to working and start time tracking
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
            tracing::info!("Run paused, stopping worker spawn");
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

        // Get the runner config (use default local runner)
        let runner_config = global_config.get_runner("local").unwrap_or_default();
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
        };

        // Spawn the worker
        match runner.spawn(&spawn_config).await {
            Ok(result) => {
                // Update worker with PID
                if let Some(pid) = result.pid {
                    let _ = state.update_worker(
                        worker_name,
                        WorkerUpdate {
                            pid: Some(pid as i64),
                            ..Default::default()
                        },
                    );
                }
                tracing::info!(
                    "Spawned worker {} for board dispatch (type: {})",
                    worker_name,
                    result.handle.runner_type
                );
            }
            Err(e) => {
                tracing::error!("Failed to spawn worker {}: {}", worker_name, e);
                // Continue with other workers
            }
        }
    }

    // Record dispatch for each root task
    for task_id in &root_task_ids {
        service
            .record_dispatch(task_id, &run_name)
            .map_err(|e| format!("Failed to record dispatch: {}", e))?;
    }

    // Return the run detail
    get_run_detail(run_name).await
}
