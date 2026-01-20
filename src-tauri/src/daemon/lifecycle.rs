//! Lifecycle polling loop for the daemon
//!
//! Handles:
//! - Triggering eval when all workers become inactive
//! - Scaling up workers when tasks are available
//! - Enforcing time limits
//! - Triggering learnings compaction
//! - Auto-exit when idle

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;

use crate::core::api_types::RunStatus;
use crate::core::config;
use crate::core::lifecycle::{LifecycleEvent, LifecycleManager, LocalLifecycleManager};
use crate::core::orchestrator::Orchestrator;
use crate::core::server::AppState;
use crate::core::state::Status;
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
                if let Err(e) = process_active_run(&run.name) {
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
                super::server::cleanup_socket(&super::socket_path(), &super::pid_path());
                std::process::exit(0);
            }
        }
    }
}

/// Process an active run using LifecycleManager
fn process_active_run(run_name: &str) -> anyhow::Result<()> {
    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let db_path = files.db_path();

    if !db_path.exists() {
        return Ok(());
    }

    // Get agent command from config
    let agent_command = crate::cli::config::get_agent_command();

    // Create lifecycle manager
    let lifecycle = match LocalLifecycleManager::new(run_name, run_dir, agent_command) {
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
                    for action in actions {
                        tracing::debug!("[Daemon] Run '{}' action: {:?}", run_name, action);
                    }
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
