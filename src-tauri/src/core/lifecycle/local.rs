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
use crate::core::delta::{BoardNode, BoardNodeStatus, DeltaState, NodeKind};
use crate::core::files::Files;
use crate::core::runner::{create_lifecycle_runner_for_handle, WorkerHandle};
use crate::core::snapshot::{
    create_archive_strategy, host_session_path, AgentSnapshot, ArchiveStrategy, WorkDirSnapshot,
    WorkerStateHandle,
};
use crate::core::state::{FailureReason, SQLiteState, Status, WorkerStatus, WorkerUpdate};
// Note: Workers are no longer spawned directly from the lifecycle manager.
// The daemon handles spawning via the orchestrator, which uses the runner system.
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info, warn};

static WORKER_NAME_SEQ: AtomicU64 = AtomicU64::new(1);

/// Local lifecycle manager using SQLite state.
pub struct LocalLifecycleManager {
    context: LifecycleContext,
    state: SQLiteState,
    files: Files,
}

impl LocalLifecycleManager {
    /// Create a new LocalLifecycleManager.
    pub async fn new(
        runtime_name: impl Into<String>,
        runtime_dir: PathBuf,
        agent_command: Vec<String>,
    ) -> LifecycleResult<Self> {
        let runtime_name = runtime_name.into();
        let files = Files::new(&runtime_dir);
        let state = SQLiteState::new(&runtime_name).await?;

        Ok(Self {
            context: LifecycleContext::new(&runtime_name, runtime_dir, agent_command),
            state,
            files,
        })
    }

    /// Create from existing state (for use in contexts where state is already open).
    pub fn from_state(
        runtime_name: impl Into<String>,
        runtime_dir: PathBuf,
        agent_command: Vec<String>,
        state: SQLiteState,
    ) -> Self {
        let files = Files::new(&runtime_dir);
        Self {
            context: LifecycleContext::new(runtime_name, runtime_dir, agent_command),
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

    /// Get DeltaState for this run's project, if linked to a project.
    async fn get_delta_state(&self) -> Option<DeltaState> {
        let project_id = self.state.get_project_id().await.ok().flatten()?;
        let route_id = self.state.get_route_id().await.ok()?;
        Some(DeltaState::with_route(project_id, route_id))
    }

    /// Get claimable nodes (board nodes that can be claimed).
    /// Uses board nodes from the project's delta state.
    async fn get_claimable_nodes(&self) -> LifecycleResult<Vec<BoardNode>> {
        if let Some(delta_state) = self.get_delta_state().await {
            Ok(delta_state.get_claimable_nodes().await?)
        } else {
            Ok(vec![])
        }
    }

    /// Get all board nodes from the project's delta state.
    async fn get_all_nodes(&self) -> LifecycleResult<Vec<BoardNode>> {
        if let Some(delta_state) = self.get_delta_state().await {
            Ok(delta_state.get_nodes().await?)
        } else {
            Ok(vec![])
        }
    }

    /// Claim a board node for a worker.
    async fn claim_node(&self, node_id: &str, worker_name: &str) -> LifecycleResult<BoardNode> {
        let delta_state = self
            .get_delta_state()
            .await
            .ok_or_else(|| LifecycleError::State("Run not linked to project".into()))?;
        Ok(delta_state.claim_node(node_id, worker_name).await?)
    }

    fn claim_priority(node: &BoardNode) -> (u8, u8, i32) {
        let kind_rank = match node.kind {
            NodeKind::Check => 0,
            NodeKind::Task | NodeKind::Plan => 1,
            NodeKind::Feature => 2,
        };
        (kind_rank, 0, node.position)
    }

    fn generate_ephemeral_worker_name(used: &HashSet<String>) -> String {
        loop {
            let base = crate::core::names::generate_worker_name();
            let seq = WORKER_NAME_SEQ.fetch_add(1, Ordering::Relaxed);
            let candidate = format!("{}-{:x}", base, seq);
            if !used.contains(&candidate) {
                return candidate;
            }
        }
    }

    /// Kill all workers in the run.
    ///
    /// This is the public interface for killing workers, used when deleting runs
    /// or other cleanup operations. Uses the Runner trait to properly stop
    /// both local processes and Docker containers.
    pub async fn kill_all_workers(&self) -> LifecycleResult<Vec<String>> {
        self.kill_all_workers_internal().await
    }

    /// Resume workers that are paused, in error state, or awaiting tasks.
    ///
    /// This is the public interface for resuming workers after an eval fails
    /// or when workers need to be restarted.
    pub async fn resume_awaiting_workers(&self) -> LifecycleResult<Vec<String>> {
        let actions = self.resume_awaiting_workers_internal().await?;
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
    async fn kill_all_workers_internal(&self) -> LifecycleResult<Vec<String>> {
        let workers = self.state.get_workers().await?;
        let mut killed = Vec::new();

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

                match runner.stop(&handle).await {
                    Ok(()) => {
                        info!(
                            "Stopped worker {} (runner_type: {}, runner_id: {})",
                            worker.name, runner_type, runner_id
                        );
                        killed.push(worker.name.clone());
                    }
                    Err(e) => {
                        warn!(
                            "Runner stop failed for {} (runner_type: {}): {}",
                            worker.name, runner_type, e
                        );
                    }
                }
            } else {
                warn!("Worker {} has no runner info, cannot stop", worker.name);
            }

            // Clear PID and runner info from database
            if worker.pid.is_some() || worker.runner_id.is_some() {
                if let Err(e) = self
                    .state
                    .update_worker(
                        &worker.name,
                        WorkerUpdate {
                            pid: Some(None),
                            runner_id: None,
                            ..Default::default()
                        },
                    )
                    .await
                {
                    warn!(
                        "Failed to clear worker {} state after kill: {}",
                        worker.name, e
                    );
                }
            }
        }

        Ok(killed)
    }

    /// Pause all workers (kill processes and mark as Paused).
    ///
    /// For storage-backed ephemeral hosts, creates a snapshot of the work directory
    /// before stopping the worker so it can be restored on resume.
    async fn pause_all_workers_internal(&self) -> LifecycleResult<Vec<String>> {
        // Load config to get runner and storage settings
        let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

        let workers = self.state.get_workers().await?;
        let mut paused = Vec::new();

        for worker in workers {
            // Skip already inactive workers
            if worker.status.is_inactive() {
                continue;
            }

            // Build unified state handle from work dir snapshot and agent session
            let runner_config = config.sandbox_config();
            let mut state_handle = WorkerStateHandle::new();

            // Create unified archive strategy
            let archive_strategy: Option<Box<dyn ArchiveStrategy>> =
                match create_archive_strategy(&runner_config, &config.storage).await {
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

            // Archive work directory for storage-backed ephemeral hosts.
            if let (Some(ref work_dir_str), Some(ref strategy)) =
                (&worker.work_dir, &archive_strategy)
            {
                // Skip archive for no-op strategies (files persist on disk)
                if !strategy.is_noop() {
                    let work_dir = PathBuf::from(work_dir_str);
                    let archive_key =
                        format!("{}/{}/workdir", self.context.runtime_name, worker.name);

                    match strategy.archive(&archive_key, &work_dir).await {
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

            // Archive agent session for session resume on storage-backed ephemeral hosts.
            if let (Some(ref session_id), Some(ref strategy)) =
                (&worker.session_id, &archive_strategy)
            {
                // Skip archive for no-op strategies (files persist on disk)
                if !strategy.is_noop() {
                    let session_dir = host_session_path(&self.context.runtime_dir, &worker.name);
                    let archive_key =
                        format!("{}/{}/session", self.context.runtime_name, worker.name);

                    match strategy.archive(&archive_key, &session_dir).await {
                        Ok(handle) => {
                            info!(
                                "Archived agent session for worker {}: {} -> {}",
                                worker.name,
                                strategy.strategy_type(),
                                handle.storage_id
                            );
                            state_handle.agent_session = Some(AgentSnapshot {
                                agent_type: "codex".to_string(),
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
                        agent_type: "codex".to_string(),
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

                if let Err(e) = runner.stop(&handle).await {
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
                        pid: Some(None),
                        runner_id: None,
                        status: Some(WorkerStatus::Paused),
                        state_handle: Some(state_handle_json),
                        ..Default::default()
                    },
                )
                .await?;
            paused.push(worker.name.clone());
        }

        Ok(paused)
    }

    /// Resume workers that are in paused/awaiting/error state.
    ///
    /// Returns a list of ResumeWorker actions. The caller (daemon) handles actual
    /// spawning via the orchestrator using resume_worker(). The orchestrator
    /// will restore snapshots for ephemeral runners.
    async fn resume_awaiting_workers_internal(&self) -> LifecycleResult<Vec<LifecycleAction>> {
        let workers = self.state.get_workers().await?;
        let all_nodes = self.get_all_nodes().await.unwrap_or_default();

        let to_resume: Vec<_> = workers
            .iter()
            .filter(|w| {
                // Never auto-resume workers waiting for human input
                if w.hitl_waiting {
                    return false;
                }

                let Some(task_id) = w.assigned_task_id.as_ref() else {
                    return false;
                };

                let task_still_working = all_nodes.iter().any(|n| {
                    n.id == *task_id
                        && n.status == BoardNodeStatus::Working
                        && n.claimed_by.as_deref() == Some(w.name.as_str())
                });
                if !task_still_working {
                    return false;
                }

                w.status == WorkerStatus::Paused
                    || w.status == WorkerStatus::Error
                    || w.status == WorkerStatus::Awaiting
            })
            .collect();

        if to_resume.is_empty() {
            return Ok(Vec::new());
        }

        // Check if run is paused - don't resume workers if so
        let status = self.state.status().await?;
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
                .unwrap_or_else(|| self.context.runtime_dir.join("work").join(&worker.name));

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

    /// Autoscaling is disabled in the explicit-delegation runtime.
    ///
    /// New workers are spawned only through orchestrator delegation.
    async fn maybe_scale_up_internal(&self) -> LifecycleResult<Option<LifecycleAction>> {
        Ok(None)
    }

    /// Evaluate scaling needs and return actions for spawning/waking workers.
    ///
    /// This is the event-driven scaling evaluation that replaces the old polling-based
    /// approach. It's called when the scaling_check_requested flag is set.
    pub async fn evaluate_scaling(&self) -> LifecycleResult<Vec<LifecycleAction>> {
        use crate::core::git::create_worker_clone;

        // Don't scale if run is paused
        let status = self.state.status().await?;
        if status == Status::Paused {
            debug!("evaluate_scaling: run is paused, not scaling");
            return Ok(vec![]);
        }

        let mut claimable = self.get_claimable_nodes().await?;
        claimable.sort_by_key(Self::claim_priority);

        let workers = self.state.get_workers().await?;
        let max_workers = workers
            .iter()
            .filter(|w| w.status == WorkerStatus::Working || w.assigned_task_id.is_some())
            .count();
        let all_nodes = self.get_all_nodes().await?;
        let mut actions = vec![];
        let mut used_names: HashSet<String> = workers.iter().map(|w| w.name.clone()).collect();

        // Ephemeral workers: cleanup idle workers with no active assignment,
        // and only resume workers that still own a working task.
        let mut resumed_workers = HashSet::new();
        for worker in workers.iter().filter(|w| w.status != WorkerStatus::Working) {
            if worker.hitl_waiting {
                continue;
            }

            let assigned_task_id = match worker.assigned_task_id.as_ref() {
                Some(task_id) => task_id,
                None => {
                    if let Err(e) = self.state.delete_worker(&worker.name).await {
                        warn!(
                            "evaluate_scaling: failed to delete finished worker {}: {}",
                            worker.name, e
                        );
                    } else {
                        info!("evaluate_scaling: deleted finished worker {}", worker.name);
                        used_names.remove(&worker.name);
                    }
                    continue;
                }
            };

            let task_is_still_working = all_nodes.iter().any(|n| {
                n.id == *assigned_task_id
                    && n.status == BoardNodeStatus::Working
                    && n.claimed_by.as_deref() == Some(worker.name.as_str())
            });

            if task_is_still_working {
                let work_dir = worker
                    .work_dir
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.context.runtime_dir.join("work").join(&worker.name));

                actions.push(LifecycleAction::ResumeWorker {
                    worker_name: worker.name.clone(),
                    work_dir,
                    resume_session_id: worker.session_id.clone(),
                    state_handle: worker
                        .state_handle
                        .as_ref()
                        .and_then(|json| serde_json::from_str::<WorkerStateHandle>(json).ok()),
                });
                resumed_workers.insert(worker.name.clone());
                info!(
                    "evaluate_scaling: resuming worker {} for assigned task {}",
                    worker.name, assigned_task_id
                );
            } else if let Err(e) = self.state.delete_worker(&worker.name).await {
                warn!(
                    "evaluate_scaling: failed to delete stale worker {}: {}",
                    worker.name, e
                );
            } else {
                info!(
                    "evaluate_scaling: deleted stale worker {} (task no longer working)",
                    worker.name
                );
                used_names.remove(&worker.name);
            }
        }

        if claimable.is_empty() {
            return Ok(actions);
        }

        let active_count = workers
            .iter()
            .filter(|w| w.status == WorkerStatus::Working)
            .count();
        let occupied_slots = active_count + resumed_workers.len();
        let available_slots = max_workers.saturating_sub(occupied_slots);

        if available_slots == 0 {
            return Ok(actions);
        }

        let project_path_str = match self.state.get_project_path().await? {
            Some(p) => p,
            None => {
                warn!("evaluate_scaling: no project path, cannot spawn new workers");
                return Ok(actions);
            }
        };

        let project_path = PathBuf::from(&project_path_str);
        let staging_dir = self.context.runtime_dir.join("work").join("staging");
        let to_spawn = available_slots.min(claimable.len());

        for node in claimable.into_iter().take(to_spawn) {
            let new_name = Self::generate_ephemeral_worker_name(&used_names);
            used_names.insert(new_name.clone());

            let worker_dir = match create_worker_clone(
                &self.context.runtime_name,
                &project_path,
                &new_name,
                Some(&staging_dir),
                &self.context.runtime_dir,
            ) {
                Ok(dir) => dir,
                Err(e) => {
                    warn!(
                        "evaluate_scaling: failed to create worker clone for {}: {}",
                        new_name, e
                    );
                    used_names.remove(&new_name);
                    continue;
                }
            };

            let location = Config::load()
                .map(|(cfg, _)| cfg.sandbox_config().execution_kind().to_string())
                .unwrap_or_else(|_| "local".to_string());

            if let Err(e) = self
                .state
                .add_worker(
                    &new_name,
                    worker_dir.to_str().unwrap_or("."),
                    &location,
                    None,
                )
                .await
            {
                warn!("evaluate_scaling: failed to add worker {}: {}", new_name, e);
                used_names.remove(&new_name);
                continue;
            }

            if let Err(e) = self.claim_node(&node.id, &new_name).await {
                warn!(
                    "evaluate_scaling: failed to claim node {} for worker {}: {}",
                    node.id, new_name, e
                );
                let _ = self.state.delete_worker(&new_name).await;
                used_names.remove(&new_name);
                continue;
            }

            if let Err(e) = self
                .state
                .update_worker(
                    &new_name,
                    WorkerUpdate {
                        assigned_task_id: Some(Some(node.id.clone())),
                        ..Default::default()
                    },
                )
                .await
            {
                warn!(
                    "evaluate_scaling: failed to set assigned_task_id for {}: {}",
                    new_name, e
                );
            }

            actions.push(LifecycleAction::SpawnWorker {
                worker_name: new_name.clone(),
                work_dir: worker_dir,
                assigned_task_id: Some(node.id.clone()),
            });
            info!(
                "evaluate_scaling: spawning new worker {} with node {}",
                new_name, node.id
            );
        }

        Ok(actions)
    }

    /// Spawn the eval agent as a background process.
    fn spawn_eval_agent(&self) -> LifecycleResult<()> {
        // Get the hirsel executable
        let hirsel_exe =
            std::env::current_exe().map_err(|e| LifecycleError::Io(std::io::Error::other(e)))?;

        // Get agent command from config
        let (config, _) = Config::load().unwrap_or_else(|e| {
            warn!(
                "Failed to load config for eval spawn, using defaults: {}",
                e
            );
            (Config::default(), vec![])
        });
        let agent_command = config.agent.command.clone();

        // Spawn the eval subprocess
        // Capture stderr to a log file for debugging
        let eval_log_path = self.context.runtime_dir.join("eval_spawn.log");
        let log_file = match std::fs::File::create(&eval_log_path) {
            Ok(f) => Some(f),
            Err(e) => {
                warn!("Failed to create eval log file {:?}: {}", eval_log_path, e);
                None
            }
        };

        let mut cmd = Command::new(&hirsel_exe);
        cmd.arg("__eval-run")
            .arg("--runtime")
            .arg(&self.context.runtime_name)
            .arg("--runtime-dir")
            .arg(&self.context.runtime_dir)
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
            "Spawned eval agent for runtime {}, pid={}",
            self.context.runtime_name,
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
            .args(["summary", &self.context.runtime_name])
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
    async fn maybe_trigger_eval(&self) -> LifecycleResult<bool> {
        // Check if all workers are inactive
        if !self.state.all_workers_inactive().await? {
            debug!("maybe_trigger_eval: not all workers inactive, skipping");
            return Ok(false);
        }

        // Check if run is still in working status
        let status = self.state.status().await?;
        if status != Status::Working {
            debug!(
                "maybe_trigger_eval: run status is {:?}, not Working, skipping",
                status
            );
            return Ok(false);
        }

        // Check if there are still claimable nodes
        let claimable = self.get_claimable_nodes().await?;

        if !claimable.is_empty() {
            // There are still nodes to do - don't trigger eval or mark done
            // The daemon will try to resume/scale workers on the next poll
            debug!(
                "maybe_trigger_eval: {} claimable nodes remain, not triggering eval",
                claimable.len()
            );
            return Ok(false);
        }

        // Check if there's an eval script configured
        let eval_path = self.files.eval_spec();
        if !eval_path.exists() {
            // No eval script and no claimable nodes - check if ALL nodes are done
            let nodes = self.get_all_nodes().await?;

            // Nodes are complete if:
            // - Work nodes: Done or Validated
            // - Eval nodes: Done
            let incomplete_nodes: Vec<_> = nodes
                .iter()
                .filter(|n| {
                    use crate::core::delta::NodeKind;
                    match n.kind {
                        NodeKind::Task | NodeKind::Feature | NodeKind::Plan => {
                            n.status != BoardNodeStatus::Done
                                && n.status != BoardNodeStatus::Validated
                        }
                        NodeKind::Check => n.status != BoardNodeStatus::Done,
                    }
                })
                .collect();

            if !incomplete_nodes.is_empty() {
                // There are still incomplete nodes (blocked, working, etc.)
                debug!(
                    "maybe_trigger_eval: {} incomplete nodes remain (not claimable), waiting",
                    incomplete_nodes.len()
                );
                return Ok(false);
            }

            // All nodes done - kill any remaining workers and set run to Done status
            let killed = self.kill_all_workers_internal().await?;
            if !killed.is_empty() {
                info!(
                    "maybe_trigger_eval: killed {} remaining worker(s): {:?}",
                    killed.len(),
                    killed
                );
            }

            info!("maybe_trigger_eval: all workers inactive, all nodes done, no eval script, marking run as Done");
            self.state.set_status(Status::Done).await?;

            return Ok(false);
        }

        // Trigger eval
        info!("maybe_trigger_eval: all workers inactive, triggering eval");
        self.state.set_status(Status::Eval).await?;

        // Spawn the eval agent in a background process
        self.spawn_eval_agent()?;

        Ok(true)
    }
}

impl LifecycleManager for LocalLifecycleManager {
    async fn process_event(&self, event: LifecycleEvent) -> LifecycleResult<Vec<LifecycleAction>> {
        let mut actions = Vec::new();

        match event {
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
                    if self.maybe_trigger_eval().await? {
                        actions.push(LifecycleAction::EvalTriggered);
                    }
                }
            }

            LifecycleEvent::TimeCheck => {
                // Check if time has expired
                let expired = self.state.is_time_expired().await?;

                if expired {
                    self.handle_time_expired().await?;
                    actions.push(LifecycleAction::RunFailed {
                        reason: FailureReason::TimeLimit,
                    });
                }

                // Check if we should trigger eval (for daemon polling case)
                if self.maybe_trigger_eval().await? {
                    actions.push(LifecycleAction::EvalTriggered);
                }

                // Check if we should scale up
                if let Some(action) = self.maybe_scale_up_internal().await? {
                    actions.push(action);
                }
            }
        }

        if actions.is_empty() {
            actions.push(LifecycleAction::None);
        }

        Ok(actions)
    }

    async fn pause_run(&self, _reason: &str) -> LifecycleResult<Vec<String>> {
        // Check current status
        let status = self.state.status().await?;

        if !RunStateMachine::can_transition(status, Status::Paused) {
            return Err(LifecycleError::InvalidTransition {
                from: status.to_string(),
                to: Status::Paused.to_string(),
            });
        }

        // Cancel any running evals
        if let Err(e) = self.state.cancel_running_evals("Run paused").await {
            warn!("Failed to cancel running evals during pause: {}", e);
        }

        // Pause all workers
        let paused = self.pause_all_workers_internal().await?;

        // Update status
        self.state.set_status(Status::Paused).await?;

        Ok(paused)
    }

    async fn resume_run(&self) -> LifecycleResult<Vec<LifecycleAction>> {
        // Check current status
        let status = self.state.status().await?;

        if !RunStateMachine::can_transition(status, Status::Working) {
            return Err(LifecycleError::InvalidTransition {
                from: status.to_string(),
                to: Status::Working.to_string(),
            });
        }

        // Check if there was an eval that was paused
        let was_in_eval = self.state.has_paused_eval().await?;

        if was_in_eval {
            if let Err(e) = self.state.clear_paused_evals().await {
                warn!("Failed to clear paused evals on resume: {}", e);
            }
            self.state.set_status(Status::Working).await?;

            if self.maybe_trigger_eval().await? {
                return Ok(Vec::new());
            }
        }

        // Clear HITL waiting flags for all workers - this allows workers
        // that were waiting for human input to continue
        self.state.clear_all_hitl_waiting().await?;

        // Update status first
        self.state.set_status(Status::Working).await?;

        // Resume existing workers (returns ResumeWorker actions)
        let resume_actions = self.resume_awaiting_workers_internal().await?;

        // Note: Scaling is now handled by the daemon through SpawnWorker actions.
        // The daemon will process actions and spawn workers via the orchestrator.

        // Check if eval should be triggered
        if let Err(e) = self.maybe_trigger_eval().await {
            warn!("Failed to check eval trigger on resume: {}", e);
        }

        Ok(resume_actions)
    }

    async fn handle_time_expired(&self) -> LifecycleResult<()> {
        // Only handle if not already failed
        let status = self.state.status().await?;
        if status == Status::Failed {
            info!("Time already expired, skipping handler");
            return Ok(());
        }

        // Cancel any running evals
        let cancelled = self
            .state
            .cancel_running_evals("Time limit reached")
            .await?;
        if cancelled > 0 {
            info!("Cancelled {} running eval(s) due to timeout", cancelled);
        }

        // Kill all worker processes
        let killed = self.kill_all_workers_internal().await?;
        if !killed.is_empty() {
            info!("Killed {} worker(s) on timeout: {:?}", killed.len(), killed);
        }

        // Set all active workers to PAUSED status
        let workers = self.state.get_workers().await?;
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
                    .await?;
            }
        }

        // Set runtime status to Failed with TimeLimit reason
        self.state.set_failed(FailureReason::TimeLimit).await?;
        info!("Runtime status set to Failed (time_limit)");

        // Write timeout event to database
        if let Err(e) = self
            .state
            .insert_text_event("system", "\n[time limit reached - run timed out]")
            .await
        {
            warn!("Failed to write timeout event to database: {}", e);
        }

        // Trigger summary generation in background
        self.spawn_background_summary();

        Ok(())
    }

    async fn run_status(&self) -> LifecycleResult<Status> {
        self.state
            .status()
            .await
            .map_err(|e| LifecycleError::State(e.to_string()))
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
            vec!["codex".to_string()],
        );
        assert_eq!(ctx.runtime_name, "test-run");
        assert_eq!(ctx.runtime_dir, PathBuf::from("/tmp/test"));
    }
}
