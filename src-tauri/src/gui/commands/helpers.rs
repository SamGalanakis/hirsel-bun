//! Helper functions shared across GUI commands

use chrono::Utc;

// Re-export shared helper functions from core::api_types
pub use crate::core::api_types::{
    calculate_duration_minutes, convert_status, is_completed_status, parse_elapsed_minutes,
    parse_timestamp,
};
use crate::core::{config, state::SQLiteState};

/// Internal cooldown for compaction checks (10 seconds)
const COMPACTION_INTERNAL_COOLDOWN_SECONDS: i64 = 10;

/// Trigger automatic learnings compaction if needed.
/// This is called from get_run_detail polling. It spawns a subprocess
/// to handle compaction (like improve does) to avoid Send issues with ACP.
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
