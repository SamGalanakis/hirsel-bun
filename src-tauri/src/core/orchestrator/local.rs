//! Local orchestrator implementation
//!
//! Implements the Orchestrator trait using direct local state access.
//! This wraps the existing GUI command logic into the orchestrator interface.

use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;

use super::{
    HealthResponse, Orchestrator, OrchestratorError, OrchestratorResult, StartRunRequest,
    TailscaleOAuth, WorkerStateHandle,
};
use crate::core::api_types::{
    convert_status, parse_elapsed_minutes, ConfigResponse, Eval, EvalStatus, HistoryEntry,
    RunDetail, RunSummary, SheepConfig, Worker, WorkerEventResponse, WorkerEventsResponse,
    WorkerLocation, WorkerStatus,
};
use crate::core::config::{self, Config};
use crate::core::delta::{DeltaState, NodeKind, ProjectRunStatus};
use crate::core::draft::create_workspace_provider;
use crate::core::names::{get_available_names, slugify};
use crate::core::ops::{
    compute_multi_worker_config, register_workers, setup_run_workspace, RunSetupConfig,
};
use crate::core::project::ProjectStore;
use crate::core::route::RouteStore;
use crate::core::runner::{create_runner, Runner, WorkerSpawnConfig as RunnerSpawnConfig};
use crate::core::state::{SQLiteState, Status, WorkerUpdate};
use crate::core::Files;

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
    async fn get_state(&self, run_name: &str) -> OrchestratorResult<SQLiteState> {
        let db_path = self.config.runs_dir().join(run_name).join("hirsel.db");
        if !db_path.exists() {
            return Err(OrchestratorError::RunNotFound(run_name.to_string()));
        }
        SQLiteState::new(run_name)
            .await
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    /// Convert core worker to GUI worker type
    fn convert_worker(
        &self,
        w: &crate::core::state::Worker,
        claimed_task_map: &HashMap<String, String>,
    ) -> Worker {
        use crate::core::metrics;

        let status: WorkerStatus = w.status.into();

        let location = match w.location.as_str() {
            "remote" => WorkerLocation::Remote,
            _ => WorkerLocation::Local,
        };

        // Get current task from pre-built map (O(1) instead of O(n))
        let current_task = claimed_task_map.get(&w.name).cloned();

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
            sheep_config: SheepConfig::for_check(e.id as u32),
        }
    }
}

#[async_trait]
impl Orchestrator for LocalOrchestrator {
    // -------------------------------------------------------------------------
    // Run Management
    // -------------------------------------------------------------------------

    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>> {
        // Use project_runs table as source of truth (one run per project)
        let project_runs = DeltaState::list_all_project_runs()
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to list project runs: {}", e)))?;

        let runs_dir = self.config.runs_dir();
        let mut runs = Vec::new();

        for (project_run, project_name) in project_runs {
            let run_name = &project_run.run_name;
            let project_id = project_run.project_id;
            let route_id = project_run.route_id;

            // Convert project run status to API status
            let status = match project_run.status {
                ProjectRunStatus::Working => crate::core::api_types::RunStatus::Working,
                ProjectRunStatus::Paused => crate::core::api_types::RunStatus::Paused,
                ProjectRunStatus::Failed => crate::core::api_types::RunStatus::Failed,
            };

            // Get task counts from board nodes (lightweight count query)
            let delta_state = DeltaState::with_route(project_id, route_id);
            let (tasks_done, tasks_total) = delta_state.get_node_counts().await.unwrap_or((0, 0));

            // Get worker counts and other data from per-run DB if available
            let db_path = runs_dir.join(run_name).join("hirsel.db");
            let (
                workers_active,
                workers_total,
                workers_desired,
                elapsed_minutes,
                time_limit_minutes,
                has_unread_messages,
            ) = if db_path.exists() {
                if let Ok(state) = SQLiteState::new(run_name).await {
                    if let Ok(summary) = state.get_run_summary().await {
                        (
                            summary.workers_active,
                            summary.workers_total,
                            summary.workers_desired,
                            summary.elapsed_minutes,
                            summary.time_limit_minutes.map(|m| m as u32),
                            summary.unread_count > 0,
                        )
                    } else {
                        (0, 0, 0, 0.0, None, false)
                    }
                } else {
                    (0, 0, 0, 0.0, None, false)
                }
            } else {
                (0, 0, 0, 0.0, None, false)
            };

            runs.push(RunSummary {
                name: run_name.clone(),
                status,
                tasks_done,
                tasks_total,
                workers_active,
                workers_total,
                workers_desired,
                elapsed_minutes,
                time_limit_minutes,
                has_unread_messages,
                created_at: project_run.created_at,
                project_id: Some(project_id),
                project_name: Some(project_name),
            });
        }

        Ok(runs)
    }

    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail> {
        let state = self.get_state(name).await?;

        let status = state
            .status()
            .await
            .unwrap_or(crate::core::state::Status::Draft);
        let run_status = convert_status(status);

        let request = state.get_request().await.ok().flatten();
        let project_path = state.get_project_path().await.ok().flatten();
        let worker_scale = state.get_worker_scale().await.ok().flatten();
        let time_limit_minutes = state
            .get_time_limit_minutes()
            .await
            .ok()
            .flatten()
            .map(|m| m as u32);
        let started_at = state.get_started_at().await.ok().flatten();
        let summary = state.get_summary().await.ok().flatten();
        let created_at = state
            .get_created_at()
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let iteration_count = state.get_iteration_count().await.unwrap_or(0) as u32;
        let human_in_the_loop = state.get_human_in_the_loop().await.unwrap_or(true);
        let waiting_reason = state.get_waiting_reason().await.ok().flatten();
        let unread_count = 0u32;

        // Get task counts from board nodes (project runs)
        let (tasks_done, tasks_total) = match (
            state.get_project_id().await.ok().flatten(),
            state.get_route_id().await.ok(),
        ) {
            (Some(project_id), Some(route_id)) => {
                let delta_state = DeltaState::with_route(project_id, route_id);
                if let Ok(nodes) = delta_state.get_nodes().await {
                    let done = nodes.iter().filter(|n| n.status.is_complete()).count() as u32;
                    (done, nodes.len() as u32)
                } else {
                    (0, 0)
                }
            }
            _ => (0, 0),
        };

        let workers = match state.get_workers().await {
            Ok(w) => w,
            Err(e) => {
                tracing::debug!(
                    "[Orchestrator] Failed to get workers for run '{}': {} - using empty list",
                    name,
                    e
                );
                Vec::new()
            }
        };
        let workers_active = workers
            .iter()
            .filter(|w| w.status == crate::core::state::WorkerStatus::Working)
            .count() as u32;
        // workers_total is the actual number of worker records.
        let workers_total = workers.len() as u32;
        // workers_desired is derived from worker_scale, falling back to workers_total.
        let workers_desired = worker_scale
            .as_ref()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(workers_total);

        // Calculate elapsed minutes
        let elapsed_minutes = if let Ok(Some(time_info)) = state.get_time_info().await {
            time_info.elapsed_minutes
        } else if let Some(ref sa) = started_at {
            parse_elapsed_minutes(sa)
        } else {
            parse_elapsed_minutes(&created_at)
        };

        let remote_url = state.get_remote_url().await.ok().flatten();
        let branch = state.get_branch().await.ok().flatten();

        let agent_type = self.config.agent.agent_type();
        let metrics_available = agent_type.supports_context_tracking();

        // Get runner configuration
        let runner = state.get_default_runner().await.ok().flatten();
        let worker_runners = state.get_worker_runners().await.ok().flatten();

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
            workers_desired,
            elapsed_minutes,
            agent_type: format!("{:?}", agent_type).to_lowercase(),
            metrics_available,
            runner,
            worker_runners,
            project_id: state.get_project_id().await.ok().flatten(),
            project_name: state.get_project_name().await.ok().flatten(),
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
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;

        lifecycle
            .pause_run("User requested pause")
            .await
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
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;

        let actions = lifecycle
            .resume_run()
            .await
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
        let state = self.get_state(name).await?;

        // Validate run is in a deliverable state
        let status = state.status().await?;
        if !matches!(status, Status::Done | Status::Delivered) {
            return Err(OrchestratorError::InvalidOperation(format!(
                "Cannot deliver run in '{}' state. Run must be 'Done' first.",
                status
            )));
        }

        // Get project path and remote URL
        let project_path_str = state.get_project_path().await?;
        let remote_url = state.get_remote_url().await?;
        let saved_branch = state.get_branch().await?;

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
            .await?
            .unwrap_or_else(|| "docs".to_string());
        let persist_docs = state.get_persist_docs_changes().await?;

        let docs_config = crate::core::ops::DocsDeliveryConfig {
            workspace_dir: &project_path,
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
            .await?;

        Ok(branch)
    }

    // -------------------------------------------------------------------------
    // Workers
    // -------------------------------------------------------------------------

    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>> {
        let state = self.get_state(run).await?;

        let core_workers = state.get_workers().await?;

        // Lightweight mapping: worker_name -> current task name (claimed_by or assigned_task_id fallback)
        let current_task_map = match (
            state.get_project_id().await.ok().flatten(),
            state.get_route_id().await.ok(),
        ) {
            (Some(project_id), Some(route_id)) => {
                let delta = DeltaState::with_route(project_id, route_id);
                let mut map = delta.get_claimed_task_map().await.unwrap_or_default();

                // If a worker has an assigned_task_id but nothing is currently claimed_by them,
                // surface that as current_task for better diagnostics.
                let mut assigned_by_worker: HashMap<String, String> = HashMap::new();
                let mut assigned_ids: Vec<String> = Vec::new();
                for w in &core_workers {
                    if map.contains_key(&w.name) {
                        continue;
                    }
                    if let Some(id) = w
                        .assigned_task_id
                        .as_ref()
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                    {
                        assigned_by_worker.insert(w.name.clone(), id.to_string());
                        assigned_ids.push(id.to_string());
                    }
                }

                if !assigned_ids.is_empty() {
                    let id_to_name = delta
                        .get_node_name_map(&assigned_ids)
                        .await
                        .unwrap_or_default();
                    for (worker, id) in assigned_by_worker {
                        let name = id_to_name.get(&id).cloned().unwrap_or(id);
                        map.insert(worker, name);
                    }
                }

                map
            }
            _ => HashMap::new(),
        };

        let workers = core_workers
            .iter()
            .map(|w| self.convert_worker(w, &current_task_map))
            .collect();

        Ok(workers)
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> OrchestratorResult<()> {
        use crate::cli::config::get_agent_command;
        use crate::core::state::WorkerUpdate;
        use crate::core::workers::{is_pid_alive, spawn_worker, WorkerSpawnConfig};

        let run_dir = config::run_dir(run);
        let state = self.get_state(run).await?;

        // Find the worker by name
        let workers = state.get_workers().await?;

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
                    pid: Some(None),
                    ..Default::default()
                },
            )
            .await?;

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
            is_plan_task: false,
        };

        spawn_worker(config, &state)
            .await
            .map_err(|e| OrchestratorError::Other(e.to_string()))?;

        Ok(())
    }

    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> OrchestratorResult<WorkerEventsResponse> {
        let state = self.get_state(run).await?;

        let limit = limit.unwrap_or(1000);
        let events = state.get_worker_events(worker, after_id, limit).await?;

        let last_id = events.last().map(|e| e.id);

        // Get worker status to determine if still streaming
        let worker_status = state
            .get_worker(worker)
            .await
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
    // Evals
    // -------------------------------------------------------------------------

    async fn list_evals(&self, run: &str) -> OrchestratorResult<Vec<Eval>> {
        let state = self.get_state(run).await?;

        let core_evals = state.get_evals(100).await?;

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
        let state = self.get_state(run).await?;

        let limit = limit.unwrap_or(100) as i64;
        let core_history = state.get_history(limit).await?;

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
            llm: self.config.llm.clone().into(),
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
                if let Ok(store) = CredentialStore::open().await {
                    if store.load("git_github_token").await.is_ok() {
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
        let state = SQLiteState::new(run_name)
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to open state: {}", e)))?;

        // Check that no workers are active
        let workers = state.get_workers().await?;
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
        state
            .set_starting_point(Some(&sp_json))
            .await
            .map_err(|e| {
                OrchestratorError::Other(format!("Failed to set starting_point: {}", e))
            })?;

        // Update project path
        state
            .set_project_path(workspace_info.path.to_str().unwrap_or("."))
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set project path: {}", e)))?;

        // Update branch if available
        if let Some(ref branch) = workspace_info.default_branch {
            state
                .set_branch(Some(branch))
                .await
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
                if let Ok(existing_state) = SQLiteState::new(&run_name).await {
                    if let Ok(status) = existing_state.status().await {
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

        // Note: Task content is stored in board nodes and accessed via MCP tools.
        // No spec.md or task files are written to disk.

        // 3.5. Load project/route and resolve starting_point
        let store = ProjectStore::open().await?;
        let project = store
            .get_project(request.project_id)
            .await
            .map_err(|e| OrchestratorError::State(format!("Project not found: {}", e)))?;

        let route_store = RouteStore::new(request.project_id)
            .await
            .map_err(|e| OrchestratorError::State(format!("Failed to open route store: {}", e)))?;

        let route_id = if let Some(route_id) = request.route_id {
            route_id
        } else if let Some(route_id) = project.active_route_id {
            route_id
        } else {
            route_store
                .create_main_route()
                .await
                .map_err(|e| {
                    OrchestratorError::State(format!("Failed to create fallback main route: {}", e))
                })?
                .id
        };

        let route = route_store.get_route(route_id).await.map_err(|e| {
            OrchestratorError::State(format!("Failed to load route {}: {}", route_id, e))
        })?;

        // Resolve starting_point (request overrides route default repo)
        let default_repo_starting_point = route_store
            .get_default_repo_starting_point(route_id)
            .await
            .map_err(|e| {
                OrchestratorError::State(format!(
                    "Route has no usable default repo starting point: {}",
                    e
                ))
            })?;
        let starting_point = request
            .starting_point
            .unwrap_or(default_repo_starting_point);

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
        let state = SQLiteState::new(&run_name)
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to create state: {}", e)))?;
        state
            .init_state(Some(project_path.to_str().unwrap_or(".")))
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to init state: {}", e)))?;

        // Store project association
        state
            .set_project_id(project.id)
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set project_id: {}", e)))?;
        state
            .set_project_name(&project.name)
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set project_name: {}", e)))?;
        state
            .set_route_id(route_id)
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set route_id: {}", e)))?;

        // Store starting_point in database for cloning
        let sp_json = serde_json::to_string(&starting_point).map_err(|e| {
            OrchestratorError::Other(format!("Failed to serialize starting_point: {}", e))
        })?;
        state
            .set_starting_point(Some(&sp_json))
            .await
            .map_err(|e| {
                OrchestratorError::Other(format!("Failed to set starting_point: {}", e))
            })?;

        // Set run properties
        state
            .set_request(Some(&request.spec))
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set request: {}", e)))?;

        // Set default branch if available from workspace init
        if let Some(ref branch) = workspace_info.default_branch {
            state
                .set_branch(Some(branch))
                .await
                .map_err(|e| OrchestratorError::Other(format!("Failed to set branch: {}", e)))?;
        }

        let scale_max = request.worker_scale.unwrap_or_else(|| {
            route
                .worker_scale
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5)
        });
        state
            .set_worker_scale(&scale_max.to_string())
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set worker scale: {}", e)))?;

        if let Some(limit) = request.time_limit_minutes.or(route.time_limit_minutes) {
            state
                .set_time_limit_minutes(Some(limit))
                .await
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to set time limit: {}", e))
                })?;
        }

        let hitl = request.human_in_the_loop.unwrap_or(route.human_in_the_loop);
        state
            .set_human_in_the_loop(hitl)
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set HITL: {}", e)))?;

        let effective_runner = request.runner.clone().or(route.runner.clone());

        // Set default runner if specified
        if let Some(ref runner) = effective_runner {
            state.set_default_runner(Some(runner)).await.map_err(|e| {
                OrchestratorError::Other(format!("Failed to set default runner: {}", e))
            })?;
        }

        // Set per-worker runner assignments if specified
        if let Some(ref worker_runners) = request.worker_runners {
            state
                .set_worker_runners(Some(worker_runners))
                .await
                .map_err(|e| {
                    OrchestratorError::Other(format!("Failed to set worker runners: {}", e))
                })?;
        }

        // Store full runner configs (capture at run creation time)
        // This ensures config changes don't affect in-progress runs
        {
            let mut runner_configs = HashMap::new();

            // Add default runner config
            let default_runner_name = effective_runner.as_deref().unwrap_or("local");
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
                    .await
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

        // Pre-claim first available board node for first worker
        let mut first_assigned_task: Option<String> = None;
        let mut first_is_plan_task = false;
        let delta_state = DeltaState::with_route(project.id, route_id);
        match delta_state.get_claimable_nodes().await {
            Ok(claimable) => {
                if let Some(node) = claimable.first() {
                    match delta_state.claim_node(&node.id, first_worker).await {
                        Ok(_) => {
                            tracing::info!("Pre-claimed node '{}' for {}", node.id, first_worker);
                            first_is_plan_task = node.kind == NodeKind::Plan;
                            first_assigned_task = Some(node.id.clone());
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to pre-claim node '{}' for {}: {}",
                                node.id,
                                first_worker,
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to get claimable nodes: {}", e);
            }
        }

        // Store docs config from route settings
        state
            .set_docs_path(Some(&route.docs_path))
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to set docs path: {}", e)))?;
        state
            .set_persist_docs_changes(route.persist_docs_changes)
            .await
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
            docs_path: route.docs_path.clone(),
        };

        let setup_result = setup_run_workspace(&setup_config)
            .map_err(|e| OrchestratorError::Other(format!("Failed to setup workspace: {}", e)))?;

        // 8. Register workers in state (use runner name as location)
        let worker_location = effective_runner.as_deref().unwrap_or("local");
        register_workers(&state, &setup_result.worker_dirs, worker_location)
            .await
            .map_err(|e| OrchestratorError::Other(format!("Failed to register workers: {}", e)))?;

        // 9. Spawn workers and set to Working
        {
            let agent_command = get_agent_command();

            // Collect API keys from environment for Docker/remote runners
            let env_vars: HashMap<String, String> = std::env::vars()
                .filter(|(k, _)| k.starts_with("OPENAI_") || k.starts_with("CODEX_"))
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
                    .await
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

                // Get assigned task for this worker (first worker gets pre-claimed task)
                let (assigned_task_id, is_plan_task) = if i == 0 {
                    (first_assigned_task.clone(), first_is_plan_task)
                } else {
                    (None, false)
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
                    is_plan_task,
                };

                match runner.spawn(&spawn_config).await {
                    Ok(result) => {
                        // Update worker with PID, runner info, and assigned task
                        let pid = result.pid.map(|p| p as i64);
                        let _ = state
                            .update_worker(
                                worker_name,
                                WorkerUpdate {
                                    pid: pid.map(Some),
                                    runner_id: Some(result.handle.runner_id.clone()),
                                    runner_type: Some(result.handle.runner_type.clone()),
                                    status: Some(crate::core::state::WorkerStatus::Working),
                                    assigned_task_id: assigned_task_id
                                        .as_ref()
                                        .map(|t| Some(t.clone())),
                                    ..Default::default()
                                },
                            )
                            .await;
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
            state.set_status(Status::Working).await?;
            state.set_started_at(None).await?;

            // Ensure daemon is running for lifecycle management (eval triggering, time limits)
            #[cfg(feature = "server")]
            if crate::daemon::DaemonClient::connect_or_start().is_ok() {
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
        let state = self.get_state(run_name).await?;

        // Check if run is paused
        let status = state.status().await?;
        if status == Status::Paused {
            return Err(OrchestratorError::InvalidOperation(
                "Cannot spawn worker: run is paused".into(),
            ));
        }

        // Get all workers for leader/teammates info
        let workers = state.get_workers().await?;

        // Determine if multi-worker mode
        let is_multi_worker = workers.len() > 1
            || state
                .get_worker_scale()
                .await
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
            .await
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
            .filter(|(k, _)| k.starts_with("OPENAI_") || k.starts_with("CODEX_"))
            .collect();

        // Get assigned task from worker record (set by evaluate_scaling before spawn)
        let assigned_task_id = state
            .get_worker(worker_name)
            .await
            .ok()
            .flatten()
            .and_then(|w| w.assigned_task_id);

        // Detect task kind by looking up the assigned node
        let is_plan_task = if let Some(ref task_id) = assigned_task_id {
            if let (Some(project_id), Ok(route_id)) = (
                state.get_project_id().await.ok().flatten(),
                state.get_route_id().await,
            ) {
                let delta = DeltaState::with_route(project_id, route_id);
                delta
                    .get_node(task_id)
                    .await
                    .map(|n| n.kind == NodeKind::Plan)
                    .unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        };

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
            is_plan_task,
        };

        // Spawn via runner (handles local/docker/fly/ssh correctly)
        match runner.spawn(&spawn_config).await {
            Ok(result) => {
                // Update worker with PID and runner info
                let pid = result.pid.map(|p| p as i64);
                let update_result = state
                    .update_worker(
                        worker_name,
                        WorkerUpdate {
                            pid: pid.map(Some),
                            runner_id: Some(result.handle.runner_id.clone()),
                            runner_type: Some(result.handle.runner_type.clone()),
                            status: Some(crate::core::state::WorkerStatus::Working),
                            ..Default::default()
                        },
                    )
                    .await;

                if let Err(ref e) = update_result {
                    tracing::error!(
                        "spawn_single_worker: FAILED to update worker '{}' status to Working: {}",
                        worker_name,
                        e
                    );
                }
                update_result?;

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
        let state = self.get_state(run_name).await?;

        // Check if run is paused
        let status = state.status().await?;
        if status == Status::Paused {
            return Err(OrchestratorError::InvalidOperation(
                "Cannot resume worker: run is paused".into(),
            ));
        }

        // Get worker info
        let worker = state
            .get_worker(worker_name)
            .await?
            .ok_or_else(|| OrchestratorError::WorkerNotFound(worker_name.to_string()))?;

        // Get runner config for this worker (from stored configs)
        let runner_config = state
            .get_runner_config_for_worker(worker_name)
            .await
            .unwrap_or_default();

        // Check if local workers are allowed
        if runner_config.host_type() == "local" && !self.config.allow_local_workers {
            return Err(OrchestratorError::InvalidOperation(
                "Local workers are not allowed on this coordinator. Configure a remote runner (fly, ssh).".to_string()
            ));
        }

        let runner: Box<dyn Runner> = create_runner(&runner_config);

        // 1. Kill any stale process before resuming
        // If we're resuming, any existing process is stale and should be killed
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
                    "resume_worker: killing stale process {} for worker '{}'",
                    runner_id,
                    worker_name
                );
                if let Err(e) = runner.stop(&handle).await {
                    tracing::warn!(
                        "resume_worker: failed to stop stale process for '{}': {}",
                        worker_name,
                        e
                    );
                }
                // Brief wait for process cleanup
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
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
            let _ = state
                .update_worker(
                    worker_name,
                    WorkerUpdate {
                        state_handle: Some(None),
                        ..Default::default()
                    },
                )
                .await;
        }

        // 5. Get all workers for leader/teammates info
        let workers = state.get_workers().await?;

        // Determine if multi-worker mode
        let is_multi_worker = workers.len() > 1
            || state
                .get_worker_scale()
                .await
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
            .filter(|(k, _)| k.starts_with("OPENAI_") || k.starts_with("CODEX_"))
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
        let is_plan_task = if let Some(ref task_id) = assigned_task_id {
            if let (Some(project_id), Ok(route_id)) = (
                state.get_project_id().await.ok().flatten(),
                state.get_route_id().await,
            ) {
                let delta = DeltaState::with_route(project_id, route_id);
                delta
                    .get_node(task_id)
                    .await
                    .map(|n| n.kind == NodeKind::Plan)
                    .unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        };

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
            is_plan_task,
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
                            pid: pid.map(Some),
                            runner_id: Some(result.handle.runner_id.clone()),
                            runner_type: Some(result.handle.runner_type.clone()),
                            status: Some(crate::core::state::WorkerStatus::Working),
                            ..Default::default()
                        },
                    )
                    .await?;

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
        let store = crate::core::project::ProjectStore::open().await?;
        store
            .create_project(&req)
            .await
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    async fn get_project(&self, id: i64) -> OrchestratorResult<crate::core::project::Project> {
        let store = crate::core::project::ProjectStore::open().await?;
        store.get_project(id).await.map_err(|e| match e {
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
        let store = crate::core::project::ProjectStore::open().await?;
        store
            .get_project_by_name(name)
            .await
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    async fn list_projects(&self) -> OrchestratorResult<Vec<crate::core::project::Project>> {
        let store = crate::core::project::ProjectStore::open().await?;
        store
            .list_projects()
            .await
            .map_err(|e| OrchestratorError::State(e.to_string()))
    }

    async fn update_project(
        &self,
        id: i64,
        req: crate::core::project::UpdateProjectRequest,
    ) -> OrchestratorResult<crate::core::project::Project> {
        let store = crate::core::project::ProjectStore::open().await?;
        store
            .update_project(id, &req)
            .await
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
        let store = crate::core::project::ProjectStore::open().await?;
        store.delete_project(id).await?;

        // Clear shepherd chat messages for this project
        let shepherd_store = crate::core::shepherd_chat::ShepherdChatStore::open().await?;
        shepherd_store.clear_project_messages(id).await?;

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
