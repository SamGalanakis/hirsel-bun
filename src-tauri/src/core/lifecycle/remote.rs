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

/// Remote lifecycle manager - delegates to coordinator via HTTP.
///
/// For remote workers, most lifecycle operations are no-ops since the
/// coordinator (running LocalLifecycleManager) handles:
/// - Eval triggering when workers go inactive
/// - Worker scaling
/// - Time limit enforcement
pub struct RemoteLifecycleManager {
    context: LifecycleContext,
    #[allow(dead_code)]
    api_url: String,
    #[allow(dead_code)]
    worker_name: String,
}

impl RemoteLifecycleManager {
    pub fn new(
        run_name: impl Into<String>,
        api_url: impl Into<String>,
        worker_name: impl Into<String>,
    ) -> Self {
        Self {
            context: LifecycleContext::new(
                run_name,
                PathBuf::new(), // Not used for remote
                vec![],         // Not used for remote
            ),
            api_url: api_url.into(),
            worker_name: worker_name.into(),
        }
    }
}

impl LifecycleManager for RemoteLifecycleManager {
    // All operations return None/empty - coordinator handles lifecycle
    fn process_event(&self, _event: LifecycleEvent) -> LifecycleResult<Vec<LifecycleAction>> {
        Ok(vec![LifecycleAction::None])
    }

    fn pause_run(&self, _reason: &str) -> LifecycleResult<Vec<String>> {
        Err(LifecycleError::Config(
            "Cannot pause from remote worker".into(),
        ))
    }

    fn resume_run(&self) -> LifecycleResult<Vec<String>> {
        Err(LifecycleError::Config(
            "Cannot resume from remote worker".into(),
        ))
    }

    fn worker_done(&self, _worker_name: &str) -> LifecycleResult<Vec<LifecycleAction>> {
        // Just return - coordinator handles eval triggering via status change
        Ok(vec![LifecycleAction::None])
    }

    fn handle_time_expired(&self) -> LifecycleResult<()> {
        // Coordinator handles time expiration
        Ok(())
    }

    fn all_workers_inactive(&self) -> LifecycleResult<bool> {
        Ok(false) // Coordinator queries this
    }

    fn should_trigger_eval(&self) -> LifecycleResult<bool> {
        Ok(false) // Coordinator decides this
    }

    fn can_scale_up(&self) -> LifecycleResult<bool> {
        Ok(false) // Coordinator handles scaling
    }

    fn run_status(&self) -> LifecycleResult<Status> {
        Err(LifecycleError::Config("Use HTTP API for status".into()))
    }

    fn context(&self) -> &LifecycleContext {
        &self.context
    }
}
