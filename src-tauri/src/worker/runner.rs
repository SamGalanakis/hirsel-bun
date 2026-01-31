//! Worker subprocess main loop implementation.
//!
//! The WorkerRunner manages the lifecycle of a single worker subprocess:
//! - Initializes connection to run state
//! - Spawns and communicates with the AI agent
//! - Handles task claim/done cycle
//! - Manages heartbeats and status updates
//! - Coordinates with other workers via messaging
//!
//! Workers are state-agnostic - they don't know if they're accessing state
//! locally (SQLiteState) or remotely (HttpState). The HIRSEL_API_URL
//! environment variable determines which backend is used.

use crate::cli::{MsgSubcommands, TaskSubcommands, WorkerCommands};
use crate::core::state::{SQLiteState, StateError, TaskType, WorkerStatus, WorkerUpdate};
use crate::core::state_access::{StateAccess, StateAccessError};
use crate::core::Files;
use crate::worker::http_state::HttpState;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Errors that can occur during worker operations.
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("Run directory not found: {0}")]
    RunNotFound(PathBuf),

    #[error("Worker not registered: {0}")]
    WorkerNotRegistered(String),

    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("State access error: {0}")]
    StateAccess(#[from] StateAccessError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No task claimed")]
    NoTaskClaimed,

    #[error("Task already claimed: {0}")]
    TaskAlreadyClaimed(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Result type for worker operations.
pub type WorkerResult<T> = Result<T, WorkerError>;

/// Configuration for a worker subprocess.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Name of this worker (e.g., "achilles", "ajax").
    pub worker_name: String,
    /// Name of the run.
    pub run_name: String,
    /// Path to the run directory.
    pub run_dir: PathBuf,
    /// Agent command to use for spawning workers.
    pub agent_command: Vec<String>,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval: u64,
}

impl WorkerConfig {
    /// Create a new worker configuration from environment variables.
    ///
    /// Expects HIRSEL_RUN and HIRSEL_WORKER environment variables.
    pub fn from_env() -> WorkerResult<Self> {
        let run_name = std::env::var("HIRSEL_RUN")
            .map_err(|_| WorkerError::Config("HIRSEL_RUN not set".into()))?;

        let worker_name = std::env::var("HIRSEL_WORKER")
            .map_err(|_| WorkerError::Config("HIRSEL_WORKER not set".into()))?;

        // Get agent command from environment or default
        let agent_command = std::env::var("HIRSEL_AGENT_COMMAND")
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| vec!["hirsel".to_string(), "__acp-bridge".to_string()]);

        // Get run directory - check HIRSEL_RUN_DIR first (for Docker/custom mounts),
        // then fall back to HIRSEL_ROOT/runs/run_name
        let run_dir = if let Ok(dir) = std::env::var("HIRSEL_RUN_DIR") {
            PathBuf::from(dir)
        } else {
            let hirsel_root = std::env::var("HIRSEL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(".hirsel")
                });
            hirsel_root.join("runs").join(&run_name)
        };

        if !run_dir.exists() {
            return Err(WorkerError::RunNotFound(run_dir));
        }

        Ok(Self {
            worker_name,
            run_name,
            run_dir,
            agent_command,
            heartbeat_interval: 30,
        })
    }

    /// Create a worker configuration with explicit values.
    pub fn new(
        worker_name: String,
        run_name: String,
        run_dir: PathBuf,
        agent_command: Vec<String>,
    ) -> Self {
        Self {
            worker_name,
            run_name,
            run_dir,
            agent_command,
            heartbeat_interval: 30,
        }
    }
}

/// State backend enum - either local SQLite or remote HTTP.
enum StateBackend {
    Local(SQLiteState),
    Remote {
        state: HttpState,
        runtime: tokio::runtime::Runtime,
    },
}

/// The main worker subprocess runner.
///
/// Workers are state-agnostic - they don't know if they're accessing state
/// locally or remotely. The backend is chosen based on HIRSEL_API_URL.
pub struct WorkerRunner {
    config: WorkerConfig,
    backend: StateBackend,
    _files: Option<Files>,
    last_heartbeat: Instant,
}

impl WorkerRunner {
    /// Create a new worker runner.
    ///
    /// If HIRSEL_API_URL is set, uses HttpState to communicate with a
    /// remote coordinator. Otherwise, uses SQLiteState for local access.
    pub fn new(config: WorkerConfig) -> WorkerResult<Self> {
        // Check if we should use remote state
        if let Ok(api_url) = std::env::var("HIRSEL_API_URL") {
            // Remote mode - use HttpState
            let runtime = tokio::runtime::Runtime::new()
                .map_err(|e| WorkerError::Config(format!("Failed to create runtime: {}", e)))?;

            let state = HttpState::new(&api_url, &config.worker_name, 30);

            // Verify connection and worker exists
            let workers = runtime
                .block_on(state.get_workers())
                .map_err(|e| WorkerError::StateAccess(e.into()))?;

            if !workers.iter().any(|w| w.name == config.worker_name) {
                return Err(WorkerError::WorkerNotRegistered(config.worker_name.clone()));
            }

            Ok(Self {
                config,
                backend: StateBackend::Remote { state, runtime },
                _files: None,
                last_heartbeat: Instant::now(),
            })
        } else {
            // Local mode - use SQLiteState
            let files = Files::new(&config.run_dir);
            let state = SQLiteState::new(files.db_path()).map_err(WorkerError::State)?;

            // Verify worker exists in database
            let workers = state.get_workers().map_err(WorkerError::State)?;
            if !workers.iter().any(|w| w.name == config.worker_name) {
                return Err(WorkerError::WorkerNotRegistered(config.worker_name.clone()));
            }

            Ok(Self {
                config,
                backend: StateBackend::Local(state),
                _files: Some(files),
                last_heartbeat: Instant::now(),
            })
        }
    }

    /// Get the worker name.
    pub fn worker_name(&self) -> &str {
        &self.config.worker_name
    }

    /// Get the run directory.
    pub fn run_dir(&self) -> &PathBuf {
        &self.config.run_dir
    }

    /// Get access to the state for direct queries.
    /// Returns None if using remote state (HttpState).
    pub fn local_state(&self) -> Option<&SQLiteState> {
        match &self.backend {
            StateBackend::Local(state) => Some(state),
            StateBackend::Remote { .. } => None,
        }
    }

    /// Execute an async operation on the state backend.
    fn run_async<F, T>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        match &self.backend {
            StateBackend::Local(_) => {
                // For local state, we create a minimal runtime just to execute the future.
                // Since SQLiteState's async methods are just sync wrappers, this is fast.
                futures::executor::block_on(f)
            }
            StateBackend::Remote { runtime, .. } => runtime.block_on(f),
        }
    }

    /// Get a reference to the state as a trait object for async operations.
    fn state(&self) -> &dyn StateAccess {
        match &self.backend {
            StateBackend::Local(state) => state,
            StateBackend::Remote { state, .. } => state,
        }
    }

    /// Update worker heartbeat in the database.
    pub fn heartbeat(&mut self) -> WorkerResult<()> {
        let now = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.6f")
            .to_string();
        let worker_name = self.config.worker_name.clone();
        let update = WorkerUpdate {
            last_heartbeat: Some(now),
            ..Default::default()
        };
        self.run_async(self.state().update_worker(&worker_name, update))?;
        self.last_heartbeat = Instant::now();
        Ok(())
    }

    /// Check if heartbeat is due.
    pub fn heartbeat_due(&self) -> bool {
        self.last_heartbeat.elapsed() >= Duration::from_secs(self.config.heartbeat_interval)
    }

    /// Set worker status.
    pub fn set_status(&self, status: WorkerStatus) -> WorkerResult<()> {
        let worker_name = self.config.worker_name.clone();
        let update = WorkerUpdate {
            status: Some(status),
            ..Default::default()
        };
        self.run_async(self.state().update_worker(&worker_name, update))?;
        Ok(())
    }

    // =========================================================================
    // Task Operations
    // =========================================================================

    /// Get the full task tree with hierarchy and status.
    /// Returns a hierarchical structure with dependencies.
    pub fn get_task_tree(&self) -> WorkerResult<String> {
        let tasks = self.run_async(self.state().get_tasks())?;

        // Build a map for quick lookups
        let task_map: std::collections::HashMap<String, &crate::core::state::Task> =
            tasks.iter().map(|t| (t.id.clone(), t)).collect();

        // Find root tasks (no parent) and build tree structure
        let mut roots: Vec<serde_json::Value> = Vec::new();
        let mut children_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for t in &tasks {
            if let Some(parent_id) = &t.parent_id {
                children_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(t.id.clone());
            }
        }

        fn build_node(
            task: &crate::core::state::Task,
            children_map: &std::collections::HashMap<String, Vec<String>>,
            task_map: &std::collections::HashMap<String, &crate::core::state::Task>,
            state: &dyn crate::core::state_access::StateAccess,
            runner: &WorkerRunner,
        ) -> serde_json::Value {
            let blocked = runner
                .run_async(state.is_task_blocked(&task.id))
                .unwrap_or(false);

            let children: Vec<serde_json::Value> = children_map
                .get(&task.id)
                .map(|child_ids| {
                    child_ids
                        .iter()
                        .filter_map(|cid| task_map.get(cid))
                        .map(|c| build_node(c, children_map, task_map, state, runner))
                        .collect()
                })
                .unwrap_or_default();

            let mut node = serde_json::json!({
                "id": task.id,
                "name": task.name,
                "type": task.task_type.as_str(),
                "status": task.status.as_str(),
                "claimed_by": task.claimed_by,
                "blocked": blocked,
            });

            if !task.blocked_by.is_empty() {
                node["blocked_by"] = serde_json::json!(&task.blocked_by);
            }

            if task.task_type == crate::core::state::TaskType::Eval {
                if let Ok(validates) = runner.run_async(state.get_validated_tasks(&task.id)) {
                    if !validates.is_empty() {
                        node["validates"] = serde_json::json!(validates);
                    }
                }
                if let Some(result) = &task.eval_result {
                    node["eval_result"] = serde_json::json!(result.as_str());
                }
            }

            if !children.is_empty() {
                node["children"] = serde_json::json!(children);
            }

            node
        }

        for t in &tasks {
            if t.parent_id.is_none() {
                roots.push(build_node(t, &children_map, &task_map, self.state(), self));
            }
        }

        let output = serde_json::json!({
            "tasks": roots,
            "total_count": tasks.len(),
        });
        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Get tasks that are ready to claim (unblocked, unclaimed, todo status).
    pub fn get_available_tasks(&self) -> WorkerResult<String> {
        let claimable = self.run_async(self.state().get_claimable_tasks())?;

        let tasks: Vec<serde_json::Value> = claimable
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "type": t.task_type.as_str(),
                    "parent": t.parent_id,
                })
            })
            .collect();

        let output = serde_json::json!({
            "available_count": tasks.len(),
            "tasks": tasks,
        });
        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Get tasks claimed by this worker.
    pub fn get_my_tasks(&self) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let claimed = self.run_async(self.state().get_claimed_task(&worker_name))?;

        let tasks: Vec<serde_json::Value> = claimed
            .into_iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "type": t.task_type.as_str(),
                    "status": t.status.as_str(),
                    "claimed_at": t.claimed_at,
                })
            })
            .collect();

        let output = serde_json::json!({
            "claimed_count": tasks.len(),
            "tasks": tasks,
        });
        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Get full details for a specific task.
    pub fn get_task_details(&self, task_id: &str) -> WorkerResult<String> {
        let tasks = self.run_async(self.state().get_tasks())?;
        let task = tasks
            .into_iter()
            .find(|t| t.id == task_id)
            .ok_or_else(|| WorkerError::Config(format!("Task '{}' not found", task_id)))?;

        let blocked = self
            .run_async(self.state().is_task_blocked(&task.id))
            .unwrap_or(false);

        let mut output = serde_json::json!({
            "id": task.id,
            "name": task.name,
            "type": task.task_type.as_str(),
            "status": task.status.as_str(),
            "claimed_by": task.claimed_by,
            "claimed_at": task.claimed_at,
            "created_at": task.created_at,
            "completed_at": task.completed_at,
            "parent": task.parent_id,
            "blocked": blocked,
        });

        if !task.blocked_by.is_empty() {
            output["blocked_by"] = serde_json::json!(&task.blocked_by);
        }

        if task.task_type == crate::core::state::TaskType::Eval {
            if let Ok(validates) = self.run_async(self.state().get_validated_tasks(&task.id)) {
                if !validates.is_empty() {
                    output["validates"] = serde_json::json!(validates);
                }
            }
            if let Some(result) = &task.eval_result {
                output["eval_result"] = serde_json::json!(result.as_str());
            }
            if let Some(feedback) = &task.eval_feedback {
                output["eval_feedback"] = serde_json::json!(feedback);
            }
        }

        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Claim a task with detailed rejection info.
    pub fn task_claim(&self, task_id: &str) -> WorkerResult<String> {
        use crate::core::state::ClaimTaskResult;

        let worker_name = self.config.worker_name.clone();
        let result = self.run_async(self.state().try_claim_task(task_id, &worker_name))?;

        match result {
            ClaimTaskResult::Success { task } => Ok(serde_json::json!({
                "success": true,
                "task_id": task.id,
                "task_name": task.name,
                "claimed_by": self.config.worker_name,
            })
            .to_string()),
            ClaimTaskResult::Rejected {
                reason,
                alternatives,
            } => {
                let message = format_claim_rejection(&reason);
                Ok(serde_json::json!({
                    "success": false,
                    "message": message,
                    "reason": reason,
                    "alternatives": alternatives,
                })
                .to_string())
            }
        }
    }

    /// Mark a task as done.
    pub fn task_done(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let tid = match task_id {
            Some(id) => id.to_string(),
            None => {
                // Get currently claimed task
                let task = self
                    .run_async(self.state().get_claimed_task(&worker_name))?
                    .ok_or(WorkerError::NoTaskClaimed)?;
                task.id
            }
        };

        self.run_async(self.state().complete_task(&tid, &worker_name))?;

        // Completing a task might unblock other tasks, so wake awaiting workers
        self.try_resume_awaiting_workers();

        Ok(serde_json::json!({
            "success": true,
            "task_id": tid,
        })
        .to_string())
    }

    /// Unclaim a task.
    pub fn task_unclaim(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let tid = match task_id {
            Some(id) => id.to_string(),
            None => {
                let task = self
                    .run_async(self.state().get_claimed_task(&worker_name))?
                    .ok_or(WorkerError::NoTaskClaimed)?;
                task.id
            }
        };

        self.run_async(self.state().unclaim_task(&tid, &worker_name))?;

        // Unclaiming a task makes it available, so wake awaiting workers
        self.try_resume_awaiting_workers();

        Ok(serde_json::json!({
            "success": true,
            "task_id": tid,
        })
        .to_string())
    }

    /// Add a new task.
    pub fn task_add(
        &self,
        task_id: &str,
        name: &str,
        parent: Option<&str>,
        blocked_by: &[String],
    ) -> WorkerResult<String> {
        let blocked_refs: Vec<&str> = blocked_by.iter().map(|s| s.as_str()).collect();

        self.run_async(self.state().add_task(
            task_id,
            name,
            parent,
            if blocked_refs.is_empty() {
                None
            } else {
                Some(blocked_refs.as_slice())
            },
        ))?;

        // New task might be claimable, so wake awaiting workers
        if blocked_refs.is_empty() {
            self.try_resume_awaiting_workers();
        }

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Add a new eval task with validates relationship.
    pub fn add_eval(
        &self,
        eval_id: &str,
        name: &str,
        validates: &[String],
    ) -> WorkerResult<String> {
        use crate::core::state::TaskType;

        let validates_refs: Vec<&str> = validates.iter().map(|s| s.as_str()).collect();

        self.run_async(self.state().add_task_with_type(
            eval_id,
            name,
            None,                            // No parent
            None,                            // No blocked_by (uses validates)
            TaskType::Eval,                  // Eval type
            Some(validates_refs.as_slice()), // Validates relationship
            None,                            // No board_task_id
        ))?;

        Ok(serde_json::json!({
            "success": true,
            "eval_id": eval_id,
            "validates": validates,
        })
        .to_string())
    }

    /// Notify that tasks may have become available.
    ///
    /// Workers don't handle lifecycle management directly - the daemon polls
    /// every 5 seconds and handles spawning/resuming workers via the orchestrator.
    /// This method is kept for interface compatibility but is now a no-op.
    fn try_resume_awaiting_workers(&self) {
        // Workers are "dumb" - they just do tasks and report status.
        // The daemon handles all lifecycle management (scaling, resume, eval triggers).
        // This is intentionally a no-op; the daemon will detect available tasks
        // on its next polling cycle and handle worker scaling/resuming.
        tracing::debug!(
            "[{}] Task completed - daemon will handle worker scaling on next poll",
            self.config.worker_name
        );
    }

    /// Delete a task.
    pub fn task_delete(&self, task_id: &str) -> WorkerResult<String> {
        self.run_async(self.state().delete_task(task_id))?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Reopen a completed task.
    pub fn task_undone(&self, task_id: &str) -> WorkerResult<String> {
        self.run_async(self.state().reopen_task(task_id))?;

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Wait for tasks to become available.
    ///
    /// Returns the current list of claimable tasks. If empty, the worker should
    /// call `work_done` to signal completion - the orchestrator will restart
    /// the worker (with session resume) when new tasks become available.
    pub fn task_await(&self) -> WorkerResult<String> {
        // Set worker status to awaiting
        self.set_status(WorkerStatus::Awaiting)?;

        // Check for available tasks
        let claimable = self.run_async(self.state().get_claimable_tasks())?;

        Ok(serde_json::json!({
            "available_tasks": claimable.len(),
            "tasks": claimable.iter().map(|t| serde_json::json!({
                "id": t.id,
                "name": t.name,
            })).collect::<Vec<_>>(),
        })
        .to_string())
    }

    // =========================================================================
    // Message Operations
    // =========================================================================

    /// Send a message to a thread.
    /// When thread is "user", messages are sent to the worker's own DM thread
    /// and HITL pause is triggered automatically (if HITL mode is enabled).
    pub fn msg_send(&self, thread: &str, message: &str) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let is_user_dm = thread == "user";

        // Translate "user" thread to worker's own DM thread
        let actual_thread = if is_user_dm {
            worker_name.as_str()
        } else {
            thread
        };

        self.run_async(
            self.state()
                .add_message(actual_thread, &worker_name, message),
        )?;

        // Auto-trigger HITL pause when messaging the user (if HITL enabled)
        let hitl_enabled = self
            .run_async(self.state().get_human_in_the_loop())
            .unwrap_or(true);
        let waiting = is_user_dm && hitl_enabled;

        if waiting {
            self.set_status(WorkerStatus::Awaiting)?;
            self.run_async(self.state().update_worker(
                &worker_name,
                WorkerUpdate {
                    hitl_waiting: Some(true),
                    waiting_thread: Some(actual_thread.to_string()),
                    ..Default::default()
                },
            ))?;
        }

        Ok(serde_json::json!({
            "success": true,
            "thread": actual_thread,
            "waiting": waiting,
        })
        .to_string())
    }

    /// Read messages from a thread (or all threads).
    pub fn msg_read(&self, thread: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let messages = if let Some(t) = thread {
            self.run_async(self.state().get_unread_messages(t, &worker_name))?
        } else {
            // Read from all threads
            self.run_async(self.state().get_all_unread_messages(&worker_name))?
        };

        Ok(serde_json::json!({
            "messages": messages.iter().map(|m| serde_json::json!({
                "thread": m.thread,
                "sender": m.sender,
                "content": m.content,
                "timestamp": m.timestamp,
                "waiting": m.waiting,
            })).collect::<Vec<_>>(),
        })
        .to_string())
    }

    /// List available message threads.
    pub fn msg_list(&self) -> WorkerResult<String> {
        let threads = self.run_async(self.state().get_threads())?;

        Ok(serde_json::json!({
            "threads": threads,
        })
        .to_string())
    }

    /// Check inbox for new messages.
    pub fn msg_inbox(&self) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let threads = self.run_async(self.state().get_threads())?;

        let mut inbox = Vec::new();
        for thread in &threads {
            let messages =
                self.run_async(self.state().get_unread_messages(thread, &worker_name))?;

            if !messages.is_empty() {
                inbox.push(serde_json::json!({
                    "thread": thread,
                    "count": messages.len(),
                    "messages": messages.iter().map(|m| serde_json::json!({
                        "sender": m.sender,
                        "content": m.content,
                        "timestamp": m.timestamp,
                    })).collect::<Vec<_>>(),
                }));
            }
        }

        Ok(serde_json::json!({
            "inbox": inbox,
        })
        .to_string())
    }

    // =========================================================================
    // New Chat API (cleaner interface)
    // =========================================================================

    /// List available chat contacts.
    /// Returns: user (human), group (team), other workers, scribe.
    pub fn list_contacts(&self) -> WorkerResult<String> {
        let workers = self.run_async(self.state().get_workers())?;
        let worker_names: Vec<String> = workers
            .iter()
            .filter(|w| w.name != self.config.worker_name)
            .map(|w| w.name.clone())
            .collect();

        let is_multi_worker = workers.len() > 1;

        Ok(serde_json::json!({
            "contacts": {
                "user": true,
                "group": is_multi_worker,
                "workers": worker_names,
                "scribe": true,
            },
            "note": "Use 'user' for human, 'group' for team chat, worker name for DM"
        })
        .to_string())
    }

    /// Get chat message history, optionally filtered by contact.
    pub fn chat_history(&self, with: Option<&str>, limit: Option<usize>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let limit = limit.unwrap_or(50);

        // Translate contact to thread name
        let thread = with.map(|w| if w == "user" { worker_name.as_str() } else { w });

        let messages = if let Some(t) = thread {
            self.run_async(self.state().get_unread_messages(t, &worker_name))?
        } else {
            self.run_async(self.state().get_all_unread_messages(&worker_name))?
        };

        // Apply limit
        let messages: Vec<_> = messages.into_iter().take(limit).collect();

        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "from": m.sender,
                    "thread": m.thread,
                    "content": m.content,
                    "timestamp": m.timestamp,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "messages": msgs,
            "count": msgs.len(),
        })
        .to_string())
    }

    /// Send a chat message to a specific contact.
    pub fn chat_send(&self, to: &str, message: &str) -> WorkerResult<String> {
        // Translate "user" to worker's own thread for DM semantics
        let thread = if to == "user" {
            self.config.worker_name.as_str()
        } else {
            to
        };

        self.msg_send(thread, message)
    }

    /// Check for unread messages, optionally filtered by contact.
    pub fn chat_unread(&self, with: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();

        // Translate contact to thread name
        let thread = with.map(|w| {
            if w == "user" {
                worker_name.clone()
            } else {
                w.to_string()
            }
        });

        let threads = if let Some(t) = thread {
            vec![t]
        } else {
            self.run_async(self.state().get_threads())?
        };

        let mut unread = Vec::new();
        for thread in &threads {
            let messages =
                self.run_async(self.state().get_unread_messages(thread, &worker_name))?;

            if !messages.is_empty() {
                unread.push(serde_json::json!({
                    "from": thread,
                    "count": messages.len(),
                    "preview": messages.first().map(|m| m.content.chars().take(100).collect::<String>()),
                }));
            }
        }

        Ok(serde_json::json!({
            "has_unread": !unread.is_empty(),
            "threads": unread,
        })
        .to_string())
    }

    // =========================================================================
    // Work Done
    // =========================================================================

    /// Signal that worker has no more work to do.
    ///
    /// This sets the worker to Awaiting status. The daemon will detect this
    /// on its next polling cycle and handle eval triggering if all workers
    /// are inactive.
    ///
    /// Workers are "dumb" - they just do tasks and report status.
    /// The daemon handles all lifecycle management.
    pub fn work_done(&self) -> WorkerResult<String> {
        use crate::core::state::WorkerUpdate;

        // Set status to Awaiting
        self.set_status(WorkerStatus::Awaiting)?;

        // Clear assigned task (signals we're ready for a new assignment)
        self.run_async(self.state().update_worker(
            &self.config.worker_name,
            WorkerUpdate {
                assigned_task_id: Some(None),
                ..Default::default()
            },
        ))?;

        // Trigger scaling check - daemon might spawn us again with a new task
        self.run_async(self.state().request_scaling_check())?;

        tracing::info!(
            "[{}] work_done: status set to Awaiting, daemon will handle lifecycle",
            self.config.worker_name
        );

        Ok(serde_json::json!({
            "success": true,
            "worker": self.config.worker_name,
            "status": "awaiting",
            "message": "Work complete. Exiting - will be respawned if more tasks available.",
        })
        .to_string())
    }

    // =========================================================================
    // Time Status
    // =========================================================================

    /// Get time information for the run.
    pub fn get_time_info(&self) -> WorkerResult<Option<crate::core::state::TimeInfo>> {
        Ok(self.run_async(self.state().get_time_info())?)
    }

    // =========================================================================
    // Scribe - Documentation
    // =========================================================================

    /// Record a learning for the Scribe to integrate into docs.
    ///
    /// Learnings are batched and processed by an ephemeral Scribe agent
    /// that maintains documentation in the run's docs/ directory.
    pub fn scribe(&self, content: &str) -> WorkerResult<String> {
        let worker_name = &self.config.worker_name;
        self.run_async(self.state().add_scribe_submission(worker_name, content))?;

        Ok(serde_json::json!({
            "success": true,
            "message": "Learning recorded. The Scribe will integrate it into docs shortly.",
        })
        .to_string())
    }

    /// Read documentation maintained by the Scribe.
    ///
    /// Returns all docs or a specific file from the run's docs/ directory.
    pub fn read_docs(&self, file: Option<&str>) -> WorkerResult<String> {
        let files = Files::new(&self.config.run_dir);
        let docs = files.read_docs(file).map_err(|e| WorkerError::Io(e))?;

        Ok(serde_json::to_string(&docs).unwrap_or_else(|_| "{}".to_string()))
    }

    // =========================================================================
    // Eval Operations
    // =========================================================================

    /// Handle eval pass - validates all tasks in the validates list.
    /// Only available for eval task types.
    pub fn eval_pass(&self) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();

        // Get current task and verify it's an eval
        let task = self
            .run_async(self.state().get_claimed_task(&worker_name))?
            .ok_or(WorkerError::NoTaskClaimed)?;

        if task.task_type != TaskType::Eval {
            return Err(WorkerError::Config(
                "eval_pass is only available for eval tasks".into(),
            ));
        }

        // Call eval_pass on state
        self.run_async(self.state().eval_pass(&task.id, &worker_name))?;

        // Signal exit after response
        tracing::info!(
            "[{}] eval_pass: marking eval {} as passed",
            self.config.worker_name,
            task.id
        );

        Ok(serde_json::json!({
            "success": true,
            "task_id": task.id,
            "result": "pass",
            "message": "Eval passed. Validated tasks are now marked as validated.",
        })
        .to_string())
    }

    /// Handle eval fail - creates a repair task as child of the eval.
    /// Only available for eval task types.
    pub fn eval_fail(&self, feedback: &str) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();

        // Get current task and verify it's an eval
        let task = self
            .run_async(self.state().get_claimed_task(&worker_name))?
            .ok_or(WorkerError::NoTaskClaimed)?;

        if task.task_type != TaskType::Eval {
            return Err(WorkerError::Config(
                "eval_fail is only available for eval tasks".into(),
            ));
        }

        // Call eval_fail on state
        let repair_id = self.run_async(self.state().eval_fail(&task.id, &worker_name, feedback))?;

        tracing::info!(
            "[{}] eval_fail: eval {} failed, created repair task {}",
            self.config.worker_name,
            task.id,
            repair_id
        );

        Ok(serde_json::json!({
            "success": true,
            "task_id": task.id,
            "result": "fail",
            "repair_task_id": repair_id,
            "feedback": feedback,
            "message": "Eval failed. A repair task has been created.",
        })
        .to_string())
    }

    // =========================================================================
    // Command Execution
    // =========================================================================

    /// Execute a worker command and return JSON output.
    pub fn execute_command(&mut self, command: WorkerCommands) -> WorkerResult<String> {
        // Update heartbeat on each command
        if self.heartbeat_due() {
            self.heartbeat()?;
        }

        match command {
            WorkerCommands::Done => self.work_done(),

            WorkerCommands::Task(task_cmd) => match task_cmd {
                TaskSubcommands::List => self.get_task_tree(),
                TaskSubcommands::Add(args) => self.task_add(
                    &args.task_id,
                    &args.name,
                    args.parent.as_deref(),
                    &args.blocked_by,
                ),
                TaskSubcommands::Claim(args) => self.task_claim(&args.task_id),
                TaskSubcommands::Done(args) => self.task_done(args.task_id.as_deref()),
                TaskSubcommands::Unclaim(args) => self.task_unclaim(args.task_id.as_deref()),
                TaskSubcommands::Undone(args) => self.task_undone(&args.task_id),
                TaskSubcommands::Delete(args) => self.task_delete(&args.task_id),
                TaskSubcommands::Await => self.task_await(),
            },

            WorkerCommands::Msg(msg_cmd) => match msg_cmd {
                MsgSubcommands::Send(args) => self.msg_send(&args.thread, &args.message),
                MsgSubcommands::Read(args) => self.msg_read(args.thread.as_deref()),
                MsgSubcommands::List => self.msg_list(),
                MsgSubcommands::Inbox => self.msg_inbox(),
            },
        }
    }
}

/// Format a claim rejection reason into a human-readable message
fn format_claim_rejection(reason: &crate::core::state::ClaimRejectReason) -> String {
    use crate::core::state::ClaimRejectReason;

    match reason {
        ClaimRejectReason::NotFound { task_id } => {
            format!("Task '{}' not found", task_id)
        }
        ClaimRejectReason::AlreadyComplete { task_id, status } => {
            format!("Task '{}' is already {} - no work needed", task_id, status)
        }
        ClaimRejectReason::Blocked { task_id, blockers } => {
            let blocker_names: Vec<String> = blockers
                .iter()
                .map(|b| format!("{} ({})", b.task_id, b.status))
                .collect();
            format!(
                "Task '{}' is blocked by: {}. Complete those first.",
                task_id,
                blocker_names.join(", ")
            )
        }
        ClaimRejectReason::ClaimedByOther {
            task_id,
            claimed_by,
        } => {
            format!(
                "Task '{}' is already claimed by {}. Pick a different task.",
                task_id, claimed_by
            )
        }
        ClaimRejectReason::WorkerBusy { existing_task_id } => {
            format!(
                "You already have task '{}' claimed. Complete or unclaim it first.",
                existing_task_id
            )
        }
        ClaimRejectReason::HasChildren { task_id, children } => {
            format!(
                "Task '{}' has children ({}). Claim and complete child tasks instead.",
                task_id,
                children.join(", ")
            )
        }
        ClaimRejectReason::EvalNotReady {
            task_id,
            pending_tasks,
        } => {
            let pending_names: Vec<String> = pending_tasks.iter().map(|t| t.id.clone()).collect();
            format!(
                "Eval '{}' cannot be claimed yet. These tasks must be done first: {}",
                task_id,
                pending_names.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_config_new() {
        let config = WorkerConfig::new(
            "achilles".into(),
            "test-run".into(),
            PathBuf::from("/tmp/test-run"),
            vec!["hirsel".to_string(), "__acp-bridge".to_string()],
        );
        assert_eq!(config.worker_name, "achilles");
        assert_eq!(config.run_name, "test-run");
        assert_eq!(config.heartbeat_interval, 30);
    }

    #[test]
    fn test_worker_config_from_env_missing_vars() {
        // Clear env vars to ensure they're not set
        std::env::remove_var("HIRSEL_RUN");
        std::env::remove_var("HIRSEL_WORKER");

        let result = WorkerConfig::from_env();
        assert!(matches!(result, Err(WorkerError::Config(_))));
    }
}
