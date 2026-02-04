//! Draft-related commands
//!
//! Commands for managing drafts: creating, updating, and starting runs.
//! Uses the workspace abstraction for initializing draft workspaces.

use super::runs::get_run_detail;
use super::types::{DraftUpdateRequest, RepoValidation};
use super::ResultExt;
use crate::core::api_types::{RunDetail, RunStatus};
use crate::core::draft::{create_workspace_provider, StartingPoint};
use crate::core::git;
use crate::core::names::generate_run_name;
use crate::core::ops::{clone_run as ops_clone_run, CloneRunConfig};
use crate::core::{config, state::SQLiteState};

/// Validate a repository path or URL
///
/// Checks if the path/URL is a valid git repository and returns branch information.
/// Used for git URL validation when creating a draft from a git repository.
#[tauri::command]
pub async fn validate_repo(path: String) -> Result<RepoValidation, String> {
    let path = path.trim();

    if path.is_empty() {
        return Ok(RepoValidation {
            valid: false,
            error: Some("Path is required".to_string()),
            is_remote: false,
            branches: Vec::new(),
            current_branch: None,
            repo_url: String::new(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        });
    }

    // Check if it's a remote URL
    let is_remote = git::is_remote_url(path);

    if is_remote {
        // Parse URL to extract branch if present
        let parsed = git::parse_github_url(path);
        let repo_url = parsed.repo_url.clone();

        // Try to list branches
        match git::list_remote_branches(&repo_url) {
            Ok(branches) => {
                let url_branch_valid = parsed
                    .branch
                    .as_ref()
                    .map(|b| branches.contains(b))
                    .unwrap_or(false);

                Ok(RepoValidation {
                    valid: true,
                    error: None,
                    is_remote: true,
                    branches,
                    current_branch: None,
                    repo_url,
                    url_branch: parsed.branch,
                    url_branch_valid,
                    needs_dir_create: false,
                    needs_git_init: false,
                })
            }
            Err(e) => Ok(RepoValidation {
                valid: false,
                error: Some(format!("Failed to access repository: {}", e)),
                is_remote: true,
                branches: Vec::new(),
                current_branch: None,
                repo_url,
                url_branch: parsed.branch,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: false,
            }),
        }
    } else {
        // Local path - check if it exists and is a git repo
        let local_path = std::path::Path::new(path);

        if !local_path.exists() {
            return Ok(RepoValidation {
                valid: false,
                error: None, // Not an error, just needs creation
                is_remote: false,
                branches: Vec::new(),
                current_branch: None,
                repo_url: path.to_string(),
                url_branch: None,
                url_branch_valid: false,
                needs_dir_create: true,
                needs_git_init: true,
            });
        }

        let git_dir = local_path.join(".git");
        if !git_dir.exists() {
            return Ok(RepoValidation {
                valid: false,
                error: None, // Not an error, just needs git init
                is_remote: false,
                branches: Vec::new(),
                current_branch: None,
                repo_url: path.to_string(),
                url_branch: None,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: true,
            });
        }

        // It's a valid local git repo - list branches
        match git::list_branches(local_path) {
            Ok(branches) => {
                let current = git::get_current_branch(local_path).ok();
                Ok(RepoValidation {
                    valid: true,
                    error: None,
                    is_remote: false,
                    branches,
                    current_branch: current,
                    repo_url: path.to_string(),
                    url_branch: None,
                    url_branch_valid: false,
                    needs_dir_create: false,
                    needs_git_init: false,
                })
            }
            Err(e) => Ok(RepoValidation {
                valid: false,
                error: Some(format!("Failed to read repository: {}", e)),
                is_remote: false,
                branches: Vec::new(),
                current_branch: None,
                repo_url: path.to_string(),
                url_branch: None,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: false,
            }),
        }
    }
}

/// Create a new draft run
///
/// Creates a draft run with a random friendly name. The draft can be configured
/// before being started. No workers are spawned until start_draft is called.
///
/// Workspace is NOT created here - it's created when start_draft is called with
/// the user's chosen starting point.
#[tauri::command]
pub async fn create_draft() -> Result<RunDetail, String> {
    use crate::core::Files;
    use std::fs;

    // Generate a unique run name
    let mut run_name = generate_run_name();
    let mut run_dir = config::run_dir(&run_name);

    // Ensure the name is unique by appending a number if needed
    let mut counter = 1;
    while run_dir.exists() {
        run_name = format!("{}-{}", generate_run_name(), counter);
        run_dir = config::run_dir(&run_name);
        counter += 1;
        if counter > 100 {
            return Err("Failed to generate unique run name".to_string());
        }
    }

    // Create run directory
    fs::create_dir_all(&run_dir).map_err(|e| format!("Failed to create run directory: {}", e))?;

    // Initialize Files helper and create required directories
    let files = Files::new(&run_dir);
    files
        .init_dirs()
        .map_err(|e| format!("Failed to init dirs: {}", e))?;

    // Create empty spec.md
    fs::write(
        run_dir.join("spec.md"),
        "# Specification\n\nDescribe the task for the AI workers...\n",
    )
    .map_err(|e| format!("Failed to create spec file: {}", e))?;

    // Create empty tasks.md
    fs::write(
        run_dir.join("tasks.md"),
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Scope |\n",
    ).map_err(|e| format!("Failed to create tasks file: {}", e))?;

    // Initialize database - NO workspace path yet
    let state = SQLiteState::new(&run_name)
        .await
        .map_err(|e| format!("Failed to create database: {}", e))?;

    // Initialize state without workspace path (will be set in start_draft)
    state
        .init_state(None)
        .await
        .map_err(|e| format!("Failed to init state: {}", e))?;
    state
        .set_status(crate::core::state::Status::Draft)
        .await
        .map_err(|e| format!("Failed to set draft status: {}", e))?;

    // Set defaults
    state
        .set_worker_scale("1")
        .await
        .map_err(|e| format!("Failed to set worker scale: {}", e))?;
    state
        .set_human_in_the_loop(true)
        .await
        .map_err(|e| format!("Failed to set HITL: {}", e))?;

    // Return the run detail
    let created_at = chrono::Utc::now().to_rfc3339();

    // Get agent type from global config
    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));
    let agent_type = global_config.agent.agent_type();
    let metrics_available = agent_type.supports_context_tracking();

    Ok(RunDetail {
        name: run_name,
        status: RunStatus::Draft,
        request: None,
        project_path: None, // No workspace yet - will be created in start_draft
        remote_url: None,
        branch: None,
        worker_scale: Some("1".to_string()),
        time_limit_minutes: None,
        started_at: None,
        summary: None,
        created_at: created_at.clone(),
        updated_at: created_at,
        iteration_count: 0,
        human_in_the_loop: true,
        waiting_reason: None,
        unread_count: 0,
        tasks_done: 0,
        tasks_total: 1,
        workers_active: 0,
        workers_total: 0,
        elapsed_minutes: 0.0,
        agent_type: format!("{:?}", agent_type).to_lowercase(),
        metrics_available,
        runner: None,
        worker_runners: None,
        project_id: None,
        project_name: None,
    })
}

/// Clone an existing run to a new draft
///
/// Creates a new draft run with the same settings, spec, and eval as the source run.
/// Does not copy messages, tasks (except scope), workers, or any runtime state.
#[tauri::command]
pub async fn clone_run(source_run: String, new_name: String) -> Result<RunDetail, String> {
    // Use the shared ops implementation
    let config = CloneRunConfig::new(&source_run, &new_name);
    let result = ops_clone_run(config).await.str_err()?;

    // Get agent type from global config for the RunDetail response
    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));
    let agent_type = global_config.agent.agent_type();
    let metrics_available = agent_type.supports_context_tracking();

    let created_at = chrono::Utc::now().to_rfc3339();

    Ok(RunDetail {
        name: result.new_name,
        status: RunStatus::Draft,
        request: Some(result.spec_content),
        project_path: result.project_path,
        remote_url: None,
        branch: None,
        worker_scale: Some(result.worker_scale),
        time_limit_minutes: result.time_limit_minutes.map(|t| t as u32),
        started_at: None,
        summary: None,
        created_at: created_at.clone(),
        updated_at: created_at,
        iteration_count: 0,
        human_in_the_loop: result.human_in_the_loop,
        waiting_reason: None,
        unread_count: 0,
        tasks_done: 0,
        tasks_total: 1,
        workers_active: 0,
        workers_total: 0,
        elapsed_minutes: 0.0,
        agent_type: format!("{:?}", agent_type).to_lowercase(),
        metrics_available,
        runner: None,
        worker_runners: None,
        project_id: None,
        project_name: None,
    })
}

/// Update a draft run's configuration
///
/// Allows updating the spec, worker scale, time limit, HITL mode, and project path
/// before the draft is started.
#[tauri::command]
pub async fn update_draft(run_name: String, updates: DraftUpdateRequest) -> Result<(), String> {
    use std::fs;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(&run_name)
        .await
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify it's a draft
    let status = state
        .status()
        .await
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status != crate::core::state::Status::Draft {
        return Err("Can only update draft runs".to_string());
    }

    // Update spec
    if let Some(spec) = updates.spec {
        let spec_path = run_dir.join("spec.md");
        fs::write(&spec_path, &spec).map_err(|e| format!("Failed to write spec: {}", e))?;
        state
            .set_request(Some(&spec))
            .await
            .map_err(|e| format!("Failed to update request: {}", e))?;
    }

    // Update worker scale
    if let Some(scale) = updates.worker_scale {
        state
            .set_worker_scale(&scale)
            .await
            .map_err(|e| format!("Failed to update worker scale: {}", e))?;
    }

    // Update time limit
    if let Some(limit) = updates.time_limit_minutes {
        state
            .set_time_limit_minutes(Some(limit))
            .await
            .map_err(|e| format!("Failed to update time limit: {}", e))?;
    }

    // Update HITL
    if let Some(hitl) = updates.human_in_the_loop {
        state
            .set_human_in_the_loop(hitl)
            .await
            .map_err(|e| format!("Failed to update HITL: {}", e))?;
    }

    // Update project path
    if let Some(path) = updates.project_path {
        state
            .set_project_path(&path)
            .await
            .map_err(|e| format!("Failed to update project path: {}", e))?;
    }

    // Update branch
    if let Some(branch) = updates.branch {
        state
            .set_branch(Some(&branch))
            .await
            .map_err(|e| format!("Failed to update branch: {}", e))?;
    }

    // Update default runner
    if let Some(runner) = updates.runner {
        state
            .set_default_runner(Some(&runner))
            .await
            .map_err(|e| format!("Failed to update runner: {}", e))?;
    }

    // Update per-worker runner assignments
    if let Some(worker_runners) = updates.worker_runners {
        state
            .set_worker_runners(Some(&worker_runners))
            .await
            .map_err(|e| format!("Failed to update worker runners: {}", e))?;
    }

    // Handle rename if requested
    if let Some(new_name) = updates.name {
        if new_name != run_name {
            let new_run_dir = config::run_dir(&new_name);
            if new_run_dir.exists() {
                return Err(format!("Run '{}' already exists", new_name));
            }
            fs::rename(&run_dir, &new_run_dir)
                .map_err(|e| format!("Failed to rename run: {}", e))?;
        }
    }

    Ok(())
}

/// Change the starting point for a draft run
///
/// Deletes the existing workspace and re-initializes it with a new starting point.
/// Only works for drafts (not running or completed runs).
#[tauri::command]
pub async fn change_starting_point(
    run_name: String,
    starting_point: StartingPoint,
) -> Result<RunDetail, String> {
    use std::fs;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(&run_name)
        .await
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify it's a draft
    let status = state
        .status()
        .await
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status != crate::core::state::Status::Draft {
        return Err("Can only change starting point for draft runs".to_string());
    }

    // Delete existing workspace if it exists
    let workspace_dir = run_dir.join("workspace");
    if workspace_dir.exists() {
        fs::remove_dir_all(&workspace_dir)
            .map_err(|e| format!("Failed to remove existing workspace: {}", e))?;
    }

    // Clear workspace-related state fields
    state
        .clear_project_path()
        .await
        .map_err(|e| format!("Failed to clear project path: {}", e))?;
    state
        .set_branch(None)
        .await
        .map_err(|e| format!("Failed to clear branch: {}", e))?;

    // Initialize new workspace
    let workspace = create_workspace_provider(None);
    let workspace_info = workspace
        .init(&run_name, &starting_point)
        .await
        .map_err(|e| format!("Failed to initialize workspace: {}", e))?;

    // Update state with new workspace info
    state
        .set_project_path(workspace_info.path.to_str().unwrap_or("."))
        .await
        .map_err(|e| format!("Failed to set project path: {}", e))?;

    if let Some(ref branch) = workspace_info.default_branch {
        state
            .set_branch(Some(branch))
            .await
            .map_err(|e| format!("Failed to set branch: {}", e))?;
    }

    // Return updated run detail
    get_run_detail(run_name).await
}

/// Start a draft run
///
/// Creates the workspace based on the starting point, spawns workers,
/// and transitions the draft to a running state.
#[tauri::command]
pub async fn start_draft(
    run_name: String,
    starting_point: Option<StartingPoint>,
    profile: Option<String>,
) -> Result<RunDetail, String> {
    use crate::cli::config::get_agent_command;
    use crate::cli::helpers::WorkerScale;
    use crate::core::names::get_available_names;
    use crate::core::ops::{compute_multi_worker_config, setup_run_workspace, RunSetupConfig};
    use crate::core::runner::{create_runner, WorkerSpawnConfig as RunnerSpawnConfig};
    use crate::core::state::WorkerUpdate;

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state = SQLiteState::new(&run_name)
        .await
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify it's a draft
    let status = state
        .status()
        .await
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status != crate::core::state::Status::Draft {
        return Err("Can only start draft runs".to_string());
    }

    // Check if workspace already exists (e.g., from clone_run)
    let existing_workspace = state
        .get_project_path()
        .await
        .map_err(|e| format!("Failed to get project path: {}", e))?;

    let project_path = if let Some(ref path_str) = existing_workspace {
        // Workspace already exists
        let path = std::path::PathBuf::from(path_str);
        if !path.exists() {
            return Err(format!("Workspace does not exist: {}", path_str));
        }
        path
    } else {
        // Create workspace from starting point
        // Priority: 1. Passed starting_point, 2. Stored in database, 3. Default to Greenfield
        let sp = if let Some(sp) = starting_point {
            sp
        } else if let Ok(Some(sp_json)) = state.get_starting_point().await {
            serde_json::from_str::<StartingPoint>(&sp_json)
                .map_err(|e| format!("Failed to parse stored starting_point: {}", e))?
        } else {
            StartingPoint::Greenfield
        };

        let workspace = create_workspace_provider(profile.as_deref());
        let workspace_info = workspace
            .init(&run_name, &sp)
            .await
            .map_err(|e| format!("Failed to initialize workspace: {}", e))?;

        // Store workspace path in state
        state
            .set_project_path(workspace_info.path.to_str().unwrap_or("."))
            .await
            .map_err(|e| format!("Failed to set project path: {}", e))?;

        // Store starting_point in state (if not already stored)
        if state.get_starting_point().await.ok().flatten().is_none() {
            let sp_json = serde_json::to_string(&sp)
                .map_err(|e| format!("Failed to serialize starting_point: {}", e))?;
            state
                .set_starting_point(Some(&sp_json))
                .await
                .map_err(|e| format!("Failed to set starting_point: {}", e))?;
        }

        // Set branch if available from workspace init
        if let Some(ref branch) = workspace_info.default_branch {
            state
                .set_branch(Some(branch))
                .await
                .map_err(|e| format!("Failed to set branch: {}", e))?;
        }

        workspace_info.path
    };

    // Parse worker scale
    let worker_scale_str = state
        .get_worker_scale()
        .await
        .map_err(|e| format!("Failed to get worker scale: {}", e))?
        .unwrap_or_else(|| "1".to_string());
    let scale = WorkerScale::parse(&worker_scale_str)
        .map_err(|e| format!("Invalid worker scale: {}", e))?;

    // Get worker names
    let initial_count = scale.initial_count();
    let worker_names = get_available_names(initial_count, &[]);

    // Determine if multi-worker mode (current or potential via autoscale)
    let (is_multi_worker, leader) = compute_multi_worker_config(&worker_names, scale.max);

    // Load config for scribe docs settings
    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Store docs config in run state
    state
        .set_docs_path(Some(&global_config.scribe_docs_path))
        .await
        .map_err(|e| format!("Failed to set docs path: {}", e))?;
    state
        .set_persist_docs_changes(global_config.scribe_persist_docs_changes)
        .await
        .map_err(|e| format!("Failed to set persist_docs_changes: {}", e))?;

    // Set up workspace, worker clones, and chats using shared ops
    let setup_config = RunSetupConfig {
        run_name: run_name.clone(),
        project_path: project_path.clone(),
        run_dir: run_dir.clone(),
        worker_names: worker_names.clone(),
        additional_chat_workers: Vec::new(), // GUI only has local workers
        is_multi_worker,
        leader_name: leader.clone(),
        docs_path: global_config.scribe_docs_path.clone(),
    };

    let setup_result = setup_run_workspace(&setup_config).str_err()?;

    // Register workers in state with per-worker runner assignments
    for (worker_name, work_dir) in &setup_result.worker_dirs {
        let runner = state
            .get_runner_for_worker(worker_name)
            .await
            .map_err(|e| format!("Failed to get runner for {}: {}", worker_name, e))?;
        state
            .add_worker(worker_name, work_dir.to_str().unwrap_or("."), &runner)
            .await
            .map_err(|e| format!("Failed to register worker {}: {}", worker_name, e))?;
    }

    // Set status to working and start time tracking
    state
        .set_status(crate::core::state::Status::Working)
        .await
        .map_err(|e| format!("Failed to set status: {}", e))?;

    // Always set started_at when run starts (for elapsed time calculation)
    state
        .set_started_at(None)
        .await
        .map_err(|e| format!("Failed to set started_at: {}", e))?;

    // Save spec content to database for display in Specs tab
    let spec_path = run_dir.join("spec.md");
    if spec_path.exists() {
        if let Ok(spec_content) = std::fs::read_to_string(&spec_path) {
            let _ = state.set_request(Some(&spec_content)).await;
        }
    }

    // Load global config for runner definitions
    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Get agent command
    let agent_command = get_agent_command();

    // Spawn workers using the appropriate runner for each
    for (i, (worker_name, work_dir)) in setup_result.worker_dirs.iter().enumerate() {
        // Check if run was paused while spawning
        if state
            .status()
            .await
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

        // Get the runner name for this worker from run state
        let runner_name = state
            .get_runner_for_worker(worker_name)
            .await
            .map_err(|e| format!("Failed to get runner for {}: {}", worker_name, e))?;

        // Look up runner config from global config
        let runner_config = global_config.get_runner(&runner_name).unwrap_or_default();

        // Create the runner
        let runner = create_runner(&runner_config);

        // Build spawn config for the runner
        let spawn_config = RunnerSpawnConfig {
            run_name: run_name.clone(),
            worker_name: worker_name.clone(),
            work_dir: work_dir.clone(),
            run_dir: run_dir.clone(),
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
            Ok(result) => {
                // Update worker with PID
                if let Some(pid) = result.pid {
                    let _ = state
                        .update_worker(
                            worker_name,
                            WorkerUpdate {
                                pid: Some(pid as i64),
                                ..Default::default()
                            },
                        )
                        .await;
                }
                tracing::info!(
                    "Spawned worker {} on runner {} (type: {})",
                    worker_name,
                    runner_name,
                    result.handle.runner_type
                );
            }
            Err(e) => {
                tracing::error!(
                    "Failed to spawn worker {} on runner {}: {}",
                    worker_name,
                    runner_name,
                    e
                );
                // Continue with other workers
            }
        }
    }

    // Return updated run detail
    get_run_detail(run_name).await
}
