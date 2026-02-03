//! Remote lifecycle manager for workers communicating with a coordinator.
//!
//! Remote workers don't manage lifecycle directly - they just report status
//! changes to the coordinator, which handles eval triggering, scaling, etc.

use super::{
    LifecycleAction, LifecycleContext, LifecycleError, LifecycleEvent, LifecycleManager,
    LifecycleResult,
};
use crate::core::state::Status;
use std::path::PathBuf;

/// Remote lifecycle manager - a no-op implementation for remote workers.
///
/// Remote workers don't manage lifecycle directly. All lifecycle operations
/// are handled by the coordinator (running LocalLifecycleManager), which:
/// - Triggers evals when workers go inactive
/// - Manages worker scaling
/// - Enforces time limits
///
/// This implementation returns no-ops for all operations, allowing the
/// coordinator to drive lifecycle decisions via the HTTP API.
pub struct RemoteLifecycleManager {
    context: LifecycleContext,
}

impl RemoteLifecycleManager {
    pub fn new(
        run_name: impl Into<String>,
        _api_url: impl Into<String>,
        _worker_name: impl Into<String>,
    ) -> Self {
        Self {
            context: LifecycleContext::new(
                run_name,
                PathBuf::new(), // Not used for remote
                vec![],         // Not used for remote
            ),
        }
    }
}

impl LifecycleManager for RemoteLifecycleManager {
    // All operations return None/empty - coordinator handles lifecycle
    async fn process_event(&self, _event: LifecycleEvent) -> LifecycleResult<Vec<LifecycleAction>> {
        Ok(vec![LifecycleAction::None])
    }

    async fn pause_run(&self, _reason: &str) -> LifecycleResult<Vec<String>> {
        Err(LifecycleError::Config(
            "Cannot pause from remote worker".into(),
        ))
    }

    async fn resume_run(&self) -> LifecycleResult<Vec<LifecycleAction>> {
        Err(LifecycleError::Config(
            "Cannot resume from remote worker".into(),
        ))
    }

    async fn worker_done(&self, _worker_name: &str) -> LifecycleResult<Vec<LifecycleAction>> {
        // Just return - coordinator handles eval triggering via status change
        Ok(vec![LifecycleAction::None])
    }

    async fn handle_time_expired(&self) -> LifecycleResult<()> {
        // Coordinator handles time expiration
        Ok(())
    }

    async fn all_workers_inactive(&self) -> LifecycleResult<bool> {
        Ok(false) // Coordinator queries this
    }

    async fn should_trigger_eval(&self) -> LifecycleResult<bool> {
        Ok(false) // Coordinator decides this
    }

    async fn can_scale_up(&self) -> LifecycleResult<bool> {
        Ok(false) // Coordinator handles scaling
    }

    async fn run_status(&self) -> LifecycleResult<Status> {
        Err(LifecycleError::Config("Use HTTP API for status".into()))
    }

    fn context(&self) -> &LifecycleContext {
        &self.context
    }
}
