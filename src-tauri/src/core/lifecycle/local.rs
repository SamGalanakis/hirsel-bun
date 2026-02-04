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
use crate::core::delta::{DeltaState, LiveNode, LiveNodeStatus};
use crate::core::files::Files;
use crate::core::runner::{create_lifecycle_runner_for_handle, WorkerHandle};
use crate::core::snapshot::{
    create_archive_strategy, host_session_path, AgentSnapshot, ArchiveStrategy, WorkDirSnapshot,
    WorkerStateHandle,
};
use crate::core::state::{FailureReason, SQLiteState, Status, WorkerStatus, WorkerUpdate};
use crate::core::ProjectMessagesStore;
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
    pub async fn new(
        run_name: impl Into<String>,
        run_dir: PathBuf,
        agent_command: Vec<String>,
    ) -> LifecycleResult<Self> {
        let run_name = run_name.into();
        let files = Files::new(&run_dir);
        let state = SQLiteState::new(&run_name).await?;

        Ok(Self {
            context: LifecycleContext::new(&run_name, run_dir, agent_command),
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

    /// Get DeltaState for this run's project, if linked to a project.
    async fn get_delta_state(&self) -> Option<DeltaState> {
        let project_id = self.state.get_project_id().await.ok().flatten()?;
        let route_id = self.state.get_route_id().await.ok()?;
        Some(DeltaState::with_route(project_id, route_id))
    }

    /// Send a system message to the group chat (meadow).
    /// Uses project messages if the run is linked to a project.
    async fn send_system_message(&self, message: &str) {
        if let (Ok(Some(project_id)), Ok(route_id)) = (
            self.state.get_project_id().await,
            self.state.get_route_id().await,
        ) {
            if let Ok(store) = ProjectMessagesStore::open().await {
                if let Err(e) = store
                    .add_message(project_id, route_id, "meadow", "system", message, false)
                    .await
                {
                    warn!("Failed to send system message to project: {}", e);
                }
            }
        }
    }

    /// Send a system message to a specific worker (their DM thread).
    /// Uses project messages if the run is linked to a project.
    async fn send_system_message_to_worker(&self, worker_name: &str, message: &str) {
        if let (Ok(Some(project_id)), Ok(route_id)) = (
            self.state.get_project_id().await,
            self.state.get_route_id().await,
        ) {
            if let Ok(store) = ProjectMessagesStore::open().await {
                if let Err(e) = store
                    .add_message(project_id, route_id, worker_name, "system", message, false)
                    .await
                {
                    warn!(
                        "Failed to send system message to worker {}: {}",
                        worker_name, e
                    );
                }
            }
        }
    }

    /// Get claimable nodes (live nodes that can be claimed).
    /// Uses live nodes from the project's delta state.
    async fn get_claimable_nodes(&self) -> LifecycleResult<Vec<LiveNode>> {
        if let Some(delta_state) = self.get_delta_state().await {
            Ok(delta_state.get_claimable_nodes().await?)
        } else {
            Ok(vec![])
        }
    }

    /// Get all live nodes from the project's delta state.
    async fn get_all_nodes(&self) -> LifecycleResult<Vec<LiveNode>> {
        if let Some(delta_state) = self.get_delta_state().await {
            Ok(delta_state.get_live_nodes().await?)
        } else {
            Ok(vec![])
        }
    }

    /// Claim a live node for a worker.
    async fn claim_node(&self, node_id: &str, worker_name: &str) -> LifecycleResult<LiveNode> {
        let delta_state = self
            .get_delta_state()
            .await
            .ok_or_else(|| LifecycleError::State("Run not linked to project".into()))?;
        Ok(delta_state.claim_live_node(node_id, worker_name).await?)
    }

    /// Pick the best node for a worker based on tree-walk distance.
    ///
    /// For work nodes: prefer nodes CLOSE to the worker's last completed task
    /// For eval nodes: prefer nodes FAR from the worker's last completed task
    fn pick_node_for_worker(
        &self,
        claimable: &[LiveNode],
        worker: &crate::core::state::Worker,
        all_nodes: &[LiveNode],
    ) -> Option<LiveNode> {
        use crate::core::delta::NodeType;

        if claimable.is_empty() {
            return None;
        }

        // If no history, just return first node
        let last_task_id = match &worker.last_task_id {
            Some(id) => id,
            None => return Some(claimable[0].clone()),
        };

        // Calculate tree distances from last_task_id using BFS
        let distances = self.calculate_node_distances(all_nodes, last_task_id);

        // Score each claimable node
        let mut scored: Vec<_> = claimable
            .iter()
            .map(|n| {
                let dist = distances.get(&n.id).copied().unwrap_or(usize::MAX);
                let score = match n.node_type {
                    NodeType::Eval => {
                        // Eval: prefer FAR (high distance = high score = pick first)
                        dist
                    }
                    NodeType::Task => {
                        // Work: prefer CLOSE (low distance = high score)
                        usize::MAX.saturating_sub(dist)
                    }
                };
                (n, score)
            })
            .collect();

        // Sort by score descending (highest score first)
        scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
        scored.first().map(|(n, _)| (*n).clone())
    }

    /// Calculate tree distances from a given node using BFS.
    fn calculate_node_distances(
        &self,
        nodes: &[LiveNode],
        from_id: &str,
    ) -> std::collections::HashMap<String, usize> {
        use std::collections::{HashMap, VecDeque};

        let mut distances: HashMap<String, usize> = HashMap::new();
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();

        distances.insert(from_id.to_string(), 0);
        queue.push_back((from_id.to_string(), 0));

        while let Some((id, dist)) = queue.pop_front() {
            // Find the current node
            let node = match nodes.iter().find(|n| n.id == id) {
                Some(n) => n,
                None => continue,
            };

            // Parent edge
            if let Some(ref parent) = node.parent_id {
                if !distances.contains_key(parent) {
                    distances.insert(parent.clone(), dist + 1);
                    queue.push_back((parent.clone(), dist + 1));
                }
            }

            // Child edges
            for child in nodes.iter().filter(|c| c.parent_id.as_ref() == Some(&id)) {
                if !distances.contains_key(&child.id) {
                    distances.insert(child.id.clone(), dist + 1);
                    queue.push_back((child.id.clone(), dist + 1));
                }
            }
        }

        distances
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
                            pid: None,
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
    /// For ephemeral hosts (Fly), creates a snapshot of the work directory
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
            let runner_config = self
                .state
                .get_runner_config_for_worker(&worker.name)
                .await
                .unwrap_or_default();
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

            // Archive work directory (for ephemeral hosts)
            if let (Some(ref work_dir_str), Some(ref strategy)) =
                (&worker.work_dir, &archive_strategy)
            {
                // Skip archive for no-op strategies (files persist on disk)
                if !strategy.is_noop() {
                    let work_dir = PathBuf::from(work_dir_str);
                    let archive_key = format!("{}/{}/workdir", self.context.run_name, worker.name);

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

            // Archive agent session (for session resume on ephemeral hosts)
            if let (Some(ref session_id), Some(ref strategy)) =
                (&worker.session_id, &archive_strategy)
            {
                // Skip archive for no-op strategies (files persist on disk)
                if !strategy.is_noop() {
                    let session_dir = host_session_path(&self.context.run_dir, &worker.name);
                    let archive_key = format!("{}/{}/session", self.context.run_name, worker.name);

                    match strategy.archive(&archive_key, &session_dir).await {
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
                        pid: None,
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
        // Get claimable nodes (needed for awaiting workers)
        let claimable = self.get_claimable_nodes().await?;

        // Get workers that need to be resumed
        let workers = self.state.get_workers().await?;

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
    async fn maybe_scale_up_internal(&self) -> LifecycleResult<Option<LifecycleAction>> {
        use crate::core::git::create_worker_clone;
        use crate::core::workers::WorkerScale;

        // Check if autoscaling is enabled
        let scale_str = match self.state.get_worker_scale().await? {
            Some(s) => s,
            None => return Ok(None),
        };

        let scale = match WorkerScale::parse(&scale_str) {
            Some(s) => s,
            None => return Ok(None),
        };

        // Don't scale up if run is paused
        let status = self.state.status().await?;
        if status == Status::Paused {
            debug!("maybe_scale_up: run is paused, not scaling");
            return Ok(None);
        }

        // Get current workers and claimable nodes
        let workers = self.state.get_workers().await?;
        let current_count = workers.len();
        let claimable = self.get_claimable_nodes().await?;
        let claimable_count = claimable.len();

        debug!(
            "maybe_scale_up: {} claimable nodes, {} workers, max {}",
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
        let project_path_str = match self.state.get_project_path().await? {
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

        // Pick the first claimable node to assign to this worker
        let node = match claimable.first() {
            Some(n) => n,
            None => {
                warn!("maybe_scale_up: no claimable nodes available (race condition?)");
                return Ok(None);
            }
        };

        // Add worker to state (status will be set to Working by spawn_single_worker)
        // Use the default runner from the run config, not hardcoded "local"
        let location = self
            .state
            .get_default_runner()
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "local".to_string());
        self.state
            .add_worker(&new_name, worker_dir.to_str().unwrap_or("."), &location)
            .await?;

        // Claim node for new worker
        if let Err(e) = self.claim_node(&node.id, &new_name).await {
            warn!(
                "maybe_scale_up: failed to claim node {} for worker {}: {}",
                node.id, new_name, e
            );
            // Clean up the worker we just added
            let _ = self.state.delete_worker(&new_name).await;
            return Ok(None);
        }

        // Set assigned_task_id for new worker
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
                "maybe_scale_up: failed to set assigned_task_id for {}: {}",
                new_name, e
            );
        }

        // Create worker chat file
        let chat_file = self.files.chats_dir().join(format!("{}.md", new_name));
        if let Err(e) = std::fs::write(&chat_file, format!("# {} Chat\n\n", new_name)) {
            warn!("maybe_scale_up: failed to create worker chat: {}", e);
        }

        // Announce in group chat
        self.send_system_message(&format!(
            "New worker **{}** has joined and is assigned task **{}**.",
            new_name, node.id
        ))
        .await;

        info!(
            "maybe_scale_up: spawning worker {} with task {}",
            new_name, node.id
        );

        // Return the SpawnWorker action - daemon will handle actual spawning via orchestrator
        Ok(Some(LifecycleAction::SpawnWorker {
            worker_name: new_name,
            work_dir: worker_dir,
            assigned_task_id: Some(node.id.clone()),
        }))
    }

    /// Evaluate scaling needs and return actions for spawning/waking workers.
    ///
    /// This is the event-driven scaling evaluation that replaces the old polling-based
    /// approach. It's called when the scaling_check_requested flag is set.
    pub async fn evaluate_scaling(&self) -> LifecycleResult<Vec<LifecycleAction>> {
        use crate::core::git::create_worker_clone;
        use crate::core::workers::WorkerScale;

        // Don't scale if run is paused
        let status = self.state.status().await?;
        if status == Status::Paused {
            debug!("evaluate_scaling: run is paused, not scaling");
            return Ok(vec![]);
        }

        // Get scaling configuration
        let scale_str = match self.state.get_worker_scale().await? {
            Some(s) => s,
            None => return Ok(vec![]),
        };
        let scale = match WorkerScale::parse(&scale_str) {
            Some(s) => s,
            None => return Ok(vec![]),
        };
        let max_workers = scale.max as usize;

        // Get claimable nodes and workers
        let claimable = self.get_claimable_nodes().await?;

        let workers = self.state.get_workers().await?;

        let active_count = workers
            .iter()
            .filter(|w| w.status == WorkerStatus::Working)
            .count();

        let mut idle_workers: Vec<_> = workers
            .iter()
            .filter(|w| w.status == WorkerStatus::Awaiting && !w.hitl_waiting)
            .collect();

        let mut actions = vec![];

        // Scale down: if we have more workers than max_workers, delete excess idle workers
        // Only delete idle workers (Awaiting, not hitl_waiting, no assigned task)
        // Never kill working workers
        let total_workers = workers.len();
        if total_workers > max_workers {
            let excess = total_workers - max_workers;
            let mut deleted_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            // Find idle workers without assigned tasks that we can delete
            for worker in &idle_workers {
                if deleted_names.len() >= excess {
                    break;
                }
                // Only delete workers that have no assigned task
                if worker.assigned_task_id.is_none() {
                    // Delete the worker from the database
                    // Note: We don't need to kill the process since idle workers have no process
                    if let Err(e) = self.state.delete_worker(&worker.name).await {
                        warn!(
                            "evaluate_scaling: failed to delete excess worker {}: {}",
                            worker.name, e
                        );
                    } else {
                        info!(
                            "evaluate_scaling: deleted excess idle worker {} (scaling down to {})",
                            worker.name, max_workers
                        );
                        deleted_names.insert(worker.name.clone());
                    }
                }
            }

            // Filter out deleted workers from idle_workers list
            if !deleted_names.is_empty() {
                idle_workers.retain(|w| !deleted_names.contains(&w.name));
            }
        }

        // First: check for idle workers that ALREADY have an assigned task (status=working)
        // These need to be respawned immediately without claiming a new task
        for worker in &idle_workers {
            if let Some(ref assigned_task_id) = worker.assigned_task_id {
                // Worker has an assigned task - verify it's still in "working" status
                let all_nodes = self.get_all_nodes().await.unwrap_or_default();
                let task_still_doing = all_nodes
                    .iter()
                    .any(|n| n.id == *assigned_task_id && n.status == LiveNodeStatus::Working);

                if task_still_doing {
                    // Get work_dir from database, fallback to standard location
                    let work_dir = worker
                        .work_dir
                        .as_ref()
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| self.context.run_dir.join("work").join(&worker.name));

                    actions.push(LifecycleAction::ResumeWorker {
                        worker_name: worker.name.clone(),
                        work_dir,
                        resume_session_id: worker.session_id.clone(),
                        state_handle: worker
                            .state_handle
                            .as_ref()
                            .and_then(|json| serde_json::from_str::<WorkerStateHandle>(json).ok()),
                    });

                    info!(
                        "evaluate_scaling: respawning worker {} with existing assigned task {}",
                        worker.name, assigned_task_id
                    );
                }
            }
        }

        // If no claimable tasks, return any actions from existing assignments
        if claimable.is_empty() {
            debug!(
                "evaluate_scaling: no claimable tasks, {} actions from existing assignments",
                actions.len()
            );
            return Ok(actions);
        }

        // Calculate how many workers we need for claimable tasks
        let needed = claimable
            .len()
            .min(max_workers)
            .saturating_sub(active_count);

        // Track count of workers already handled (with existing assignments)
        let existing_actions_count = actions.len();

        // Filter out workers that already have actions (from existing assignments above)
        // Build a set of worker names that already have actions
        let workers_with_actions: std::collections::HashSet<String> = actions
            .iter()
            .filter_map(|a| match a {
                LifecycleAction::ResumeWorker { worker_name, .. } => Some(worker_name.clone()),
                _ => None,
            })
            .collect();

        let available_idle_workers: Vec<_> = idle_workers
            .iter()
            .filter(|w| !workers_with_actions.contains(&w.name))
            .collect();

        // Drop the HashSet so we can mutate actions again
        drop(workers_with_actions);

        if needed == 0 && available_idle_workers.is_empty() {
            debug!("evaluate_scaling: no additional workers needed");
            return Ok(actions);
        }

        let mut nodes_to_assign: Vec<_> = claimable.clone();
        let all_nodes = self.get_all_nodes().await?;

        // Second: wake available idle workers with NEW nodes from claimable
        let workers_to_wake = needed.min(available_idle_workers.len());
        for worker in available_idle_workers.iter().take(workers_to_wake) {
            if let Some(node) = self.pick_node_for_worker(&nodes_to_assign, worker, &all_nodes) {
                nodes_to_assign.retain(|n| n.id != node.id);

                // Assign node to worker in database
                if let Err(e) = self.claim_node(&node.id, &worker.name).await {
                    warn!(
                        "evaluate_scaling: failed to claim node {} for worker {}: {}",
                        node.id, worker.name, e
                    );
                    continue;
                }

                // Update worker's assigned_task_id
                if let Err(e) = self
                    .state
                    .update_worker(
                        &worker.name,
                        WorkerUpdate {
                            assigned_task_id: Some(Some(node.id.clone())),
                            ..Default::default()
                        },
                    )
                    .await
                {
                    warn!(
                        "evaluate_scaling: failed to update assigned_task_id for worker {}: {}",
                        worker.name, e
                    );
                }

                // Get work_dir from database, fallback to standard location
                let work_dir = worker
                    .work_dir
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.context.run_dir.join("work").join(&worker.name));

                actions.push(LifecycleAction::ResumeWorker {
                    worker_name: worker.name.clone(),
                    work_dir,
                    resume_session_id: worker.session_id.clone(),
                    state_handle: worker
                        .state_handle
                        .as_ref()
                        .and_then(|json| serde_json::from_str::<WorkerStateHandle>(json).ok()),
                });

                info!(
                    "evaluate_scaling: waking idle worker {} with node {}",
                    worker.name, node.id
                );
            }
        }

        // Then: spawn new workers for remaining nodes
        // Account for both:
        // - existing_actions_count: workers being resumed with their existing assignments
        // - spawned_count: idle workers woken with new assignments in the loop above
        let spawned_count = actions.len() - existing_actions_count;
        let total_resuming = existing_actions_count + spawned_count;
        let remaining = needed.saturating_sub(total_resuming);
        if remaining > 0 {
            let project_path_str = match self.state.get_project_path().await? {
                Some(p) => p,
                None => {
                    warn!("evaluate_scaling: no project path, cannot spawn new workers");
                    return Ok(actions);
                }
            };

            let project_path = PathBuf::from(&project_path_str);
            let staging_dir = self.context.run_dir.join("work").join("staging");
            let existing_names: Vec<String> = workers.iter().map(|w| w.name.clone()).collect();

            for _ in 0..remaining {
                if nodes_to_assign.is_empty() {
                    break;
                }

                // Get node to assign (just take first available for new workers)
                let node = nodes_to_assign.remove(0);

                // Generate new worker name
                let new_name = crate::core::names::get_available_name(&existing_names);

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
                        warn!(
                            "evaluate_scaling: failed to create worker clone for {}: {}",
                            new_name, e
                        );
                        continue;
                    }
                };

                // Add worker to state
                let location = self
                    .state
                    .get_default_runner()
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "local".to_string());

                if let Err(e) = self
                    .state
                    .add_worker(&new_name, worker_dir.to_str().unwrap_or("."), &location)
                    .await
                {
                    warn!("evaluate_scaling: failed to add worker {}: {}", new_name, e);
                    continue;
                }

                // Claim node for new worker
                if let Err(e) = self.claim_node(&node.id, &new_name).await {
                    warn!(
                        "evaluate_scaling: failed to claim node {} for new worker {}: {}",
                        node.id, new_name, e
                    );
                    continue;
                }

                // Set assigned_task_id for new worker
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

                // Create worker chat file
                let chat_file = self.files.chats_dir().join(format!("{}.md", new_name));
                if let Err(e) = std::fs::write(&chat_file, format!("# {} Chat\n\n", new_name)) {
                    warn!(
                        "evaluate_scaling: failed to create worker chat for {}: {}",
                        new_name, e
                    );
                }

                // Announce in group chat
                self.send_system_message(&format!(
                    "New worker **{}** has joined and is assigned task **{}**.",
                    new_name, node.id
                ))
                .await;

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
        let eval_log_path = self.context.run_dir.join("eval_spawn.log");
        let log_file = match std::fs::File::create(&eval_log_path) {
            Ok(f) => Some(f),
            Err(e) => {
                warn!("Failed to create eval log file {:?}: {}", eval_log_path, e);
                None
            }
        };

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
                    use crate::core::delta::NodeType;
                    match n.node_type {
                        NodeType::Task => {
                            n.status != LiveNodeStatus::Done
                                && n.status != LiveNodeStatus::Validated
                        }
                        NodeType::Eval => n.status != LiveNodeStatus::Done,
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
            LifecycleEvent::WorkerDone { worker_name } => {
                debug!("Processing WorkerDone event for {}", worker_name);
                actions.extend(self.worker_done(&worker_name).await?);
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
                    if self.maybe_trigger_eval().await? {
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
                let resume_actions = self.resume_awaiting_workers_internal().await?;
                actions.extend(resume_actions);

                if let Some(action) = self.maybe_scale_up_internal().await? {
                    actions.push(action);
                }
            }

            LifecycleEvent::TaskAdded { task_id: _ }
            | LifecycleEvent::TaskUnclaimed { task_id: _ } => {
                // New or unclaimed task - try to resume awaiting workers
                let resume_actions = self.resume_awaiting_workers_internal().await?;
                actions.extend(resume_actions);

                if let Some(action) = self.maybe_scale_up_internal().await? {
                    actions.push(action);
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

            LifecycleEvent::PauseRequested { reason } => {
                let paused = self.pause_run(&reason).await?;
                if !paused.is_empty() {
                    actions.push(LifecycleAction::WorkersPaused(paused));
                }
                actions.push(LifecycleAction::RunStatusChanged(Status::Paused));
            }

            LifecycleEvent::ResumeRequested => {
                let resume_actions = self.resume_run().await?;
                actions.extend(resume_actions);
                actions.push(LifecycleAction::RunStatusChanged(Status::Working));
            }

            LifecycleEvent::EvalCompleted { success, feedback } => {
                if success {
                    self.state.set_status(Status::Done).await?;
                    actions.push(LifecycleAction::RunCompleted);
                } else {
                    // Eval failed - check if we should retry or fail the run
                    debug!("Eval failed with feedback: {}", feedback);
                    // Resume workers to continue working
                    self.state.set_status(Status::Working).await?;
                    let resume_actions = self.resume_awaiting_workers_internal().await?;
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

    async fn worker_done(&self, worker_name: &str) -> LifecycleResult<Vec<LifecycleAction>> {
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
            .await?;

        // Check if eval should be triggered
        if self.maybe_trigger_eval().await? {
            actions.push(LifecycleAction::EvalTriggered);
        }

        Ok(actions)
    }

    async fn handle_time_expired(&self) -> LifecycleResult<()> {
        // Only handle if not already failed
        let status = self.state.status().await?;
        if status == Status::Failed {
            info!("Time already expired, skipping handler");
            return Ok(());
        }

        // Send final message
        let workers = self.state.get_workers().await?;
        let is_multi_worker = workers.len() > 1;

        let message = "Time limit reached. Run failed.";

        // Send to group chat or individual worker DM
        if is_multi_worker {
            self.send_system_message(message).await;
        } else if let Some(worker) = workers.first() {
            self.send_system_message_to_worker(&worker.name, message)
                .await;
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

        // Set run status to Failed with TimeLimit reason
        self.state.set_failed(FailureReason::TimeLimit).await?;
        info!("Run status set to Failed (time_limit)");

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

    async fn all_workers_inactive(&self) -> LifecycleResult<bool> {
        self.state
            .all_workers_inactive()
            .await
            .map_err(|e| LifecycleError::State(e.to_string()))
    }

    async fn should_trigger_eval(&self) -> LifecycleResult<bool> {
        // Check if all workers are inactive
        if !self.all_workers_inactive().await? {
            return Ok(false);
        }

        // Check if run is in Working status
        let status = self.run_status().await?;
        if status != Status::Working {
            return Ok(false);
        }

        // Check if eval script exists
        Ok(self.files.eval_spec().exists())
    }

    async fn can_scale_up(&self) -> LifecycleResult<bool> {
        use crate::core::workers::WorkerScale;

        // Check if autoscaling is enabled
        let scale_str = match self.state.get_worker_scale().await? {
            Some(s) => s,
            None => return Ok(false),
        };

        let scale = match WorkerScale::parse(&scale_str) {
            Some(s) => s,
            None => return Ok(false),
        };

        // Get current count
        let workers = self.state.get_workers().await?;

        // Check if we have claimable nodes
        let claimable = self.get_claimable_nodes().await?;

        Ok(!claimable.is_empty() && scale.can_scale_up(workers.len()))
    }

    async fn run_status(&self) -> LifecycleResult<Status> {
        self.state
            .status()
            .await
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
