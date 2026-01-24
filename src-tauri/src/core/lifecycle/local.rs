//! Local implementation of LifecycleManager using SQLite state.
//!
//! This implementation directly accesses the SQLite database and manages
//! local worker processes. It consolidates logic previously scattered across
//! workers.rs, daemon/lifecycle.rs, and other modules.

use super::{
    LifecycleAction, LifecycleContext, LifecycleError, LifecycleEvent, LifecycleManager,
    LifecycleResult, RunStateMachine,
};
use crate::core::config::Config;
use crate::core::files::Files;
use crate::core::runner::{create_lifecycle_runner_for_handle, WorkerHandle};
use crate::core::snapshot::{
    create_archive_strategy, host_session_path, AgentSnapshot, ArchiveStrategy, WorkDirSnapshot,
    WorkerStateHandle,
};
use crate::core::state::{FailureReason, SQLiteState, Status, WorkerStatus, WorkerUpdate};
// Note: Workers are no longer spawned directly from the lifecycle manager.
// The daemon handles spawning via the orchestrator, which uses the runner system.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tracing::{debug, info, warn};

/// Local lifecycle manager using SQLite state.
pub struct LocalLifecycleManager {
    context: LifecycleContext,
    state: SQLiteState,
    files: Files,
}

impl LocalLifecycleManager {
    /// Create a new LocalLifecycleManager.
    pub fn new(
        run_name: impl Into<String>,
        run_dir: PathBuf,
        agent_command: Vec<String>,
    ) -> LifecycleResult<Self> {
        let files = Files::new(&run_dir);
        let state =
            SQLiteState::new(files.db_path()).map_err(|e| LifecycleError::State(e.to_string()))?;

        Ok(Self {
            context: LifecycleContext::new(run_name, run_dir, agent_command),
            state,
            files,
        })
    }

    /// Create from existing state (for use in contexts where state is already open).
    pub fn from_state(
        run_name: impl Into<String>,
        run_dir: PathBuf,
        agent_command: Vec<String>,
        state: SQLiteState,
    ) -> Self {
        let files = Files::new(&run_dir);
        Self {
            context: LifecycleContext::new(run_name, run_dir, agent_command),
            state,
            files,
        }
    }

    /// Get a reference to the SQLite state.
    pub fn state(&self) -> &SQLiteState {
        &self.state
    }

    /// Get a reference to the Files helper.
    pub fn files(&self) -> &Files {
        &self.files
    }

    /// Kill all workers in the run.
    ///
    /// This is the public interface for killing workers, used when deleting runs
    /// or other cleanup operations. Uses the Runner trait to properly stop
    /// both local processes and Docker containers.
    pub fn kill_all_workers(&self) -> LifecycleResult<Vec<String>> {
        self.kill_all_workers_internal()
    }

    /// Resume workers that are paused, in error state, or awaiting tasks.
    ///
    /// This is the public interface for resuming workers after an eval fails
    /// or when workers need to be restarted.
    pub fn resume_awaiting_workers(&self) -> LifecycleResult<Vec<String>> {
        let actions = self.resume_awaiting_workers_internal()?;
        Ok(actions
            .into_iter()
            .filter_map(|action| {
                if let LifecycleAction::ResumeWorker { worker_name, .. } = action {
                    Some(worker_name)
                } else {
                    None
                }
            })
            .collect())
    }

    // =========================================================================
    // Internal Helpers
    // =========================================================================

    /// Kill all workers in the run using the Runner trait.
    ///
    /// This handles both local processes and Docker containers based on
    /// the stored runner_type.
    fn kill_all_workers_internal(&self) -> LifecycleResult<Vec<String>> {
        let workers = self
            .state
            .get_workers()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        let mut killed = Vec::new();

        // Helper to run async code - handles being called from within a tokio runtime
        fn run_async<F, T>(f: F) -> Result<T, String>
        where
            F: FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>
                + Send
                + 'static,
            T: Send + 'static,
        {
            if tokio::runtime::Handle::try_current().is_ok() {
                // Already in a tokio runtime - spawn a thread with its own runtime
                std::thread::scope(|s| {
                    s.spawn(|| {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| format!("Failed to create runtime: {}", e))?;
                        Ok(rt.block_on(f()))
                    })
                    .join()
                    .unwrap()
                })
            } else {
                // Not in a runtime - create one
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("Failed to create runtime: {}", e))?;
                Ok(rt.block_on(f()))
            }
        }

        for worker in workers {
            // Stop using runner_id and runner_type
            if let (Some(runner_id), Some(runner_type)) =
                (worker.runner_id.as_ref(), worker.runner_type.as_ref())
            {
                let handle = WorkerHandle {
                    worker_name: worker.name.clone(),
                    runner_id: runner_id.clone(),
                    runner_type: runner_type.clone(),
                };

                let runner = create_lifecycle_runner_for_handle(&handle);
                let worker_name = worker.name.clone();
                let runner_type_clone = runner_type.clone();
                let runner_id_clone = runner_id.clone();

                let stop_result =
                    run_async(move || Box::pin(async move { runner.stop(&handle).await }));

                match stop_result {
                    Ok(Ok(())) => {
                        info!(
                            "Stopped worker {} (runner_type: {}, runner_id: {})",
                            worker_name, runner_type_clone, runner_id_clone
                        );
                        killed.push(worker_name);
                    }
                    Ok(Err(e)) => {
                        warn!(
                            "Runner stop failed for {} (runner_type: {}): {}",
                            worker_name, runner_type_clone, e
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to run stop for {} (runner_type: {}): {}",
                            worker_name, runner_type_clone, e
                        );
                    }
                }
            } else {
                warn!("Worker {} has no runner info, cannot stop", worker.name);
            }

            // Clear PID and runner info from database
            if worker.pid.is_some() || worker.runner_id.is_some() {
                let _ = self.state.update_worker(
                    &worker.name,
                    WorkerUpdate {
                        pid: None,
                        runner_id: None,
                        ..Default::default()
                    },
                );
            }
        }

        Ok(killed)
    }

    /// Pause all workers (kill processes and mark as Paused).
    ///
    /// For ephemeral hosts (Fly), creates a snapshot of the work directory
    /// before stopping the worker so it can be restored on resume.
    fn pause_all_workers_internal(&self) -> LifecycleResult<Vec<String>> {
        // Helper to run async code - handles being called from within or outside a runtime
        fn run_async<F: std::future::Future>(f: F) -> F::Output {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                // Already in a runtime - use block_in_place to avoid nesting
                tokio::task::block_in_place(|| handle.block_on(f))
            } else {
                // Not in a runtime - create one
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create runtime")
                    .block_on(f)
            }
        }

        // Load config to get runner and storage settings
        let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

        let workers = self
            .state
            .get_workers()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        let mut paused = Vec::new();

        for worker in workers {
            // Skip already inactive workers
            if worker.status.is_inactive() {
                continue;
            }

            // Build unified state handle from work dir snapshot and agent session
            // Use stored runner configs (captured at run creation) with fallback to global config
            let runner_config = self
                .state
                .get_runner_config_for_worker(&worker.name, &config)
                .unwrap_or_default();
            let mut state_handle = WorkerStateHandle::new();

            // Create unified archive strategy
            let archive_strategy: Option<Box<dyn ArchiveStrategy>> =
                match run_async(create_archive_strategy(&runner_config, &config.storage)) {
                    Ok(strategy) => Some(strategy),
                    Err(e) => {
                        // No archive strategy is expected for local runners
                        debug!(
                            "No archive strategy for worker {} (expected for local): {}",
                            worker.name, e
                        );
                        None
                    }
                };

            // Archive work directory (for ephemeral hosts)
            if let (Some(ref work_dir_str), Some(ref strategy)) =
                (&worker.work_dir, &archive_strategy)
            {
                // Skip archive for no-op strategies (files persist on disk)
                if !strategy.is_noop() {
                    let work_dir = PathBuf::from(work_dir_str);
                    let archive_key = format!("{}/{}/workdir", self.context.run_name, worker.name);

                    match run_async(strategy.archive(&archive_key, &work_dir)) {
                        Ok(handle) => {
                            info!(
                                "Archived work dir for worker {}: {} -> {}",
                                worker.name,
                                strategy.strategy_type(),
                                handle.storage_id
                            );
                            state_handle.work_dir = Some(WorkDirSnapshot {
                                strategy_type: handle.strategy_type,
                                storage_id: handle.storage_id,
                                size_bytes: handle.size_bytes,
                            });
                        }
                        Err(e) => {
                            warn!(
                                "Failed to archive work dir for worker {}: {}",
                                worker.name, e
                            );
                        }
                    }
                }
            }

            // Archive agent session (for session resume on ephemeral hosts)
            if let (Some(ref session_id), Some(ref strategy)) =
                (&worker.session_id, &archive_strategy)
            {
                // Skip archive for no-op strategies (files persist on disk)
                if !strategy.is_noop() {
                    let session_dir = host_session_path(&self.context.run_dir, &worker.name);
                    let archive_key = format!("{}/{}/session", self.context.run_name, worker.name);

                    match run_async(strategy.archive(&archive_key, &session_dir)) {
                        Ok(handle) => {
                            info!(
                                "Archived agent session for worker {}: {} -> {}",
                                worker.name,
                                strategy.strategy_type(),
                                handle.storage_id
                            );
                            state_handle.agent_session = Some(AgentSnapshot {
                                agent_type: "claude".to_string(),
                                session_id: session_id.clone(),
                                storage_id: handle.storage_id,
                            });
                        }
                        Err(e) => {
                            warn!(
                                "Failed to archive agent session for worker {}: {}",
                                worker.name, e
                            );
                        }
                    }
                } else {
                    // For no-op strategy, still store the session_id for resume
                    state_handle.agent_session = Some(AgentSnapshot {
                        agent_type: "claude".to_string(),
                        session_id: session_id.clone(),
                        storage_id: String::new(),
                    });
                }
            }

            // Serialize unified state handle (only if we have state to persist)
            let state_handle_json = if state_handle.has_state() {
                serde_json::to_string(&state_handle).ok()
            } else {
                None
            };

            // Stop using runner_id and runner_type
            if let (Some(runner_id), Some(runner_type)) =
                (worker.runner_id.as_ref(), worker.runner_type.as_ref())
            {
                let handle = WorkerHandle {
                    worker_name: worker.name.clone(),
                    runner_id: runner_id.clone(),
                    runner_type: runner_type.clone(),
                };

                let runner = create_lifecycle_runner_for_handle(&handle);

                if let Err(e) = run_async(runner.stop(&handle)) {
                    warn!(
                        "Runner stop failed for {} (runner_type: {}): {}",
                        worker.name, runner_type, e
                    );
                } else {
                    info!(
                        "Stopped worker {} (runner_type: {}, runner_id: {})",
                        worker.name, runner_type, runner_id
                    );
                }
            } else {
                warn!("Worker {} has no runner info, cannot stop", worker.name);
            }

            // Mark as paused with unified state handle
            self.state
                .update_worker(
                    &worker.name,
                    WorkerUpdate {
                        pid: None,
                        runner_id: None,
                        status: Some(WorkerStatus::Paused),
                        state_handle: Some(state_handle_json),
                        ..Default::default()
                    },
                )
                .map_err(|e| LifecycleError::State(e.to_string()))?;
            paused.push(worker.name.clone());
        }

        Ok(paused)
    }

    /// Resume workers that are in paused/awaiting/error state.
    ///
    /// Returns a list of ResumeWorker actions. The caller (daemon) handles actual
    /// spawning via the orchestrator using resume_worker(). The orchestrator
    /// will restore snapshots for ephemeral runners.
    fn resume_awaiting_workers_internal(&self) -> LifecycleResult<Vec<LifecycleAction>> {
        // Get claimable tasks (needed for awaiting workers)
        let claimable = self
            .state
            .get_claimable_tasks()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Get workers that need to be resumed
        let workers = self
            .state
            .get_workers()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        let to_resume: Vec<_> = workers
            .iter()
            .filter(|w| {
                // Never auto-resume workers waiting for human input
                if w.hitl_waiting {
                    return false;
                }
                w.status == WorkerStatus::Paused
                    || w.status == WorkerStatus::Error
                    || (w.status == WorkerStatus::Awaiting && !claimable.is_empty())
            })
            .collect();

        if to_resume.is_empty() {
            return Ok(Vec::new());
        }

        // Check if run is paused - don't resume workers if so
        let status = self
            .state
            .status()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        if status == Status::Paused {
            debug!("resume_awaiting_workers: run is paused, not resuming");
            return Ok(Vec::new());
        }

        let mut actions = Vec::new();

        for worker in to_resume {
            // Get work_dir from database, fallback to standard location
            // Note: work_dir should never be empty, but if it is, use default
            let work_dir = worker
                .work_dir
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| self.context.run_dir.join("work").join(&worker.name));

            // Parse unified state handle if exists (don't restore here - orchestrator will do it)
            let state_handle = worker.state_handle.as_ref().and_then(|json| {
                match serde_json::from_str::<WorkerStateHandle>(json) {
                    Ok(handle) => Some(handle),
                    Err(e) => {
                        warn!(
                            "Failed to parse state handle for worker {}: {}",
                            worker.name, e
                        );
                        None
                    }
                }
            });

            info!(
                "resume_awaiting_workers: worker {} ready to resume, work_dir: {}, has_work_dir_snapshot: {}, has_agent_session: {}",
                worker.name,
                work_dir.display(),
                state_handle.as_ref().map(|h| h.work_dir.is_some()).unwrap_or(false),
                state_handle.as_ref().map(|h| h.agent_session.is_some()).unwrap_or(false)
            );

            actions.push(LifecycleAction::ResumeWorker {
                worker_name: worker.name.clone(),
                work_dir,
                resume_session_id: worker.session_id.clone(),
                state_handle,
            });
        }

        Ok(actions)
    }

    /// Try to scale up workers if autoscale is enabled and tasks are available.
    ///
    /// Returns a `SpawnWorker` action if a new worker should be spawned.
    /// The caller (daemon) handles actual spawning via the orchestrator.
    fn maybe_scale_up_internal(&self) -> LifecycleResult<Option<LifecycleAction>> {
        use crate::core::git::create_worker_clone;
        use crate::core::workers::WorkerScale;

        // Check if autoscaling is enabled
        let scale_str = match self
            .state
            .get_worker_scale()
            .map_err(|e| LifecycleError::State(e.to_string()))?
        {
            Some(s) => s,
            None => return Ok(None),
        };

        let scale = match WorkerScale::parse(&scale_str) {
            Some(s) => s,
            None => return Ok(None),
        };

        // Don't scale up if run is paused
        let status = self
            .state
            .status()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        if status == Status::Paused {
            debug!("maybe_scale_up: run is paused, not scaling");
            return Ok(None);
        }

        // Get current workers and claimable tasks
        let workers = self
            .state
            .get_workers()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        let current_count = workers.len();
        let claimable = self
            .state
            .get_claimable_tasks()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        let claimable_count = claimable.len();

        debug!(
            "maybe_scale_up: {} claimable tasks, {} workers, max {}",
            claimable_count, current_count, scale.max
        );

        // Scale up if: we have claimable tasks AND we haven't hit max workers
        if claimable_count == 0 || !scale.can_scale_up(current_count) {
            return Ok(None);
        }

        // Get a new worker name
        let existing_names: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();
        let new_name = crate::core::names::get_available_name(&existing_names);

        // Get project path
        let project_path_str = match self
            .state
            .get_project_path()
            .map_err(|e| LifecycleError::State(e.to_string()))?
        {
            Some(p) => p,
            None => {
                warn!("maybe_scale_up: no project path, cannot scale");
                return Ok(None);
            }
        };

        let project_path = PathBuf::from(&project_path_str);
        let staging_dir = self.context.run_dir.join("work").join("staging");

        // Create worker clone
        let worker_dir = match create_worker_clone(
            &self.context.run_name,
            &project_path,
            &new_name,
            Some(&staging_dir),
            &self.context.run_dir,
        ) {
            Ok(dir) => dir,
            Err(e) => {
                warn!("maybe_scale_up: failed to create worker clone: {}", e);
                return Ok(None);
            }
        };

        // Add worker to state (status will be set to Working by spawn_single_worker)
        // Use the default runner from the run config, not hardcoded "local"
        let location = self
            .state
            .get_default_runner()
            .ok()
            .flatten()
            .unwrap_or_else(|| "local".to_string());
        self.state
            .add_worker(&new_name, worker_dir.to_str().unwrap_or("."), &location)
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Create worker chat file
        let chat_file = self.files.chats_dir().join(format!("{}.md", new_name));
        if let Err(e) = std::fs::write(&chat_file, format!("# {} Chat\n\n", new_name)) {
            warn!("maybe_scale_up: failed to create worker chat: {}", e);
        }

        // Announce in group chat
        let reason = format!(
            "Autoscaling: {} tasks available, {} workers total",
            claimable.len(),
            current_count + 1
        );
        self.state
            .add_message(
                "group",
                "System",
                &format!(
                    "New worker **{}** has joined the team. {}",
                    new_name, reason
                ),
                false,
            )
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        info!(
            "maybe_scale_up: prepared worker {} for spawning, returning SpawnWorker action",
            new_name
        );

        // Return the SpawnWorker action - daemon will handle actual spawning via orchestrator
        Ok(Some(LifecycleAction::SpawnWorker {
            worker_name: new_name,
            work_dir: worker_dir,
        }))
    }

    /// Spawn the eval agent as a background process.
    fn spawn_eval_agent(&self) -> LifecycleResult<()> {
        // Get the hirsel executable
        let hirsel_exe =
            std::env::current_exe().map_err(|e| LifecycleError::Io(std::io::Error::other(e)))?;

        // Get agent command from config
        let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
        let agent_command = config.agent.command.clone();

        // Spawn the eval subprocess
        // Capture stderr to a log file for debugging
        let eval_log_path = self.context.run_dir.join("eval_spawn.log");
        let log_file = std::fs::File::create(&eval_log_path).ok();

        let mut cmd = Command::new(&hirsel_exe);
        cmd.arg("__eval-run")
            .arg("--run")
            .arg(&self.context.run_name)
            .arg("--run-dir")
            .arg(&self.context.run_dir)
            .arg("--agent-command")
            .arg(serde_json::to_string(&agent_command).unwrap_or_else(|_| "[]".to_string()))
            .stdin(Stdio::null())
            .stdout(Stdio::null());

        // Capture stderr to log file for debugging, fall back to null
        if let Some(file) = log_file {
            cmd.stderr(Stdio::from(file));
        } else {
            cmd.stderr(Stdio::null());
        }

        // Spawn detached
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let child = cmd
            .spawn()
            .map_err(|e| LifecycleError::Worker(format!("Failed to spawn eval agent: {}", e)))?;

        info!(
            "Spawned eval agent for run {}, pid={}",
            self.context.run_name,
            child.id()
        );

        Ok(())
    }

    /// Spawn summary generation in a background process.
    fn spawn_background_summary(&self) {
        let hirsel_exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                warn!("Failed to get current exe for summary: {}", e);
                return;
            }
        };

        match Command::new(&hirsel_exe)
            .args(["summary", &self.context.run_name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => {
                info!("Spawned summary generation for timed out run");
            }
            Err(e) => {
                warn!("Failed to spawn summary generation: {}", e);
            }
        }
    }

    /// Check if eval should be triggered and trigger it.
    ///
    /// Returns true if eval was triggered, false otherwise.
    fn maybe_trigger_eval(&self) -> LifecycleResult<bool> {
        // Check if all workers are inactive
        if !self
            .state
            .all_workers_inactive()
            .map_err(|e| LifecycleError::State(e.to_string()))?
        {
            debug!("maybe_trigger_eval: not all workers inactive, skipping");
            return Ok(false);
        }

        // Check if run is still in working status
        let status = self
            .state
            .status()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        if status != Status::Working {
            debug!(
                "maybe_trigger_eval: run status is {:?}, not Working, skipping",
                status
            );
            return Ok(false);
        }

        // Check if there are still claimable tasks
        let claimable = self
            .state
            .get_claimable_tasks()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        if !claimable.is_empty() {
            // There are still tasks to do - don't trigger eval or mark done
            // The daemon will try to resume/scale workers on the next poll
            debug!(
                "maybe_trigger_eval: {} claimable tasks remain, not triggering eval",
                claimable.len()
            );
            return Ok(false);
        }

        // Check if there's an eval script configured
        let eval_path = self.files.eval_spec();
        if !eval_path.exists() {
            // No eval script and no claimable tasks - check if ALL tasks are done
            let tasks = self
                .state
                .get_tasks()
                .map_err(|e| LifecycleError::State(e.to_string()))?;

            let incomplete_tasks: Vec<_> = tasks
                .iter()
                .filter(|t| t.status != crate::core::state::TaskStatus::Done)
                .collect();

            if !incomplete_tasks.is_empty() {
                // There are still incomplete tasks (blocked, doing, etc.)
                debug!(
                    "maybe_trigger_eval: {} incomplete tasks remain (not claimable), waiting",
                    incomplete_tasks.len()
                );
                return Ok(false);
            }

            // All tasks done - kill any remaining workers and set run to Done status
            let killed = self.kill_all_workers_internal()?;
            if !killed.is_empty() {
                info!(
                    "maybe_trigger_eval: killed {} remaining worker(s): {:?}",
                    killed.len(),
                    killed
                );
            }

            info!("maybe_trigger_eval: all workers inactive, all tasks done, no eval script, marking run as Done");
            self.state
                .set_status(Status::Done)
                .map_err(|e| LifecycleError::State(e.to_string()))?;

            return Ok(false);
        }

        // Trigger eval
        info!("maybe_trigger_eval: all workers inactive, triggering eval");
        self.state
            .set_status(Status::Eval)
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Spawn the eval agent in a background process
        self.spawn_eval_agent()?;

        Ok(true)
    }
}

impl LifecycleManager for LocalLifecycleManager {
    fn process_event(&self, event: LifecycleEvent) -> LifecycleResult<Vec<LifecycleAction>> {
        let mut actions = Vec::new();

        match event {
            LifecycleEvent::WorkerDone { worker_name } => {
                debug!("Processing WorkerDone event for {}", worker_name);
                actions.extend(self.worker_done(&worker_name)?);
            }

            LifecycleEvent::WorkerStatusChanged {
                worker_name,
                old,
                new,
            } => {
                debug!(
                    "Processing WorkerStatusChanged: {} {:?} -> {:?}",
                    worker_name, old, new
                );
                if new == WorkerStatus::Awaiting {
                    // Worker became inactive - check if we should trigger eval
                    if self.maybe_trigger_eval()? {
                        actions.push(LifecycleAction::EvalTriggered);
                    }
                }
            }

            LifecycleEvent::TaskCompleted {
                task_id: _,
                worker_name: _,
            } => {
                // Task completion might unblock other tasks
                // Try to resume awaiting workers and scale up
                let resume_actions = self.resume_awaiting_workers_internal()?;
                actions.extend(resume_actions);

                if let Some(action) = self.maybe_scale_up_internal()? {
                    actions.push(action);
                }
            }

            LifecycleEvent::TaskAdded { task_id: _ }
            | LifecycleEvent::TaskUnclaimed { task_id: _ } => {
                // New or unclaimed task - try to resume awaiting workers
                let resume_actions = self.resume_awaiting_workers_internal()?;
                actions.extend(resume_actions);

                if let Some(action) = self.maybe_scale_up_internal()? {
                    actions.push(action);
                }
            }

            LifecycleEvent::TimeCheck => {
                // Check if time has expired
                let expired = self
                    .state
                    .is_time_expired()
                    .map_err(|e| LifecycleError::State(e.to_string()))?;

                if expired {
                    self.handle_time_expired()?;
                    actions.push(LifecycleAction::RunFailed {
                        reason: FailureReason::TimeLimit,
                    });
                }

                // Check if we should trigger eval (for daemon polling case)
                if self.maybe_trigger_eval()? {
                    actions.push(LifecycleAction::EvalTriggered);
                }

                // Check if we should scale up
                if let Some(action) = self.maybe_scale_up_internal()? {
                    actions.push(action);
                }
            }

            LifecycleEvent::PauseRequested { reason } => {
                let paused = self.pause_run(&reason)?;
                if !paused.is_empty() {
                    actions.push(LifecycleAction::WorkersPaused(paused));
                }
                actions.push(LifecycleAction::RunStatusChanged(Status::Paused));
            }

            LifecycleEvent::ResumeRequested => {
                let resume_actions = self.resume_run()?;
                actions.extend(resume_actions);
                actions.push(LifecycleAction::RunStatusChanged(Status::Working));
            }

            LifecycleEvent::EvalCompleted { success, feedback } => {
                if success {
                    self.state
                        .set_status(Status::Done)
                        .map_err(|e| LifecycleError::State(e.to_string()))?;
                    actions.push(LifecycleAction::RunCompleted);
                } else {
                    // Eval failed - check if we should retry or fail the run
                    debug!("Eval failed with feedback: {}", feedback);
                    // Resume workers to continue working
                    self.state
                        .set_status(Status::Working)
                        .map_err(|e| LifecycleError::State(e.to_string()))?;
                    let resume_actions = self.resume_awaiting_workers_internal()?;
                    actions.extend(resume_actions);
                    actions.push(LifecycleAction::RunStatusChanged(Status::Working));
                }
            }
        }

        if actions.is_empty() {
            actions.push(LifecycleAction::None);
        }

        Ok(actions)
    }

    fn pause_run(&self, _reason: &str) -> LifecycleResult<Vec<String>> {
        // Check current status
        let status = self
            .state
            .status()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        if !RunStateMachine::can_transition(status, Status::Paused) {
            return Err(LifecycleError::InvalidTransition {
                from: status.to_string(),
                to: Status::Paused.to_string(),
            });
        }

        // Cancel any running evals
        let _ = self.state.cancel_running_evals("Run paused");

        // Pause all workers
        let paused = self.pause_all_workers_internal()?;

        // Update status
        self.state
            .set_status(Status::Paused)
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        Ok(paused)
    }

    fn resume_run(&self) -> LifecycleResult<Vec<LifecycleAction>> {
        // Check current status
        let status = self
            .state
            .status()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        if !RunStateMachine::can_transition(status, Status::Working) {
            return Err(LifecycleError::InvalidTransition {
                from: status.to_string(),
                to: Status::Working.to_string(),
            });
        }

        // Check if there was an eval that was paused
        let was_in_eval = self
            .state
            .has_paused_eval()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        if was_in_eval {
            let _ = self.state.clear_paused_evals();
            self.state
                .set_status(Status::Working)
                .map_err(|e| LifecycleError::State(e.to_string()))?;

            if self.maybe_trigger_eval()? {
                return Ok(Vec::new());
            }
        }

        // Clear HITL waiting flags for all workers - this allows workers
        // that were waiting for human input to continue
        self.state
            .clear_all_hitl_waiting()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Update status first
        self.state
            .set_status(Status::Working)
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Resume existing workers (returns ResumeWorker actions)
        let resume_actions = self.resume_awaiting_workers_internal()?;

        // Note: Scaling is now handled by the daemon through SpawnWorker actions.
        // The daemon will process actions and spawn workers via the orchestrator.

        // Check if eval should be triggered
        let _ = self.maybe_trigger_eval();

        Ok(resume_actions)
    }

    fn worker_done(&self, worker_name: &str) -> LifecycleResult<Vec<LifecycleAction>> {
        let mut actions = Vec::new();

        // Update worker status to awaiting
        self.state
            .update_worker(
                worker_name,
                WorkerUpdate {
                    status: Some(WorkerStatus::Awaiting),
                    ..Default::default()
                },
            )
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Check if eval should be triggered
        if self.maybe_trigger_eval()? {
            actions.push(LifecycleAction::EvalTriggered);
        }

        Ok(actions)
    }

    fn handle_time_expired(&self) -> LifecycleResult<()> {
        // Only handle if not already failed
        let status = self
            .state
            .status()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        if status == Status::Failed {
            info!("Time already expired, skipping handler");
            return Ok(());
        }

        // Send final message
        let workers = self
            .state
            .get_workers()
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        let is_multi_worker = workers.len() > 1;

        let message = "Time limit reached. Run failed.";
        let thread = if is_multi_worker { "group" } else { "user" };

        self.state
            .add_message(thread, "System", message, false)
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Cancel any running evals
        let cancelled = self
            .state
            .cancel_running_evals("Time limit reached")
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        if cancelled > 0 {
            info!("Cancelled {} running eval(s) due to timeout", cancelled);
        }

        // Kill all worker processes
        let killed = self.kill_all_workers_internal()?;
        if !killed.is_empty() {
            info!("Killed {} worker(s) on timeout: {:?}", killed.len(), killed);
        }

        // Set all active workers to PAUSED status
        for worker in &workers {
            if matches!(
                worker.status,
                WorkerStatus::Working | WorkerStatus::Awaiting
            ) {
                self.state
                    .update_worker(
                        &worker.name,
                        WorkerUpdate {
                            status: Some(WorkerStatus::Paused),
                            ..Default::default()
                        },
                    )
                    .map_err(|e| LifecycleError::State(e.to_string()))?;
            }
        }

        // Set run status to Failed with TimeLimit reason
        self.state
            .set_failed(FailureReason::TimeLimit)
            .map_err(|e| LifecycleError::State(e.to_string()))?;
        info!("Run status set to Failed (time_limit)");

        // Write timeout event to database
        let _ = self
            .state
            .insert_text_event("system", "\n[time limit reached - run timed out]");

        // Trigger summary generation in background
        self.spawn_background_summary();

        Ok(())
    }

    fn all_workers_inactive(&self) -> LifecycleResult<bool> {
        self.state
            .all_workers_inactive()
            .map_err(|e| LifecycleError::State(e.to_string()))
    }

    fn should_trigger_eval(&self) -> LifecycleResult<bool> {
        // Check if all workers are inactive
        if !self.all_workers_inactive()? {
            return Ok(false);
        }

        // Check if run is in Working status
        let status = self.run_status()?;
        if status != Status::Working {
            return Ok(false);
        }

        // Check if eval script exists
        Ok(self.files.eval_spec().exists())
    }

    fn can_scale_up(&self) -> LifecycleResult<bool> {
        use crate::core::workers::WorkerScale;

        // Check if autoscaling is enabled
        let scale_str = match self
            .state
            .get_worker_scale()
            .map_err(|e| LifecycleError::State(e.to_string()))?
        {
            Some(s) => s,
            None => return Ok(false),
        };

        let scale = match WorkerScale::parse(&scale_str) {
            Some(s) => s,
            None => return Ok(false),
        };

        // Get current count
        let workers = self
            .state
            .get_workers()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        // Check if we have claimable tasks
        let claimable = self
            .state
            .get_claimable_tasks()
            .map_err(|e| LifecycleError::State(e.to_string()))?;

        Ok(!claimable.is_empty() && scale.can_scale_up(workers.len()))
    }

    fn run_status(&self) -> LifecycleResult<Status> {
        self.state
            .status()
            .map_err(|e| LifecycleError::State(e.to_string()))
    }

    fn context(&self) -> &LifecycleContext {
        &self.context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_context_new() {
        let ctx = LifecycleContext::new(
            "test-run",
            PathBuf::from("/tmp/test"),
            vec!["hirsel".to_string(), "__acp-bridge".to_string()],
        );
        assert_eq!(ctx.run_name, "test-run");
        assert_eq!(ctx.run_dir, PathBuf::from("/tmp/test"));
        assert_eq!(ctx.agent_command.len(), 2);
    }
}
