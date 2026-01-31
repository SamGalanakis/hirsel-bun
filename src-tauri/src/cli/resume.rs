//! Implementation of the `hirsel resume` command.
//!
//! Resumes a paused, runaway, or timed-out run by respawning workers.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator, CliOutput};
use crate::core::orchestrator::OrchestratorError;
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

/// Regex patterns for time limit parsing
static TIME_HOUR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)h$").expect("invalid regex"));
static TIME_MIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)m$").expect("invalid regex"));
static TIME_HOUR_MIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)h(\d+)m$").expect("invalid regex"));

#[derive(Serialize)]
struct ResumeData {
    #[serde(skip_serializing_if = "Option::is_none")]
    time_limit: Option<String>,
}

/// Run the resume command
pub fn run_resume(run_name: &str, time_limit: Option<&str>, json: bool) -> anyhow::Result<()> {
    run_resume_with_profile(run_name, time_limit, None, json)
}

/// Run the resume command with a specific profile
pub fn run_resume_with_profile(
    run_name: &str,
    time_limit: Option<&str>,
    profile: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let output = CliOutput::new(json);

    // Parse time limit if provided
    let time_limit_minutes = if let Some(limit_str) = time_limit {
        match parse_time_limit(limit_str) {
            Ok(minutes) => Some(minutes as u32),
            Err(e) => {
                output.error_continue(&e);
                return Ok(());
            }
        }
    } else {
        None
    };

    let orch = get_orchestrator(profile)?;

    match block_on(orch.resume_run(run_name, time_limit_minutes)) {
        Ok(()) => {
            let data = ResumeData {
                time_limit: time_limit.map(|s| s.to_string()),
            };
            output.success_with_data(&format!("Resumed run '{}'", run_name), data);
            if !json {
                if let Some(limit) = time_limit_minutes {
                    println!("Time limit set: {} minutes", limit);
                }
                println!();
                println!("Use 'hirsel attach {}' to watch progress.", run_name);
            }
            Ok(())
        }
        Err(OrchestratorError::RunNotFound(name)) => {
            output.error_continue(&format!("Run '{}' not found", name));
            Ok(())
        }
        Err(OrchestratorError::InvalidOperation(msg)) => {
            output.error_continue(&msg);
            Ok(())
        }
        Err(e) => {
            if json {
                output.error_continue(&e.to_string());
                Ok(())
            } else {
                Err(e.into())
            }
        }
    }
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
