//! Lifecycle polling loop for the daemon
//!
//! Handles:
//! - Triggering eval when all workers become inactive
//! - Scaling up workers when tasks are available (via orchestrator)
//! - Enforcing time limits
//! - Resuming workers (via orchestrator)
//! - Processing scribe batches for documentation updates
//! - Auto-exit when idle

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;

use crate::core::api_types::RunStatus;
use crate::core::config::{self, Config};
use crate::core::delta::{list_working_project_runs, DeltaRunner, DeltaState, LiveNodeStatus};
use crate::core::lifecycle::{
    LifecycleAction, LifecycleEvent, LifecycleManager, LocalLifecycleManager,
};
use crate::core::orchestrator::{create_local_orchestrator, Orchestrator};
use crate::core::scribe;
use crate::core::server::AppState;
use crate::core::service_worker::ScribeService;
use crate::core::state::{SQLiteState, Status};
use crate::core::Files;

use super::server::DaemonConfig;

/// Run the lifecycle polling loop
pub async fn run_polling_loop(state: Arc<AppState>, config: DaemonConfig) {
    let mut tick = interval(Duration::from_secs(5));
    let mut last_active = Instant::now();

    tracing::info!("[Daemon] Starting lifecycle polling loop");

    loop {
        tick.tick().await;

        // Get all runs
        let runs = match state.orchestrator.list_runs().await {
            Ok(runs) => runs,
            Err(e) => {
                tracing::warn!("[Daemon] Failed to list runs: {}", e);
                continue;
            }
        };

        let mut has_active_runs = false;

        for run in &runs {
            // Check if run is active
            let is_active = matches!(run.status, RunStatus::Working | RunStatus::Eval);

            if is_active {
                has_active_runs = true;
                last_active = Instant::now();

                // Process active run using LifecycleManager
                if let Err(e) = process_active_run(&run.name).await {
                    tracing::warn!("[Daemon] Error processing run '{}': {}", run.name, e);
                }
            }
        }

        // Process working project runs (delta dispatch system)
        if let Ok(project_runs) = list_working_project_runs() {
            for (project_id, run_name) in project_runs {
                has_active_runs = true;
                last_active = Instant::now();

                // Process delta submissions for this project run
                let runner = DeltaRunner::new(project_id);
                match runner.process_pending(&state.orchestrator).await {
                    Ok(processed) => {
                        if processed > 0 {
                            tracing::info!(
                                "[Daemon] Processed {} delta submissions for project run '{}'",
                                processed,
                                run_name
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[Daemon] Error processing project run '{}': {}",
                            run_name,
                            e
                        );
                    }
                }

                // Sync task statuses back to live nodes
                if let Err(e) =
                    sync_task_statuses_to_live_nodes(project_id, &run_name, &state.orchestrator)
                        .await
                {
                    tracing::debug!("[Daemon] Task status sync for '{}': {}", run_name, e);
                }
            }
        }

        // Check for idle timeout
        if config.idle_timeout_secs > 0 && !has_active_runs {
            let idle_duration = last_active.elapsed();
            if idle_duration.as_secs() >= config.idle_timeout_secs {
                tracing::info!(
                    "[Daemon] Idle for {} seconds, exiting",
                    idle_duration.as_secs()
                );
                // Clean up and exit
                super::server::cleanup_pid_file(&super::pid_path());
                std::process::exit(0);
            }
        }
    }
}

/// Process an active run using LifecycleManager
///
/// This is the core lifecycle loop. The daemon polls every 5 seconds and:
/// 1. Creates a LocalLifecycleManager for the run
/// 2. Processes TimeCheck event which returns lifecycle actions
/// 3. Handles actions like SpawnWorker via the orchestrator
async fn process_active_run(run_name: &str) -> anyhow::Result<()> {
    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let db_path = files.db_path();

    if !db_path.exists() {
        return Ok(());
    }

    // Get agent command from config
    let agent_command = crate::cli::config::get_agent_command();

    // Create lifecycle manager
    let lifecycle =
        match LocalLifecycleManager::new(run_name, run_dir.clone(), agent_command.clone()) {
            Ok(lm) => lm,
            Err(e) => {
                tracing::warn!(
                    "[Daemon] Failed to create lifecycle manager for '{}': {}",
                    run_name,
                    e
                );
                return Ok(());
            }
        };

    // Check run status
    let status = lifecycle.run_status().unwrap_or(Status::Draft);

    match status {
        Status::Working => {
            // Process TimeCheck event - handles time limit, eval triggering, and scaling
            match lifecycle.process_event(LifecycleEvent::TimeCheck) {
                Ok(actions) => {
                    for action in &actions {
                        tracing::debug!("[Daemon] Run '{}' action: {:?}", run_name, action);
                    }

                    // Handle actions that require spawning via orchestrator
                    handle_lifecycle_actions(run_name, &run_dir, actions).await;
                }
                Err(e) => {
                    tracing::warn!(
                        "[Daemon] Failed to process TimeCheck for '{}': {}",
                        run_name,
                        e
                    );
                }
            }

            // Process scribe batches for documentation updates
            if let Err(e) = maybe_process_scribe(run_name, &files) {
                tracing::debug!("[Daemon] Scribe processing for '{}': {}", run_name, e);
            }
        }
        Status::Eval => {
            // Check time limit even during eval
            if lifecycle.state().is_time_expired()? {
                if let Err(e) = lifecycle.handle_time_expired() {
                    tracing::warn!(
                        "[Daemon] Failed to handle time expired for '{}': {}",
                        run_name,
                        e
                    );
                }
            }

            // Check if eval process crashed (PID no longer alive)
            if let Ok(Some(eval)) = lifecycle.state().get_running_eval() {
                if let Some(pid) = eval.pid {
                    if !crate::core::runner::local::LocalRunner::is_pid_alive(pid) {
                        tracing::warn!(
                            "[Daemon] Eval process (pid={}) for run '{}' is no longer alive - marking as failed",
                            pid, run_name
                        );
                        if let Err(e) = lifecycle.state().complete_eval(
                            eval.id,
                            false,
                            "Eval process crashed or exited unexpectedly",
                        ) {
                            tracing::error!(
                                "[Daemon] Failed to mark crashed eval as failed for '{}': {}",
                                run_name,
                                e
                            );
                        }

                        // Re-trigger eval
                        let agent_command = crate::cli::config::get_agent_command();
                        if let Ok(lm) =
                            LocalLifecycleManager::new(run_name, run_dir.clone(), agent_command)
                        {
                            // Set back to Working so maybe_trigger_eval can fire
                            if let Err(e) = lm.state().set_status(Status::Working) {
                                tracing::error!(
                                    "[Daemon] Failed to reset status for eval re-trigger on '{}': {}",
                                    run_name, e
                                );
                            }
                            match lm.process_event(LifecycleEvent::TimeCheck) {
                                Ok(actions) => {
                                    handle_lifecycle_actions(run_name, &run_dir, actions).await;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "[Daemon] Failed to re-trigger eval for '{}': {}",
                                        run_name,
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    Ok(())
}

/// Check and process scribe batches if the batch window has expired.
///
/// Uses ScribeService which handles local vs remote execution internally.
fn maybe_process_scribe(run_name: &str, files: &Files) -> anyhow::Result<()> {
    let config = Config::load().map(|(c, _)| c).unwrap_or_else(|e| {
        tracing::warn!(
            "[Daemon] Failed to load config for scribe, using defaults: {}",
            e
        );
        Config::default()
    });

    if !config.scribe_enabled {
        return Ok(());
    }

    // Check if we should process
    let should_process = {
        let state = SQLiteState::new(files.db_path())?;
        scribe::should_process_batch(&state, &config)
    };

    if should_process {
        let run_name = run_name.to_string();
        let scribe_service = ScribeService::with_config(config);

        // Spawn async task to process the batch
        tokio::spawn(async move {
            match scribe_service.process_batch(&run_name).await {
                Ok(result) => {
                    tracing::info!(
                        "[Daemon] Scribe batch processed for '{}': {} submissions",
                        run_name,
                        result.submissions_processed
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[Daemon] Scribe processing failed for '{}': {}",
                        run_name,
                        e
                    );
                }
            }
        });
    }

    Ok(())
}

/// Handle lifecycle actions that require spawning workers via the orchestrator.
///
/// This ensures workers are spawned using the correct runner (local/docker/fly/ssh)
/// based on the run's configuration.
async fn handle_lifecycle_actions(
    run_name: &str,
    run_dir: &std::path::Path,
    actions: Vec<LifecycleAction>,
) {
    // Create orchestrator for spawning
    let orchestrator = match create_local_orchestrator() {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(
                "[Daemon] Failed to create orchestrator for '{}': {}",
                run_name,
                e
            );
            return;
        }
    };

    for action in actions {
        match action {
            LifecycleAction::SpawnWorker {
                worker_name,
                work_dir,
            } => {
                tracing::info!(
                    "[Daemon] Spawning worker '{}' for run '{}' via orchestrator",
                    worker_name,
                    run_name
                );

                match orchestrator
                    .spawn_single_worker(run_name, &worker_name, &work_dir, None)
                    .await
                {
                    Ok(()) => {
                        tracing::info!(
                            "[Daemon] Successfully spawned worker '{}' for run '{}'",
                            worker_name,
                            run_name
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[Daemon] Failed to spawn worker '{}' for run '{}': {}",
                            worker_name,
                            run_name,
                            e
                        );
                    }
                }
            }

            LifecycleAction::ResumeWorker {
                worker_name,
                work_dir,
                resume_session_id,
                state_handle,
            } => {
                tracing::info!(
                    "[Daemon] Resuming worker '{}' for run '{}' via orchestrator (has_state: {})",
                    worker_name,
                    run_name,
                    state_handle
                        .as_ref()
                        .map(|h| h.has_state())
                        .unwrap_or(false)
                );

                // Use resume_worker which handles:
                // 1. Check if already running (skip if yes)
                // 2. Restore work snapshot if runner is ephemeral
                // 3. Restore agent session if handle exists
                // 4. Spawn worker via runner
                match orchestrator
                    .resume_worker(
                        run_name,
                        &worker_name,
                        &work_dir,
                        resume_session_id.as_deref(),
                        state_handle.as_ref(),
                    )
                    .await
                {
                    Ok(()) => {
                        tracing::info!(
                            "[Daemon] Successfully resumed worker '{}' for run '{}'",
                            worker_name,
                            run_name
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[Daemon] Failed to resume worker '{}' for run '{}': {}",
                            worker_name,
                            run_name,
                            e
                        );
                    }
                }
            }

            LifecycleAction::WorkersResumed(workers) => {
                // Workers to resume - spawn each one via orchestrator
                let state = match SQLiteState::new(run_dir.join("hirsel.db")) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("[Daemon] Failed to open state for '{}': {}", run_name, e);
                        continue;
                    }
                };

                for worker_name in workers {
                    // Get worker info for work_dir and session_id
                    let worker = match state.get_worker(&worker_name) {
                        Ok(Some(w)) => w,
                        Ok(None) => {
                            tracing::warn!(
                                "[Daemon] Worker '{}' not found for resume",
                                worker_name
                            );
                            continue;
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[Daemon] Failed to get worker '{}' info: {}",
                                worker_name,
                                e
                            );
                            continue;
                        }
                    };

                    // Get work_dir from database, fallback to standard location
                    // Note: work_dir should never be empty, but if it is, use default
                    let work_dir = worker
                        .work_dir
                        .as_ref()
                        .filter(|s| !s.is_empty())
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| run_dir.join("work").join(&worker_name));

                    match orchestrator
                        .spawn_single_worker(
                            run_name,
                            &worker_name,
                            &work_dir,
                            worker.session_id.as_deref(),
                        )
                        .await
                    {
                        Ok(()) => {
                            tracing::info!(
                                "[Daemon] Successfully resumed worker '{}' for run '{}'",
                                worker_name,
                                run_name
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[Daemon] Failed to resume worker '{}' for run '{}': {}",
                                worker_name,
                                run_name,
                                e
                            );
                        }
                    }
                }
            }

            // Other actions are handled directly by the lifecycle manager
            LifecycleAction::EvalTriggered => {
                tracing::info!("[Daemon] Eval triggered for run '{}'", run_name);
            }
            LifecycleAction::RunFailed { reason } => {
                tracing::info!("[Daemon] Run '{}' failed: {:?}", run_name, reason);
            }
            LifecycleAction::RunCompleted => {
                tracing::info!("[Daemon] Run '{}' completed", run_name);
            }
            _ => {}
        }
    }
}

/// Sync task completion statuses from run to live nodes
///
/// Polls the run's task statuses and updates corresponding live nodes
/// when tasks complete (done/failed). This is the callback mechanism
/// from the run system back to the delta dispatch system.
async fn sync_task_statuses_to_live_nodes(
    project_id: i64,
    run_name: &str,
    orchestrator: &dyn Orchestrator,
) -> anyhow::Result<()> {
    // Get tasks from the run
    let tasks = match orchestrator.list_tasks(run_name).await {
        Ok(tasks) => tasks,
        Err(crate::core::orchestrator::OrchestratorError::RunNotFound(_)) => {
            // Run doesn't exist yet (first dispatch hasn't completed)
            return Ok(());
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to list tasks: {}", e));
        }
    };

    let state = DeltaState::new(project_id);

    // Update live nodes based on task status
    for task in tasks {
        // Skip tasks without a board_task_id (not from delta dispatch)
        let node_id = match task.board_task_id {
            Some(ref id) => id,
            None => continue,
        };

        // Get current live node status
        let current_status = match state.get_live_node(node_id) {
            Ok(node) => node.status,
            Err(_) => continue, // Node doesn't exist
        };

        // Determine expected status from task
        use crate::core::api_types::TaskStatus;
        let expected_status = match task.status {
            TaskStatus::Done | TaskStatus::Validated => LiveNodeStatus::Done,
            TaskStatus::NeedsRepair => LiveNodeStatus::Failed,
            TaskStatus::Doing => LiveNodeStatus::Working,
            _ => LiveNodeStatus::Pending,
        };

        // Update if changed (and not going backwards)
        if current_status != expected_status {
            // Don't downgrade from done/failed back to working/pending
            let should_update = match (current_status, expected_status) {
                (LiveNodeStatus::Done, _) | (LiveNodeStatus::Failed, _) => false,
                _ => true,
            };

            if should_update {
                if let Err(e) = state.update_live_node_status(node_id, expected_status, None) {
                    tracing::warn!(
                        "[Daemon] Failed to update live node {} status: {}",
                        node_id,
                        e
                    );
                } else {
                    tracing::debug!(
                        "[Daemon] Updated live node {} status to {:?}",
                        node_id,
                        expected_status
                    );
                }

                // If task completed, also update the delta submission
                if matches!(
                    expected_status,
                    LiveNodeStatus::Done | LiveNodeStatus::Failed
                ) {
                    let runner = DeltaRunner::new(project_id);
                    // Find the submission by live_node_id
                    if let Ok(db) = rusqlite::Connection::open(config::global_db_path()) {
                        if let Ok(submission_id) = db.query_row::<i64, _, _>(
                            "SELECT id FROM delta_submissions WHERE live_node_id = ?1 AND status = 'processing'",
                            [node_id],
                            |row| row.get(0),
                        ) {
                            let success = expected_status == LiveNodeStatus::Done;
                            if let Err(e) = runner.complete_submission(submission_id, success, None) {
                                tracing::warn!(
                                    "[Daemon] Failed to complete submission {}: {}",
                                    submission_id,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
