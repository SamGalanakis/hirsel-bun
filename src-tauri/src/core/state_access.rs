//! Worker-facing state operations for task claiming, messaging, and heartbeats.
//!
//! This module provides the `StateAccess` trait that abstracts low-level state
//! operations needed by workers during run execution. Workers use this trait
//! to interact with run state without knowing if they're accessing SQLite directly
//! (local workers) or via HTTP (remote workers through SSH tunnels).
//!
//! ## Messaging Architecture
//!
//! Messages are stored at the **project level** in the global database (`project_messages` table).
//! This allows the Sheepfold UI and workers to share the same messaging system:
//! - `"chat"` thread = group chat (all workers + human)
//! - Worker name threads (e.g., `"willow-coopworth"`) = DMs
//!
//! Workers use `"user"` as a semantic alias that maps to their own name (DM with human).
//! Workers use `"group"` as a semantic alias that maps to `"chat"` (group chat).
//!
//! ## StateAccess vs Orchestrator
//!
//! These two traits serve different purposes:
//!
//! - **`StateAccess`** (this module): Worker-side, per-run operations during execution
//!   - Task claiming and completion
//!   - Worker heartbeats and status updates
//!   - Message sending between workers (via project messages)
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

use crate::core::state::{Eval, Status, TimeInfo, Worker, WorkerStatus, WorkerUpdate};

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
    // Message operations (project-level via Sheepfold)
    //
    // Messages are stored in the global database's project_messages table.
    // Thread naming:
    // - "chat" = group chat (all workers + human)
    // - Worker names = DMs (e.g., "willow-coopworth")
    //
    // Workers should use get_project_id() to get the project_id for messaging.
    // =========================================================================

    /// Add a message to a project thread.
    /// Thread should be "chat" for group chat or a worker name for DM.
    async fn add_project_message(
        &self,
        project_id: i64,
        thread: &str,
        sender: &str,
        content: &str,
        waiting: bool,
    ) -> StateAccessResult<i64>;

    /// Get messages from a project thread.
    async fn get_project_messages(
        &self,
        project_id: i64,
        thread: &str,
        limit: i64,
    ) -> StateAccessResult<Vec<crate::core::ProjectMessage>>;

    /// Get unread messages for a reader in a project thread.
    async fn get_unread_project_messages(
        &self,
        project_id: i64,
        thread: &str,
        reader: &str,
    ) -> StateAccessResult<Vec<crate::core::ProjectMessage>>;

    /// Get all unread messages for a reader across all threads in a project.
    async fn get_all_unread_project_messages(
        &self,
        project_id: i64,
        reader: &str,
    ) -> StateAccessResult<Vec<crate::core::ProjectMessage>>;

    /// Mark messages as read in a project thread.
    async fn mark_project_messages_read(
        &self,
        project_id: i64,
        thread: &str,
        reader: &str,
    ) -> StateAccessResult<()>;

    /// Get all thread names for a project.
    async fn get_project_threads(&self, project_id: i64) -> StateAccessResult<Vec<String>>;

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
    // Board Integration
    // =========================================================================

    /// Get the project ID if this run is linked to a board project
    async fn get_project_id(&self) -> StateAccessResult<Option<i64>>;

    /// Add a board node (for worker-added tasks in board runs)
    ///
    /// This creates a board node in the global database with source='worker'.
    /// Only works if the run has a project_id set (is linked to a board).
    /// For eval nodes, `validates` writes validated_by on target tasks.
    async fn add_node(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        kind: &str, // "task" or "eval"
        content: &str,
        validates: Option<&[&str]>,
    ) -> StateAccessResult<()>;

    /// Claim a node for a worker
    async fn claim_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::BoardNode>;

    /// Complete a node
    async fn complete_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::BoardNode>;

    /// Unclaim a node
    async fn unclaim_node(&self, id: &str) -> StateAccessResult<()>;

    /// Get the node currently claimed by a worker
    async fn get_claimed_node(
        &self,
        worker_name: &str,
    ) -> StateAccessResult<Option<crate::core::delta::BoardNode>>;

    /// Get claimable nodes
    async fn get_claimable_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::BoardNode>>;

    /// Get all nodes
    async fn get_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::BoardNode>>;

    /// Check if a node is blocked
    async fn is_node_blocked(&self, id: &str) -> StateAccessResult<bool>;

    /// Check pass - validates all nodes
    async fn node_check_pass(&self, check_id: &str, worker_name: &str) -> StateAccessResult<()>;

    /// Check fail - creates repair node, returns repair node ID
    async fn node_check_fail(
        &self,
        check_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> StateAccessResult<String>;

    /// Set tokens used on a node
    async fn set_node_tokens(&self, id: &str, tokens: i64) -> StateAccessResult<()>;

    /// Get all node IDs validated by a check node
    async fn get_validated_nodes(&self, check_id: &str) -> StateAccessResult<Vec<String>>;

    /// Delete a node by ID (only worker-created nodes can be deleted)
    async fn delete_node(&self, id: &str) -> StateAccessResult<()>;
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
        Ok(SQLiteState::status(self).await?)
    }

    async fn set_status(&self, status: Status) -> StateAccessResult<()> {
        Ok(SQLiteState::set_status(self, status).await?)
    }

    async fn add_worker(
        &self,
        name: &str,
        work_dir: &str,
        location: &str,
    ) -> StateAccessResult<Option<Worker>> {
        Ok(SQLiteState::add_worker(self, name, work_dir, location).await?)
    }

    async fn get_worker(&self, name: &str) -> StateAccessResult<Option<Worker>> {
        Ok(SQLiteState::get_worker(self, name).await?)
    }

    async fn get_workers(&self) -> StateAccessResult<Vec<Worker>> {
        Ok(SQLiteState::get_workers(self).await?)
    }

    async fn update_worker(&self, name: &str, updates: WorkerUpdate) -> StateAccessResult<()> {
        Ok(SQLiteState::update_worker(self, name, updates).await?)
    }

    async fn get_active_workers(&self) -> StateAccessResult<Vec<Worker>> {
        Ok(SQLiteState::get_active_workers(self).await?)
    }

    async fn all_workers_done(&self) -> StateAccessResult<bool> {
        // Check if all workers are in Awaiting status
        let workers = SQLiteState::get_workers(self).await?;
        Ok(workers.iter().all(|w| w.status == WorkerStatus::Awaiting))
    }

    async fn pause_all_workers(&self, reason: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::pause_all_workers(self, reason).await?)
    }

    async fn resume_all_workers(&self) -> StateAccessResult<()> {
        Ok(SQLiteState::resume_all_workers(self).await?)
    }

    async fn add_project_message(
        &self,
        project_id: i64,
        thread: &str,
        sender: &str,
        content: &str,
        waiting: bool,
    ) -> StateAccessResult<i64> {
        use crate::core::ProjectMessagesStore;
        let route_id = self.get_route_id().await?;
        let store = ProjectMessagesStore::open()
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        let msg = store
            .add_message(project_id, route_id, thread, sender, content, waiting)
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        Ok(msg.id)
    }

    async fn get_project_messages(
        &self,
        project_id: i64,
        thread: &str,
        limit: i64,
    ) -> StateAccessResult<Vec<crate::core::ProjectMessage>> {
        use crate::core::ProjectMessagesStore;
        let route_id = self.get_route_id().await?;
        let store = ProjectMessagesStore::open()
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        store
            .get_messages(project_id, route_id, thread, Some(limit))
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))
    }

    async fn get_unread_project_messages(
        &self,
        project_id: i64,
        thread: &str,
        reader: &str,
    ) -> StateAccessResult<Vec<crate::core::ProjectMessage>> {
        use crate::core::ProjectMessagesStore;
        let route_id = self.get_route_id().await?;
        let store = ProjectMessagesStore::open()
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        // Get all messages in thread, filter to unread
        let threads = store
            .get_threads(project_id, route_id, reader)
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;

        // Find the specific thread's last_read info
        let thread_info = threads.iter().find(|t| t.thread == thread);
        let unread_count = thread_info.map(|t| t.unread_count).unwrap_or(0);

        if unread_count == 0 {
            return Ok(vec![]);
        }

        // Get recent messages (unread ones)
        let messages = store
            .get_messages(project_id, route_id, thread, Some(unread_count))
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;

        // Filter out messages from the reader themselves
        Ok(messages
            .into_iter()
            .filter(|m| m.sender != reader)
            .collect())
    }

    async fn get_all_unread_project_messages(
        &self,
        project_id: i64,
        reader: &str,
    ) -> StateAccessResult<Vec<crate::core::ProjectMessage>> {
        use crate::core::ProjectMessagesStore;
        let route_id = self.get_route_id().await?;
        let store = ProjectMessagesStore::open()
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        let threads = store
            .get_threads(project_id, route_id, reader)
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;

        let mut all_unread = Vec::new();
        for thread_info in threads {
            if thread_info.unread_count > 0 {
                let messages = store
                    .get_messages(
                        project_id,
                        route_id,
                        &thread_info.thread,
                        Some(thread_info.unread_count),
                    )
                    .await
                    .map_err(|e| StateAccessError::Database(e.to_string()))?;
                // Filter out messages from the reader themselves
                all_unread.extend(messages.into_iter().filter(|m| m.sender != reader));
            }
        }
        Ok(all_unread)
    }

    async fn mark_project_messages_read(
        &self,
        project_id: i64,
        thread: &str,
        reader: &str,
    ) -> StateAccessResult<()> {
        use crate::core::ProjectMessagesStore;
        let route_id = self.get_route_id().await?;
        let store = ProjectMessagesStore::open()
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        store
            .mark_messages_read(project_id, route_id, thread, reader)
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))
    }

    async fn get_project_threads(&self, project_id: i64) -> StateAccessResult<Vec<String>> {
        use crate::core::ProjectMessagesStore;
        let route_id = self.get_route_id().await?;
        let store = ProjectMessagesStore::open()
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        // Use a dummy reader since we just want thread names
        let threads = store
            .get_threads(project_id, route_id, "")
            .await
            .map_err(|e| StateAccessError::Database(e.to_string()))?;
        Ok(threads.into_iter().map(|t| t.thread).collect())
    }

    async fn start_eval(
        &self,
        branch: &str,
        eval_name: Option<&str>,
        log_file: Option<&str>,
    ) -> StateAccessResult<i64> {
        Ok(SQLiteState::start_eval(self, branch, eval_name, log_file).await?)
    }

    async fn complete_eval(
        &self,
        eval_id: i64,
        success: bool,
        feedback: &str,
    ) -> StateAccessResult<()> {
        Ok(SQLiteState::complete_eval(self, eval_id, success, feedback).await?)
    }

    async fn get_eval(&self, eval_id: i64) -> StateAccessResult<Option<Eval>> {
        Ok(SQLiteState::get_eval(self, eval_id).await?)
    }

    async fn get_evals(&self, limit: i64) -> StateAccessResult<Vec<Eval>> {
        Ok(SQLiteState::get_evals(self, limit).await?)
    }

    async fn get_running_eval(&self) -> StateAccessResult<Option<Eval>> {
        Ok(SQLiteState::get_running_eval(self).await?)
    }

    async fn cancel_running_evals(&self, reason: &str) -> StateAccessResult<i64> {
        Ok(SQLiteState::cancel_running_evals(self, reason).await?)
    }

    async fn get_request(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_request(self).await?)
    }

    async fn set_request(&self, request: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_request(self, request).await?)
    }

    async fn get_project_path(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_project_path(self).await?)
    }

    async fn set_project_path(&self, path: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::set_project_path(self, path).await?)
    }

    async fn get_waiting_reason(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_waiting_reason(self).await?)
    }

    async fn set_waiting_reason(&self, reason: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_waiting_reason(self, reason).await?)
    }

    async fn get_human_in_the_loop(&self) -> StateAccessResult<bool> {
        Ok(SQLiteState::get_human_in_the_loop(self).await?)
    }

    async fn set_human_in_the_loop(&self, enabled: bool) -> StateAccessResult<()> {
        Ok(SQLiteState::set_human_in_the_loop(self, enabled).await?)
    }

    async fn get_summary(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_summary(self).await?)
    }

    async fn set_summary(&self, summary: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::set_summary(self, summary).await?)
    }

    async fn get_worker_scale(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_worker_scale(self).await?)
    }

    async fn set_worker_scale(&self, scale: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::set_worker_scale(self, scale).await?)
    }

    async fn get_time_limit_minutes(&self) -> StateAccessResult<Option<i64>> {
        Ok(SQLiteState::get_time_limit_minutes(self).await?)
    }

    async fn set_time_limit_minutes(&self, minutes: Option<i64>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_time_limit_minutes(self, minutes).await?)
    }

    async fn get_started_at(&self) -> StateAccessResult<Option<String>> {
        Ok(SQLiteState::get_started_at(self).await?)
    }

    async fn set_started_at(&self, timestamp: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_started_at(self, timestamp).await?)
    }

    async fn get_time_info(&self) -> StateAccessResult<Option<TimeInfo>> {
        Ok(SQLiteState::get_time_info(self).await?)
    }

    async fn is_time_expired(&self) -> StateAccessResult<bool> {
        Ok(SQLiteState::is_time_expired(self).await?)
    }

    async fn get_last_time_notification_pct(&self) -> StateAccessResult<Option<i64>> {
        Ok(SQLiteState::get_last_time_notification_pct(self).await?)
    }

    async fn set_last_time_notification_pct(&self, pct: i64) -> StateAccessResult<()> {
        Ok(SQLiteState::set_last_time_notification_pct(self, pct).await?)
    }

    async fn clear_time_tracking(&self) -> StateAccessResult<()> {
        Ok(SQLiteState::clear_time_tracking(self).await?)
    }

    async fn get_iteration_count(&self) -> StateAccessResult<i64> {
        Ok(SQLiteState::get_iteration_count(self).await?)
    }

    async fn increment_iteration(&self) -> StateAccessResult<i64> {
        Ok(SQLiteState::increment_iteration(self).await?)
    }

    async fn get_history(
        &self,
        limit: i64,
    ) -> StateAccessResult<Vec<crate::core::state::HistoryEntry>> {
        Ok(SQLiteState::get_history(self, limit).await?)
    }

    async fn add_scribe_submission(
        &self,
        worker_name: &str,
        content: &str,
    ) -> StateAccessResult<i64> {
        Ok(SQLiteState::add_scribe_submission(self, worker_name, content).await?)
    }

    async fn read_docs(
        &self,
        file: Option<&str>,
    ) -> StateAccessResult<crate::core::files::DocsContent> {
        use crate::core::storage::create_default_local_storage;
        use crate::core::Files;

        // Get run_dir from run_name
        let run_dir = crate::core::config::run_dir(self.run_name());
        let files = Files::new(&run_dir);
        let storage = create_default_local_storage();

        files
            .read_docs_async(&storage, file)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to read docs: {}", e)))
    }

    async fn init_state(&self, project_path: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::init_state(self, project_path).await?)
    }

    async fn heartbeat(&self) -> StateAccessResult<Status> {
        // For local SQLiteState, heartbeat just returns current status
        // (no network operation needed)
        Ok(SQLiteState::status(self).await?)
    }

    async fn request_scaling_check(&self) -> StateAccessResult<()> {
        Ok(SQLiteState::request_scaling_check(self).await?)
    }

    async fn get_project_id(&self) -> StateAccessResult<Option<i64>> {
        Ok(SQLiteState::get_project_id(self).await?)
    }

    async fn add_node(
        &self,
        id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
        kind: &str,
        content: &str,
        validates: Option<&[&str]>,
    ) -> StateAccessResult<()> {
        use crate::core::delta::{DeltaState, NodeKind};

        // Get project_id and route_id from run state
        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot add node: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        // Create delta state for this project and route
        let delta_state = DeltaState::with_route(project_id, route_id);

        // Parse node kind
        let kind = NodeKind::from_str(kind);

        // Create the node
        delta_state
            .create_node_from_worker(id, name, parent_id, blocked_by, kind, content, validates)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to create node: {}", e)))?;

        Ok(())
    }

    async fn claim_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::BoardNode> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot claim node: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .claim_node(id, worker_name)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to claim node: {}", e)))
    }

    async fn complete_node(
        &self,
        id: &str,
        worker_name: &str,
    ) -> StateAccessResult<crate::core::delta::BoardNode> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot complete node: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .complete_node(id, worker_name)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to complete node: {}", e)))
    }

    async fn unclaim_node(&self, id: &str) -> StateAccessResult<()> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot unclaim node: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .unclaim_node(id)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to unclaim node: {}", e)))
    }

    async fn get_claimed_node(
        &self,
        worker_name: &str,
    ) -> StateAccessResult<Option<crate::core::delta::BoardNode>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get claimed node: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .get_claimed_node_for_worker(worker_name)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to get claimed node: {}", e)))
    }

    async fn get_claimable_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::BoardNode>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get claimable nodes: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state.get_claimable_nodes().await.map_err(|e| {
            StateAccessError::Database(format!("Failed to get claimable nodes: {}", e))
        })
    }

    async fn get_nodes(&self) -> StateAccessResult<Vec<crate::core::delta::BoardNode>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get nodes: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .get_nodes()
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to get nodes: {}", e)))
    }

    async fn is_node_blocked(&self, id: &str) -> StateAccessResult<bool> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot check node blocked: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .is_node_blocked(id)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to check node blocked: {}", e)))
    }

    async fn node_check_pass(&self, check_id: &str, worker_name: &str) -> StateAccessResult<()> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot check pass: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .check_pass(check_id, worker_name)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to check pass: {}", e)))
    }

    async fn node_check_fail(
        &self,
        check_id: &str,
        worker_name: &str,
        feedback: &str,
    ) -> StateAccessResult<String> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot check fail: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .check_fail(check_id, worker_name, feedback)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to check fail: {}", e)))
    }

    async fn set_node_tokens(&self, id: &str, tokens: i64) -> StateAccessResult<()> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot set node tokens: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .set_node_tokens(id, tokens)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to set node tokens: {}", e)))
    }

    async fn get_validated_nodes(&self, check_id: &str) -> StateAccessResult<Vec<String>> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot get validated nodes: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state.get_checked_nodes(check_id).await.map_err(|e| {
            StateAccessError::Database(format!("Failed to get validated nodes: {}", e))
        })
    }

    async fn delete_node(&self, id: &str) -> StateAccessResult<()> {
        use crate::core::delta::DeltaState;

        let project_id = SQLiteState::get_project_id(self).await?.ok_or_else(|| {
            StateAccessError::InvalidOperation(
                "Cannot delete node: run is not linked to a board project".to_string(),
            )
        })?;
        let route_id = self.get_route_id().await?;

        let delta_state = DeltaState::with_route(project_id, route_id);
        delta_state
            .delete_node(id)
            .await
            .map_err(|e| StateAccessError::Database(format!("Failed to delete node: {}", e)))
    }
}
