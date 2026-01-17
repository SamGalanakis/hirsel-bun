//! Draft-related commands
//!
//! Commands for managing drafts: creating, updating, validating repos, and starting runs.

use super::runs::get_run_detail;
use super::types::{DraftUpdateRequest, RepoValidation, RunDetail, RunStatus};
use crate::core::names::generate_run_name;
use crate::core::ops::{clone_run as ops_clone_run, CloneRunConfig};
use crate::core::{config, state::SQLiteState};

/// Validate a repository path or URL
///
/// Checks if the path/URL is valid, extracts branch information from URLs,
/// and lists available branches in the repository.
#[tauri::command]
pub async fn validate_repo(path: String) -> Result<RepoValidation, String> {
    use crate::core::git::{
        get_current_branch, get_repo, is_remote_url, list_branches, list_remote_branches,
        parse_github_url,
    };

    let trimmed = path.trim();

    if trimmed.is_empty() {
        return Ok(RepoValidation {
            valid: false,
            error: Some("Path is empty".to_string()),
            is_remote: false,
            branches: vec![],
            current_branch: None,
            repo_url: String::new(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        });
    }

    let is_remote = is_remote_url(trimmed);

    if is_remote {
        // Parse the URL to extract potential branch
        let parsed = parse_github_url(trimmed);

        // Try to list remote branches
        match list_remote_branches(&parsed.repo_url) {
            Ok(branches) => {
                // Check if URL branch exists
                let url_branch_valid = if let Some(ref branch) = parsed.branch {
                    branches.iter().any(|b| b == branch)
                } else {
                    true // No branch specified is valid
                };

                // If branch was specified but doesn't exist, return error
                if parsed.branch.is_some() && !url_branch_valid {
                    return Ok(RepoValidation {
                        valid: false,
                        error: Some(format!(
                            "Branch '{}' not found in repository",
                            parsed.branch.as_ref().unwrap()
                        )),
                        is_remote: true,
                        branches,
                        current_branch: None,
                        repo_url: parsed.repo_url,
                        url_branch: parsed.branch,
                        url_branch_valid: false,
                        needs_dir_create: false,
                        needs_git_init: false,
                    });
                }

                Ok(RepoValidation {
                    valid: true,
                    error: None,
                    is_remote: true,
                    branches,
                    current_branch: None,
                    repo_url: parsed.repo_url,
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
                branches: vec![],
                current_branch: None,
                repo_url: parsed.repo_url,
                url_branch: parsed.branch,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: false,
            }),
        }
    } else {
        // Local path
        let path = std::path::Path::new(trimmed);

        // Check if directory needs to be created
        let needs_dir_create = !path.exists();

        // Check if git needs to be initialized (directory exists but no .git)
        // We check for .git directly, not whether it's inside a git repo
        let needs_git_init = !needs_dir_create && !path.join(".git").exists();

        // If needs setup, return with flags but valid=false
        if needs_dir_create || needs_git_init {
            return Ok(RepoValidation {
                valid: false,
                error: None, // No error - just needs setup
                is_remote: false,
                branches: vec![],
                current_branch: None,
                repo_url: trimmed.to_string(),
                url_branch: None,
                url_branch_valid: false,
                needs_dir_create,
                needs_git_init,
            });
        }

        // Check if it's a git repository (should always succeed now since we checked .git exists)
        if get_repo(Some(path)).is_err() {
            return Ok(RepoValidation {
                valid: false,
                error: Some("Failed to open git repository".to_string()),
                is_remote: false,
                branches: vec![],
                current_branch: None,
                repo_url: trimmed.to_string(),
                url_branch: None,
                url_branch_valid: false,
                needs_dir_create: false,
                needs_git_init: false,
            });
        }

        // Get branches and current branch
        let branches = list_branches(path).unwrap_or_default();
        let current_branch = get_current_branch(path).ok();

        Ok(RepoValidation {
            valid: true,
            error: None,
            is_remote: false,
            branches,
            current_branch,
            repo_url: trimmed.to_string(),
            url_branch: None,
            url_branch_valid: false,
            needs_dir_create: false,
            needs_git_init: false,
        })
    }
}

/// Initialize a project directory for use with Hirsel
///
/// Creates the directory if it doesn't exist and initializes a git repository
/// if needed. Returns updated validation info after setup.
#[tauri::command]
pub async fn init_project_repo(path: String) -> Result<RepoValidation, String> {
    use crate::core::git::{get_current_branch, get_repo, list_branches};
    use crate::core::ops::ensure_project_directory;

    let trimmed = path.trim();
    let path = std::path::Path::new(trimmed);

    // Use the shared ops implementation to create dir and init git
    ensure_project_directory(path).map_err(|e| e.to_string())?;

    // Verify git repo is now valid
    if get_repo(Some(path)).is_err() {
        return Err("Failed to initialize git repository".to_string());
    }

    // Get branches and current branch
    let branches = list_branches(path).unwrap_or_default();
    let current_branch = get_current_branch(path).ok();

    Ok(RepoValidation {
        valid: true,
        error: None,
        is_remote: false,
        branches,
        current_branch,
        repo_url: trimmed.to_string(),
        url_branch: None,
        url_branch_valid: false,
        needs_dir_create: false,
        needs_git_init: false,
    })
}

/// Create a new draft run
///
/// Creates a draft run with a random friendly name. The draft can be configured
/// before being started. No workers are spawned until start_draft is called.
#[tauri::command]
pub async fn create_draft(project_path: Option<String>) -> Result<RunDetail, String> {
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
        "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Read spec, create exploration tasks |\n",
    ).map_err(|e| format!("Failed to create tasks file: {}", e))?;

    // Initialize database
    let db_path = run_dir.join("hirsel.db");
    let state =
        SQLiteState::new(db_path).map_err(|e| format!("Failed to create database: {}", e))?;

    // Initialize state with Draft status
    state
        .init_state(project_path.as_deref())
        .map_err(|e| format!("Failed to init state: {}", e))?;
    state
        .set_status(crate::core::state::Status::Draft)
        .map_err(|e| format!("Failed to set draft status: {}", e))?;

    // Set defaults
    state
        .set_worker_scale("1")
        .map_err(|e| format!("Failed to set worker scale: {}", e))?;
    state
        .set_human_in_the_loop(true)
        .map_err(|e| format!("Failed to set HITL: {}", e))?;

    // Add scope task
    let _ = state.add_task("scope", "Read spec, create exploration tasks", None, None);

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
        project_path,
        remote_url: None,
        branch: None,
        worker_scale: Some("1".to_string()),
        time_limit_minutes: None,
        started_at: None,
        summary: None,
        created_at: created_at.clone(),
        updated_at: created_at,
        iteration_count: 0,
        max_iterations: None,
        human_in_the_loop: true,
        waiting_reason: None,
        unread_count: 0,
        tasks_done: 0,
        tasks_total: 1,
        workers_active: 0,
        workers_total: 0,
        elapsed_minutes: 0.0,
        learnings_count: 0,
        learnings_processed_at: None,
        agent_type: format!("{:?}", agent_type).to_lowercase(),
        metrics_available,
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
    let result = ops_clone_run(config).map_err(|e| e.to_string())?;

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
        max_iterations: result.max_iterations.map(|m| m as u32),
        human_in_the_loop: result.human_in_the_loop,
        waiting_reason: None,
        unread_count: 0,
        tasks_done: 0,
        tasks_total: 1,
        workers_active: 0,
        workers_total: 0,
        elapsed_minutes: 0.0,
        learnings_count: 0,
        learnings_processed_at: None,
        agent_type: format!("{:?}", agent_type).to_lowercase(),
        metrics_available,
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

    let state = SQLiteState::new(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify it's a draft
    let status = state
        .status()
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
            .map_err(|e| format!("Failed to update request: {}", e))?;
    }

    // Update worker scale
    if let Some(scale) = updates.worker_scale {
        state
            .set_worker_scale(&scale)
            .map_err(|e| format!("Failed to update worker scale: {}", e))?;
    }

    // Update time limit
    if let Some(limit) = updates.time_limit_minutes {
        state
            .set_time_limit_minutes(Some(limit))
            .map_err(|e| format!("Failed to update time limit: {}", e))?;
    }

    // Update HITL
    if let Some(hitl) = updates.human_in_the_loop {
        state
            .set_human_in_the_loop(hitl)
            .map_err(|e| format!("Failed to update HITL: {}", e))?;
    }

    // Update project path
    if let Some(path) = updates.project_path {
        state
            .set_project_path(&path)
            .map_err(|e| format!("Failed to update project path: {}", e))?;
    }

    // Update branch
    if let Some(branch) = updates.branch {
        state
            .set_branch(Some(&branch))
            .map_err(|e| format!("Failed to update branch: {}", e))?;
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

/// Start a draft run
///
/// Spawns workers and transitions the draft to a running state.
/// The draft must have a project path set.
#[tauri::command]
pub async fn start_draft(run_name: String) -> Result<RunDetail, String> {
    use crate::cli::config::get_agent_command;
    use crate::cli::go::{get_available_names, WorkerScale};
    use crate::core::git::{
        checkout_branch_at_path, clone_remote_with_branch, get_repo_root, is_remote_url,
    };
    use crate::core::ops::{
        compute_multi_worker_config, register_workers, setup_run_workspace, spawn_local_workers,
        RunSetupConfig, SpawnWorkersConfig,
    };

    let run_dir = config::run_dir(&run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return Err(format!("Run '{}' not found", run_name));
    }

    let state =
        SQLiteState::new(db_path.clone()).map_err(|e| format!("Failed to open database: {}", e))?;

    // Verify it's a draft
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status != crate::core::state::Status::Draft {
        return Err("Can only start draft runs".to_string());
    }

    // Get project path - required for starting
    let project_path_str = state
        .get_project_path()
        .map_err(|e| format!("Failed to get project path: {}", e))?
        .ok_or_else(|| "Project path is required to start a run".to_string())?;

    // Get selected branch (optional - will use default if not set)
    let selected_branch = state
        .get_branch()
        .map_err(|e| format!("Failed to get branch: {}", e))?;

    // Handle remote URLs - clone to local directory
    let project_path = if is_remote_url(&project_path_str) {
        // Clone remote repo to run directory, checking out selected branch
        let clone_dir = run_dir.join("repo");
        let local_path =
            clone_remote_with_branch(&project_path_str, &clone_dir, selected_branch.as_deref())
                .map_err(|e| format!("Failed to clone remote repository: {}", e))?;

        // Store the remote URL for delivery
        state
            .set_remote_url(Some(&project_path_str))
            .map_err(|e| format!("Failed to store remote URL: {}", e))?;

        // Update project_path to local clone
        state
            .set_project_path(local_path.to_str().unwrap_or(&project_path_str))
            .map_err(|e| format!("Failed to update project path: {}", e))?;

        local_path
    } else {
        let project_path = std::path::PathBuf::from(&project_path_str);

        if !project_path.exists() {
            return Err(format!("Project path does not exist: {}", project_path_str));
        }

        // Verify project is a git repo
        let repo_root = get_repo_root(Some(&project_path))
            .map_err(|_| format!("Project path is not a git repository: {}", project_path_str))?;

        // Checkout selected branch in local repo if specified
        if let Some(ref branch) = selected_branch {
            checkout_branch_at_path(&repo_root, branch)
                .map_err(|e| format!("Failed to checkout branch '{}': {}", branch, e))?;
        }

        repo_root
    };

    // Parse worker scale
    let worker_scale_str = state
        .get_worker_scale()
        .map_err(|e| format!("Failed to get worker scale: {}", e))?
        .unwrap_or_else(|| "1".to_string());
    let scale = WorkerScale::parse(&worker_scale_str)
        .map_err(|e| format!("Invalid worker scale: {}", e))?;

    // Get worker names
    let initial_count = scale.initial_count();
    let worker_names = get_available_names(initial_count, &[]);

    // Determine if multi-worker mode (current or potential via autoscale)
    let (is_multi_worker, leader) = compute_multi_worker_config(&worker_names, scale.max);

    // Set up workspace, worker clones, and chats using shared ops
    let setup_config = RunSetupConfig {
        run_name: run_name.clone(),
        project_path: project_path.clone(),
        run_dir: run_dir.clone(),
        worker_names: worker_names.clone(),
        additional_chat_workers: Vec::new(), // GUI only has local workers
        is_multi_worker,
        leader_name: leader.clone(),
    };

    let setup_result = setup_run_workspace(&setup_config).map_err(|e| e.to_string())?;

    // Register workers in state
    register_workers(&state, &setup_result.worker_dirs, "local").map_err(|e| e.to_string())?;

    // Pre-claim scope for first worker
    let first_worker = &worker_names[0];
    let _ = state.claim_task("scope", first_worker);

    // Set status to working and start time tracking
    state
        .set_status(crate::core::state::Status::Working)
        .map_err(|e| format!("Failed to set status: {}", e))?;

    // Set started_at if time limit is set
    if state.get_time_limit_minutes().ok().flatten().is_some() {
        state
            .set_started_at(None)
            .map_err(|e| format!("Failed to set started_at: {}", e))?;
    }

    // Save spec content to database for display in Specs tab
    let spec_path = run_dir.join("spec.md");
    if spec_path.exists() {
        if let Ok(spec_content) = std::fs::read_to_string(&spec_path) {
            let _ = state.set_request(Some(&spec_content));
        }
    }

    // Spawn worker processes using shared ops
    let agent_command = get_agent_command();
    let spawn_config = SpawnWorkersConfig {
        run_name: run_name.clone(),
        run_dir: run_dir.clone(),
        spec_path,
        agent_command,
        is_multi_worker,
        leader_name: leader,
        all_worker_names: worker_names,
    };

    spawn_local_workers(&spawn_config, &setup_result.worker_dirs, &state);

    // Return updated run detail
    get_run_detail(run_name).await
}
