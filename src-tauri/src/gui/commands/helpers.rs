//! Helper functions shared across GUI commands

#[allow(unused_imports)]
use chrono::Utc;

// Re-export shared helper functions from core::api_types
pub use crate::core::api_types::{
    calculate_duration_minutes, convert_status, is_completed_status, parse_elapsed_minutes,
    parse_timestamp,
};
#[allow(unused_imports)]
use crate::core::{config, state::SQLiteState, state::Status};

/// Internal cooldown for compaction checks (10 seconds)
#[allow(dead_code)]
const COMPACTION_INTERNAL_COOLDOWN_SECONDS: i64 = 10;

/// Trigger automatic learnings compaction if needed.
/// This is called from get_run_detail polling. It spawns a subprocess
/// to handle compaction (like improve does) to avoid Send issues with ACP.
///
/// DEPRECATED: Compaction is now handled by the daemon lifecycle polling loop.
/// This function is kept for backwards compatibility but should not be called.
#[allow(dead_code)]
pub fn trigger_compaction_if_needed(run_name: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let run_dir = config::run_dir(run_name);
    let files = crate::core::Files::new(&run_dir);
    let state =
        SQLiteState::new(files.db_path()).map_err(|e| format!("Failed to open database: {}", e))?;

    let (global_config, _) =
        config::Config::load().unwrap_or_else(|_| (config::Config::default(), vec![]));

    // Quick checks before spawning subprocess
    if !global_config.compaction_enabled {
        return Ok(());
    }

    // Internal cooldown check (10 seconds) - just to prevent rapid-fire triggers
    if let Ok(Some(last_compaction)) = state.get_last_compaction_at() {
        if let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(&last_compaction) {
            let now = Utc::now();
            let elapsed_seconds = (now - last_time.with_timezone(&Utc)).num_seconds();
            if elapsed_seconds < COMPACTION_INTERNAL_COOLDOWN_SECONDS {
                tracing::warn!(
                    "Compaction triggered within {}s of last compaction ({}s ago) for run '{}'",
                    COMPACTION_INTERNAL_COOLDOWN_SECONDS,
                    elapsed_seconds,
                    run_name
                );
                return Ok(()); // Internal cooldown not elapsed
            }
        }
    }

    // Check if compaction is actually needed (threshold check)
    let check_result =
        crate::core::compaction::check_learnings_compaction_with_config(&state, &global_config);
    if !matches!(check_result, Ok(Some(_))) {
        return Ok(()); // Not needed
    }

    // Spawn compaction subprocess
    let hirsel_exe =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;

    let mut cmd = Command::new(&hirsel_exe);
    cmd.arg("__compact-learnings")
        .arg(run_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Spawn detached
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    match cmd.spawn() {
        Ok(child) => {
            tracing::info!(
                "Spawned compaction process for '{}', pid={}",
                run_name,
                child.id()
            );
            Ok(())
        }
        Err(e) => Err(format!("Failed to spawn compaction: {}", e)),
    }
}

/// Check if all workers are inactive and trigger eval if needed.
/// This is called from get_run_detail polling to catch cases where
/// the MCP subprocess's maybe_trigger_eval call failed.
///
/// DEPRECATED: Eval triggering is now handled by the daemon lifecycle polling loop.
/// This function is kept for backwards compatibility but should not be called.
#[allow(dead_code)]
pub fn trigger_eval_if_needed(run_name: &str) -> Result<bool, String> {
    use crate::core::workers::maybe_trigger_eval;

    let run_dir = config::run_dir(run_name);
    let files = crate::core::Files::new(&run_dir);
    let state =
        SQLiteState::new(files.db_path()).map_err(|e| format!("Failed to open database: {}", e))?;

    // Only check if run is in Working status
    let status = state
        .status()
        .map_err(|e| format!("Failed to get status: {}", e))?;
    if status != Status::Working {
        return Ok(false);
    }

    // Check if all workers are inactive
    let all_inactive = state
        .all_workers_inactive()
        .map_err(|e| format!("Failed to check workers: {}", e))?;
    if !all_inactive {
        return Ok(false);
    }

    // All workers inactive + status is Working = should trigger eval or mark done
    tracing::info!(
        "GUI polling detected all workers inactive for '{}', triggering eval check",
        run_name
    );

    match maybe_trigger_eval(run_name, &run_dir) {
        Ok(triggered) => {
            if triggered {
                tracing::info!("Triggered eval for run '{}'", run_name);
            } else {
                tracing::info!("Marked run '{}' as Done (no eval script)", run_name);
            }
            Ok(triggered)
        }
        Err(e) => Err(format!("Failed to trigger eval: {}", e)),
    }
}
