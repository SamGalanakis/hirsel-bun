//! Worker subprocess main loop implementation.
//!
//! The WorkerRunner manages the lifecycle of a single worker subprocess:
//! - Initializes connection to route runtime state
//! - Spawns and communicates with the AI agent
//! - Handles task claim/done cycle
//! - Manages heartbeats and status updates
//! - Reports upward to the orchestrator via structured concerns/progress
//!
//! Workers always access local SQLite state on the single host backend.

use crate::cli::{TaskSubcommands, WorkerCommands};
use crate::core::state::{SQLiteState, StateError, WorkerStatus, WorkerUpdate};
use crate::core::state_access::{StateAccess, StateAccessError};
use crate::core::Files;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Errors that can occur during worker operations.
#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("Runtime directory not found: {0}")]
    RuntimeNotFound(PathBuf),

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
    /// Name of the runtime.
    pub runtime_name: String,
    /// Path to the runtime directory.
    pub runtime_dir: PathBuf,
    /// Agent command to use for spawning workers.
    pub agent_command: Vec<String>,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval: u64,
}

impl WorkerConfig {
    /// Create a new worker configuration from environment variables.
    ///
    /// Expects HIRSEL_RUNTIME and HIRSEL_WORKER environment variables.
    pub fn from_env() -> WorkerResult<Self> {
        let runtime_name = std::env::var("HIRSEL_RUNTIME")
            .map_err(|_| WorkerError::Config("HIRSEL_RUNTIME not set".into()))?;

        let worker_name = std::env::var("HIRSEL_WORKER")
            .map_err(|_| WorkerError::Config("HIRSEL_WORKER not set".into()))?;

        // Get agent command from environment or default
        let agent_command = std::env::var("HIRSEL_AGENT_COMMAND")
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| vec!["hirsel".to_string(), "__worker-runtime".to_string()]);

        // Get runtime directory. Check HIRSEL_RUNTIME_DIR first (for custom mounts),
        // then fall back to HIRSEL_ROOT/runtimes/runtime_name.
        let runtime_dir = if let Ok(dir) = std::env::var("HIRSEL_RUNTIME_DIR") {
            PathBuf::from(dir)
        } else {
            let hirsel_root = std::env::var("HIRSEL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(".hirsel")
                });
            hirsel_root.join("runtimes").join(&runtime_name)
        };

        if !runtime_dir.exists() {
            return Err(WorkerError::RuntimeNotFound(runtime_dir));
        }

        Ok(Self {
            worker_name,
            runtime_name,
            runtime_dir,
            agent_command,
            heartbeat_interval: 30,
        })
    }

    /// Create a worker configuration with explicit values.
    pub fn new(
        worker_name: String,
        runtime_name: String,
        runtime_dir: PathBuf,
        agent_command: Vec<String>,
    ) -> Self {
        Self {
            worker_name,
            runtime_name,
            runtime_dir,
            agent_command,
            heartbeat_interval: 30,
        }
    }
}

/// Local SQLite state backend for worker subprocesses.
struct StateBackend {
    state: SQLiteState,
    runtime: tokio::runtime::Runtime,
}

/// The main worker subprocess runner.
///
pub struct WorkerRunner {
    config: WorkerConfig,
    backend: StateBackend,
    _files: Files,
    last_heartbeat: Instant,
}

impl WorkerRunner {
    /// Create a new worker runner backed by local SQLite state.
    pub fn new(config: WorkerConfig) -> WorkerResult<Self> {
        let files = Files::new(&config.runtime_dir);

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| WorkerError::Config(format!("Failed to create runtime: {}", e)))?;

        let state = runtime
            .block_on(SQLiteState::new(&config.runtime_name))
            .map_err(WorkerError::State)?;

        let workers = runtime
            .block_on(state.get_workers())
            .map_err(WorkerError::State)?;
        if !workers.iter().any(|w| w.name == config.worker_name) {
            return Err(WorkerError::WorkerNotRegistered(config.worker_name.clone()));
        }

        Ok(Self {
            config,
            backend: StateBackend { state, runtime },
            _files: files,
            last_heartbeat: Instant::now(),
        })
    }

    /// Get the worker name.
    pub fn worker_name(&self) -> &str {
        &self.config.worker_name
    }

    /// Get the runtime directory.
    pub fn runtime_dir(&self) -> &PathBuf {
        &self.config.runtime_dir
    }

    /// Get access to the local SQLite state for direct queries.
    pub fn local_state(&self) -> &SQLiteState {
        &self.backend.state
    }

    /// Execute an async operation on the state backend.
    fn run_async<F, T>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        self.backend.runtime.block_on(f)
    }

    /// Get a reference to the state as a trait object for async operations.
    fn state(&self) -> &dyn StateAccess {
        &self.backend.state
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
    // Task operations backed by the route-scoped work tree state.
    // =========================================================================

    /// Get the full task tree with hierarchy and status.
    /// Returns a hierarchical structure with dependencies using persisted work items.
    pub fn get_task_tree(&self) -> WorkerResult<String> {
        use crate::core::delta::{BoardNode, NodeKind};

        let nodes = self.run_async(self.state().get_nodes())?;

        // Build a map for quick lookups
        let node_map: std::collections::HashMap<String, &BoardNode> =
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
            node: &BoardNode,
            children_map: &std::collections::HashMap<String, Vec<String>>,
            node_map: &std::collections::HashMap<String, &BoardNode>,
            state: &dyn crate::core::state_access::StateAccess,
            runner: &WorkerRunner,
        ) -> serde_json::Value {
            let blocked = runner
                .run_async(state.is_node_blocked(&node.id))
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
                "type": node.kind.as_str(),
                "status": node.status.as_str(),
                "claimed_by": node.claimed_by,
                "blocked": blocked,
            });

            if !node.blocked_by.is_empty() {
                json["blocked_by"] = serde_json::json!(&node.blocked_by);
            }

            if node.kind == NodeKind::Check {
                if let Ok(validates) = runner.run_async(state.get_validated_nodes(&node.id)) {
                    if !validates.is_empty() {
                        json["validates"] = serde_json::json!(validates);
                    }
                }
                if let Some(result) = &node.check_result {
                    json["check_result"] = serde_json::json!(result.as_str());
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
                    "type": n.kind.as_str(),
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
        let claimed = self.run_async(self.state().get_claimed_node(&worker_name))?;

        let tasks: Vec<serde_json::Value> = claimed
            .into_iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "type": n.kind.as_str(),
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

    /// Get full details for a specific board node.
    pub fn get_task_details(&self, task_id: &str) -> WorkerResult<String> {
        use crate::core::delta::NodeKind;

        let nodes = self.run_async(self.state().get_nodes())?;
        let node = nodes
            .into_iter()
            .find(|n| n.id == task_id)
            .ok_or_else(|| WorkerError::Config(format!("Task '{}' not found", task_id)))?;

        let blocked = self
            .run_async(self.state().is_node_blocked(&node.id))
            .unwrap_or(false);

        let mut output = serde_json::json!({
            "id": node.id,
            "name": node.name,
            "type": node.kind.as_str(),
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

        if node.kind == NodeKind::Check {
            if let Ok(validates) = self.run_async(self.state().get_validated_nodes(&node.id)) {
                if !validates.is_empty() {
                    output["validates"] = serde_json::json!(validates);
                }
            }
            if let Some(result) = &node.check_result {
                output["check_result"] = serde_json::json!(result.as_str());
            }
            if let Some(feedback) = &node.check_feedback {
                output["check_feedback"] = serde_json::json!(feedback);
            }
        }

        serde_json::to_string_pretty(&output)
            .map_err(|e| WorkerError::Config(format!("Serialization error: {}", e)))
    }

    /// Mark a task (board node) as done and return control to the orchestrator.
    ///
    /// This:
    /// 1. Marks the task as complete (unblocks dependent tasks) - if a task is claimed
    /// 2. Sets worker to Awaiting status so the current process can exit cleanly
    ///
    /// Workers do not auto-pick follow-up work. The orchestrator decides what to
    /// delegate next after this worker finishes.
    ///
    /// For runtimes without persisted work items, this just signals completion
    /// without completing a specific task.
    pub fn task_done(&self, task_id: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();

        // Try to get the task ID - either from parameter or from claimed node
        let tid = match task_id {
            Some(id) => Some(id.to_string()),
            None => {
                // Try to get currently claimed node - but don't fail if none
                self.run_async(self.state().get_claimed_node(&worker_name))
                    .ok()
                    .flatten()
                    .map(|node| node.id)
            }
        };

        // If we have a task to complete, mark it as done
        if let Some(ref task_id) = tid {
            // Mark the node as done
            if let Err(e) = self.run_async(self.state().complete_node(task_id, &worker_name)) {
                tracing::warn!(
                    "[{}] Failed to complete node '{}': {} (continuing with worker exit)",
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
                "[{}] Completed task '{}', returning control to orchestrator",
                self.config.worker_name,
                task_id
            );
        } else {
            // No task was claimed - this is valid for legacy direct worker flows
            tracing::info!(
                "[{}] No task claimed, marking worker awaiting so orchestration can continue",
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

        Ok(serde_json::json!({
            "success": true,
            "task_id": tid,
            "status": "awaiting",
            "message": "Work complete. Worker will exit and control returns to the orchestrator.",
        })
        .to_string())
    }

    /// Add a new task as a board node.
    pub fn task_add(
        &self,
        task_id: &str,
        name: &str,
        parent: Option<&str>,
        blocked_by: &[String],
    ) -> WorkerResult<String> {
        let blocked_refs: Vec<&str> = blocked_by.iter().map(|s| s.as_str()).collect();

        self.run_async(self.state().add_node(
            task_id,
            name,
            parent,
            if blocked_refs.is_empty() {
                None
            } else {
                Some(blocked_refs.as_slice())
            },
            "task", // kind
            "",     // content - empty for worker-added tasks
            None,   // validates - not used for tasks
        ))?;

        tracing::debug!("[{}] Added node '{}'", self.config.worker_name, task_id);

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
        })
        .to_string())
    }

    /// Add a new check node to the board.
    ///
    /// The validates list writes validated_by on target nodes via the junction table.
    /// If no parent is given but validates is non-empty, auto-parents under the
    /// root feature of the first validated node.
    pub fn add_check(
        &self,
        check_id: &str,
        name: &str,
        parent: Option<&str>,
        validates: &[String],
    ) -> WorkerResult<String> {
        let validates_refs: Vec<&str> = validates.iter().map(|s| s.as_str()).collect();

        // Auto-parent: if no explicit parent, walk up from first validated node
        let resolved_parent = match parent {
            Some(p) => Some(p.to_string()),
            None if !validates.is_empty() => self.find_root_feature(&validates[0]),
            None => None,
        };

        self.run_async(self.state().add_node(
            check_id,
            name,
            resolved_parent.as_deref(),
            None, // No blocked_by (check uses validates relationship)
            "check",
            "", // No content
            if validates_refs.is_empty() {
                None
            } else {
                Some(validates_refs.as_slice())
            },
        ))?;

        Ok(serde_json::json!({
            "success": true,
            "check_id": check_id,
            "validates": validates,
        })
        .to_string())
    }

    /// Walk up the parent chain to find the root feature of a node.
    fn find_root_feature(&self, node_id: &str) -> Option<String> {
        use crate::core::delta::NodeKind;

        let nodes = self.run_async(self.state().get_nodes()).ok()?;
        let find = |id: &str| nodes.iter().find(|n| n.id == id);

        let mut current = find(node_id)?;
        loop {
            match &current.parent_id {
                Some(pid) => {
                    current = find(pid)?;
                }
                None => {
                    return if current.kind == NodeKind::Feature {
                        Some(current.id.clone())
                    } else {
                        None
                    };
                }
            }
        }
    }

    /// Delete a task (board node) by ID.
    ///
    /// Only tasks with source='worker' can be deleted (not spec tasks).
    /// Cannot delete tasks that are currently claimed or completed.
    pub fn delete_task(&self, task_id: &str) -> WorkerResult<String> {
        use crate::core::delta::{BoardNodeSource, BoardNodeStatus};

        // Get the node first to validate it can be deleted
        let nodes = self.run_async(self.state().get_nodes())?;
        let node = nodes
            .iter()
            .find(|n| n.id == task_id)
            .ok_or_else(|| WorkerError::Config(format!("Task '{}' not found", task_id)))?;

        // Check if it's a worker-created task
        if node.source != BoardNodeSource::Worker {
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
        if node.status == BoardNodeStatus::Done {
            return Ok(serde_json::json!({
                "success": false,
                "error": "Cannot delete a completed task",
                "task_id": task_id,
            })
            .to_string());
        }

        // Delete the node
        self.run_async(self.state().delete_node(task_id))?;

        tracing::debug!("[{}] Deleted node '{}'", self.config.worker_name, task_id);

        Ok(serde_json::json!({
            "success": true,
            "task_id": task_id,
            "message": format!("Task '{}' deleted", task_id),
        })
        .to_string())
    }

    // =========================================================================
    // Concern / Report Operations
    // =========================================================================

    fn get_project_id_for_reporting(&self) -> WorkerResult<i64> {
        self.run_async(self.state().get_project_id())?
            .ok_or_else(|| {
                WorkerError::Config("Reporting requires a route-bound project runtime".to_string())
            })
    }

    fn submit_concern(
        &self,
        kind: &str,
        severity: &str,
        summary: &str,
        details: Option<&str>,
        status: &str,
        blocking: bool,
    ) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let project_id = self.get_project_id_for_reporting()?;
        let concern = self.run_async(self.state().create_worker_concern(
            project_id,
            &worker_name,
            kind,
            severity,
            summary,
            details,
            status,
            Some("worker"),
        ))?;

        if blocking {
            self.set_status(WorkerStatus::Awaiting)?;
            self.run_async(self.state().update_worker(
                &worker_name,
                WorkerUpdate {
                    hitl_waiting: Some(false),
                    waiting_thread: Some("orchestrator".to_string()),
                    ..Default::default()
                },
            ))?;
        }

        Ok(serde_json::json!({
            "success": true,
            "concern_id": concern.id,
            "kind": concern.kind,
            "severity": concern.severity,
            "status": concern.status,
            "blocking": blocking,
        })
        .to_string())
    }

    pub fn report_progress(&self, summary: &str, details: Option<&str>) -> WorkerResult<String> {
        let worker_name = self.config.worker_name.clone();
        let claimed = self.run_async(self.state().get_claimed_node(&worker_name))?;

        if let Some(node) = claimed {
            self.run_async(self.state().record_work_item_event(
                &node.id,
                "worker",
                &worker_name,
                "progress",
                summary,
                details,
            ))?;
            return Ok(serde_json::json!({
                "success": true,
                "recorded": true,
                "item_id": node.id,
            })
            .to_string());
        }

        Ok(serde_json::json!({
            "success": true,
            "recorded": false,
        })
        .to_string())
    }

    pub fn raise_concern(
        &self,
        kind: &str,
        summary: &str,
        details: Option<&str>,
        severity: Option<&str>,
        blocking: bool,
    ) -> WorkerResult<String> {
        self.submit_concern(
            kind,
            severity.unwrap_or(if blocking { "high" } else { "medium" }),
            summary,
            details,
            "open",
            blocking,
        )
    }

    pub fn request_decision(&self, summary: &str, details: Option<&str>) -> WorkerResult<String> {
        self.submit_concern("decision_needed", "high", summary, details, "open", true)
    }

    // =========================================================================
    // Time Status
    // =========================================================================

    /// Get time information for the run.
    pub fn get_time_info(&self) -> WorkerResult<Option<crate::core::state::TimeInfo>> {
        Ok(self.run_async(self.state().get_time_info())?)
    }

    /// Record durable project context for later condensation by the scribe.
    pub fn scribe(&self, content: &str) -> WorkerResult<String> {
        let worker_name = &self.config.worker_name;
        self.run_async(self.state().add_scribe_submission(worker_name, content))?;

        Ok(serde_json::json!({
            "success": true,
            "message": "Recorded retained context for project scribe processing.",
        })
        .to_string())
    }

    /// Read the project-level retained context artifact.
    pub fn read_retained_context(&self) -> WorkerResult<String> {
        let markdown = self
            .run_async(self.state().read_retained_context())
            .map_err(|e| WorkerError::Config(format!("Failed to read retained context: {}", e)))?;

        Ok(serde_json::json!({ "markdown": markdown }).to_string())
    }

    // =========================================================================
    // Check Operations
    // =========================================================================

    /// Handle check pass - validates all nodes in the validates list.
    /// Only available for check node types.
    pub fn check_pass(&self) -> WorkerResult<String> {
        use crate::core::delta::NodeKind;

        let worker_name = self.config.worker_name.clone();

        // Get current node and verify it's a check
        let node = self
            .run_async(self.state().get_claimed_node(&worker_name))?
            .ok_or(WorkerError::NoTaskClaimed)?;

        if node.kind != NodeKind::Check {
            return Err(WorkerError::Config(
                "check_pass is only available for check nodes".into(),
            ));
        }

        self.run_async(self.state().node_check_pass(&node.id, &worker_name))?;
        tracing::info!(
            "[{}] check_pass: marking check {} as passed",
            self.config.worker_name,
            node.id
        );

        Ok(serde_json::json!({
            "success": true,
            "task_id": node.id,
            "result": "pass",
            "message": "Check passed. Validated nodes are now marked as validated.",
        })
        .to_string())
    }

    /// Handle check fail - creates a repair node as child of the check.
    /// Only available for check node types.
    pub fn check_fail(&self, feedback: &str) -> WorkerResult<String> {
        use crate::core::delta::NodeKind;

        let worker_name = self.config.worker_name.clone();

        // Get current node and verify it's a check
        let node = self
            .run_async(self.state().get_claimed_node(&worker_name))?
            .ok_or(WorkerError::NoTaskClaimed)?;

        if node.kind != NodeKind::Check {
            return Err(WorkerError::Config(
                "check_fail is only available for check nodes".into(),
            ));
        }

        let repair_id = self.run_async(self.state().node_check_fail(
            &node.id,
            &worker_name,
            feedback,
        ))?;
        tracing::info!(
            "[{}] check_fail: check {} failed, created repair node {}",
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
            "message": "Check failed. A repair node has been created.",
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
            WorkerCommands::Done => self.task_done(None),

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
            vec!["hirsel".to_string(), "__worker-runtime".to_string()],
        );
        assert_eq!(config.worker_name, "achilles");
        assert_eq!(config.runtime_name, "test-run");
        assert_eq!(config.heartbeat_interval, 30);
    }

    #[test]
    fn test_worker_config_from_env_missing_vars() {
        // Clear env vars to ensure they're not set
        std::env::remove_var("HIRSEL_RUNTIME");
        std::env::remove_var("HIRSEL_WORKER");

        let result = WorkerConfig::from_env();
        assert!(matches!(result, Err(WorkerError::Config(_))));
    }
}
