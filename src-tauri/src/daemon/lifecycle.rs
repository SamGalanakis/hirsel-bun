//! Lifecycle polling loop for the daemon
//!
//! Handles:
//! - Triggering eval when all workers become inactive
//! - Scaling up workers when tasks are available (via orchestrator)
//! - Enforcing time limits
//! - Resuming workers (via orchestrator)
//! - Auto-exit when idle

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;

use crate::core::api_types::RunStatus;
use crate::core::config;
use crate::core::lifecycle::{
    LifecycleAction, LifecycleEvent, LifecycleManager, LocalLifecycleManager,
};
use crate::core::orchestrator::{create_local_orchestrator, Orchestrator};
use crate::core::server::AppState;
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
    let lifecycle = match LocalLifecycleManager::new(run_name, run_dir.clone(), agent_command) {
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
        }
        _ => {}
    }

    Ok(())
}

/// Handle lifecycle actions that require spawning workers via the orchestrator.
///
/// This ensures workers are spawned using the correct runner (local/docker/fly/sprite)
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
