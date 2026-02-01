//! Local run manager implementation.
//!
//! Wraps the existing LocalOrchestrator and LocalLifecycleManager to provide
//! the unified RunManager interface.

use async_trait::async_trait;

use super::{RunManager, RunManagerError, RunManagerResult};
use crate::core::api_types::{
    ConfigResponse, Eval, HistoryEntry, Message, RunDetail, RunSummary, Task, ThreadSummary,
    Worker, WorkerEventsResponse,
};
use crate::core::config::Config;
use crate::core::lifecycle::{
    LifecycleAction, LifecycleEvent, LifecycleManager, LocalLifecycleManager,
};
use crate::core::orchestrator::{HealthResponse, LocalOrchestrator, Orchestrator};

/// Local run manager that wraps LocalOrchestrator and LocalLifecycleManager.
pub struct LocalRunManager {
    orchestrator: LocalOrchestrator,
}

impl LocalRunManager {
    /// Create a new local run manager with the given configuration.
    pub fn new(config: Config) -> Self {
        let orchestrator = LocalOrchestrator::new(config);
        Self { orchestrator }
    }
}

#[async_trait]
impl RunManager for LocalRunManager {
    // =========================================================================
    // Run CRUD
    // =========================================================================

    async fn list_runs(&self) -> RunManagerResult<Vec<RunSummary>> {
        self.orchestrator
            .list_runs()
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn get_run(&self, name: &str) -> RunManagerResult<RunDetail> {
        self.orchestrator
            .get_run(name)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn delete_run(&self, name: &str) -> RunManagerResult<()> {
        self.orchestrator
            .delete_run(name)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    // =========================================================================
    // Run Lifecycle
    // =========================================================================

    async fn pause_run(&self, name: &str) -> RunManagerResult<()> {
        self.orchestrator
            .pause_run(name)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn resume_run(
        &self,
        name: &str,
        time_limit_minutes: Option<u32>,
    ) -> RunManagerResult<()> {
        self.orchestrator
            .resume_run(name, time_limit_minutes)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn deliver_run(&self, name: &str, branch: Option<String>) -> RunManagerResult<String> {
        self.orchestrator
            .deliver_run(name, branch)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    // =========================================================================
    // Workers
    // =========================================================================

    async fn list_workers(&self, run: &str) -> RunManagerResult<Vec<Worker>> {
        self.orchestrator
            .list_workers(run)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn restart_worker(&self, run: &str, worker: &str) -> RunManagerResult<()> {
        self.orchestrator
            .restart_worker(run, worker)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn get_worker_events(
        &self,
        run: &str,
        worker: &str,
        after_id: Option<i64>,
        limit: Option<i64>,
    ) -> RunManagerResult<WorkerEventsResponse> {
        self.orchestrator
            .get_worker_events(run, worker, after_id, limit)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    // =========================================================================
    // Tasks
    // =========================================================================

    async fn list_tasks(&self, run: &str) -> RunManagerResult<Vec<Task>> {
        self.orchestrator
            .list_tasks(run)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn add_task(&self, run: &str, content: &str) -> RunManagerResult<Task> {
        self.orchestrator
            .add_task(run, content)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn delete_task(&self, run: &str, task_id: &str) -> RunManagerResult<()> {
        self.orchestrator
            .delete_task(run, task_id)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn complete_task(&self, run: &str, task_id: &str) -> RunManagerResult<()> {
        self.orchestrator
            .complete_task(run, task_id)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn reopen_task(&self, run: &str, task_id: &str) -> RunManagerResult<()> {
        self.orchestrator
            .reopen_task(run, task_id)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    // =========================================================================
    // Messages
    // =========================================================================

    async fn list_threads(&self, run: &str) -> RunManagerResult<Vec<ThreadSummary>> {
        self.orchestrator
            .list_threads(run)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn get_messages(&self, run: &str, thread: &str) -> RunManagerResult<Vec<Message>> {
        self.orchestrator
            .get_messages(run, thread)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn send_message(
        &self,
        run: &str,
        thread: &str,
        content: &str,
    ) -> RunManagerResult<Message> {
        self.orchestrator
            .send_message(run, thread, content)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    // =========================================================================
    // Evals & History
    // =========================================================================

    async fn list_evals(&self, run: &str) -> RunManagerResult<Vec<Eval>> {
        self.orchestrator
            .list_evals(run)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn get_history(
        &self,
        run: &str,
        limit: Option<u32>,
    ) -> RunManagerResult<Vec<HistoryEntry>> {
        self.orchestrator
            .get_history(run, limit)
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    // =========================================================================
    // Lifecycle Polling
    // =========================================================================

    async fn poll_lifecycle(&self, run: &str) -> RunManagerResult<()> {
        use crate::core::config::run_dir;

        let run_dir = run_dir(run);

        // Get agent command
        let agent_cmd = crate::cli::config::get_agent_command();

        // Create lifecycle manager (it opens the state DB internally)
        let lifecycle = LocalLifecycleManager::new(run.to_string(), run_dir.clone(), agent_cmd)
            .map_err(|e| RunManagerError::State(e.to_string()))?;

        // Process time check event
        let actions = lifecycle
            .process_event(LifecycleEvent::TimeCheck)
            .map_err(|e| RunManagerError::State(e.to_string()))?;

        // Execute actions
        for action in actions {
            self.execute_lifecycle_action(run, action).await?;
        }

        Ok(())
    }

    // =========================================================================
    // Config & Health
    // =========================================================================

    async fn get_config(&self) -> RunManagerResult<ConfigResponse> {
        self.orchestrator
            .get_config()
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }

    async fn health(&self) -> RunManagerResult<HealthResponse> {
        self.orchestrator
            .health()
            .await
            .map_err(|e| RunManagerError::Other(e.to_string()))
    }
}

impl LocalRunManager {
    /// Execute a lifecycle action.
    async fn execute_lifecycle_action(
        &self,
        run: &str,
        action: LifecycleAction,
    ) -> RunManagerResult<()> {
        match action {
            LifecycleAction::None => Ok(()),

            LifecycleAction::SpawnWorker {
                worker_name,
                work_dir,
                assigned_task_id: _,
            } => {
                tracing::info!(
                    "RunManager: spawning worker '{}' for run '{}'",
                    worker_name,
                    run
                );
                self.orchestrator
                    .spawn_single_worker(run, &worker_name, &work_dir, None)
                    .await
                    .map_err(|e| RunManagerError::Other(e.to_string()))
            }

            LifecycleAction::ResumeWorker {
                worker_name,
                work_dir,
                resume_session_id,
                state_handle,
            } => {
                tracing::info!(
                    "RunManager: resuming worker '{}' for run '{}'",
                    worker_name,
                    run
                );
                self.orchestrator
                    .resume_worker(
                        run,
                        &worker_name,
                        &work_dir,
                        resume_session_id.as_deref(),
                        state_handle.as_ref(),
                    )
                    .await
                    .map_err(|e| RunManagerError::Other(e.to_string()))
            }

            LifecycleAction::WorkersPaused(names) => {
                tracing::info!("RunManager: workers paused for run '{}': {:?}", run, names);
                Ok(())
            }

            LifecycleAction::WorkersKilled(names) => {
                tracing::info!("RunManager: workers killed for run '{}': {:?}", run, names);
                Ok(())
            }

            LifecycleAction::WorkersResumed(names) => {
                tracing::info!("RunManager: workers resumed for run '{}': {:?}", run, names);
                Ok(())
            }

            LifecycleAction::RunStatusChanged(status) => {
                tracing::info!("RunManager: run '{}' status changed to {:?}", run, status);
                Ok(())
            }

            LifecycleAction::EvalTriggered => {
                tracing::info!("RunManager: eval triggered for run '{}'", run);
                Ok(())
            }

            LifecycleAction::RunCompleted => {
                tracing::info!("RunManager: run '{}' completed", run);
                Ok(())
            }

            LifecycleAction::RunFailed { reason } => {
                tracing::warn!("RunManager: run '{}' failed: {}", run, reason);
                Ok(())
            }

            LifecycleAction::TimeWarning { percent } => {
                tracing::info!("RunManager: run '{}' time warning: {}%", run, percent);
                Ok(())
            }
        }
    }
}
