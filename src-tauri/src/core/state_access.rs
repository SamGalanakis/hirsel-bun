//! Worker-facing state operations for task claiming, messaging, and heartbeats.
//!
//! This module provides the `StateAccess` trait that abstracts low-level state
//! operations needed by workers during run execution. Workers use this trait
//! to interact with run state without knowing if they're accessing SQLite directly
//! (local workers) or via HTTP (remote workers through SSH tunnels).
//!
//! ## StateAccess vs Orchestrator
//!
//! These two traits serve different purposes:
//!
//! - **`StateAccess`** (this module): Worker-side, per-run operations during execution
//!   - Task claiming and completion
//!   - Worker heartbeats and status updates
//!   - Message sending between workers
//!   - Reading/writing run configuration
//!
//! - **`Orchestrator`** (see `orchestrator` module): Coordinator-side, cross-run management
//!   - Creating and deleting runs
//!   - Spawning workers
//!   - Managing run lifecycle (pause, resume, deliver)
//!   - Listing runs and their status
//!
//! Workers receive a `Box<dyn StateAccess>` and use it for all state operations.
//! The CLI/GUI uses `Box<dyn Orchestrator>` for run management commands.

use async_trait::async_trait;

use crate::core::state::{Eval, Message, Status, TimeInfo, Worker, WorkerStatus, WorkerUpdate};

/// Error type for state access operations
#[derive(Debug, thiserror::Error)]
pub enum StateAccessError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

pub type StateAccessResult<T> = Result<T, StateAccessError>;

/// Protocol for state operations - implemented by SQLiteState and HttpState.
///
/// This defines the interface that both local (SQLite) and remote (HTTP) state
/// implementations must support. Workers use this interface and don't care
/// whether they're accessing state directly or via HTTP.
///
/// Note: We don't require `Sync` because SQLiteState uses RefCell internally.
/// Each worker owns its own state instance.
#[async_trait(?Send)]
pub trait StateAccess: Send {
    // =========================================================================
    // Run status
    // =========================================================================

    async fn status(&self) -> StateAccessResult<Status>;
    async fn set_status(&self, status: Status) -> StateAccessResult<()>;

    // =========================================================================
    // Worker operations
    // =========================================================================

    async fn add_worker(
        &self,
        name: &str,
        work_dir: &str,
        location: &str,
    ) -> StateAccessResult<Option<Worker>>;

    async fn get_worker(&self, name: &str) -> StateAccessResult<Option<Worker>>;

    async fn get_workers(&self) -> StateAccessResult<Vec<Worker>>;

    async fn update_worker(&self, name: &str, updates: WorkerUpdate) -> StateAccessResult<()>;

    async fn get_active_workers(&self) -> StateAccessResult<Vec<Worker>>;

    async fn all_workers_done(&self) -> StateAccessResult<bool>;

    async fn pause_all_workers(&self, reason: &str) -> StateAccessResult<()>;

    async fn resume_all_workers(&self) -> StateAccessResult<()>;

    // =========================================================================
    // Message operations
    // =========================================================================

    async fn add_message(
        &self,
        thread: &str,
        sender: &str,
        content: &str,
    ) -> StateAccessResult<i64>;

    async fn get_messages(&self, thread: &str, limit: i64) -> StateAccessResult<Vec<Message>>;

    async fn get_unread_messages(
        &self,
        thread: &str,
        reader: &str,
    ) -> StateAccessResult<Vec<Message>>;

    async fn get_all_unread_messages(&self, reader: &str) -> StateAccessResult<Vec<Message>>;

    async fn mark_messages_read(
        &self,
        thread: &str,
        reader: &str,
        up_to_id: Option<i64>,
    ) -> StateAccessResult<()>;

    async fn get_threads(&self) -> StateAccessResult<Vec<String>>;

    // =========================================================================
    // Eval operations
    // =========================================================================

    async fn start_eval(
        &self,
        branch: &str,
        eval_name: Option<&str>,
        log_file: Option<&str>,
    ) -> StateAccessResult<i64>;

    async fn complete_eval(
        &self,
        eval_id: i64,
        success: bool,
        feedback: &str,
    ) -> StateAccessResult<()>;

    async fn get_eval(&self, eval_id: i64) -> StateAccessResult<Option<Eval>>;

    async fn get_evals(&self, limit: i64) -> StateAccessResult<Vec<Eval>>;

    async fn get_running_eval(&self) -> StateAccessResult<Option<Eval>>;

    async fn cancel_running_evals(&self, reason: &str) -> StateAccessResult<i64>;

    // =========================================================================
    // State config getters/setters
    // =========================================================================

    async fn get_request(&self) -> StateAccessResult<Option<String>>;

    async fn set_request(&self, request: Option<&str>) -> StateAccessResult<()>;

    async fn get_project_path(&self) -> StateAccessResult<Option<String>>;

    async fn set_project_path(&self, path: &str) -> StateAccessResult<()>;

    async fn get_waiting_reason(&self) -> StateAccessResult<Option<String>>;

    async fn set_waiting_reason(&self, reason: Option<&str>) -> StateAccessResult<()>;

    async fn get_human_in_the_loop(&self) -> StateAccessResult<bool>;

    async fn set_human_in_the_loop(&self, enabled: bool) -> StateAccessResult<()>;

    async fn get_summary(&self) -> StateAccessResult<Option<String>>;

    async fn set_summary(&self, summary: &str) -> StateAccessResult<()>;

    async fn get_worker_scale(&self) -> StateAccessResult<Option<String>>;

    async fn set_worker_scale(&self, scale: &str) -> StateAccessResult<()>;

    // =========================================================================
    // Time tracking
    // =========================================================================

    async fn get_time_limit_minutes(&self) -> StateAccessResult<Option<i64>>;

    async fn set_time_limit_minutes(&self, minutes: Option<i64>) -> StateAccessResult<()>;

    async fn get_started_at(&self) -> StateAccessResult<Option<String>>;

    async fn set_started_at(&self, timestamp: Option<&str>) -> StateAccessResult<()>;

    async fn get_time_info(&self) -> StateAccessResult<Option<TimeInfo>>;

    async fn is_time_expired(&self) -> StateAccessResult<bool>;

    async fn get_last_time_notification_pct(&self) -> StateAccessResult<Option<i64>>;

    async fn set_last_time_notification_pct(&self, pct: i64) -> StateAccessResult<()>;

    async fn clear_time_tracking(&self) -> StateAccessResult<()>;

    // =========================================================================
    // Iteration tracking
    // =========================================================================

    async fn get_iteration_count(&self) -> StateAccessResult<i64>;

    async fn increment_iteration(&self) -> StateAccessResult<i64>;

    // =========================================================================
    // Scribe - Documentation
    // =========================================================================

    /// Record a learning for the Scribe to integrate into documentation.
    async fn add_scribe_submission(
        &self,
        worker_name: &str,
        content: &str,
    ) -> StateAccessResult<i64>;

    /// Read project documentation maintained by the Scribe.
    async fn read_docs(
        &self,
        file: Option<&str>,
    ) -> StateAccessResult<crate::core::files::DocsContent>;

    // =========================================================================
    // History
    // =========================================================================

    async fn get_history(
        &self,
        limit: i64,
    ) -> StateAccessResult<Vec<crate::core::state::HistoryEntry>>;

    // =========================================================================
    // Lifecycle
    // =========================================================================

    async fn init_state(&self, project_path: Option<&str>) -> StateAccessResult<()>;

    /// Heartbeat for remote workers - updates last_heartbeat timestamp
    async fn heartbeat(&self) -> StateAccessResult<Status>;

    // =========================================================================
    // Scaling
    // =========================================================================

    /// Request a scaling check (triggers event-driven worker scaling)
    async fn request_scaling_check(&self) -> StateAccessResult<()>;

    // =========================================================================
    // Board Integration (Delta Dispatch)
    // =========================================================================

    /// Get the project ID if this run is linked to a board project
    async fn get_project_id(&self) -> StateAccessResult<Option<i64>>;

    /// Add a live node to the board (for worker-added tasks in board runs)
    ///
    /// This creates a live_node in the global database with source='worker'.
    /// Only works if the run has a project_id set (is linked to a board).
    async fn add_live_node(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        node_type: &str, // "task" or "eval"
        content: &str,
    ) -> StateAccessResult<()>;

    /// Claim a live node for a worker
    async fn claim_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::LiveNode>;

    /// Complete a live node
    async fn complete_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::LiveNode>;

    /// Unclaim a live node
    async fn unclaim_live_node(&self, id: &str) -> StateAccessResult<()>;

    /// Get the live node currently claimed by a worker
    async fn get_claimed_live_node(
        &self,
        worker_name: &str,
    ) -> StateAccessResult<Option<crate::core::delta::LiveNode>>;

    /// Get claimable live nodes
    async fn get_claimable_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::LiveNode>>;

    /// Get all live nodes
    async fn get_live_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::LiveNode>>;

    /// Check if a live node is blocked
    async fn is_live_node_blocked(&self, id: &str) -> StateAccessResult<bool>;

    /// Eval pass - validates all nodes
    async fn live_node_eval_pass(&self, eval_id: &str, worker_name: &str) -> StateAccessResult<()>;

    /// Eval fail - creates repair node, returns repair node ID
    async fn live_node_eval_fail(
        &self,
        eval_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> StateAccessResult<String>;

    /// Set tokens used on a live node
    async fn set_live_node_tokens(&self, id: &str, tokens: i64) -> StateAccessResult<()>;

    /// Get all node IDs validated by an eval node
    async fn get_validated_nodes(&self, eval_id: &str) -> StateAccessResult<Vec<String>>;
}

// =============================================================================
// SQLiteState Implementation
// =============================================================================

use crate::core::state::SQLiteState;

impl From<crate::core::state::StateError> for StateAccessError {
    fn from(e: crate::core::state::StateError) -> Self {
        StateAccessError::Database(e.to_string())
    }
}

#[async_trait(?Send)]
impl StateAccess for SQLiteState {
    async fn status(&self) -> StateAccessResult<Status> {
        Ok(SQLiteState::status(self)?)
    }

    async fn set_status(&self, status: Status) -> StateAccessResult<()> {
        Ok(SQLiteState::set_status(self, status)?)
    }

    async fn add_worker(
        &self,
        name: &str,
        work_dir: &str,
        location: &str,
    ) -> StateAccessResult<Option<Worker>> {
        Ok(SQLiteState::add_worker(self, name, work_dir, location)?)
    }

    async fn get_worker(&self, name: &str) -> StateAccessResult<Option<Worker>> {
        Ok(SQLiteState::get_worker(self, name)?)
    }

    async fn get_workers(&self) -> StateAccessResult<Vec<Worker>> {
        Ok(SQLiteState::get_workers(self)?)
    }

    async fn update_worker(&self, name: &str, updates: WorkerUpdate) -> StateAccessResult<()> {
        Ok(SQLiteState::update_worker(self, name, updates)?)
    }

    async fn get_active_workers(&self) -> StateAccessResult<Vec<Worker>> {
        Ok(SQLiteState::get_active_workers(self)?)
    }

    async fn all_workers_done(&self) -> StateAccessResult<bool> {
        // Check if all workers are in Awaiting status
        let workers = SQLiteState::get_workers(self)?;
        Ok(workers.iter().all(|w| w.status == WorkerStatus::Awaiting))
    }

    async fn pause_all_workers(&self, reason: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::pause_all_workers(self, reason)?)
    }

    async fn resume_all_workers(&self) -> StateAccessResult<()> {
        Ok(SQLiteState::resume_all_workers(self)?)
    }

    async fn add_message(
        &self,
        thread: &str,
        sender: &str,
        content: &str,
    ) -> StateAccessResult<i64> {
        Ok(SQLiteState::add_message(
            self, thread, sender, content, false,
        )?)
    }

    async fn get_messages(&self, thread: &str, limit: i64) -> StateAccessResult<Vec<Message>> {
        Ok(SQLiteState::get_messages(self, thread, limit)?)
    }

    async fn get_unread_messages(
        &self,
        thread: &str,
        reader: &str,
    ) -> StateAccessResult<Vec<Message>> {
        Ok(SQLiteState::get_unread_messages(self, thread, reader)?)
    }

    async fn get_all_unread_messages(&self, reader: &str) -> StateAccessResult<Vec<Message>> {
        Ok(SQLiteState::get_all_unread_messages(self, reader)?)
    }

    async fn mark_messages_read(
        &self,
        thread: &str,
        reader: &str,
        up_to_id: Option<i64>,
    ) -> StateAccessResult<()> {
        Ok(SQLiteState::mark_messages_read(
            self, thread, reader, up_to_id,
        )?)
    }

    async fn get_threads(&self) -> StateAccessResult<Vec<String>> {
        Ok(SQLiteState::get_threads(self)?)
    }

    async fn start_eval(
        &self,
        branch: &str,
        eval_name: Option<&str>,
        log_file: Option<&str>,
    ) -> StateAccessResult<i64> {
        Ok(SQLiteState::start_eval(self, branch, eval_name, log_file)?)
    }

    async fn complete_eval(
        &self,
        eval_id: i64,
        success: bool,
        feedback: &str,
    ) -> StateAccessResult<()> {
        Ok(SQLiteState::complete_eval(
            self, eval_id, success, feedback,
        )?)
    }

    async fn get_eval(&self, eval_id: i64) -> StateAccessResult<Option<Eval>> {
        Ok(SQLiteState::get_eval(self, eval_id)?)
    }

    async fn get_evals(&self, limit: i64) -> StateAccessResult<Vec<Eval>> {
        Ok(SQLiteState::get_evals(self, limit)?)
    }

    async fn get_running_eval(&self) -> StateAccessResult<Option<Eval>> {
        Ok(SQLiteState::get_running_eval(self)?)
    }

    async fn cancel_running_evals(&self, reason: &str) -> StateAccessResult<i64> {
        Ok(SQLiteState::cancel_running_evals(self, reason)?)
    }

    async fn get_request(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_request(self)?)
    }

    async fn set_request(&self, request: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_request(self, request)?)
    }

    async fn get_project_path(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_project_path(self)?)
    }

    async fn set_project_path(&self, path: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::set_project_path(self, path)?)
    }

    async fn get_waiting_reason(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_waiting_reason(self)?)
    }

    async fn set_waiting_reason(&self, reason: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_waiting_reason(self, reason)?)
    }

    async fn get_human_in_the_loop(&self) -> StateAccessResult<bool> {
        Ok(SQLiteState::get_human_in_the_loop(self)?)
    }

    async fn set_human_in_the_loop(&self, enabled: bool) -> StateAccessResult<()> {
        Ok(SQLiteState::set_human_in_the_loop(self, enabled)?)
    }

    async fn get_summary(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_summary(self)?)
    }

    async fn set_summary(&self, summary: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::set_summary(self, summary)?)
    }

    async fn get_worker_scale(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_worker_scale(self)?)
    }

    async fn set_worker_scale(&self, scale: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::set_worker_scale(self, scale)?)
    }

    async fn get_time_limit_minutes(&self) -> StateAccessResult<Option<i64>> {
        Ok(SQLiteState::get_time_limit_minutes(self)?)
    }

    async fn set_time_limit_minutes(&self, minutes: Option<i64>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_time_limit_minutes(self, minutes)?)
    }

    async fn get_started_at(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_started_at(self)?)
    }

    async fn set_started_at(&self, timestamp: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_started_at(self, timestamp)?)
    }

    async fn get_time_info(&self) -> StateAccessResult<Option<TimeInfo>> {
        Ok(SQLiteState::get_time_info(self)?)
    }

    async fn is_time_expired(&self) -> StateAccessResult<bool> {
        Ok(SQLiteState::is_time_expired(self)?)
    }

    async fn get_last_time_notification_pct(&self) -> StateAccessResult<Option<i64>> {
        Ok(SQLiteState::get_last_time_notification_pct(self)?)
    }

    async fn set_last_time_notification_pct(&self, pct: i64) -> StateAccessResult<()> {
        Ok(SQLiteState::set_last_time_notification_pct(self, pct)?)
    }

    async fn clear_time_tracking(&self) -> StateAccessResult<()> {
        Ok(SQLiteState::clear_time_tracking(self)?)
    }

    async fn get_iteration_count(&self) -> StateAccessResult<i64> {
        Ok(SQLiteState::get_iteration_count(self)?)
    }

    async fn increment_iteration(&self) -> StateAccessResult<i64> {
        Ok(SQLiteState::increment_iteration(self)?)
    }

    async fn get_history(
        &self,
        limit: i64,
    ) -> StateAccessResult<Vec<crate::core::state::HistoryEntry>> {
        Ok(SQLiteState::get_history(self, limit)?)
    }

    async fn add_scribe_submission(
        &self,
        worker_name: &str,
        content: &str,
    ) -> StateAccessResult<i64> {
        Ok(SQLiteState::add_scribe_submission(
            self,
            worker_name,
            content,
        )?)
    }

    async fn read_docs(
        &self,
        file: Option<&str>,
    ) -> StateAccessResult<crate::core::files::DocsContent> {
        use crate::core::Files;

        // Get run_dir from db_path (db_path is run_dir/hirsel.db)
        let run_dir = self
            .db_path()
            .parent()
            .ok_or_else(|| StateAccessError::Database("Invalid db path".to_string()))?;
        let files = Files::new(run_dir);

        files
            .read_docs(file)
            .map_err(|e| StateAccessError::Database(format!("Failed to read docs: {}", e)))
    }

    async fn init_state(&self, project_path: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::init_state(self, project_path)?)
    }

    async fn heartbeat(&self) -> StateAccessResult<Status> {
        // For local SQLiteState, heartbeat just returns current status
        // (no network operation needed)
        Ok(SQLiteState::status(self)?)
    }

    async fn request_scaling_check(&self) -> StateAccessResult<()> {
        Ok(SQLiteState::request_scaling_check(self)?)
    }

    async fn get_project_id(&self) -> StateAccessResult<Option<i64>> {
        Ok(SQLiteState::get_project_id(self)?)
    }

    async fn add_live_node(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        node_type: &str,
        content: &str,
    ) -> StateAccessResult<()> {
        use crate::core::delta::{DeltaState, NodeType};

        // Get project_id from run state
        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot add live node: run is not linked to a board project".to_string(),
            )
        })?;

        // Create delta state for this project
        let delta_state = DeltaState::new(project_id);

        // Parse node type
        let node_type = NodeType::from_str(node_type);

        // Create the live node
        delta_state
            .create_live_node_from_worker(id, name, parent_id, blocked_by, node_type, content)
            .map_err(|e| {
                StateAccessError::Database(format!("Failed to create live node: {}", e))
            })?;

        Ok(())
    }

    async fn claim_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::LiveNode> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot claim live node: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state
            .claim_live_node(id, worker_name)
            .map_err(|e| StateAccessError::Database(format!("Failed to claim live node: {}", e)))
    }

    async fn complete_live_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::LiveNode> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot complete live node: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state
            .complete_live_node(id, worker_name)
            .map_err(|e| StateAccessError::Database(format!("Failed to complete live node: {}", e)))
    }

    async fn unclaim_live_node(&self, id: &str) -> StateAccessResult<()> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot unclaim live node: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state
            .unclaim_live_node(id)
            .map_err(|e| StateAccessError::Database(format!("Failed to unclaim live node: {}", e)))
    }

    async fn get_claimed_live_node(
        &self,
        worker_name: &str,
    ) -> StateAccessResult<Option<crate::core::delta::LiveNode>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get claimed live node: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state
            .get_claimed_node_for_worker(worker_name)
            .map_err(|e| {
                StateAccessError::Database(format!("Failed to get claimed live node: {}", e))
            })
    }

    async fn get_claimable_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::LiveNode>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get claimable nodes: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state.get_claimable_nodes().map_err(|e| {
            StateAccessError::Database(format!("Failed to get claimable nodes: {}", e))
        })
    }

    async fn get_live_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::LiveNode>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get live nodes: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state
            .get_live_nodes()
            .map_err(|e| StateAccessError::Database(format!("Failed to get live nodes: {}", e)))
    }

    async fn is_live_node_blocked(&self, id: &str) -> StateAccessResult<bool> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot check live node blocked: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state.is_node_blocked(id).map_err(|e| {
            StateAccessError::Database(format!("Failed to check live node blocked: {}", e))
        })
    }

    async fn live_node_eval_pass(&self, eval_id: &str, worker_name: &str) -> StateAccessResult<()> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot eval pass: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state
            .eval_pass(eval_id, worker_name)
            .map_err(|e| StateAccessError::Database(format!("Failed to eval pass: {}", e)))
    }

    async fn live_node_eval_fail(
        &self,
        eval_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> StateAccessResult<String> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot eval fail: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state
            .eval_fail(eval_id, worker_name, feedback)
            .map_err(|e| StateAccessError::Database(format!("Failed to eval fail: {}", e)))
    }

    async fn set_live_node_tokens(&self, id: &str, tokens: i64) -> StateAccessResult<()> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot set live node tokens: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state.set_node_tokens(id, tokens).map_err(|e| {
            StateAccessError::Database(format!("Failed to set live node tokens: {}", e))
        })
    }

    async fn get_validated_nodes(&self, eval_id: &str) -> StateAccessResult<Vec<String>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self)?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get validated nodes: run is not linked to a board project".to_string(),
            )
        })?;

        let delta_state = DeltaState::new(project_id);
        delta_state.get_validated_nodes(eval_id).map_err(|e| {
            StateAccessError::Database(format!("Failed to get validated nodes: {}", e))
        })
    }
}
