//! Implementation of the `hirsel resume` command.
//!
//! Resumes a paused, runaway, or timed-out run by respawning workers.

use crate::core::{Config, SQLiteState, Status, WorkerStatus, WorkerUpdate};
use regex::Regex;
use std::sync::LazyLock;

/// Regex patterns for time limit parsing
static TIME_HOUR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)h$").expect("invalid regex"));
static TIME_MIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)m$").expect("invalid regex"));
static TIME_HOUR_MIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)h(\d+)m$").expect("invalid regex"));

/// Run the resume command
pub fn run_resume(run_name: &str, time_limit: Option<&str>, json: bool) -> anyhow::Result<()> {
    let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
    let run_dir = config.runs_dir().join(run_name);

    if !run_dir.exists() {
        if json {
            println!(
                r#"{{"success": false, "error": "Run '{}' not found"}}"#,
                run_name
            );
        } else {
            eprintln!("Run '{}' not found", run_name);
        }
        return Ok(());
    }

    let db_path = run_dir.join("hirsel.db");
    let state = SQLiteState::new(db_path)?;

    let current_status = state.status()?;

    // Check if run is in a resumable state
    if !matches!(
        current_status,
        Status::Paused | Status::Runaway | Status::TimedOut
    ) {
        if json {
            println!(
                r#"{{"success": false, "error": "Run is not paused (status: {})"}}"#,
                current_status
            );
        } else {
            eprintln!("Run is not paused (status: {})", current_status);
        }
        return Ok(());
    }

    // Get paused workers
    let workers = state.get_workers()?;
    let paused_workers: Vec<_> = workers
        .iter()
        .filter(|w| w.status == WorkerStatus::Paused)
        .collect();

    if paused_workers.is_empty() {
        if json {
            println!(r#"{{"success": false, "error": "No paused workers to resume"}}"#);
        } else {
            eprintln!("No paused workers to resume");
        }
        return Ok(());
    }

    // Handle time limit
    state.clear_time_tracking()?;

    if let Some(limit_str) = time_limit {
        match parse_time_limit(limit_str) {
            Ok(minutes) => {
                state.set_time_limit_minutes(Some(minutes))?;
                state.set_started_at(None)?;
                if !json {
                    println!("Time limit set: {} minutes", minutes);
                }
            }
            Err(e) => {
                if json {
                    println!(r#"{{"success": false, "error": "{}"}}"#, e);
                } else {
                    eprintln!("{}", e);
                }
                return Ok(());
            }
        }
    } else {
        // Clear any previous time limit
        state.set_time_limit_minutes(None)?;
    }

    // Set status to working
    state.set_status(Status::Working)?;

    // Update workers to working status (actual respawning would be done by worker system)
    let mut resumed_count = 0;
    let mut resumed_names = Vec::new();

    for worker in &paused_workers {
        state.update_worker(
            &worker.name,
            WorkerUpdate {
                status: Some(WorkerStatus::Working),
                needs_restart: Some(true), // Signal that worker needs respawning
                ..Default::default()
            },
        )?;
        resumed_count += 1;
        resumed_names.push(worker.name.clone());

        if !json {
            println!("● Resuming {}", worker.name);
        }
    }

    if json {
        let time_limit_msg = time_limit
            .map(|t| format!(r#", "time_limit": "{}""#, t))
            .unwrap_or_default();
        println!(
            r#"{{"success": true, "resumed_workers": {}, "resumed_names": {:?}{}}}"#,
            resumed_count, resumed_names, time_limit_msg
        );
    } else {
        println!("Resumed {} worker(s)", resumed_count);
        println!();
        println!(
            "Workers marked for restart. Use 'hirsel attach {}' to watch progress.",
            run_name
        );
    }

    Ok(())
}

/// Parse time limit string into minutes
///
/// Accepts formats like:
/// - "30" or "30m" - 30 minutes
/// - "1h" - 1 hour (60 minutes)
/// - "1h30m" - 1 hour 30 minutes (90 minutes)
pub fn parse_time_limit(time_str: &str) -> Result<i64, String> {
    let time_str = time_str.trim();

    // Try pure number (minutes)
    if let Ok(minutes) = time_str.parse::<i64>() {
        if minutes < 1 {
            return Err("Time limit must be at least 1 minute".to_string());
        }
        return Ok(minutes);
    }

    // Try patterns like "1h", "30m", "1h30m"
    if let Some(caps) = TIME_HOUR_RE.captures(time_str) {
        let hours: i64 = caps[1].parse().unwrap();
        return Ok(hours * 60);
    }

    if let Some(caps) = TIME_MIN_RE.captures(time_str) {
        let minutes: i64 = caps[1].parse().unwrap();
        if minutes < 1 {
            return Err("Time limit must be at least 1 minute".to_string());
        }
        return Ok(minutes);
    }

    if let Some(caps) = TIME_HOUR_MIN_RE.captures(time_str) {
        let hours: i64 = caps[1].parse().unwrap();
        let minutes: i64 = caps[2].parse().unwrap();
        return Ok(hours * 60 + minutes);
    }

    Err(format!(
        "Invalid time format '{}'. Use: 30, 30m, 1h, or 1h30m",
        time_str
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_limit_minutes() {
        assert_eq!(parse_time_limit("30").unwrap(), 30);
        assert_eq!(parse_time_limit("30m").unwrap(), 30);
        assert_eq!(parse_time_limit("60m").unwrap(), 60);
    }

    #[test]
    fn test_parse_time_limit_hours() {
        assert_eq!(parse_time_limit("1h").unwrap(), 60);
        assert_eq!(parse_time_limit("2h").unwrap(), 120);
    }

    #[test]
    fn test_parse_time_limit_combined() {
        assert_eq!(parse_time_limit("1h30m").unwrap(), 90);
        assert_eq!(parse_time_limit("2h15m").unwrap(), 135);
    }

    #[test]
    fn test_parse_time_limit_invalid() {
        assert!(parse_time_limit("abc").is_err());
        assert!(parse_time_limit("0").is_err());
        assert!(parse_time_limit("0m").is_err());
        assert!(parse_time_limit("1d").is_err());
    }
}
