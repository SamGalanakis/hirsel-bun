//! Local orchestrator implementation
//!
//! Implements the Orchestrator trait using direct local state access.
//! This wraps the existing GUI command logic into the orchestrator interface.

use async_trait::async_trait;
use chrono::Utc;

use super::{HealthResponse, Orchestrator, OrchestratorError, OrchestratorResult};
use crate::core::api_types::{
    calculate_duration_minutes, convert_status, is_completed_status, parse_elapsed_minutes,
    ConfigResponse, Eval, EvalStatus, HistoryEntry, Message, RunDetail, RunSummary, SheepConfig,
    Task, TaskStatus, ThreadSummary, Worker, WorkerEventResponse, WorkerEventsResponse,
    WorkerLocation, WorkerStatus,
};
use crate::core::config::{self, Config};
use crate::core::state::SQLiteState;
use crate::core::Files;

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
        tasks: &[crate::core::state::Task],
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

        // Find current task for this worker
        let current_task = tasks
            .iter()
            .find(|t| {
                t.claimed_by.as_deref() == Some(&w.name)
                    && t.status == crate::core::state::TaskStatus::Doing
            })
            .map(|t| t.name.clone());

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

    /// Convert core task to GUI task type
    fn convert_task(&self, t: &crate::core::state::Task) -> Task {
        let status = match t.status {
            crate::core::state::TaskStatus::Todo => TaskStatus::Todo,
            crate::core::state::TaskStatus::Doing => TaskStatus::Doing,
            crate::core::state::TaskStatus::Done => TaskStatus::Done,
        };

        // Parse blocked_by string into Vec<String>
        let blocked_by = t.blocked_by.as_ref().map(|b| {
            b.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });

        Task {
            id: t.id.clone(),
            description: t.name.clone(),
            status,
            claimed_by: t.claimed_by.clone(),
            claimed_at: t.claimed_at.clone(),
            completed_at: t.completed_at.clone(),
            parent_id: t.parent_id.clone(),
            blocked_by,
            tokens_used: t.tokens_used.map(|n| n as u64),
            created_at: t.created_at.clone(),
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
        let max_iterations = state.get_max_iterations().ok().flatten().map(|m| m as u32);
        let human_in_the_loop = state.get_human_in_the_loop().unwrap_or(true);
        let waiting_reason = state.get_waiting_reason().ok().flatten();
        let unread_count = state.get_unread_count().unwrap_or(0) as u32;

        // Get tasks and workers for counts
        let tasks = state.get_tasks().unwrap_or_default();
        let workers = state.get_workers().unwrap_or_default();

        let tasks_done = tasks
            .iter()
            .filter(|t| t.status == crate::core::state::TaskStatus::Done)
            .count() as u32;
        let tasks_total = tasks.len() as u32;
        let workers_active = workers
            .iter()
            .filter(|w| w.status == crate::core::state::WorkerStatus::Working)
            .count() as u32;
        let workers_total = workers.len() as u32;

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
        let learnings_count = state.get_messages_count("learnings").unwrap_or(0) as u32;
        let learnings_processed_at = state.get_learnings_processed_at().ok().flatten();

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
            max_iterations,
            human_in_the_loop,
            waiting_reason,
            unread_count,
            tasks_done,
            tasks_total,
            workers_active,
            workers_total,
            elapsed_minutes,
            learnings_count,
            learnings_processed_at,
            agent_type: format!("{:?}", agent_type).to_lowercase(),
            metrics_available,
            runner,
            worker_runners,
        })
    }

    async fn delete_run(&self, name: &str) -> OrchestratorResult<()> {
        use crate::core::ops::{delete_run as ops_delete_run, DeleteRunConfig};

        let config = DeleteRunConfig::for_gui(name);
        ops_delete_run(config).map_err(|e| OrchestratorError::Other(e.to_string()))?;
        Ok(())
    }

    async fn pause_run(&self, name: &str) -> OrchestratorResult<()> {
        use crate::core::workers::pause_all_workers;

        let state = self.get_state(name)?;

        // Check current status
        let status = state
            .status()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        if status == crate::core::state::Status::Paused {
            return Ok(()); // Already paused
        }
        if status != crate::core::state::Status::Working
            && status != crate::core::state::Status::Eval
        {
            return Err(OrchestratorError::InvalidOperation(format!(
                "Cannot pause run in '{}' status",
                status
            )));
        }

        // Cancel any running evals
        let _ = state.cancel_running_evals("Run paused");

        // Pause all workers
        pause_all_workers(&state).map_err(|e| OrchestratorError::Other(e.to_string()))?;

        // Update status
        state
            .set_status(crate::core::state::Status::Paused)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        Ok(())
    }

    async fn resume_run(
        &self,
        name: &str,
        _time_limit_minutes: Option<u32>,
    ) -> OrchestratorResult<()> {
        use crate::cli::config::get_agent_command;
        use crate::core::workers::{maybe_scale_up, maybe_trigger_eval, resume_awaiting_workers};

        let run_dir = config::run_dir(name);
        let state = self.get_state(name)?;

        // Check current status
        let status = state
            .status()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        if status == crate::core::state::Status::Working {
            return Ok(()); // Already running
        }
        if status != crate::core::state::Status::Paused
            && status != crate::core::state::Status::Failed
        {
            return Err(OrchestratorError::InvalidOperation(format!(
                "Cannot resume run in '{}' status",
                status
            )));
        }

        // Check if there was an eval that was paused
        let was_in_eval = state
            .has_paused_eval()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        if was_in_eval {
            let _ = state.clear_paused_evals();
            state
                .set_status(crate::core::state::Status::Working)
                .map_err(|e| OrchestratorError::State(e.to_string()))?;

            if maybe_trigger_eval(name, &run_dir).unwrap_or(false) {
                return Ok(());
            }
        }

        // Update status first
        state
            .set_status(crate::core::state::Status::Working)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        // Resume existing workers
        let agent_command = get_agent_command();
        let _ = resume_awaiting_workers(name, &run_dir, &agent_command);

        // Try to scale up if more tasks are available
        loop {
            match maybe_scale_up(name, &run_dir, &agent_command) {
                Ok(Some(_)) => continue,
                _ => break,
            }
        }

        // Check if eval should be triggered
        let _ = maybe_trigger_eval(name, &run_dir);

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

        let tasks = state.get_tasks().unwrap_or_default();

        let workers = core_workers
            .iter()
            .map(|w| self.convert_worker(w, &tasks))
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
        let files = Files::new(&run_dir);
        let agent_command = get_agent_command();
        let config = WorkerSpawnConfig {
            run_name: run.to_string(),
            worker_name: worker_data.name.clone(),
            work_dir,
            run_dir: run_dir.clone(),
            spec_path: files.spec(),
            agent_command,
            is_leader: worker_data.id == 1,
            leader_name,
            teammates: if is_multi_worker {
                Some(teammates)
            } else {
                None
            },
            resume_session_id: worker_data.session_id.clone(),
            credentials: None,
        };

        spawn_worker(config, &state).map_err(|e| OrchestratorError::Other(e.to_string()))?;

        Ok(())
    }

    async fn get_worker_log(
        &self,
        run: &str,
        worker: &str,
        lines: Option<usize>,
    ) -> OrchestratorResult<String> {
        use std::io::Read;

        let run_dir = config::run_dir(run);
        let files = Files::new(&run_dir);
        let log_path = files.worker_log(worker);

        if !log_path.exists() {
            return Ok(String::new());
        }

        let mut file = std::fs::File::open(&log_path)
            .map_err(|e| OrchestratorError::Other(format!("Failed to open log file: {}", e)))?;

        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| OrchestratorError::Other(format!("Failed to read log file: {}", e)))?;

        // If lines limit is specified, return only the last N lines
        if let Some(limit) = lines {
            let all_lines: Vec<&str> = content.lines().collect();
            if all_lines.len() > limit {
                content = all_lines[all_lines.len() - limit..].join("\n");
            }
        }

        Ok(content)
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
    // Tasks
    // -------------------------------------------------------------------------

    async fn list_tasks(&self, run: &str) -> OrchestratorResult<Vec<Task>> {
        let state = self.get_state(run)?;

        let core_tasks = state
            .get_tasks()
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let tasks = core_tasks.iter().map(|t| self.convert_task(t)).collect();

        Ok(tasks)
    }

    async fn add_task(&self, run: &str, content: &str) -> OrchestratorResult<Task> {
        let state = self.get_state(run)?;

        // Generate a unique task ID
        let task_id = uuid::Uuid::new_v4().to_string();

        state
            .add_task(&task_id, content, None, None)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        let task = state
            .get_task(&task_id)
            .map_err(|e| OrchestratorError::State(e.to_string()))?
            .ok_or_else(|| OrchestratorError::TaskNotFound(task_id.clone()))?;

        Ok(self.convert_task(&task))
    }

    async fn delete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        let state = self.get_state(run)?;

        state
            .delete_task(task_id)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        Ok(())
    }

    async fn complete_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        let state = self.get_state(run)?;

        state
            .complete_task(task_id, "user")
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        Ok(())
    }

    async fn reopen_task(&self, run: &str, task_id: &str) -> OrchestratorResult<()> {
        let state = self.get_state(run)?;

        state
            .reopen_task(task_id)
            .map_err(|e| OrchestratorError::State(e.to_string()))?;

        Ok(())
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
            max_iterations: self.config.max_iterations,
            user_message_pause: self.config.user_message_pause.clone(),
            human_in_the_loop: self.config.human_in_the_loop,
            compaction_enabled: self.config.compaction_enabled,
            compaction_threshold: self.config.compaction_threshold,
            compaction_keep_messages: self.config.compaction_keep_messages,
            auto_improve: self.config.auto_improve,
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
        })
    }

    async fn health(&self) -> OrchestratorResult<HealthResponse> {
        Ok(HealthResponse {
            status: "ok".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }
}
