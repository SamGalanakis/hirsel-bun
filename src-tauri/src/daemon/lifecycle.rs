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
use crate::core::orchestrator::Orchestrator;
use crate::core::server::AppState;
use crate::core::state::{SQLiteState, Status};
use crate::core::workers;
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

                // Process active run (sync operations only)
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

/// Process an active run (sync operations only)
fn process_active_run(run_name: &str) -> anyhow::Result<()> {
    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let db_path = files.db_path();

    if !db_path.exists() {
        return Ok(());
    }

    let sqlite_state = SQLiteState::new(db_path)?;

    // Check run status
    let status = sqlite_state.status()?;

    match status {
        Status::Working => {
            // Check time limit
            check_time_limit(run_name, &sqlite_state)?;

            // Check if eval should be triggered
            check_eval_trigger(run_name, &run_dir, &sqlite_state)?;

            // Check if we should scale up
            check_scale_up(run_name, &run_dir)?;

            // Note: compaction is handled via subprocess spawning in workers.rs
            // and doesn't need to be triggered here
        }
        Status::Eval => {
            // Check time limit even during eval
            check_time_limit(run_name, &sqlite_state)?;
        }
        _ => {}
    }

    Ok(())
}

/// Check and enforce time limit
fn check_time_limit(run_name: &str, state: &SQLiteState) -> anyhow::Result<()> {
    let time_limit_minutes = state.get_time_limit_minutes()?;

    if let Some(limit) = time_limit_minutes {
        if let Some(started_at_str) = state.get_started_at()? {
            // Parse the ISO timestamp
            if let Ok(started_at) = chrono::DateTime::parse_from_rfc3339(&started_at_str) {
                let elapsed_minutes =
                    (chrono::Utc::now() - started_at.with_timezone(&chrono::Utc)).num_minutes();

                if elapsed_minutes >= limit {
                    tracing::info!(
                        "[Daemon] Run '{}' exceeded time limit ({} >= {} minutes), pausing",
                        run_name,
                        elapsed_minutes,
                        limit
                    );

                    // Pause the run
                    workers::pause_all_workers(state)?;
                    state.set_status(Status::Paused)?;
                }
            }
        }
    }

    Ok(())
}

/// Check if eval should be triggered
fn check_eval_trigger(
    run_name: &str,
    run_dir: &std::path::Path,
    state: &SQLiteState,
) -> anyhow::Result<()> {
    // Check if all workers are inactive
    let workers = state.get_workers()?;
    let all_inactive = workers
        .iter()
        .all(|w| !matches!(w.status.as_str(), "Working" | "working"));

    if all_inactive && !workers.is_empty() {
        tracing::debug!(
            "[Daemon] All workers inactive for run '{}', checking eval",
            run_name
        );

        // Use existing maybe_trigger_eval logic
        match workers::maybe_trigger_eval(run_name, run_dir) {
            Ok(triggered) => {
                if triggered {
                    tracing::info!("[Daemon] Triggered eval for run '{}'", run_name);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "[Daemon] Failed to check/trigger eval for '{}': {}",
                    run_name,
                    e
                );
            }
        }
    }

    Ok(())
}

/// Check if workers should scale up
fn check_scale_up(run_name: &str, run_dir: &std::path::Path) -> anyhow::Result<()> {
    // Get agent command from config
    let agent_command = crate::cli::config::get_agent_command();

    // Try to scale up
    match workers::maybe_scale_up(run_name, run_dir, &agent_command) {
        Ok(Some(new_worker)) => {
            tracing::info!(
                "[Daemon] Scaled up run '{}' with new worker '{}'",
                run_name,
                new_worker
            );
        }
        Ok(None) => {
            // No scaling needed
        }
        Err(e) => {
            tracing::debug!("[Daemon] Scale up check for '{}': {}", run_name, e);
        }
    }

    Ok(())
}
