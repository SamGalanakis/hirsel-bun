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
use crate::core::delta::{list_working_project_runs, BoardDeliveryStatus, Delivery, DeltaState};
use crate::core::lifecycle::{
    LifecycleAction, LifecycleEvent, LifecycleManager, LocalLifecycleManager,
};
use crate::core::orchestrator::{create_local_orchestrator, Orchestrator};
use crate::core::scribe;
use crate::core::server::AppState;
use crate::core::service_worker::{ConflictResolverServiceWrapper, ScribeService};
use crate::core::state::{SQLiteState, Status};
use crate::core::Files;

use super::server::DaemonConfig;

/// Run the lifecycle polling loop
#[tracing::instrument(skip_all)]
pub async fn run_polling_loop(state: Arc<AppState>, config: DaemonConfig) {
    let mut tick = interval(Duration::from_secs(2));
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

        // Process working project runs (track active state)
        if let Ok(project_runs) = list_working_project_runs().await {
            for (_project_id, _route_id, _run_name) in project_runs {
                has_active_runs = true;
                last_active = Instant::now();

                // Board runs are processed by the first loop (list_runs → process_active_run)
                // once dispatch_board bootstraps their per-run DB via start_run
            }
        }

        // Process deliveries that need conflict resolution
        if let Err(e) = process_resolving_deliveries().await {
            tracing::debug!("[Daemon] Conflict resolution processing: {}", e);
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
#[tracing::instrument]
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
        match LocalLifecycleManager::new(run_name, run_dir.clone(), agent_command.clone()).await {
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
    let status = match lifecycle.run_status().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                "[Daemon] Failed to get run status for '{}': {} - assuming Draft",
                run_name,
                e
            );
            Status::Draft
        }
    };

    match status {
        Status::Working => {
            use crate::core::delta::DeltaState;
            use crate::core::workers::WorkerScale;

            let workers = lifecycle.state().get_workers().await.unwrap_or_default();

            // Check for crashed workers (PID dead but status=Working)
            // This catches workers killed by OOM, SIGKILL, or other unexpected exits
            for worker in &workers {
                if worker.status == crate::core::state::WorkerStatus::Working {
                    if let Some(pid) = worker.pid {
                        if !crate::core::runner::local::LocalRunner::is_pid_alive(pid as u32) {
                            tracing::warn!(
                                "[Daemon] Worker '{}' crashed (pid {} dead), marking as Error",
                                worker.name,
                                pid
                            );
                            let _ = lifecycle
                                .state()
                                .update_worker(
                                    &worker.name,
                                    crate::core::state::WorkerUpdate {
                                        pid: Some(None),
                                        status: Some(crate::core::state::WorkerStatus::Error),
                                        assigned_task_id: Some(None), // Clear assigned task
                                        ..Default::default()
                                    },
                                )
                                .await;
                            // Request scaling check so the daemon can respawn or reassign
                            let _ = lifecycle.state().request_scaling_check().await;
                        }
                    }
                }
            }

            // Awaiting should imply "no process". If an Awaiting worker still has a live PID,
            // kill it and clear PID so scaling can resume deterministically.
            for worker in &workers {
                if worker.status == crate::core::state::WorkerStatus::Awaiting
                    && !worker.hitl_waiting
                {
                    if let Some(pid) = worker.pid {
                        if crate::core::runner::local::LocalRunner::is_pid_alive(pid as u32) {
                            tracing::warn!(
                                "[Daemon] Worker '{}' is Awaiting but pid {} is still alive; sending SIGTERM and clearing pid",
                                worker.name,
                                pid
                            );
                            #[cfg(unix)]
                            unsafe {
                                libc::kill(pid as i32, libc::SIGTERM);
                            }

                            let _ = lifecycle
                                .state()
                                .update_worker(
                                    &worker.name,
                                    crate::core::state::WorkerUpdate {
                                        pid: Some(None),
                                        ..Default::default()
                                    },
                                )
                                .await;
                            let _ = lifecycle.state().request_scaling_check().await;
                        }
                    }
                }
            }

            // Check if event-driven scaling was requested
            let scaling_requested = lifecycle
                .state()
                .consume_scaling_check()
                .await
                .unwrap_or(false);

            // Heuristic scaling: if there are claimable tasks but no active workers (or idle workers exist),
            // run scaling evaluation even if a scaling_check wasn't explicitly requested.
            let heuristic_requested = if !scaling_requested {
                let active_count = workers
                    .iter()
                    .filter(|w| w.status == crate::core::state::WorkerStatus::Working)
                    .count();
                let idle_count = workers
                    .iter()
                    .filter(|w| {
                        w.status == crate::core::state::WorkerStatus::Awaiting && !w.hitl_waiting
                    })
                    .count();

                let desired_max = lifecycle
                    .state()
                    .get_worker_scale()
                    .await
                    .ok()
                    .flatten()
                    .and_then(|s| WorkerScale::parse(&s))
                    .map(|s| s.max)
                    .unwrap_or_else(|| workers.len().max(1));

                if active_count == 0 || idle_count > 0 || workers.len() < desired_max {
                    match (
                        lifecycle.state().get_project_id().await.ok().flatten(),
                        lifecycle.state().get_route_id().await.ok(),
                    ) {
                        (Some(project_id), Some(route_id)) => {
                            let claimable = DeltaState::with_route(project_id, route_id)
                                .get_claimable_nodes()
                                .await
                                .map(|v| v.len())
                                .unwrap_or(0);
                            claimable > 0 && active_count < desired_max
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if scaling_requested || heuristic_requested {
                // Event-driven / heuristic scaling evaluation
                match lifecycle.evaluate_scaling().await {
                    Ok(actions) => {
                        if !actions.is_empty() {
                            tracing::info!(
                                "[Daemon] Scaling evaluation for '{}': {} actions",
                                run_name,
                                actions.len()
                            );
                        }
                        handle_lifecycle_actions(run_name, &run_dir, actions).await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[Daemon] Failed to evaluate scaling for '{}': {}",
                            run_name,
                            e
                        );
                    }
                }
            }

            // Process TimeCheck event - handles time limit, eval triggering
            match lifecycle.process_event(LifecycleEvent::TimeCheck).await {
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
            if let Err(e) = maybe_process_scribe(run_name, &files).await {
                tracing::debug!("[Daemon] Scribe processing for '{}': {}", run_name, e);
            }
        }
        Status::Eval => {
            // Check time limit even during eval
            if lifecycle.state().is_time_expired().await? {
                if let Err(e) = lifecycle.handle_time_expired().await {
                    tracing::warn!(
                        "[Daemon] Failed to handle time expired for '{}': {}",
                        run_name,
                        e
                    );
                }
            }

            // Check if eval process crashed (PID no longer alive)
            if let Ok(Some(eval)) = lifecycle.state().get_running_eval().await {
                if let Some(pid) = eval.pid {
                    if !crate::core::runner::local::LocalRunner::is_pid_alive(pid) {
                        tracing::warn!(
                            "[Daemon] Eval process (pid={}) for run '{}' is no longer alive - marking as failed",
                            pid, run_name
                        );
                        if let Err(e) = lifecycle
                            .state()
                            .complete_eval(
                                eval.id,
                                false,
                                "Eval process crashed or exited unexpectedly",
                            )
                            .await
                        {
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
                                .await
                        {
                            // Set back to Working so maybe_trigger_eval can fire
                            if let Err(e) = lm.state().set_status(Status::Working).await {
                                tracing::error!(
                                    "[Daemon] Failed to reset status for eval re-trigger on '{}': {}",
                                    run_name, e
                                );
                            }
                            match lm.process_event(LifecycleEvent::TimeCheck).await {
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
#[tracing::instrument(skip(_files))]
async fn maybe_process_scribe(run_name: &str, _files: &Files) -> anyhow::Result<()> {
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
        let state = SQLiteState::new(run_name).await?;
        scribe::should_process_batch(&state, &config).await
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
#[tracing::instrument(skip(actions))]
async fn handle_lifecycle_actions(
    run_name: &str,
    run_dir: &std::path::Path,
    actions: Vec<LifecycleAction>,
) {
    if actions.is_empty() {
        return;
    }

    // Bump runs generation so frontend detects state changes from lifecycle actions
    use crate::core::delta::bump_generation;
    bump_generation("runs_gen").await.ok();

    // Create orchestrator for spawning
    let orchestrator = match create_local_orchestrator() {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(
                "[Daemon] Failed to create orchestrator for '{}': {} - dropping {} lifecycle actions",
                run_name,
                e,
                actions.len()
            );
            return;
        }
    };

    for action in actions {
        match action {
            LifecycleAction::SpawnWorker {
                worker_name,
                work_dir,
                assigned_task_id,
            } => {
                tracing::info!(
                    "[Daemon] Spawning worker '{}' for run '{}' via orchestrator (task: {:?})",
                    worker_name,
                    run_name,
                    assigned_task_id
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
                let state = match SQLiteState::new(run_name).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("[Daemon] Failed to open state for '{}': {}", run_name, e);
                        continue;
                    }
                };

                for worker_name in workers {
                    // Get worker info for work_dir and session_id
                    let worker = match state.get_worker(&worker_name).await {
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

/// Process deliveries that are in the "resolving_conflicts" state
///
/// This function is called from the polling loop to check for deliveries
/// that need AI-assisted conflict resolution.
#[tracing::instrument]
async fn process_resolving_deliveries() -> anyhow::Result<()> {
    // Get all deliveries in resolving_conflicts status
    let deliveries = DeltaState::list_resolving_deliveries().await?;

    if deliveries.is_empty() {
        return Ok(());
    }

    tracing::info!(
        "[Daemon] Found {} deliveries needing conflict resolution",
        deliveries.len()
    );

    // Load config for the service wrapper
    let config = Config::load().map(|(c, _)| c).unwrap_or_default();

    for delivery in deliveries {
        if let Err(e) = process_single_delivery(&delivery, &config).await {
            tracing::warn!(
                "[Daemon] Failed to process delivery {} for project {}: {}",
                delivery.id,
                delivery.project_id,
                e
            );
        }
    }

    Ok(())
}

/// Process a single delivery that needs conflict resolution
#[tracing::instrument(skip(delivery, config), fields(delivery_id = delivery.id, project_id = delivery.project_id))]
async fn process_single_delivery(delivery: &Delivery, config: &Config) -> anyhow::Result<()> {
    let state = DeltaState::with_route(delivery.project_id, delivery.route_id);

    // Get the project run to find the work directory
    let project_run = state
        .get_project_run()
        .await?
        .ok_or_else(|| anyhow::anyhow!("No project run found"))?;

    let run_path = config::run_dir(&project_run.run_name);
    let work_dir = run_path.join("work").join("staging");

    if !work_dir.exists() {
        return Err(anyhow::anyhow!(
            "Work directory not found: {}",
            work_dir.display()
        ));
    }

    tracing::info!(
        "[Daemon] Starting conflict resolution for delivery {} in {}",
        delivery.id,
        work_dir.display()
    );

    // Get the conflicting files from the working tree
    let git_ops = crate::core::delivery::GitOperations::new(&work_dir);
    let conflicts = git_ops.get_working_tree_conflicts()?;

    if conflicts.is_empty() {
        // No conflicts - this shouldn't happen but handle gracefully
        tracing::warn!(
            "[Daemon] Delivery {} marked as resolving but no conflicts found",
            delivery.id
        );
        state
            .update_delivery_status(delivery.id, BoardDeliveryStatus::InProgress)
            .await?;
        return Ok(());
    }

    // Build context for the resolver
    let context = format!(
        "Delivery {} of board version {} to branch '{}'",
        delivery.id, delivery.version_id, delivery.target_branch
    );

    // Use the conflict resolver service wrapper which handles local vs remote execution
    let resolver_service = ConflictResolverServiceWrapper::with_config(config.clone());
    let result = resolver_service
        .resolve_conflicts(&work_dir, conflicts.clone(), &context)
        .await;

    match result {
        Ok(resolution_result) if resolution_result.success => {
            tracing::info!(
                "[Daemon] Conflict resolution succeeded for delivery {}: {} files resolved",
                delivery.id,
                resolution_result.files_resolved
            );

            // Verify no conflict markers remain
            if let Err(e) = git_ops.verify_no_conflict_markers() {
                tracing::error!(
                    "[Daemon] Conflict markers still present after resolution: {}",
                    e
                );
                state
                    .fail_delivery(delivery.id, "Conflict markers remain after AI resolution")
                    .await?;
                return Ok(());
            }

            // Complete the merge
            match git_ops.complete_merge(&format!(
                "Merge {} (conflicts resolved by AI)",
                delivery.target_branch
            )) {
                Ok(sha) => {
                    tracing::info!(
                        "[Daemon] Merge completed for delivery {}: {}",
                        delivery.id,
                        sha
                    );
                    state
                        .update_delivery_status(delivery.id, BoardDeliveryStatus::InProgress)
                        .await?;
                }
                Err(e) => {
                    tracing::error!(
                        "[Daemon] Failed to complete merge for delivery {}: {}",
                        delivery.id,
                        e
                    );
                    state
                        .fail_delivery(delivery.id, &format!("Merge failed: {}", e))
                        .await?;
                }
            }
        }
        Ok(_) => {
            // Resolution reported failure
            tracing::warn!(
                "[Daemon] Conflict resolution failed for delivery {}",
                delivery.id
            );
            state
                .fail_delivery(delivery.id, "AI conflict resolution failed")
                .await?;

            // Abort the merge
            let _ = git_ops.abort_merge();
        }
        Err(e) => {
            tracing::error!(
                "[Daemon] Conflict resolver error for delivery {}: {}",
                delivery.id,
                e
            );
            state
                .fail_delivery(delivery.id, &format!("Resolver error: {}", e))
                .await?;

            // Abort the merge
            let _ = git_ops.abort_merge();
        }
    }

    Ok(())
}
