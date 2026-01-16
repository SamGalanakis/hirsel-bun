//! State access trait for abstracting local vs remote state operations.
//!
//! This trait defines the interface that both SQLiteState (local) and HttpState (remote)
//! implement. Workers use this trait and don't care whether they're accessing state
//! directly or via HTTP.
//!
//! Mirrors the Python `StateProtocol` from `state_protocol.py`.

use async_trait::async_trait;

use crate::core::state::{
    Eval, Message, Status, Task, TimeInfo, Worker, WorkerStatus, WorkerUpdate,
};

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
    // Task operations
    // =========================================================================

    async fn add_task(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
    ) -> StateAccessResult<()>;

    async fn get_tasks(&self) -> StateAccessResult<Vec<Task>>;

    async fn get_task(&self, task_id: &str) -> StateAccessResult<Option<Task>>;

    async fn claim_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool>;

    async fn complete_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool>;

    async fn unclaim_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool>;

    async fn get_claimed_task(&self, worker_name: &str) -> StateAccessResult<Option<Task>>;

    async fn get_claimable_tasks(&self) -> StateAccessResult<Vec<Task>>;

    async fn delete_task(&self, task_id: &str) -> StateAccessResult<()>;

    async fn is_task_blocked(&self, task_id: &str) -> StateAccessResult<bool>;

    async fn get_blockers(&self, task_id: &str) -> StateAccessResult<Vec<String>>;

    async fn has_children(&self, task_id: &str) -> StateAccessResult<bool>;

    async fn get_children(&self, task_id: &str) -> StateAccessResult<Vec<Task>>;

    async fn set_task_pending_done(&self, task_id: &str) -> StateAccessResult<()>;

    async fn clear_task_pending_done(&self, task_id: &str) -> StateAccessResult<()>;

    async fn reopen_task(&self, task_id: &str) -> StateAccessResult<bool>;

    async fn set_task_tokens(&self, task_id: &str, tokens: i64) -> StateAccessResult<()>;

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

    async fn get_max_iterations(&self) -> StateAccessResult<Option<i64>>;

    async fn set_max_iterations(&self, max_iter: Option<i64>) -> StateAccessResult<()>;

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

    async fn add_task(
        &self,
        task_id: &str,
        name: &str,
        parent_id: Option<&str>,
        blocked_by: Option<&[&str]>,
    ) -> StateAccessResult<()> {
        SQLiteState::add_task(self, task_id, name, parent_id, blocked_by)?;
        Ok(())
    }

    async fn get_tasks(&self) -> StateAccessResult<Vec<Task>> {
        Ok(SQLiteState::get_tasks(self)?)
    }

    async fn get_task(&self, task_id: &str) -> StateAccessResult<Option<Task>> {
        Ok(SQLiteState::get_task(self, task_id)?)
    }

    async fn claim_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool> {
        match SQLiteState::claim_task(self, task_id, worker_name) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn complete_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool> {
        match SQLiteState::complete_task(self, task_id, worker_name) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn unclaim_task(&self, task_id: &str, worker_name: &str) -> StateAccessResult<bool> {
        match SQLiteState::unclaim_task(self, task_id, worker_name) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn get_claimed_task(&self, worker_name: &str) -> StateAccessResult<Option<Task>> {
        Ok(SQLiteState::get_claimed_task(self, worker_name)?)
    }

    async fn get_claimable_tasks(&self) -> StateAccessResult<Vec<Task>> {
        Ok(SQLiteState::get_claimable_tasks(self)?)
    }

    async fn delete_task(&self, task_id: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::delete_task(self, task_id)?)
    }

    async fn is_task_blocked(&self, task_id: &str) -> StateAccessResult<bool> {
        Ok(SQLiteState::is_task_blocked(self, task_id)?)
    }

    async fn get_blockers(&self, task_id: &str) -> StateAccessResult<Vec<String>> {
        Ok(SQLiteState::get_blockers(self, task_id)?)
    }

    async fn has_children(&self, task_id: &str) -> StateAccessResult<bool> {
        Ok(SQLiteState::has_children(self, task_id)?)
    }

    async fn get_children(&self, task_id: &str) -> StateAccessResult<Vec<Task>> {
        Ok(SQLiteState::get_children(self, task_id)?)
    }

    async fn set_task_pending_done(&self, task_id: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::set_task_pending_done(self, task_id)?)
    }

    async fn clear_task_pending_done(&self, task_id: &str) -> StateAccessResult<()> {
        Ok(SQLiteState::clear_task_pending_done(self, task_id)?)
    }

    async fn reopen_task(&self, task_id: &str) -> StateAccessResult<bool> {
        match SQLiteState::reopen_task(self, task_id) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn set_task_tokens(&self, task_id: &str, tokens: i64) -> StateAccessResult<()> {
        Ok(SQLiteState::set_task_tokens(self, task_id, tokens)?)
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

    async fn get_max_iterations(&self) -> StateAccessResult<Option<i64>> {
        Ok(SQLiteState::get_max_iterations(self)?)
    }

    async fn set_max_iterations(&self, max_iter: Option<i64>) -> StateAccessResult<()> {
        Ok(SQLiteState::set_max_iterations(self, max_iter)?)
    }

    async fn get_history(
        &self,
        limit: i64,
    ) -> StateAccessResult<Vec<crate::core::state::HistoryEntry>> {
        Ok(SQLiteState::get_history(self, limit)?)
    }

    async fn init_state(&self, project_path: Option<&str>) -> StateAccessResult<()> {
        Ok(SQLiteState::init_state(self, project_path)?)
    }

    async fn heartbeat(&self) -> StateAccessResult<Status> {
        // For local SQLiteState, heartbeat just returns current status
        // (no network operation needed)
        Ok(SQLiteState::status(self)?)
    }
}
