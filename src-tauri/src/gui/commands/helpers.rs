//! Helper functions shared across GUI commands

use chrono::{NaiveDateTime, TimeZone, Utc};

use super::types::RunStatus;
use crate::core::{config, state::SQLiteState};

/// Parse a timestamp string and return a DateTime<Utc>
pub fn parse_timestamp(timestamp: &str) -> Option<chrono::DateTime<Utc>> {
    // Try RFC3339 first (has timezone)
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(timestamp) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try parsing as NaiveDateTime (no timezone, assume UTC)
    // Format: "2024-01-13T12:30:45.123456"
    if let Ok(naive) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(Utc.from_utc_datetime(&naive));
    }

    // Try without fractional seconds
    if let Ok(naive) = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%dT%H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }

    None
}

/// Parse a timestamp string (with or without timezone) and return elapsed minutes from now
pub fn parse_elapsed_minutes(timestamp: &str) -> f64 {
    if let Some(dt) = parse_timestamp(timestamp) {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(dt);
        return elapsed.num_seconds() as f64 / 60.0;
    }
    0.0
}

/// Calculate duration in minutes between two timestamps
pub fn calculate_duration_minutes(start: &str, end: &str) -> f64 {
    if let (Some(start_dt), Some(end_dt)) = (parse_timestamp(start), parse_timestamp(end)) {
        let duration = end_dt.signed_duration_since(start_dt);
        return (duration.num_seconds() as f64 / 60.0).max(0.0);
    }
    0.0
}

/// Check if a status represents a completed run
pub fn is_completed_status(status: &RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Done
            | RunStatus::Delivered
            | RunStatus::Merged
            | RunStatus::TimedOut
            | RunStatus::EvalFailed
            | RunStatus::Runaway
    )
}

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

/// Convert core Status to GUI RunStatus
pub fn convert_status(status: crate::core::state::Status) -> RunStatus {
    match status {
        crate::core::state::Status::Draft => RunStatus::Draft,
        crate::core::state::Status::Idle => RunStatus::Idle,
        crate::core::state::Status::Working => RunStatus::Working,
        crate::core::state::Status::Paused => RunStatus::Paused,
        crate::core::state::Status::Runaway => RunStatus::Runaway,
        crate::core::state::Status::TimedOut => RunStatus::TimedOut,
        crate::core::state::Status::Eval => RunStatus::Eval,
        crate::core::state::Status::EvalFailed => RunStatus::EvalFailed,
        crate::core::state::Status::Waiting => RunStatus::Waiting,
        crate::core::state::Status::Done => RunStatus::Done,
        crate::core::state::Status::Delivered => RunStatus::Delivered,
        crate::core::state::Status::Merged => RunStatus::Merged,
    }
}
