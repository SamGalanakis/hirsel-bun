//! Local orchestrator implementation
//!
//! Implements the Orchestrator trait using direct local state access.
//! This wraps the existing GUI command logic into the orchestrator interface.

use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;

use super::{
    CreateRunRequest, CreateRunResponse, HealthResponse, Orchestrator, OrchestratorError,
    OrchestratorResult, SpawnWorkersResponse, StartRunRequest, TailscaleOAuth, WorkerStateHandle,
};
use crate::core::api_types::{
    calculate_duration_minutes, convert_status, is_completed_status, parse_elapsed_minutes,
    ConfigResponse, Eval, EvalStatus, HistoryEntry, Message, RunDetail, RunSummary, SheepConfig,
    ThreadSummary, Worker, WorkerEventResponse, WorkerEventsResponse, WorkerLocation, WorkerStatus,
};
use crate::core::config::{self, Config};
use crate::core::delta::{DeltaState, LiveNodeStatus, NodeType};
use crate::core::draft::create_workspace_provider;
use crate::core::names::{get_available_names, slugify};
use crate::core::ops::{
    compute_multi_worker_config, register_workers, setup_run_workspace, RunSetupConfig,
};
use crate::core::project::ProjectStore;
use crate::core::runner::{create_runner, Runner, WorkerSpawnConfig as RunnerSpawnConfig};
use crate::core::state::{SQLiteState, Status, WorkerUpdate};
use crate::core::Files;

/// Generate the content for the scope task.
///
/// This is written to `tasks/scope.md` and includes all leader responsibilities
/// and task planning guidance. Only the worker who gets the scope task sees this.
fn generate_scope_task_content(is_multi_worker: bool) -> String {
    let mut content = String::new();

    content.push_str("# Scope Task\n\n");
    content.push_str("You are the **leader** for this run. Your job is to review the task tree, understand the work, and unblock other tasks.\n\n");

    // Main approaches
    content.push_str("## Your Approach\n\n");
    content.push_str("Review the task tree with `get_task_tree()` and decide:\n\n");

    content.push_str("**1. Explore first** - If unfamiliar with codebase:\n");
    content.push_str("   - Create exploration tasks to understand the code\n");
    content.push_str("   - Use `scribe()` to record findings\n");
    content.push_str("   - Create implementation tasks after exploration\n\n");

    content.push_str("**2. Plan more** - If tasks need breakdown:\n");
    content.push_str("   - Create subtasks for large tasks\n");
    content.push_str("   - Add blocking relationships where needed\n\n");

    content.push_str("**3. Start directly** - If tasks are well-defined:\n");
    content.push_str("   - Complete this scope task to unblock other tasks\n");
    content.push_str("   - Begin working on available tasks\n\n");

    content.push_str("When you complete this task, blocked tasks become available for you (and teammates if multi-worker).\n\n");

    // Task design principles
    content.push_str("## Task Design Principles\n\n");
    content.push_str("**Parallel execution:**\n");
    content.push_str("- Minimize dependencies between tasks\n");
    content.push_str("- Prefer vertical slices (complete features) over horizontal layers\n");
    content.push_str("- Tasks touching same files = conflicts. Structure to minimize overlap.\n\n");
    content.push_str("**Task ordering:**\n");
    content.push_str("- Tackle unknowns (spikes) before mechanical work\n");
    content.push_str("- A failed spike might restructure the whole plan\n\n");
    content.push_str("**Dependencies (blocked_by):**\n");
    content.push_str("When in doubt, add the dependency. Better slow than broken:\n");
    content.push_str("- Task reads files another writes? → Add dependency\n");
    content.push_str("- Task calls functions another creates? → Add dependency\n");
    content.push_str("- Task tests code another implements? → Add dependency\n\n");

    // Multi-worker coordination
    if is_multi_worker {
        content.push_str("## Team Leadership\n\n");
        content
            .push_str("You are leading a team. Check `list_contacts()` to see your teammates.\n\n");

        content.push_str("**Your responsibilities:**\n");
        content.push_str("- Design tasks to minimize conflicts (different files per task)\n");
        content.push_str("- Use `group` thread to coordinate with teammates\n");
        content.push_str("- Announce major decisions in group chat\n");
        content.push_str("- When creating tasks, consider which can be parallelized\n\n");

        content.push_str("**Group Chat:**\n");
        content.push_str("Use `chat_send(\"group\", ...)` to coordinate:\n");
        content.push_str("- When tasks are ready for claiming\n");
        content.push_str("- When changing shared code (utils, models, configs)\n");
        content.push_str("- When discovering patterns others should follow\n\n");
    }

    // Completion
    content.push_str("## Completing This Task\n\n");
    content.push_str("When you've:\n");
    content.push_str("1. Reviewed the existing task tree\n");
    content.push_str("2. Created any needed exploration/planning tasks\n");
    content.push_str("3. Set up proper blocking relationships\n\n");
    content
        .push_str("Call `work_done()` to complete the scope task and unblock dependent tasks.\n");

    content
}

/// Get the coordinator's Tailscale hostname if connected to a tailnet.
///
/// Returns the DNS name (e.g., "my-machine.tailnet-name.ts.net") that workers
/// can use to reach this coordinator via Tailscale.
fn get_tailscale_hostname() -> Option<String> {
    std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| {
            v["Self"]["DNSName"]
                .as_str()
                .map(|s| s.trim_end_matches('.').to_string())
        })
}

/// Local orchestrator that operates directly on the local filesystem
pub struct LocalOrchestrator {
    config: Config,
}

impl LocalOrchestrator {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Get a reference to the config
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get state for a run, opening the SQLite database
    fn get_state(&self, run_name: &str) -> OrchestratorResult<SQLiteState> {
        let db_path = self.config.runs_dir().join(run_name).join("hirsel.db");
        if !db_path.exists() {
            return Err(OrchestratorError::RunNotFound(run_name.to_string()));
        }
        SQLiteState::new(db_path).map_err(|e| OrchestratorError::State(e.to_string()))
    }

    /// Convert core worker to GUI worker type
    fn convert_worker(
        &self,
        w: &crate::core::state::Worker,
        live_nodes: &[crate::core::delta::LiveNode],
    ) -> Worker {
        use crate::core::metrics;

        let status = match w.status {
            crate::core::state::WorkerStatus::Working => WorkerStatus::Working,
            crate::core::state::WorkerStatus::Awaiting => WorkerStatus::Awaiting,
            crate::core::state::WorkerStatus::Paused => WorkerStatus::Paused,
            crate::core::state::WorkerStatus::Error => WorkerStatus::Error,
        };

        let location = match w.location.as_str() {
            "remote" => WorkerLocation::Remote,
            _ => WorkerLocation::Local,
        };

        // Find current task for this worker from live nodes
        let current_task = live_nodes
            .iter()
            .find(|n| {
                n.claimed_by.as_deref() == Some(&w.name) && n.status == LiveNodeStatus::Working
            })
            .map(|n| n.name.clone());

        let is_leader = w.id == 1;

        // Get session metrics for this worker
        let session_metrics =
            metrics::get_session_metrics(w.session_id.as_deref(), w.work_dir.as_deref());

        Worker {
            id: w.id as u32,
            name: w.name.clone(),
            pid: w.pid.map(|p| p as u32),
            session_id: w.session_id.clone(),
            status,
            work_dir: w.work_dir.clone(),
            waiting_thread: w.waiting_thread.clone(),
            location,
            last_heartbeat: w.last_heartbeat.clone(),
            created_at: w.created_at.clone(),
            needs_restart: w.needs_restart,
            session_started_at: w.session_started_at.clone(),
            hitl_waiting: w.hitl_waiting,
            is_leader,
            context_utilization: session_metrics.context_utilization,
            input_tokens: Some(session_metrics.input_tokens),
            output_tokens: Some(session_metrics.output_tokens),
            turns: Some(session_metrics.turns),
            current_task,
            sheep_config: SheepConfig::from_name(&w.name, is_leader),
        }
    }

    /// Convert core eval to GUI eval type
    fn convert_eval(&self, e: &crate::core::state::Eval) -> Eval {
        let status = match e.status {
            crate::core::state::EvalStatus::Running => EvalStatus::Running,
            crate::core::state::EvalStatus::Passed => EvalStatus::Passed,
            crate::core::state::EvalStatus::Failed => EvalStatus::Failed,
        };

        Eval {
            id: e.id as u32,
            branch: e.branch.clone(),
            eval_name: e.eval_name.clone(),
            status,
            feedback: e.feedback.clone(),
            log_file: e.log_file.clone(),
            started_at: e.started_at.clone(),
            finished_at: e.finished_at.clone(),
            sheep_config: SheepConfig::for_eval(e.id as u32),
        }
    }
}

#[async_trait]
impl Orchestrator for LocalOrchestrator {
    // -------------------------------------------------------------------------
    // Run Management
    // -------------------------------------------------------------------------

    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>> {
        let run_names = config::list_runs().map_err(|e| OrchestratorError::Other(e.to_string()))?;
        let mut runs = Vec::new();

        for name in run_names {
            let db_path = self.config.runs_dir().join(&name).join("hirsel.db");
            if !db_path.exists() {
                continue;
            }

            let state = match SQLiteState::new(db_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let summary = match state.get_run_summary() {
                Ok(s) => s,
                Err(_) => continue,
            };

            let run_status = convert_status(summary.status);

            // For completed runs, recalculate elapsed as duration
            let elapsed_minutes = if is_completed_status(&run_status) {
                let start = summary
                    .started_at
                    .as_deref()
                    .or(summary.created_at.as_deref());
                if let (Some(start), Some(end)) = (start, summary.updated_at.as_deref()) {
                    calculate_duration_minutes(start, end)
                } else {
                    summary.elapsed_minutes
                }
            } else {
                summary.elapsed_minutes
            };

            let created_at = summary
                .created_at
                .unwrap_or_else(|| Utc::now().to_rfc3339());

            runs.push(RunSummary {
                name,
                status: run_status,
                tasks_done: summary.tasks_done,
                tasks_total: summary.tasks_total,
                workers_active: summary.workers_active,
                workers_total: summary.workers_total,
                elapsed_minutes,
                time_limit_minutes: summary.time_limit_minutes.map(|m| m as u32),
                has_unread_messages: summary.unread_count > 0,
                created_at,
                project_id: state.get_project_id().ok().flatten(),
                project_name: state.get_project_name().ok().flatten(),
            });
        }

        // Sort by created_at descending (newest first)
        runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(runs)
    }

    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail> {
        let state = self.get_state(name)?;

        let status = state.status().unwrap_or(crate::core::state::Status::Draft);
        let run_status = convert_status(status);

        let request = state.get_request().ok().flatten();
        let project_path = state.get_project_path().ok().flatten();
        let worker_scale = state.get_worker_scale().ok().flatten();
        let time_limit_minutes = state
            .get_time_limit_minutes()
            .ok()
            .flatten()
            .map(|m| m as u32);
        let started_at = state.get_started_at().ok().flatten();
        let summary = state.get_summary().ok().flatten();
        let created_at = state
            .get_created_at()
            .ok()
            .flatten()
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let iteration_count = state.get_iteration_count().unwrap_or(0) as u32;
        let human_in_the_loop = state.get_human_in_the_loop().unwrap_or(true);
        let waiting_reason = state.get_waiting_reason().ok().flatten();
        let unread_count = state.get_unread_count().unwrap_or(0) as u32;

        // Get task counts from live nodes (project runs)
        let (tasks_done, tasks_total) =
            if let Some(project_id) = state.get_project_id().ok().flatten() {
                let delta_state = DeltaState::new(project_id);
                if let Ok(nodes) = delta_state.get_live_nodes() {
                    let done = nodes.iter().filter(|n| n.status.is_complete()).count() as u32;
                    (done, nodes.len() as u32)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };

        let workers = state.get_workers().unwrap_or_default();
        let workers_active = workers
            .iter()
            .filter(|w| w.status == crate::core::state::WorkerStatus::Working)
            .count() as u32;
        // workers_total is the max scale (for autoscaling display like "1/2"), fallback to actual count
        let workers_total = worker_scale
            .as_ref()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(workers.len() as u32);

        // Calculate elapsed minutes
        let elapsed_minutes = if let Ok(Some(time_info)) = state.get_time_info() {
            time_info.elapsed_minutes
        } else if let Some(ref sa) = started_at {
            parse_elapsed_minutes(sa)
        } else {
            parse_elapsed_minutes(&created_at)
        };

        let remote_url = state.get_remote_url().ok().flatten();
        let branch = state.get_branch().ok().flatten();

        let agent_type = self.config.agent.agent_type();
        let metrics_available = agent_type.supports_context_tracking();

        // Get runner configuration
        let runner = state.get_default_runner().ok().flatten();
        let worker_runners = state.get_worker_runners().ok().flatten();

        Ok(RunDetail {
            name: name.to_string(),
            status: run_status,
            request,
            project_path,
            remote_url,
            branch,
            worker_scale,
            time_limit_minutes,
            started_at,
            summary,
            created_at: created_at.clone(),
            updated_at: created_at,
            iteration_count,
            human_in_the_loop,
            waiting_reason,
            unread_count,
            tasks_done,
            tasks_total,
            workers_active,
            workers_total,
            elapsed_minutes,
            agent_type: format!("{:?}", agent_type).to_lowercase(),
            metrics_available,
            runner,
            worker_runners,
            project_id: state.get_project_id().ok().flatten(),
            project_name: state.get_project_name().ok().flatten(),
        })
    }

    async fn delete_run(&self, name: &str) -> OrchestratorResult<()> {
        use crate::core::ops::{delete_run as ops_delete_run, DeleteRunConfig};

        let config = DeleteRunConfig::new(name);
        ops_delete_run(config)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn pause_run(&self, name: &str) -> OrchestratorResult<()> {
        use crate::cli::config::get_agent_command;
        use crate::core::lifecycle::{LifecycleManager, LocalLifecycleManager};

        let run_dir = config::run_dir(name);
        let agent_command = get_agent_command();

        // Create lifecycle manager and delegate
        let lifecycle = LocalLifecycleManager::new(name, run_dir, agent_command)
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;

        lifecycle
            .pause_run("User requested pause")
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;

        Ok(())
    }

    async fn resume_run(
        &self,
        name: &str,
        _time_limit_minutes: Option<u32>,
    ) -> OrchestratorResult<()> {
        use crate::cli::config::get_agent_command;
        use crate::core::lifecycle::{LifecycleAction, LifecycleManager, LocalLifecycleManager};

        let run_dir = config::run_dir(name);
        let agent_command = get_agent_command();

        // Create lifecycle manager and delegate
        let lifecycle = LocalLifecycleManager::new(name, run_dir.clone(), agent_command)
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;

        let actions = lifecycle
            .resume_run()
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;

        // Process the returned actions to actually resume workers
        for action in actions {
            match action {
                LifecycleAction::ResumeWorker {
                    worker_name,
                    work_dir,
                    resume_session_id,
                    state_handle,
                } => {
                    tracing::info!("Resuming worker '{}' for run '{}'", worker_name, name);

                    if let Err(e) = self
                        .resume_worker(
                            name,
                            &worker_name,
                            &work_dir,
                            resume_session_id.as_deref(),
                            state_handle.as_ref(),
                        )
                        .await
                    {
                        tracing::warn!(
                            "Failed to resume worker '{}' for run '{}': {}",
                            worker_name,
                            name,
                            e
                        );
                    }
                }
                LifecycleAction::SpawnWorker {
                    worker_name,
                    work_dir,
                    assigned_task_id: _,
                } => {
                    tracing::info!("Spawning worker '{}' for run '{}'", worker_name, name);

                    if let Err(e) = self
                        .spawn_single_worker(name, &worker_name, &work_dir, None)
                        .await
                    {
                        tracing::warn!(
                            "Failed to spawn worker '{}' for run '{}': {}",
                            worker_name,
                            name,
                            e
                        );
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn deliver_run(&self, name: &str, branch: Option<String>) -> OrchestratorResult<String> {
        use crate::core::git;

        let run_dir = config::run_dir(name);
        let state = self.get_state(name)?;

        // Get project path and remote URL
        let project_path_str = state
            .get_project_path()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        let remote_url = state
            .get_remote_url()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        let saved_branch = state
            .get_branch()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let project_path = project_path_str
            .as_ref()
            .map(std::path::PathBuf::from)
            .filter(|p| p.exists())
            .ok_or_else(|| {
                OrchestratorError::Other(format!("Project path not found for run '{}'", name))
            })?;

        // Find work directory
        let work_dir = run_dir.join("work").join("staging");
        let work_dir = if work_dir.exists() {
            work_dir
        } else {
            let fallback = run_dir.join("work");
            if fallback.exists() {
                fallback
            } else {
                return Err(OrchestratorError::Other(format!(
                    "Work directory not found for run '{}'",
                    name
                )));
            }
        };

        if !work_dir.join(".git").exists() {
            return Err(OrchestratorError::Other(
                "No git repository found in work directory".into(),
            ));
        }

        // Deliver docs (restore or persist based on settings)
        let docs_path = state
            .get_docs_path()
            .map_err(|e| OrchestratorError::State(e.to_string()))?
            .unwrap_or_else(|| "docs".to_string());
        let persist_docs = state
            .get_persist_docs_changes()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let docs_config = crate::core::ops::DocsDeliveryConfig {
            workspace_dir: &work_dir,
            run_dir: &run_dir,
            docs_path: &docs_path,
            persist: persist_docs,
        };
        crate::core::ops::deliver_docs(&docs_config)
            .map_err(|e| OrchestratorError::Other(format!("Failed to deliver docs: {}", e)))?;

        // Check for unmerged branches
        let unmerged = git::list_unmerged_branches(&work_dir)
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;
        if !unmerged.is_empty() {
            return Err(OrchestratorError::InvalidOperation(format!(
                "Unmerged branches exist: {}. All work must be merged to 'staging' before delivering.",
                unmerged.join(", ")
            )));
        }

        // Determine branch name
        let branch = branch
            .or(saved_branch)
            .unwrap_or_else(|| format!("hirsel/{}", name));

        // Deliver based on whether it's a remote or local repo
        let (success, message) = if let Some(ref url) = remote_url {
            git::push_to_remote(&work_dir, url, &branch)
                .map_err(|e| OrchestratorError::Other(e.to_string()))?
        } else {
            if git::branch_exists(&branch, Some(&project_path))
                .map_err(|e| OrchestratorError::Other(e.to_string()))?
            {
                return Err(OrchestratorError::InvalidOperation(format!(
                    "Branch '{}' already exists in project repository",
                    branch
                )));
            }

            git::push_staging_as_branch(&work_dir, &project_path, &branch)
                .map_err(|e| OrchestratorError::Other(e.to_string()))?
        };

        if !success {
            return Err(OrchestratorError::Other(format!(
                "Delivery failed: {}",
                message
            )));
        }

        // Update run status to delivered
        state
            .set_status(crate::core::state::Status::Delivered)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        Ok(branch)
    }

    // -------------------------------------------------------------------------
    // Workers
    // -------------------------------------------------------------------------

    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>> {
        let state = self.get_state(run)?;

        let core_workers = state
            .get_workers()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        // Get live nodes from project if available
        let live_nodes = if let Some(project_id) = state.get_project_id().ok().flatten() {
            DeltaState::new(project_id)
                .get_live_nodes()
                .unwrap_or_default()
        } else {
            vec![]
        };

        let workers = core_workers
            .iter()
            .map(|w| self.convert_worker(w, &live_nodes))
            .collect();

        Ok(workers)
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> OrchestratorResult<()> {
        use crate::cli::config::get_agent_command;
        use crate::core::state::WorkerUpdate;
        use crate::core::workers::{is_pid_alive, spawn_worker, WorkerSpawnConfig};

        let run_dir = config::run_dir(run);
        let state = self.get_state(run)?;

        // Find the worker by name
        let workers = state
            .get_workers()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let worker_data = workers
            .iter()
            .find(|w| w.name == worker)
            .ok_or_else(|| OrchestratorError::WorkerNotFound(worker.to_string()))?
            .clone();

        // Kill the process if it's running
        if let Some(pid) = worker_data.pid {
            if is_pid_alive(pid as u32) {
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        // Clear PID before restarting
        state
            .update_worker(
                &worker_data.name,
                WorkerUpdate {
                    pid: None,
                    ..Default::default()
                },
            )
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        // Get work directory
        let work_dir = worker_data
            .work_dir
            .as_ref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| run_dir.join("work").join(&worker_data.name));

        // Determine if multi-worker mode
        let is_multi_worker = workers.len() > 1;
        let leader_name = workers.first().map(|w| w.name.clone());
        let teammates: Vec<String> = workers
            .iter()
            .filter(|w| w.name != worker_data.name)
            .map(|w| w.name.clone())
            .collect();

        // Spawn the worker
        let agent_command = get_agent_command();
        let config = WorkerSpawnConfig {
            run_name: run.to_string(),
            worker_name: worker_data.name.clone(),
            work_dir,
            run_dir: run_dir.clone(),
            agent_command,
            is_leader: worker_data.id == 1,
            leader_name,
            teammates: if is_multi_worker {
                Some(teammates)
            } else {
                None
            },
            resume_session_id: worker_data.session_id.clone(),
            env_vars: None,
            credentials: None,
            coordinator_url: None,
            tailscale_authkey: None,
            assigned_task_id: worker_data.assigned_task_id.clone(),
        };

        spawn_worker(config, &state).map_err(|e| OrchestratorError::Other(e.to_string()))?;

        Ok(())
    }

    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> OrchestratorResult<WorkerEventsResponse> {
        let state = self.get_state(run)?;

        let limit = limit.unwrap_or(1000);
        let events = state
            .get_worker_events(worker, after_id, limit)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let last_id = events.last().map(|e| e.id);

        // Get worker status to determine if still streaming
        let worker_status = state
            .get_worker(worker)
            .ok()
            .flatten()
            .map(|w| w.status.as_str().to_string());

        let events: Vec<WorkerEventResponse> = events
            .into_iter()
            .map(|e| WorkerEventResponse {
                id: e.id,
                worker_name: e.worker_name,
                event_type: e.event_type.as_str().to_string(),
                timestamp: e.timestamp,
                content: e.content,
                tool_call_id: e.tool_call_id,
                tool_title: e.tool_title,
                tool_kind: e.tool_kind,
                tool_status: e.tool_status.map(|s| s.as_str().to_string()),
                tool_input: e.tool_input,
                tool_output: e.tool_output,
            })
            .collect();

        Ok(WorkerEventsResponse {
            events,
            last_id,
            worker_status,
        })
    }

    // -------------------------------------------------------------------------
    // Messages
    // -------------------------------------------------------------------------

    async fn list_threads(&self, run: &str) -> OrchestratorResult<Vec<ThreadSummary>> {
        let state = self.get_state(run)?;

        let thread_names = state
            .get_threads()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let mut threads = Vec::new();
        for name in thread_names {
            let message_count = state.get_thread_message_count(&name).unwrap_or(0) as u32;
            let messages = state.get_messages(&name, 1).unwrap_or_default();
            let last_message = messages.first().map(|m| m.content.clone());
            let last_timestamp = messages.first().map(|m| m.timestamp.clone());

            threads.push(ThreadSummary {
                name,
                message_count,
                unread_count: 0,
                last_message,
                last_timestamp,
            });
        }

        Ok(threads)
    }

    async fn get_messages(&self, run: &str, thread: &str) -> OrchestratorResult<Vec<Message>> {
        let state = self.get_state(run)?;

        let core_messages = state
            .get_messages(thread, 100)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let messages = core_messages
            .into_iter()
            .map(|m| Message {
                id: m.id as u32,
                thread: m.thread,
                sender: m.sender,
                content: m.content,
                waiting: m.waiting,
                read_by: None,
                timestamp: m.timestamp,
            })
            .collect();

        Ok(messages)
    }

    async fn send_message(
        &self,
        run: &str,
        thread: &str,
        content: &str,
    ) -> OrchestratorResult<Message> {
        let state = self.get_state(run)?;

        let message_id = state
            .add_message(thread, "user", content, false)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        Ok(Message {
            id: message_id as u32,
            thread: thread.to_string(),
            sender: "user".to_string(),
            content: content.to_string(),
            waiting: false,
            read_by: None,
            timestamp: Utc::now().to_rfc3339(),
        })
    }

    // -------------------------------------------------------------------------
    // Evals
    // -------------------------------------------------------------------------

    async fn list_evals(&self, run: &str) -> OrchestratorResult<Vec<Eval>> {
        let state = self.get_state(run)?;

        let core_evals = state
            .get_evals(100)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let evals = core_evals.iter().map(|e| self.convert_eval(e)).collect();

        Ok(evals)
    }

    // -------------------------------------------------------------------------
    // History
    // -------------------------------------------------------------------------

    async fn get_history(
        &self,
        run: &str,
        limit: Option<u32>,
    ) -> OrchestratorResult<Vec<HistoryEntry>> {
        let state = self.get_state(run)?;

        let limit = limit.unwrap_or(100) as i64;
        let core_history = state
            .get_history(limit)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let history = core_history
            .into_iter()
            .map(|h| HistoryEntry {
                id: h.id as u32,
                timestamp: h.timestamp,
                action: h.action,
                detail: h.detail,
            })
            .collect();

        Ok(history)
    }

    // -------------------------------------------------------------------------
    // Configuration
    // -------------------------------------------------------------------------

    async fn get_config(&self) -> OrchestratorResult<ConfigResponse> {
        let runs_dir = self.config.runs_dir().to_string_lossy().to_string();

        Ok(ConfigResponse {
            runs_dir,
            agent_command: self.config.agent.command.clone(),
            eval_timeout: self.config.eval_timeout,
            auto_learn: self.config.auto_learn,
            user_message_pause: self.config.user_message_pause.clone(),
            human_in_the_loop: self.config.human_in_the_loop,
            context_warning_threshold: self.config.context_warning_threshold,
            coordinator_port: self.config.coordinator_port,
            auth: self.config.auth.clone().into(),
            runners: self
                .config
                .runners
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into()))
                .collect(),
            default_runner: self.config.default_runner.clone(),
            worker_runners: self.config.worker_runners.clone(),
            profiles: self
                .config
                .profiles
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into()))
                .collect(),
            default_profile: self.config.default_profile.clone(),
            git: {
                use crate::core::api_types::{GitConfigResponse, GitProviderResponse};
                use crate::core::credentials::CredentialStore;

                // Check which providers have tokens configured
                let mut configured = Vec::new();
                if let Ok(store) = CredentialStore::open() {
                    if store.load("git_github_token").is_ok() {
                        configured.push(GitProviderResponse::Github);
                    }
                }

                GitConfigResponse {
                    default_provider: self.config.git.default_provider.map(|p| p.into()),
                    configured_providers: configured,
                }
            },
            storage: (&self.config.storage).into(),
        })
    }

    async fn health(&self) -> OrchestratorResult<HealthResponse> {
        Ok(HealthResponse {
            status: "ok".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    // -------------------------------------------------------------------------
    // Run Creation
    // -------------------------------------------------------------------------

    async fn create_run(&self, request: CreateRunRequest) -> OrchestratorResult<CreateRunResponse> {
        use crate::core::chats::{create_default_group_chat, create_worker_chat};
        use crate::core::files::Files;
        use crate::core::names;
        use crate::core::state::{SQLiteState, Status};

        // Slugify and validate run name
        let run_name = slugify(&request.name);
        if run_name.len() > 50 {
            return Err(OrchestratorError::InvalidOperation(format!(
                "Run name too long (max 50 chars): {}...",
                &run_name[..50]
            )));
        }

        // Get run directory
        let run_dir = config::run_dir(&run_name);
        if run_dir.exists() {
            let db_path = run_dir.join("hirsel.db");
            if db_path.exists() {
                if let Ok(existing_state) = SQLiteState::new(db_path) {
                    if let Ok(status) = existing_state.status() {
                        if status == Status::Working || status == Status::Eval {
                            return Err(OrchestratorError::InvalidOperation(format!(
                                "Run '{}' already exists and is active",
                                run_name
                            )));
                        }
                    }
                }
            }
            // Clean up old run
            let _ = std::fs::remove_dir_all(&run_dir);
        }

        // Create run directory
        std::fs::create_dir_all(&run_dir).map_err(|e| {
            OrchestratorError::Other(format!("Failed to create run directory: {}", e))
        })?;

        // Initialize Files
        let files = Files::new(run_dir.clone());
        files
            .init_dirs()
            .map_err(|e| OrchestratorError::Other(format!("Failed to init dirs: {}", e)))?;

        // Write spec file
        std::fs::write(files.spec(), &request.spec)
            .map_err(|e| OrchestratorError::Other(format!("Failed to write spec: {}", e)))?;

        // Write eval file if provided
        if let Some(ref eval_content) = request.eval {
            std::fs::write(run_dir.join("eval.md"), eval_content)
                .map_err(|e| OrchestratorError::Other(format!("Failed to write eval: {}", e)))?;
        }

        // Initialize bootstrap tasks.md
        std::fs::write(
            run_dir.join("tasks.md"),
            "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Scope |\n",
        ).map_err(|e| OrchestratorError::Other(format!("Failed to write tasks.md: {}", e)))?;

        // Create tasks detail folder
        let tasks_dir = run_dir.join("tasks");
        std::fs::create_dir_all(&tasks_dir)
            .map_err(|e| OrchestratorError::Other(format!("Failed to create tasks dir: {}", e)))?;

        // Write scope task content (leader guidance)
        let is_multi_worker = request.worker_scale.map(|n| n > 1).unwrap_or(false);
        let scope_content = generate_scope_task_content(is_multi_worker);
        std::fs::write(tasks_dir.join("scope.md"), scope_content)
            .map_err(|e| OrchestratorError::Other(format!("Failed to write scope.md: {}", e)))?;

        // Initialize workspace from starting_point if provided
        let project_path = if let Some(ref starting_point) = request.starting_point {
            let workspace = create_workspace_provider(None);
            let workspace_info = workspace
                .init(&run_name, starting_point)
                .await
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to initialize workspace: {}", e))
                })?;
            Some(workspace_info.path)
        } else {
            // No starting_point - expect files via upload_files()
            None
        };

        // Initialize SQLite state
        let db_path = run_dir.join("hirsel.db");
        let sqlite_state = SQLiteState::new(db_path)
            .map_err(|e| OrchestratorError::Other(format!("Failed to create state: {}", e)))?;
        sqlite_state
            .init_state(project_path.as_ref().and_then(|p| p.to_str()))
            .map_err(|e| OrchestratorError::Other(format!("Failed to init state: {}", e)))?;

        // Store starting_point in database for cloning
        if let Some(ref sp) = request.starting_point {
            let sp_json = serde_json::to_string(sp).map_err(|e| {
                OrchestratorError::Other(format!("Failed to serialize starting_point: {}", e))
            })?;
            sqlite_state
                .set_starting_point(Some(&sp_json))
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to set starting_point: {}", e))
                })?;
        }

        // Set run properties
        sqlite_state
            .set_request(Some(&request.spec))
            .map_err(|e| OrchestratorError::Other(format!("Failed to set request: {}", e)))?;

        if let Some(scale) = request.worker_scale {
            sqlite_state
                .set_worker_scale(&scale.to_string())
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to set worker scale: {}", e))
                })?;
        }

        if let Some(limit) = request.time_limit_minutes {
            sqlite_state
                .set_time_limit_minutes(Some(limit as i64))
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to set time limit: {}", e))
                })?;
        }

        if let Some(hitl) = request.human_in_the_loop {
            sqlite_state
                .set_human_in_the_loop(hitl)
                .map_err(|e| OrchestratorError::Other(format!("Failed to set HITL: {}", e)))?;
        }

        // Set status to Draft (not spawning workers yet)
        sqlite_state
            .set_status(Status::Draft)
            .map_err(|e| OrchestratorError::Other(format!("Failed to set status: {}", e)))?;

        // Create initial worker name
        let first_worker_name = names::generate_worker_name();

        // Determine multi-worker mode from scale
        let max_scale = request.worker_scale.unwrap_or(1);
        let is_multi_worker = max_scale > 1;

        // Create chat files
        let chats_dir = files.chats_dir();
        if is_multi_worker {
            create_default_group_chat(
                &chats_dir,
                &[first_worker_name.clone()],
                Some(&first_worker_name),
            )
            .map_err(|e| OrchestratorError::Other(format!("Failed to create group chat: {}", e)))?;
        }

        // Initialize docs directory for scribe system
        files
            .init_docs()
            .map_err(|e| OrchestratorError::Other(format!("Failed to init docs: {}", e)))?;

        create_worker_chat(&chats_dir, &first_worker_name).map_err(|e| {
            OrchestratorError::Other(format!("Failed to create worker chat: {}", e))
        })?;

        // Register initial worker (without work_dir - will be set after files upload)
        sqlite_state
            .add_worker(&first_worker_name, "", "remote")
            .map_err(|e| OrchestratorError::Other(format!("Failed to register worker: {}", e)))?;

        // Store tailscale OAuth credentials if provided
        if let Some(ref oauth) = request.tailscale_oauth {
            let oauth_json = serde_json::to_string(oauth).map_err(|e| {
                OrchestratorError::Other(format!("Failed to serialize OAuth: {}", e))
            })?;
            std::fs::write(run_dir.join(".tailscale_oauth.json"), oauth_json)
                .map_err(|e| OrchestratorError::Other(format!("Failed to write OAuth: {}", e)))?;
        }

        tracing::info!(
            "Created run '{}' with initial worker '{}'",
            run_name,
            first_worker_name
        );

        Ok(CreateRunResponse {
            name: run_name.clone(),
            run_dir: run_dir.to_string_lossy().to_string(),
            files_url: format!("/api/runs/{}/files", run_name),
        })
    }

    async fn upload_files(&self, run_name: &str, tarball: Vec<u8>) -> OrchestratorResult<()> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let run_dir = config::run_dir(run_name);
        if !run_dir.exists() {
            return Err(OrchestratorError::RunNotFound(run_name.to_string()));
        }

        // Create work directory
        let work_dir = run_dir.join("work");
        std::fs::create_dir_all(&work_dir)
            .map_err(|e| OrchestratorError::Other(format!("Failed to create work dir: {}", e)))?;

        // Extract tarball
        let decoder = GzDecoder::new(&tarball[..]);
        let mut archive = Archive::new(decoder);

        archive
            .unpack(&work_dir)
            .map_err(|e| OrchestratorError::Other(format!("Failed to extract tarball: {}", e)))?;

        tracing::info!(
            "Uploaded files for run '{}' to {}",
            run_name,
            work_dir.display()
        );
        Ok(())
    }

    async fn init_workspace(
        &self,
        run_name: &str,
        request: super::InitWorkspaceRequest,
    ) -> super::OrchestratorResult<super::InitWorkspaceResponse> {
        use crate::core::state::SQLiteState;

        let run_dir = config::run_dir(run_name);
        if !run_dir.exists() {
            return Err(OrchestratorError::RunNotFound(run_name.to_string()));
        }

        // Open state to check status and update starting_point
        let db_path = run_dir.join("hirsel.db");
        let state = SQLiteState::new(db_path)
            .map_err(|e| OrchestratorError::Other(format!("Failed to open state: {}", e)))?;

        // Check that no workers are active
        let workers = state
            .get_workers()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        let has_active = workers.iter().any(|w| !w.status.is_inactive());
        if has_active {
            return Err(OrchestratorError::InvalidOperation(
                "Cannot reinitialize workspace while workers are active".into(),
            ));
        }

        // Initialize workspace
        let workspace = create_workspace_provider(None);
        let workspace_info = workspace
            .init(run_name, &request.starting_point)
            .await
            .map_err(|e| {
                OrchestratorError::Other(format!("Failed to initialize workspace: {}", e))
            })?;

        // Store starting_point in database
        let sp_json = serde_json::to_string(&request.starting_point).map_err(|e| {
            OrchestratorError::Other(format!("Failed to serialize starting_point: {}", e))
        })?;
        state.set_starting_point(Some(&sp_json)).map_err(|e| {
            OrchestratorError::Other(format!("Failed to set starting_point: {}", e))
        })?;

        // Update project path
        state
            .set_project_path(workspace_info.path.to_str().unwrap_or("."))
            .map_err(|e| OrchestratorError::Other(format!("Failed to set project path: {}", e)))?;

        // Update branch if available
        if let Some(ref branch) = workspace_info.default_branch {
            state
                .set_branch(Some(branch))
                .map_err(|e| OrchestratorError::Other(format!("Failed to set branch: {}", e)))?;
        }

        tracing::info!(
            "Initialized workspace for run '{}' from {:?}",
            run_name,
            request.starting_point
        );

        Ok(super::InitWorkspaceResponse {
            workspace_path: workspace_info.path.to_string_lossy().to_string(),
            default_branch: workspace_info.default_branch,
        })
    }

    async fn spawn_workers(
        &self,
        run_name: &str,
        count: u32,
        assigned_task_id: Option<String>,
    ) -> OrchestratorResult<SpawnWorkersResponse> {
        use crate::cli::config::get_agent_command;
        use crate::core::chats::{create_default_group_chat, create_worker_chat};
        use crate::core::files::Files;
        use crate::core::names;
        use crate::core::runner::{create_runner, Runner, WorkerSpawnConfig as RunnerSpawnConfig};
        use crate::core::state::{SQLiteState, Status, WorkerUpdate};

        let run_dir = config::run_dir(run_name);
        if !run_dir.exists() {
            return Err(OrchestratorError::RunNotFound(run_name.to_string()));
        }

        let work_dir = run_dir.join("work");
        if !work_dir.exists() {
            return Err(OrchestratorError::InvalidOperation(
                "Work directory not found. Upload files first.".into(),
            ));
        }

        // Open state
        let db_path = run_dir.join("hirsel.db");
        let sqlite_state = SQLiteState::new(db_path)
            .map_err(|e| OrchestratorError::Other(format!("Failed to open state: {}", e)))?;

        // Check run status
        let status = sqlite_state
            .status()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        if status != Status::Draft && status != Status::Paused {
            return Err(OrchestratorError::InvalidOperation(format!(
                "Cannot spawn workers for run in '{}' status",
                status
            )));
        }

        // Get existing workers
        let existing_workers = sqlite_state
            .get_workers()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let existing_names: Vec<String> = existing_workers.iter().map(|w| w.name.clone()).collect();
        let is_multi_worker = existing_workers.len() + count as usize > 1;

        // Generate names for new workers
        let mut new_worker_names: Vec<String> = names::generate_unique_names(count as usize)
            .into_iter()
            .filter(|n| !existing_names.contains(n))
            .take(count as usize)
            .collect();

        // Need more names if we didn't get enough unique ones
        if new_worker_names.len() < count as usize {
            let mut all_used: std::collections::HashSet<String> =
                existing_names.iter().cloned().collect();
            all_used.extend(new_worker_names.iter().cloned());

            while new_worker_names.len() < count as usize {
                let name = names::generate_worker_name();
                if !all_used.contains(&name) {
                    all_used.insert(name.clone());
                    new_worker_names.push(name);
                }
            }
        }

        // Determine leader and teammates
        let leader_name = existing_workers.first().map(|w| w.name.clone());
        let all_worker_names: Vec<String> = existing_names
            .iter()
            .chain(new_worker_names.iter())
            .cloned()
            .collect();

        // Get agent command
        let agent_command = get_agent_command();
        let files = Files::new(run_dir.clone());
        let chats_dir = files.chats_dir();

        // Read tailscale OAuth credentials if present
        let tailscale_oauth: Option<TailscaleOAuth> =
            std::fs::read_to_string(run_dir.join(".tailscale_oauth.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());

        // Create TailscaleClient if OAuth credentials are available
        let tailscale_client = tailscale_oauth.as_ref().map(|oauth| {
            crate::core::tailscale::TailscaleClient::new(
                oauth.client_id.clone(),
                oauth.client_secret.clone(),
                oauth.tag.clone(),
            )
        });

        // Ensure group chat exists for multi-worker
        if is_multi_worker && !chats_dir.join("group.md").exists() {
            let _ =
                create_default_group_chat(&chats_dir, &all_worker_names, leader_name.as_deref());
        }

        // Spawn workers
        let mut spawned_workers = Vec::new();

        for (i, worker_name) in new_worker_names.iter().enumerate() {
            // First worker gets the provided task (if any)
            let task_for_worker = if i == 0 {
                assigned_task_id.clone()
            } else {
                None
            };

            // Claim task and set assigned_task_id if provided
            if let Some(ref task_id) = task_for_worker {
                // Use live nodes for project runs
                if let Some(project_id) = sqlite_state.get_project_id().ok().flatten() {
                    let delta_state = DeltaState::new(project_id);
                    if let Err(e) = delta_state.claim_live_node(task_id, worker_name) {
                        tracing::warn!(
                            "Failed to claim live node {} for worker {}: {}",
                            task_id,
                            worker_name,
                            e
                        );
                    }
                }
                if let Err(e) = sqlite_state.update_worker(
                    worker_name,
                    WorkerUpdate {
                        assigned_task_id: Some(Some(task_id.clone())),
                        ..Default::default()
                    },
                ) {
                    tracing::warn!(
                        "Failed to set assigned_task_id for worker {}: {}",
                        worker_name,
                        e
                    );
                }
            }
            // Create worker chat
            let _ = create_worker_chat(&chats_dir, worker_name);

            // Register worker
            sqlite_state
                .add_worker(worker_name, work_dir.to_str().unwrap_or("."), "remote")
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to register worker: {}", e))
                })?;

            // Build teammates list (exclude self)
            let teammates: Option<Vec<String>> = if is_multi_worker {
                Some(
                    all_worker_names
                        .iter()
                        .filter(|t| *t != worker_name)
                        .cloned()
                        .collect(),
                )
            } else {
                None
            };

            // Get runner config for this worker (from stored configs)
            let runner_config = sqlite_state
                .get_runner_config_for_worker(worker_name)
                .unwrap_or_default();

            // Check if local workers are allowed
            if runner_config.host_type() == "local" && !self.config.allow_local_workers {
                return Err(OrchestratorError::InvalidOperation(
                    "Local workers are not allowed on this coordinator. Configure a remote runner (fly, ssh).".to_string()
                ));
            }

            // For remote runners (Fly, SSH), determine coordinator URL and Tailscale auth
            let (coordinator_url, tailscale_authkey) = if runner_config.requires_coordinator_url() {
                // Validate Tailscale OAuth is configured
                let Some(ref client) = tailscale_client else {
                    return Err(OrchestratorError::Config(
                        "Fly/SSH runner requires Tailscale. Store tailscale_oauth in run config."
                            .into(),
                    ));
                };

                // Validate coordinator is connected to Tailscale
                let Some(hostname) = get_tailscale_hostname() else {
                    return Err(OrchestratorError::Config(
                        "Coordinator must be connected to Tailscale for Fly/SSH runners. Run 'tailscale up' first.".into()
                    ));
                };

                // Generate ephemeral auth key for this worker
                let authkey = client.generate_auth_key(worker_name).await.map_err(|e| {
                    OrchestratorError::Config(format!(
                        "Failed to generate Tailscale auth key for '{}': {}",
                        worker_name, e
                    ))
                })?;

                let url = format!("http://{}:{}", hostname, self.config.coordinator_port);
                (Some(url), Some(authkey))
            } else {
                (None, None)
            };

            let runner: Box<dyn Runner> = create_runner(&runner_config);

            let spawn_config = RunnerSpawnConfig {
                run_name: run_name.to_string(),
                worker_name: worker_name.clone(),
                work_dir: work_dir.clone(),
                run_dir: run_dir.clone(),
                agent_command: agent_command.clone(),
                is_leader: i == 0 && existing_workers.is_empty(), // First new worker is leader if no existing workers
                leader_name: leader_name.clone(),
                teammates,
                resume_session_id: None,
                env_vars: None,
                coordinator_url,
                tailscale_authkey,
                credentials: None,
                assigned_task_id: task_for_worker.clone(),
            };

            match runner.spawn(&spawn_config).await {
                Ok(result) => {
                    // Update worker with PID and runner info
                    let pid = result.pid.map(|p| p as i64);
                    let _ = sqlite_state.update_worker(
                        worker_name,
                        WorkerUpdate {
                            pid,
                            runner_id: Some(result.handle.runner_id.clone()),
                            runner_type: Some(result.handle.runner_type.clone()),
                            status: Some(crate::core::state::WorkerStatus::Working),
                            ..Default::default()
                        },
                    );
                    spawned_workers.push(worker_name.clone());
                    tracing::info!(
                        "Spawned worker '{}' (runner_id: {}, runner_type: {}, task: {:?})",
                        worker_name,
                        result.handle.runner_id,
                        result.handle.runner_type,
                        task_for_worker
                    );
                }
                Err(e) => {
                    return Err(OrchestratorError::Other(format!(
                        "Failed to spawn worker '{}': {}",
                        worker_name, e
                    )));
                }
            }
        }

        // Update run status to Working if we spawned any workers
        if !spawned_workers.is_empty() {
            sqlite_state
                .set_status(Status::Working)
                .map_err(|e| OrchestratorError::State(e.to_string()))?;
            sqlite_state
                .set_started_at(None)
                .map_err(|e| OrchestratorError::State(e.to_string()))?;
        }

        Ok(SpawnWorkersResponse {
            workers: spawned_workers,
        })
    }

    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail> {
        use crate::cli::config::get_agent_command;

        // 1. Slugify and validate run name
        let run_name = slugify(&request.name);
        if run_name.len() > 50 {
            return Err(OrchestratorError::InvalidOperation(format!(
                "Run name too long (max 50 chars): {}...",
                &run_name[..50]
            )));
        }

        // 2. Get run directory and check for conflicts
        let run_dir = config::run_dir(&run_name);
        if run_dir.exists() {
            let db_path = run_dir.join("hirsel.db");
            if db_path.exists() {
                if let Ok(existing_state) = SQLiteState::new(db_path) {
                    if let Ok(status) = existing_state.status() {
                        if status == Status::Working || status == Status::Eval {
                            return Err(OrchestratorError::InvalidOperation(format!(
                                "Run '{}' already exists and is active",
                                run_name
                            )));
                        }
                    }
                }
            }
            // Clean up old run
            let _ = std::fs::remove_dir_all(&run_dir);
        }

        // 3. Create run directory and initialize files
        std::fs::create_dir_all(&run_dir).map_err(|e| {
            OrchestratorError::Other(format!("Failed to create run directory: {}", e))
        })?;

        let files = Files::new(run_dir.clone());
        files
            .init_dirs()
            .map_err(|e| OrchestratorError::Other(format!("Failed to init dirs: {}", e)))?;

        // Write spec file
        std::fs::write(files.spec(), &request.spec)
            .map_err(|e| OrchestratorError::Other(format!("Failed to write spec: {}", e)))?;

        // Write eval file if provided
        if let Some(ref eval_content) = request.eval {
            std::fs::write(run_dir.join("eval.md"), eval_content)
                .map_err(|e| OrchestratorError::Other(format!("Failed to write eval: {}", e)))?;
        }

        // Initialize bootstrap tasks.md
        std::fs::write(
            run_dir.join("tasks.md"),
            "# Tasks\n\n| ID | Status | Worker | Name |\n|----|--------|--------|------|\n| scope | TODO | | Scope |\n",
        ).map_err(|e| OrchestratorError::Other(format!("Failed to write tasks.md: {}", e)))?;

        // Create tasks detail folder
        let tasks_dir = run_dir.join("tasks");
        std::fs::create_dir_all(&tasks_dir)
            .map_err(|e| OrchestratorError::Other(format!("Failed to create tasks dir: {}", e)))?;

        // Write scope task content (leader guidance)
        let is_multi_worker = request.worker_scale.map(|n| n > 1).unwrap_or(false);
        let scope_content = generate_scope_task_content(is_multi_worker);
        std::fs::write(tasks_dir.join("scope.md"), scope_content)
            .map_err(|e| OrchestratorError::Other(format!("Failed to write scope.md: {}", e)))?;

        // 3.5. Load project and resolve starting_point
        let store = ProjectStore::open().map_err(|e| OrchestratorError::State(e.to_string()))?;
        let project = store
            .get_project(request.project_id)
            .map_err(|e| OrchestratorError::State(format!("Project not found: {}", e)))?;

        // Resolve starting_point (request overrides project)
        let starting_point = request
            .starting_point
            .unwrap_or_else(|| project.starting_point.clone());

        // 4. Initialize workspace from starting point
        let workspace = create_workspace_provider(None);
        let workspace_info = workspace
            .init(&run_name, &starting_point)
            .await
            .map_err(|e| {
                OrchestratorError::Other(format!("Failed to initialize workspace: {}", e))
            })?;

        let project_path = workspace_info.path;

        // 5. Initialize SQLite state
        let db_path = run_dir.join("hirsel.db");
        let state = SQLiteState::new(db_path.clone())
            .map_err(|e| OrchestratorError::Other(format!("Failed to create state: {}", e)))?;
        state
            .init_state(Some(project_path.to_str().unwrap_or(".")))
            .map_err(|e| OrchestratorError::Other(format!("Failed to init state: {}", e)))?;

        // Store project association
        state
            .set_project_id(project.id)
            .map_err(|e| OrchestratorError::Other(format!("Failed to set project_id: {}", e)))?;
        state
            .set_project_name(&project.name)
            .map_err(|e| OrchestratorError::Other(format!("Failed to set project_name: {}", e)))?;

        // Store starting_point in database for cloning
        let sp_json = serde_json::to_string(&starting_point).map_err(|e| {
            OrchestratorError::Other(format!("Failed to serialize starting_point: {}", e))
        })?;
        state.set_starting_point(Some(&sp_json)).map_err(|e| {
            OrchestratorError::Other(format!("Failed to set starting_point: {}", e))
        })?;

        // Set run properties
        state
            .set_request(Some(&request.spec))
            .map_err(|e| OrchestratorError::Other(format!("Failed to set request: {}", e)))?;

        // Set default branch if available from workspace init
        if let Some(ref branch) = workspace_info.default_branch {
            state
                .set_branch(Some(branch))
                .map_err(|e| OrchestratorError::Other(format!("Failed to set branch: {}", e)))?;
        }

        let scale_max = request.worker_scale.unwrap_or(1);
        state
            .set_worker_scale(&scale_max.to_string())
            .map_err(|e| OrchestratorError::Other(format!("Failed to set worker scale: {}", e)))?;

        if let Some(limit) = request.time_limit_minutes {
            state.set_time_limit_minutes(Some(limit)).map_err(|e| {
                OrchestratorError::Other(format!("Failed to set time limit: {}", e))
            })?;
        }

        let hitl = request.human_in_the_loop.unwrap_or(true);
        state
            .set_human_in_the_loop(hitl)
            .map_err(|e| OrchestratorError::Other(format!("Failed to set HITL: {}", e)))?;

        // Set default runner if specified
        if let Some(ref runner) = request.runner {
            state.set_default_runner(Some(runner)).map_err(|e| {
                OrchestratorError::Other(format!("Failed to set default runner: {}", e))
            })?;
        }

        // Set per-worker runner assignments if specified
        if let Some(ref worker_runners) = request.worker_runners {
            state
                .set_worker_runners(Some(worker_runners))
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to set worker runners: {}", e))
                })?;
        }

        // Store full runner configs (capture at run creation time)
        // This ensures config changes don't affect in-progress runs
        {
            let mut runner_configs = HashMap::new();

            // Add default runner config
            let default_runner_name = request.runner.as_deref().unwrap_or("local");
            if let Some(config) = self.config.get_runner(default_runner_name) {
                runner_configs.insert(default_runner_name.to_string(), config);
            }

            // Add per-worker runner configs
            if let Some(ref worker_runners) = request.worker_runners {
                for runner_name in worker_runners.values() {
                    if !runner_configs.contains_key(runner_name) {
                        if let Some(config) = self.config.get_runner(runner_name) {
                            runner_configs.insert(runner_name.clone(), config);
                        }
                    }
                }
            }

            // Store configs if we have any
            if !runner_configs.is_empty() {
                state
                    .set_runner_configs(Some(&runner_configs))
                    .map_err(|e| {
                        OrchestratorError::Other(format!("Failed to set runner configs: {}", e))
                    })?;
            }
        }

        // 6. Parse worker scale and generate worker names
        // Always start with 1, autoscaling will add more based on scale_max
        let initial_count = 1u32;
        let worker_names = get_available_names(initial_count, &[]);

        // Determine if multi-worker mode (current or potential via autoscale)
        let (is_multi_worker, leader_name) = compute_multi_worker_config(&worker_names, scale_max);
        let first_worker = &worker_names[0];

        // Create delta state for live node operations
        let delta_state = DeltaState::new(project.id);

        // Always create scope task as a live node - this is the first task workers claim
        if let Err(e) = delta_state.create_live_node_from_worker(
            "scope",
            "Scope",
            None, // No parent
            None, // No blockers
            NodeType::Task,
            "", // No content initially
        ) {
            return Err(OrchestratorError::Other(format!(
                "Failed to create scope live node: {}",
                e
            )));
        }

        // Pre-claim scope for first worker
        if let Err(e) = delta_state.claim_live_node("scope", first_worker) {
            tracing::warn!(
                "Failed to pre-claim scope live node for {}: {}",
                first_worker,
                e
            );
        }

        // Store docs config from global settings
        state
            .set_docs_path(Some(&self.config.scribe_docs_path))
            .map_err(|e| OrchestratorError::Other(format!("Failed to set docs path: {}", e)))?;
        state
            .set_persist_docs_changes(self.config.scribe_persist_docs_changes)
            .map_err(|e| {
                OrchestratorError::Other(format!("Failed to set persist_docs_changes: {}", e))
            })?;

        // 7. Set up workspace clones and chats using shared ops
        let setup_config = RunSetupConfig {
            run_name: run_name.clone(),
            project_path: project_path.clone(),
            run_dir: run_dir.clone(),
            worker_names: worker_names.clone(),
            additional_chat_workers: Vec::new(),
            is_multi_worker,
            leader_name: leader_name.clone(),
            docs_path: self.config.scribe_docs_path.clone(),
        };

        let setup_result = setup_run_workspace(&setup_config)
            .map_err(|e| OrchestratorError::Other(format!("Failed to setup workspace: {}", e)))?;

        // 8. Register workers in state (use runner name as location)
        let worker_location = request.runner.as_deref().unwrap_or("local");
        register_workers(&state, &setup_result.worker_dirs, worker_location)
            .map_err(|e| OrchestratorError::Other(format!("Failed to register workers: {}", e)))?;

        // 9. Spawn workers and set to Working
        {
            let agent_command = get_agent_command();

            // Collect API keys from environment for Docker/remote runners
            let env_vars: HashMap<String, String> = std::env::vars()
                .filter(|(k, _)| {
                    k.starts_with("ANTHROPIC_")
                        || k.starts_with("OPENAI_")
                        || k.starts_with("CLAUDE_")
                })
                .collect();

            // Create Tailscale client if OAuth credentials provided (for remote runners)
            let tailscale_client = request.tailscale_oauth.as_ref().map(|oauth| {
                crate::core::tailscale::TailscaleClient::new(
                    oauth.client_id.clone(),
                    oauth.client_secret.clone(),
                    oauth.tag.clone(),
                )
            });

            for (i, (worker_name, work_dir)) in setup_result.worker_dirs.iter().enumerate() {
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

                // Get runner config for this worker (from stored configs)
                let runner_config = state
                    .get_runner_config_for_worker(worker_name)
                    .unwrap_or_default();

                // Check if local workers are allowed
                if runner_config.host_type() == "local" && !self.config.allow_local_workers {
                    return Err(OrchestratorError::InvalidOperation(
                        "Local workers are not allowed on this coordinator. Configure a remote runner (fly, ssh).".to_string()
                    ));
                }

                // For remote runners (Fly, SSH), determine coordinator URL and Tailscale auth
                let (coordinator_url, tailscale_authkey) = if runner_config
                    .requires_coordinator_url()
                {
                    // Validate Tailscale OAuth is configured
                    let Some(ref client) = tailscale_client else {
                        return Err(OrchestratorError::Config(
                            "Fly/SSH runner requires Tailscale. Provide tailscale_oauth in request or configure [profiles.X.access] with:\n\
                             type = \"tailscale\"\n\
                             oauth_client_id = \"...\"\n\
                             oauth_client_secret = \"...\"".into()
                        ));
                    };

                    // Validate coordinator is connected to Tailscale
                    let Some(hostname) = get_tailscale_hostname() else {
                        return Err(OrchestratorError::Config(
                            "Coordinator must be connected to Tailscale for Fly/SSH runners. Run 'tailscale up' first.".into()
                        ));
                    };

                    // Generate ephemeral auth key for this worker
                    let authkey = client.generate_auth_key(worker_name).await.map_err(|e| {
                        OrchestratorError::Config(format!(
                            "Failed to generate Tailscale auth key for '{}': {}",
                            worker_name, e
                        ))
                    })?;

                    let url = format!("http://{}:{}", hostname, self.config.coordinator_port);
                    (Some(url), Some(authkey))
                } else {
                    (None, None)
                };

                let runner: Box<dyn Runner> = create_runner(&runner_config);

                // Get assigned task for this worker (first worker gets scope task)
                let assigned_task_id = if i == 0 {
                    Some("scope".to_string())
                } else {
                    None
                };

                let spawn_config = RunnerSpawnConfig {
                    run_name: run_name.clone(),
                    worker_name: worker_name.clone(),
                    work_dir: work_dir.clone(),
                    run_dir: run_dir.clone(),
                    agent_command: agent_command.clone(),
                    is_leader,
                    leader_name: leader_name.clone(),
                    teammates,
                    resume_session_id: None,
                    env_vars: Some(env_vars.clone()),
                    coordinator_url,
                    tailscale_authkey,
                    credentials: None,
                    assigned_task_id: assigned_task_id.clone(),
                };

                match runner.spawn(&spawn_config).await {
                    Ok(result) => {
                        // Update worker with PID, runner info, and assigned task
                        let pid = result.pid.map(|p| p as i64);
                        let _ = state.update_worker(
                            worker_name,
                            WorkerUpdate {
                                pid,
                                runner_id: Some(result.handle.runner_id.clone()),
                                runner_type: Some(result.handle.runner_type.clone()),
                                status: Some(crate::core::state::WorkerStatus::Working),
                                assigned_task_id: assigned_task_id
                                    .as_ref()
                                    .map(|t| Some(t.clone())),
                                ..Default::default()
                            },
                        );
                        tracing::info!(
                            "Spawned worker '{}' (runner_id: {}, runner_type: {}, task: {:?})",
                            worker_name,
                            result.handle.runner_id,
                            result.handle.runner_type,
                            assigned_task_id
                        );
                    }
                    Err(e) => {
                        return Err(OrchestratorError::Other(format!(
                            "Failed to spawn worker '{}': {}",
                            worker_name, e
                        )));
                    }
                }
            }

            // Set status to Working and start time tracking
            state
                .set_status(Status::Working)
                .map_err(|e| OrchestratorError::State(e.to_string()))?;
            state
                .set_started_at(None)
                .map_err(|e| OrchestratorError::State(e.to_string()))?;

            // Ensure daemon is running for lifecycle management (eval triggering, time limits)
            #[cfg(feature = "server")]
            if let Ok(_) = crate::daemon::DaemonClient::connect_or_start() {
                tracing::debug!("Daemon is running for lifecycle management");
            }
        }

        // 10. Return run detail
        self.get_run(&run_name).await
    }

    async fn spawn_single_worker(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &std::path::Path,
        resume_session_id: Option<&str>,
    ) -> OrchestratorResult<()> {
        use crate::cli::config::get_agent_command;

        let run_dir = config::run_dir(run_name);
        let state = self.get_state(run_name)?;

        // Check if run is paused
        let status = state
            .status()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        if status == Status::Paused {
            return Err(OrchestratorError::InvalidOperation(
                "Cannot spawn worker: run is paused".into(),
            ));
        }

        // Get all workers for leader/teammates info
        let workers = state
            .get_workers()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        // Determine if multi-worker mode
        let is_multi_worker = workers.len() > 1
            || state
                .get_worker_scale()
                .ok()
                .flatten()
                .and_then(|s| s.parse::<usize>().ok())
                .map(|max| max > 1)
                .unwrap_or(false);

        let leader_name = workers.first().map(|w| w.name.clone());

        // Build teammates list (all workers except this one)
        let teammates: Option<Vec<String>> = if is_multi_worker {
            Some(
                workers
                    .iter()
                    .filter(|w| w.name != worker_name)
                    .map(|w| w.name.clone())
                    .collect(),
            )
        } else {
            None
        };

        // Get runner config for this worker (from stored configs)
        let runner_config = state
            .get_runner_config_for_worker(worker_name)
            .unwrap_or_default();

        // Check if local workers are allowed
        if runner_config.host_type() == "local" && !self.config.allow_local_workers {
            return Err(OrchestratorError::InvalidOperation(
                "Local workers are not allowed on this coordinator. Configure a remote runner (fly, ssh).".to_string()
            ));
        }

        // For remote runners (Fly, SSH), determine coordinator URL and Tailscale auth
        let (coordinator_url, tailscale_authkey) = if runner_config.requires_coordinator_url() {
            // Read tailscale OAuth credentials if present
            let tailscale_oauth: Option<TailscaleOAuth> =
                std::fs::read_to_string(run_dir.join(".tailscale_oauth.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());

            // Validate Tailscale OAuth is configured
            let Some(oauth) = tailscale_oauth else {
                return Err(OrchestratorError::Config(
                    "Fly/SSH runner requires Tailscale. Store tailscale_oauth when creating the run.".into()
                ));
            };

            // Validate coordinator is connected to Tailscale
            let Some(hostname) = get_tailscale_hostname() else {
                return Err(OrchestratorError::Config(
                    "Coordinator must be connected to Tailscale for Fly/SSH runners. Run 'tailscale up' first.".into()
                ));
            };

            // Create client and generate auth key
            let client = crate::core::tailscale::TailscaleClient::new(
                oauth.client_id,
                oauth.client_secret,
                oauth.tag,
            );
            let authkey = client.generate_auth_key(worker_name).await.map_err(|e| {
                OrchestratorError::Config(format!(
                    "Failed to generate Tailscale auth key for '{}': {}",
                    worker_name, e
                ))
            })?;

            let url = format!("http://{}:{}", hostname, self.config.coordinator_port);
            (Some(url), Some(authkey))
        } else {
            (None, None)
        };

        let runner: Box<dyn Runner> = create_runner(&runner_config);

        // Build spawn config
        let agent_command = get_agent_command();

        // Collect API keys from environment for Docker/remote runners
        let env_vars: HashMap<String, String> = std::env::vars()
            .filter(|(k, _)| {
                k.starts_with("ANTHROPIC_") || k.starts_with("OPENAI_") || k.starts_with("CLAUDE_")
            })
            .collect();

        // Get assigned task from worker record (set by evaluate_scaling before spawn)
        let assigned_task_id = state
            .get_worker(worker_name)
            .ok()
            .flatten()
            .and_then(|w| w.assigned_task_id);

        let spawn_config = RunnerSpawnConfig {
            run_name: run_name.to_string(),
            worker_name: worker_name.to_string(),
            work_dir: work_dir.to_path_buf(),
            run_dir: run_dir.clone(),
            agent_command,
            is_leader: false, // Scaled/resumed workers are never leader
            leader_name,
            teammates,
            resume_session_id: resume_session_id.map(String::from),
            env_vars: Some(env_vars),
            coordinator_url,
            tailscale_authkey,
            credentials: None,
            assigned_task_id,
        };

        // Spawn via runner (handles local/docker/fly/ssh correctly)
        match runner.spawn(&spawn_config).await {
            Ok(result) => {
                // Update worker with PID and runner info
                let pid = result.pid.map(|p| p as i64);
                let update_result = state.update_worker(
                    worker_name,
                    WorkerUpdate {
                        pid,
                        runner_id: Some(result.handle.runner_id.clone()),
                        runner_type: Some(result.handle.runner_type.clone()),
                        status: Some(crate::core::state::WorkerStatus::Working),
                        ..Default::default()
                    },
                );

                if let Err(ref e) = update_result {
                    tracing::error!(
                        "spawn_single_worker: FAILED to update worker '{}' status to Working: {}",
                        worker_name,
                        e
                    );
                }
                update_result.map_err(|e| OrchestratorError::State(e.to_string()))?;

                tracing::info!(
                    "spawn_single_worker: spawned '{}' with status=Working (runner_id: {}, runner_type: {})",
                    worker_name,
                    result.handle.runner_id,
                    result.handle.runner_type
                );

                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    "spawn_single_worker: failed to spawn '{}': {}",
                    worker_name,
                    e
                );
                Err(OrchestratorError::Other(format!(
                    "Failed to spawn worker '{}': {}",
                    worker_name, e
                )))
            }
        }
    }

    async fn resume_worker(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &std::path::Path,
        resume_session_id: Option<&str>,
        state_handle: Option<&WorkerStateHandle>,
    ) -> OrchestratorResult<()> {
        use crate::cli::config::get_agent_command;
        use crate::core::runner::WorkerHandle;
        use crate::core::snapshot::{create_archive_strategy, host_session_path, ArchiveHandle};

        let run_dir = config::run_dir(run_name);
        let state = self.get_state(run_name)?;

        // Check if run is paused
        let status = state
            .status()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        if status == Status::Paused {
            return Err(OrchestratorError::InvalidOperation(
                "Cannot resume worker: run is paused".into(),
            ));
        }

        // Get worker info
        let worker = state
            .get_worker(worker_name)
            .map_err(|e| OrchestratorError::State(e.to_string()))?
            .ok_or_else(|| OrchestratorError::WorkerNotFound(worker_name.to_string()))?;

        // Get runner config for this worker (from stored configs)
        let runner_config = state
            .get_runner_config_for_worker(worker_name)
            .unwrap_or_default();

        // Check if local workers are allowed
        if runner_config.host_type() == "local" && !self.config.allow_local_workers {
            return Err(OrchestratorError::InvalidOperation(
                "Local workers are not allowed on this coordinator. Configure a remote runner (fly, ssh).".to_string()
            ));
        }

        let runner: Box<dyn Runner> = create_runner(&runner_config);

        // 1. Check if already running
        if let (Some(ref runner_id), Some(ref runner_type)) =
            (&worker.runner_id, &worker.runner_type)
        {
            let handle = WorkerHandle {
                worker_name: worker_name.to_string(),
                runner_id: runner_id.clone(),
                runner_type: runner_type.clone(),
            };
            if runner.is_alive(&handle).await {
                tracing::info!(
                    "resume_worker: worker '{}' is already running, skipping spawn",
                    worker_name
                );
                return Ok(());
            }
        }

        // 2. Restore archives using unified archive strategy (for ephemeral runners)
        if runner.is_ephemeral() {
            match create_archive_strategy(&runner_config, &self.config.storage).await {
                Ok(strategy) => {
                    // Skip if no-op strategy (files persist on disk)
                    if !strategy.is_noop() {
                        // Restore work directory snapshot
                        if let Some(work_dir_snapshot) =
                            state_handle.and_then(|h| h.work_dir.as_ref())
                        {
                            tracing::info!(
                                "resume_worker: restoring work dir for ephemeral worker '{}'",
                                worker_name
                            );

                            let archive_handle = ArchiveHandle {
                                strategy_type: work_dir_snapshot.strategy_type.clone(),
                                storage_id: work_dir_snapshot.storage_id.clone(),
                                size_bytes: work_dir_snapshot.size_bytes,
                            };

                            if let Err(e) = strategy.restore(&archive_handle, work_dir).await {
                                tracing::warn!(
                                    "resume_worker: failed to restore work dir for '{}': {}",
                                    worker_name,
                                    e
                                );
                            } else {
                                tracing::info!(
                                    "resume_worker: restored {} work dir for '{}': {}",
                                    strategy.strategy_type(),
                                    worker_name,
                                    work_dir_snapshot.storage_id
                                );

                                // Delete the archive after successful restore
                                if let Err(e) = strategy.delete(&archive_handle).await {
                                    tracing::warn!(
                                        "resume_worker: failed to delete archive {} after restore: {}",
                                        work_dir_snapshot.storage_id,
                                        e
                                    );
                                }
                            }
                        }

                        // Restore agent session
                        if let Some(agent_snapshot) =
                            state_handle.and_then(|h| h.agent_session.as_ref())
                        {
                            // Only restore if there's actual archived data
                            if !agent_snapshot.storage_id.is_empty() {
                                tracing::info!(
                                    "resume_worker: restoring agent session for '{}' (session_id: {})",
                                    worker_name,
                                    agent_snapshot.session_id
                                );

                                let target_session_dir = host_session_path(&run_dir, worker_name);
                                let archive_handle = ArchiveHandle {
                                    strategy_type: strategy.strategy_type().to_string(),
                                    storage_id: agent_snapshot.storage_id.clone(),
                                    size_bytes: None,
                                };

                                if let Err(e) =
                                    strategy.restore(&archive_handle, &target_session_dir).await
                                {
                                    tracing::warn!(
                                        "resume_worker: failed to restore agent session for '{}': {}",
                                        worker_name,
                                        e
                                    );
                                } else {
                                    tracing::info!(
                                        "resume_worker: restored {} agent session for '{}'",
                                        strategy.strategy_type(),
                                        worker_name
                                    );

                                    // Delete the archive after successful restore
                                    if let Err(e) = strategy.delete(&archive_handle).await {
                                        tracing::warn!(
                                            "resume_worker: failed to delete session archive after restore: {}",
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "resume_worker: no archive strategy for '{}' (expected for local): {}",
                        worker_name,
                        e
                    );
                }
            }
        }

        // 4. Clear state handle from DB after restoration
        if state_handle.is_some() {
            let _ = state.update_worker(
                worker_name,
                WorkerUpdate {
                    state_handle: Some(None),
                    ..Default::default()
                },
            );
        }

        // 5. Get all workers for leader/teammates info
        let workers = state
            .get_workers()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        // Determine if multi-worker mode
        let is_multi_worker = workers.len() > 1
            || state
                .get_worker_scale()
                .ok()
                .flatten()
                .and_then(|s| s.parse::<usize>().ok())
                .map(|max| max > 1)
                .unwrap_or(false);

        let leader_name = workers.first().map(|w| w.name.clone());

        // Build teammates list (all workers except this one)
        let teammates: Option<Vec<String>> = if is_multi_worker {
            Some(
                workers
                    .iter()
                    .filter(|w| w.name != worker_name)
                    .map(|w| w.name.clone())
                    .collect(),
            )
        } else {
            None
        };

        // 6. Spawn worker
        let agent_command = get_agent_command();

        // Collect API keys from environment for Docker/remote runners
        let env_vars: HashMap<String, String> = std::env::vars()
            .filter(|(k, _)| {
                k.starts_with("ANTHROPIC_") || k.starts_with("OPENAI_") || k.starts_with("CLAUDE_")
            })
            .collect();

        // For remote runners (Fly, SSH), determine coordinator URL and Tailscale auth
        let (coordinator_url, tailscale_authkey) = if runner_config.requires_coordinator_url() {
            // Read tailscale OAuth credentials if present
            let tailscale_oauth: Option<TailscaleOAuth> =
                std::fs::read_to_string(run_dir.join(".tailscale_oauth.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());

            // Validate Tailscale OAuth is configured
            let Some(oauth) = tailscale_oauth else {
                return Err(OrchestratorError::Config(
                    "Fly/SSH runner requires Tailscale. Store tailscale_oauth when creating the run.".into()
                ));
            };

            // Validate coordinator is connected to Tailscale
            let Some(hostname) = get_tailscale_hostname() else {
                return Err(OrchestratorError::Config(
                    "Coordinator must be connected to Tailscale for Fly/SSH runners. Run 'tailscale up' first.".into()
                ));
            };

            // Create client and generate auth key
            let client = crate::core::tailscale::TailscaleClient::new(
                oauth.client_id,
                oauth.client_secret,
                oauth.tag,
            );
            let authkey = client.generate_auth_key(worker_name).await.map_err(|e| {
                OrchestratorError::Config(format!(
                    "Failed to generate Tailscale auth key for '{}': {}",
                    worker_name, e
                ))
            })?;

            let url = format!("http://{}:{}", hostname, self.config.coordinator_port);
            (Some(url), Some(authkey))
        } else {
            (None, None)
        };

        // Get assigned task from worker record
        let assigned_task_id = worker.assigned_task_id.clone();

        let spawn_config = RunnerSpawnConfig {
            run_name: run_name.to_string(),
            worker_name: worker_name.to_string(),
            work_dir: work_dir.to_path_buf(),
            run_dir: run_dir.clone(),
            agent_command,
            is_leader: false, // Resumed workers are never leader
            leader_name,
            teammates,
            resume_session_id: resume_session_id.map(String::from),
            env_vars: Some(env_vars),
            coordinator_url,
            tailscale_authkey,
            credentials: None,
            assigned_task_id,
        };

        // Spawn via runner
        match runner.spawn(&spawn_config).await {
            Ok(result) => {
                // Update worker with PID and runner info
                let pid = result.pid.map(|p| p as i64);
                state
                    .update_worker(
                        worker_name,
                        WorkerUpdate {
                            pid,
                            runner_id: Some(result.handle.runner_id.clone()),
                            runner_type: Some(result.handle.runner_type.clone()),
                            status: Some(crate::core::state::WorkerStatus::Working),
                            ..Default::default()
                        },
                    )
                    .map_err(|e| OrchestratorError::State(e.to_string()))?;

                tracing::info!(
                    "resume_worker: spawned '{}' (runner_id: {}, runner_type: {})",
                    worker_name,
                    result.handle.runner_id,
                    result.handle.runner_type
                );

                Ok(())
            }
            Err(e) => {
                tracing::warn!("resume_worker: failed to spawn '{}': {}", worker_name, e);
                Err(OrchestratorError::Other(format!(
                    "Failed to resume worker '{}': {}",
                    worker_name, e
                )))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Project Management
    // -------------------------------------------------------------------------

    async fn create_project(
        &self,
        req: crate::core::project::CreateProjectRequest,
    ) -> OrchestratorResult<crate::core::project::Project> {
        let store = crate::core::project::ProjectStore::open()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        store
            .create_project(&req)
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    async fn get_project(&self, id: i64) -> OrchestratorResult<crate::core::project::Project> {
        let store = crate::core::project::ProjectStore::open()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        store.get_project(id).map_err(|e| match e {
            crate::core::project::ProjectError::NotFound(_) => {
                OrchestratorError::RunNotFound(format!("Project {} not found", id))
            }
            _ => OrchestratorError::State(e.to_string()),
        })
    }

    async fn get_project_by_name(
        &self,
        name: &str,
    ) -> OrchestratorResult<Option<crate::core::project::Project>> {
        let store = crate::core::project::ProjectStore::open()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        store
            .get_project_by_name(name)
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    async fn list_projects(&self) -> OrchestratorResult<Vec<crate::core::project::Project>> {
        let store = crate::core::project::ProjectStore::open()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        store
            .list_projects()
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    async fn update_project(
        &self,
        id: i64,
        req: crate::core::project::UpdateProjectRequest,
    ) -> OrchestratorResult<crate::core::project::Project> {
        let store = crate::core::project::ProjectStore::open()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        store
            .update_project(id, &req)
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    async fn delete_project(&self, id: i64) -> OrchestratorResult<()> {
        // Get all runs for this project
        let runs = self.list_runs().await?;
        let project_runs: Vec<_> = runs
            .into_iter()
            .filter(|r| r.project_id == Some(id))
            .collect();

        // Delete all runs
        for run in project_runs {
            self.delete_run(&run.name).await?;
        }

        // Delete project from DB
        let store = crate::core::project::ProjectStore::open()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        store
            .delete_project(id)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        // Clear gyp chat messages for this project
        let gyp_store = crate::core::gyp_chat::GypChatStore::open()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;
        gyp_store
            .clear_project_messages(id)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        Ok(())
    }

    async fn list_project_runs(&self, project_id: i64) -> OrchestratorResult<Vec<RunSummary>> {
        let all_runs = self.list_runs().await?;
        Ok(all_runs
            .into_iter()
            .filter(|r| r.project_id == Some(project_id))
            .collect())
    }
}
