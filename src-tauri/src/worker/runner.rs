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
use crate::core::state::{SQLiteState, StateError, WorkerStatus, WorkerUpdate};
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
    Local {
        state: SQLiteState,
        runtime: tokio::runtime::Runtime,
    },
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

            // Create a runtime for async operations since WorkerRunner::new is sync
            let runtime = tokio::runtime::Runtime::new()
                .map_err(|e| WorkerError::Config(format!("Failed to create runtime: {}", e)))?;

            let state = runtime
                .block_on(SQLiteState::new(&config.run_name))
                .map_err(WorkerError::State)?;

            // Verify worker exists in database
            let workers = runtime
                .block_on(state.get_workers())
                .map_err(WorkerError::State)?;
            if !workers.iter().any(|w| w.name == config.worker_name) {
                return Err(WorkerError::WorkerNotRegistered(config.worker_name.clone()));
            }

            Ok(Self {
                config,
                backend: StateBackend::Local { state, runtime },
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
            StateBackend::Local { state, .. } => Some(state),
            StateBackend::Remote { .. } => None,
        }
    }

    /// Execute an async operation on the state backend.
    fn run_async<F, T>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        match &self.backend {
            StateBackend::Local { runtime, .. } => runtime.block_on(f),
            StateBackend::Remote { runtime, .. } => runtime.block_on(f),
        }
    }

    /// Get a reference to the state as a trait object for async operations.
    fn state(&self) -> &dyn StateAccess {
        match &self.backend {
            StateBackend::Local { state, .. } => state,
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
    // Task Operations (using live_nodes)
    // =========================================================================

    /// Get the full task tree with hierarchy and status.
    /// Returns a hierarchical structure with dependencies using live_nodes.
    pub fn get_task_tree(&self) -> WorkerResult<String> {
        use crate::core::delta::{LiveNode, NodeType};

        let nodes = self.run_async(self.state().get_live_nodes())?;

        // Build a map for quick lookups
        let node_map: std::collections::HashMap<String, &LiveNode> =
            nodes.iter().map(|n| (n.id.clone(), n)).collect();

        // Find root nodes (no parent) and build tree structure
        let mut roots: Vec<serde_json::Value> = Vec::new();
        let mut children_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        for n in &nodes {
            if let Some(parent_id) = &n.parent_id {
                children_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(n.id.clone());
            }
        }

        fn build_node(
            node: &LiveNode,
            children_map: &std::collections::HashMap<String, Vec<String>>,
            node_map: &std::collections::HashMap<String, &LiveNode>,
            state: &dyn crate::core::state_access::StateAccess,
            runner: &WorkerRunner,
        ) -> serde_json::Value {
            let blocked = runner
                .run_async(state.is_live_node_blocked(&node.id))
                .unwrap_or(false);

            let children: Vec<serde_json::Value> = children_map
                .get(&node.id)
                .map(|child_ids| {
                    child_ids
                        .iter()
                        .filter_map(|cid| node_map.get(cid))
                        .map(|c| build_node(c, children_map, node_map, state, runner))
                        .collect()
                })
                .unwrap_or_default();

            let mut json = serde_json::json!({
                "id": node.id,
                "name": node.name,
                "type": node.node_type.as_str(),
                "status": node.status.as_str(),
                "claimed_by": node.claimed_by,
                "blocked": blocked,
            });

            if !node.blocked_by.is_empty() {
                json["blocked_by"] = serde_json::json!(&node.blocked_by);
            }

            if node.node_type == NodeType::Eval {
                if let Ok(validates) = runner.run_async(state.get_validated_nodes(&node.id)) {
                    if !validates.is_empty() {
                        json["validates"] = serde_json::json!(validates);
                    }
                }
                if let Some(result) = &node.eval_result {
                    json["eval_result"] = serde_json::json!(result.as_str());
                }
            }

            if !children.is_empty() {
                json["children"] = serde_json::json!(children);
            }

            json
        }

        for n in &nodes {
            if n.parent_id.is_none() {
                roots.push(build_node(n, &children_map, &node_map, self.state(), self));
            }
        }

        let output = serde_json::json!({
            "tasks": roots,
            "total_count": nodes.len(),
        });
        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Get tasks that are ready to claim (unblocked, unclaimed, pending status).
    pub fn get_available_tasks(&self) -> WorkerResult<String> {
        let claimable = self.run_async(self.state().get_claimable_nodes())?;

        let tasks: Vec<serde_json::Value> = claimable
            .iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "type": n.node_type.as_str(),
                    "parent": n.parent_id,
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
        let claimed = self.run_async(self.state().get_claimed_live_node(&worker_name))?;

        let tasks: Vec<serde_json::Value> = claimed
            .into_iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "type": n.node_type.as_str(),
                    "status": n.status.as_str(),
                    "claimed_at": n.claimed_at,
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

    /// Get full details for a specific task (live node).
    pub fn get_task_details(&self, task_id: &str) -> WorkerResult<String> {
        use crate::core::delta::NodeType;

        let nodes = self.run_async(self.state().get_live_nodes())?;
        let node = nodes
            .into_iter()
            .find(|n| n.id == task_id)
            .ok_or_else(|| WorkerError::Config(format!("Task '{}' not found", task_id)))?;

        let blocked = self
            .run_async(self.state().is_live_node_blocked(&node.id))
            .unwrap_or(false);

        let mut output = serde_json::json!({
            "id": node.id,
            "name": node.name,
            "type": node.node_type.as_str(),
            "status": node.status.as_str(),
            "claimed_by": node.claimed_by,
            "claimed_at": node.claimed_at,
            "created_at": node.created_at,
            "completed_at": node.completed_at,
            "parent": node.parent_id,
            "blocked": blocked,
        });

        if !node.blocked_by.is_empty() {
            output["blocked_by"] = serde_json::json!(&node.blocked_by);
        }

        // Node content
        if !node.content.is_empty() {
            output["content"] = serde_json::json!(&node.content);
        }

        if node.node_type == NodeType::Eval {
            if let Ok(validates) = self.run_async(self.state().get_validated_nodes(&node.id)) {
                if !validates.is_empty() {
                    output["validates"] = serde_json::json!(validates);
                }
            }
            if let Some(result) = &node.eval_result {
                output["eval_result"] = serde_json::json!(result.as_str());
            }
            if let Some(feedback) = &node.eval_feedback {
                output["eval_feedback"] = serde_json::json!(feedback);
            }
        }

        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Mark a task (live node) as done and signal ready for new work.
    ///
    /// This:
    /// 1. Marks the task as complete (unblocks dependent tasks) - if a task is claimed
    /// 2. Sets worker to Awaiting status (triggers process exit)
    /// 3. Requests scaling check (daemon will respawn with new task if available)
    ///
    /// Workers are "dumb" - they do one task, then exit and get respawned.
    ///
    /// For runs without live_nodes (e.g., CLI runs via `hirsel go`), this will
    /// just signal completion without completing a specific task.
    pub fn task_done(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();

        // Try to get the task ID - either from parameter or from claimed node
        let tid = match task_id {
            Some(id) => Some(id.to_string()),
            None => {
                // Try to get currently claimed node - but don't fail if none
                self.run_async(self.state().get_claimed_live_node(&worker_name))
                    .ok()
                    .flatten()
                    .map(|node| node.id)
            }
        };

        // If we have a task to complete, mark it as done
        if let Some(ref task_id) = tid {
            // Mark the live node as done
            if let Err(e) = self.run_async(self.state().complete_live_node(task_id, &worker_name)) {
                tracing::warn!(
                    "[{}] Failed to complete live node '{}': {} (continuing with worker exit)",
                    self.config.worker_name,
                    task_id,
                    e
                );
            }

            // Update worker: clear assigned_task_id, set last_task_id for tree distance
            self.run_async(self.state().update_worker(
                &worker_name,
                WorkerUpdate {
                    assigned_task_id: Some(None),              // Clear current assignment
                    last_task_id: Some(Some(task_id.clone())), // Track for tree distance
                    ..Default::default()
                },
            ))?;

            tracing::info!(
                "[{}] Completed task '{}', setting Awaiting for respawn",
                self.config.worker_name,
                task_id
            );
        } else {
            // No task was claimed - this is valid for runs without live_nodes (CLI runs)
            tracing::info!(
                "[{}] No task claimed, setting Awaiting for respawn (CLI run mode)",
                self.config.worker_name
            );

            // Clear any assigned task in worker record
            self.run_async(self.state().update_worker(
                &worker_name,
                WorkerUpdate {
                    assigned_task_id: Some(None),
                    ..Default::default()
                },
            ))?;
        }

        // Set status to Awaiting - this triggers worker termination
        self.set_status(WorkerStatus::Awaiting)?;

        // Trigger scaling check - daemon will respawn with new task if available
        if let Err(e) = self.run_async(self.state().request_scaling_check()) {
            tracing::warn!(
                "[{}] Failed to request scaling check: {} (worker will still exit)",
                self.config.worker_name,
                e
            );
        }

        Ok(serde_json::json!({
            "success": true,
            "task_id": tid,
            "status": "awaiting",
            "message": "Work complete. Worker will exit and be respawned if more tasks available.",
        })
        .to_string())
    }

    /// Add a new task as a live node.
    pub fn task_add(
        &self,
        task_id: &str,
        name: &str,
        parent: Option<&str>,
        blocked_by: &[String],
    ) -> WorkerResult<String> {
        let blocked_refs: Vec<&str> = blocked_by.iter().map(|s| s.as_str()).collect();

        self.run_async(self.state().add_live_node(
            task_id,
            name,
            parent,
            if blocked_refs.is_empty() {
                None
            } else {
                Some(blocked_refs.as_slice())
            },
            "task", // node_type
            "",     // content - empty for worker-added tasks
        ))?;

        tracing::debug!(
            "[{}] Added live node '{}'",
            self.config.worker_name,
            task_id
        );

        // New unblocked task might be claimable - request scaling check
        if blocked_refs.is_empty() {
            if let Err(e) = self.run_async(self.state().request_scaling_check()) {
                tracing::warn!(
                    "[{}] Failed to request scaling check: {}",
                    self.config.worker_name,
                    e
                );
            }
        }

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Add a new eval task as a live node.
    ///
    /// Note: The validates relationship is stored in the content field as JSON.
    pub fn add_eval(
        &self,
        eval_id: &str,
        name: &str,
        validates: &[String],
    ) -> WorkerResult<String> {
        // Store validates in content as JSON
        let content = serde_json::json!({ "validates": validates }).to_string();

        self.run_async(self.state().add_live_node(
            eval_id, name, None, // No parent
            None, // No blocked_by (eval uses validates relationship)
            "eval", &content,
        ))?;

        Ok(serde_json::json!({
            "success": true,
            "eval_id": eval_id,
            "validates": validates,
        })
        .to_string())
    }

    /// Delete a task (live node) by ID.
    ///
    /// Only tasks with source='worker' can be deleted (not spec tasks).
    /// Cannot delete tasks that are currently claimed or completed.
    pub fn delete_task(&self, task_id: &str) -> WorkerResult<String> {
        use crate::core::delta::{LiveNodeSource, LiveNodeStatus};

        // Get the node first to validate it can be deleted
        let nodes = self.run_async(self.state().get_live_nodes())?;
        let node = nodes
            .iter()
            .find(|n| n.id == task_id)
            .ok_or_else(|| WorkerError::Config(format!("Task '{}' not found", task_id)))?;

        // Check if it's a worker-created task
        if node.source != LiveNodeSource::Worker {
            return Ok(serde_json::json!({
                "success": false,
                "error": "Cannot delete spec tasks, only worker-created tasks can be deleted",
                "task_id": task_id,
            })
            .to_string());
        }

        // Check if it's currently claimed
        if node.claimed_by.is_some() {
            return Ok(serde_json::json!({
                "success": false,
                "error": "Cannot delete a claimed task",
                "task_id": task_id,
                "claimed_by": node.claimed_by,
            })
            .to_string());
        }

        // Check if it's already completed
        if node.status == LiveNodeStatus::Done {
            return Ok(serde_json::json!({
                "success": false,
                "error": "Cannot delete a completed task",
                "task_id": task_id,
            })
            .to_string());
        }

        // Delete the node
        self.run_async(self.state().delete_live_node(task_id))?;

        tracing::debug!(
            "[{}] Deleted live node '{}'",
            self.config.worker_name,
            task_id
        );

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
            "message": format!("Task '{}' deleted", task_id),
        })
        .to_string())
    }

    // =========================================================================
    // Message Operations (Project-Level via Sheepfold)
    //
    // All messages are stored in the global project_messages table.
    // Thread naming:
    // - "chat" = group chat (all workers + human)
    // - Worker names = DMs (e.g., "willow-coopworth")
    //
    // Semantic aliases:
    // - "user" → worker's own name (DM with human)
    // - "group" → "chat" (group chat)
    // =========================================================================

    /// Get the project_id for messaging. Returns error if not in a board run.
    fn get_project_id_for_messaging(&self) -> WorkerResult<i64> {
        self.run_async(self.state().get_project_id())?
            .ok_or_else(|| {
                WorkerError::Config("Messaging requires a project context (board run)".to_string())
            })
    }

    /// Translate semantic thread names to actual thread names.
    /// - "user" → worker's own name (DM)
    /// - "group" → "chat" (group chat)
    fn translate_thread(&self, thread: &str) -> String {
        match thread {
            "user" => self.config.worker_name.clone(),
            "group" => "chat".to_string(),
            _ => thread.to_string(),
        }
    }

    /// Send a message to a thread.
    /// When thread is "user", messages are sent to the worker's own DM thread
    /// and HITL pause is triggered automatically (if HITL mode is enabled).
    pub fn msg_send(&self, thread: &str, message: &str) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let project_id = self.get_project_id_for_messaging()?;
        let is_user_dm = thread == "user";

        // Translate semantic thread to actual thread
        let actual_thread = self.translate_thread(thread);

        self.run_async(self.state().add_project_message(
            project_id,
            &actual_thread,
            &worker_name,
            message,
            is_user_dm, // waiting flag for HITL
        ))?;

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
                    waiting_thread: Some(actual_thread.clone()),
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
        let project_id = self.get_project_id_for_messaging()?;

        let messages = if let Some(t) = thread {
            let actual_thread = self.translate_thread(t);
            self.run_async(self.state().get_unread_project_messages(
                project_id,
                &actual_thread,
                &worker_name,
            ))?
        } else {
            self.run_async(
                self.state()
                    .get_all_unread_project_messages(project_id, &worker_name),
            )?
        };

        // Mark messages as read
        for msg in &messages {
            let _ = self.run_async(self.state().mark_project_messages_read(
                project_id,
                &msg.thread,
                &worker_name,
            ));
        }

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
        let project_id = self.get_project_id_for_messaging()?;
        let threads = self.run_async(self.state().get_project_threads(project_id))?;

        Ok(serde_json::json!({
            "threads": threads,
        })
        .to_string())
    }

    /// Check inbox for new messages.
    pub fn msg_inbox(&self) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let project_id = self.get_project_id_for_messaging()?;
        let threads = self.run_async(self.state().get_project_threads(project_id))?;

        let mut inbox = Vec::new();
        for thread in &threads {
            let messages = self.run_async(self.state().get_unread_project_messages(
                project_id,
                thread,
                &worker_name,
            ))?;

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
    // Chat API (cleaner interface for workers)
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
        let project_id = self.get_project_id_for_messaging()?;
        let limit = limit.unwrap_or(50) as i64;

        let messages = if let Some(contact) = with {
            let actual_thread = self.translate_thread(contact);
            self.run_async(
                self.state()
                    .get_project_messages(project_id, &actual_thread, limit),
            )?
        } else {
            // Get from all threads (limited)
            self.run_async(
                self.state()
                    .get_all_unread_project_messages(project_id, &worker_name),
            )?
        };

        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .take(limit as usize)
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
        self.msg_send(to, message)
    }

    /// Check for unread messages, optionally filtered by contact.
    pub fn chat_unread(&self, with: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let project_id = self.get_project_id_for_messaging()?;

        let threads = if let Some(contact) = with {
            vec![self.translate_thread(contact)]
        } else {
            self.run_async(self.state().get_project_threads(project_id))?
        };

        let mut unread = Vec::new();
        for thread in &threads {
            let messages = self.run_async(self.state().get_unread_project_messages(
                project_id,
                thread,
                &worker_name,
            ))?;

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
    // Work Done (alias for task_done)
    // =========================================================================

    /// Signal that worker has completed its task and is ready for new work.
    ///
    /// This is an alias for `task_done()` that auto-detects the assigned task.
    /// Kept for backward compatibility with MCP tools.
    pub fn work_done(&self) -> WorkerResult<String> {
        // task_done with None will auto-detect the claimed task
        self.task_done(None)
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
        let docs = files.read_docs(file).map_err(WorkerError::Io)?;

        Ok(serde_json::to_string(&docs).unwrap_or_else(|_| "{}".to_string()))
    }

    // =========================================================================
    // Eval Operations
    // =========================================================================

    /// Handle eval pass - validates all nodes in the validates list.
    /// Only available for eval node types.
    pub fn eval_pass(&self) -> WorkerResult<String> {
        use crate::core::delta::NodeType;

        let worker_name = self.config.worker_name.clone();

        // Get current node and verify it's an eval
        let node = self
            .run_async(self.state().get_claimed_live_node(&worker_name))?
            .ok_or(WorkerError::NoTaskClaimed)?;

        if node.node_type != NodeType::Eval {
            return Err(WorkerError::Config(
                "eval_pass is only available for eval nodes".into(),
            ));
        }

        self.run_async(self.state().live_node_eval_pass(&node.id, &worker_name))?;
        tracing::info!(
            "[{}] eval_pass: marking live node eval {} as passed",
            self.config.worker_name,
            node.id
        );

        Ok(serde_json::json!({
            "success": true,
            "task_id": node.id,
            "result": "pass",
            "message": "Eval passed. Validated nodes are now marked as validated.",
        })
        .to_string())
    }

    /// Handle eval fail - creates a repair node as child of the eval.
    /// Only available for eval node types.
    pub fn eval_fail(&self, feedback: &str) -> WorkerResult<String> {
        use crate::core::delta::NodeType;

        let worker_name = self.config.worker_name.clone();

        // Get current node and verify it's an eval
        let node = self
            .run_async(self.state().get_claimed_live_node(&worker_name))?
            .ok_or(WorkerError::NoTaskClaimed)?;

        if node.node_type != NodeType::Eval {
            return Err(WorkerError::Config(
                "eval_fail is only available for eval nodes".into(),
            ));
        }

        let repair_id = self.run_async(self.state().live_node_eval_fail(
            &node.id,
            &worker_name,
            feedback,
        ))?;
        tracing::info!(
            "[{}] eval_fail: live node eval {} failed, created repair node {}",
            self.config.worker_name,
            node.id,
            repair_id
        );

        Ok(serde_json::json!({
            "success": true,
            "task_id": node.id,
            "result": "fail",
            "repair_task_id": repair_id,
            "feedback": feedback,
            "message": "Eval failed. A repair node has been created.",
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
                TaskSubcommands::Done(args) => self.task_done(args.task_id.as_deref()),
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
