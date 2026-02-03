//! CLI helper utilities for orchestrator integration.
//!
//! This module provides helpers for CLI commands to use the Orchestrator trait,
//! enabling both local and remote operation through the same code paths.

use crate::core::orchestrator::{create_orchestrator, Orchestrator, OrchestratorError};
use serde::Serialize;
use std::future::Future;

/// Create a tokio runtime and block on a future.
///
/// This helper allows CLI commands (which run synchronously) to call
/// async orchestrator methods.
///
/// # Example
///
/// ```ignore
/// use crate::cli::helpers::{block_on, get_orchestrator};
///
/// fn list_runs(profile: Option<&str>) -> Result<(), anyhow::Error> {
///     let orch = get_orchestrator(profile)?;
///     let runs = block_on(orch.list_runs())?;
///     for run in runs {
///         println!("{}: {}", run.name, run.status);
///     }
///     Ok(())
/// }
/// ```
pub fn block_on<F: Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime")
        .block_on(f)
}

/// Get an orchestrator instance for the given profile.
///
/// If profile is None, uses the default profile from config.
/// Returns a boxed Orchestrator trait object that can be either
/// LocalOrchestrator or RemoteOrchestrator.
pub fn get_orchestrator(profile: Option<&str>) -> Result<Box<dyn Orchestrator>, OrchestratorError> {
    create_orchestrator(profile)
}

/// Helper to run an async operation and convert the result to anyhow::Error
pub fn run_async<T, F>(profile: Option<&str>, op: F) -> anyhow::Result<T>
where
    F: FnOnce(
        &dyn Orchestrator,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<T, OrchestratorError>> + '_>>,
{
    let orch = get_orchestrator(profile)?;
    let result = block_on(op(orch.as_ref()))?;
    Ok(result)
}

// =============================================================================
// CLI Output Helper
// =============================================================================

/// Helper for consistent CLI output formatting.
///
/// Handles both human-readable and JSON output formats, ensuring
/// consistent structure across all CLI commands.
///
/// # Example
///
/// ```ignore
/// use crate::cli::helpers::CliOutput;
///
/// fn pause_run(name: &str, json: bool) -> Result<(), Error> {
///     let output = CliOutput::new(json);
///
///     match do_pause(name) {
///         Ok(()) => output.success(&format!("Paused run '{}'", name)),
///         Err(e) => output.error(&e.to_string()),
///     }
///     Ok(())
/// }
/// ```
pub struct CliOutput {
    json: bool,
}

impl CliOutput {
    /// Create a new CLI output helper.
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    /// Output a success message.
    ///
    /// In JSON mode, outputs: `{"success": true, "message": "..."}`
    /// In text mode, prints the message to stdout.
    pub fn success(&self, message: &str) {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": true,
                    "message": message
                }))
                .unwrap()
            );
        } else {
            println!("{}", message);
        }
    }

    /// Output a success message with additional data.
    ///
    /// In JSON mode, outputs: `{"success": true, "message": "...", ...data}`
    /// In text mode, prints only the message to stdout.
    pub fn success_with_data<T: Serialize>(&self, message: &str, data: T) {
        if self.json {
            // Merge message into data
            let mut value = serde_json::to_value(&data).unwrap_or(serde_json::json!({}));
            if let Some(obj) = value.as_object_mut() {
                obj.insert("success".to_string(), serde_json::json!(true));
                obj.insert("message".to_string(), serde_json::json!(message));
            }
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
        } else {
            println!("{}", message);
        }
    }

    /// Output an error message and exit.
    ///
    /// In JSON mode, outputs: `{"success": false, "error": "..."}`
    /// In text mode, prints to stderr with "Error: " prefix.
    pub fn error(&self, error: &str) -> ! {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": false,
                    "error": error
                }))
                .unwrap()
            );
            std::process::exit(1);
        } else {
            eprintln!("Error: {}", error);
            std::process::exit(1);
        }
    }

    /// Output a "not found" error and exit.
    ///
    /// Convenience method for common "X not found" errors.
    pub fn not_found(&self, resource: &str, name: &str) -> ! {
        self.error(&format!("{} '{}' not found", resource, name))
    }

    /// Output an error message without exiting.
    ///
    /// Use when you want to report an error but continue execution.
    pub fn error_continue(&self, error: &str) {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": false,
                    "error": error
                }))
                .unwrap()
            );
        } else {
            eprintln!("{}", error);
        }
    }

    /// Output an error with additional context data.
    pub fn error_with_data<T: Serialize>(&self, error: &str, data: T) {
        if self.json {
            let mut value = serde_json::to_value(&data).unwrap_or(serde_json::json!({}));
            if let Some(obj) = value.as_object_mut() {
                obj.insert("success".to_string(), serde_json::json!(false));
                obj.insert("error".to_string(), serde_json::json!(error));
            }
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
        } else {
            eprintln!("{}", error);
        }
    }

    /// Check if JSON output is enabled.
    pub fn is_json(&self) -> bool {
        self.json
    }
}

// =============================================================================
// Orchestrator Error Handling Helper
// =============================================================================

/// Handle common orchestrator errors consistently across CLI commands.
///
/// This helper handles the standard error pattern used by most CLI commands:
/// - RunNotFound: Display "Run 'X' not found" error
/// - InvalidOperation: Display the operation-specific error message
/// - Other errors: In JSON mode, display error and return Ok; otherwise propagate
///
/// # Example
///
/// ```ignore
/// use crate::cli::helpers::{block_on, get_orchestrator, CliOutput, handle_orchestrator_result};
///
/// fn pause_run(name: &str, json: bool) -> anyhow::Result<()> {
///     let output = CliOutput::new(json);
///     let orch = get_orchestrator(None)?;
///
///     handle_orchestrator_result(
///         block_on(orch.pause_run(name)),
///         &output,
///         json,
///         || format!("Paused run '{}'", name),
///     )
/// }
/// ```
pub fn handle_orchestrator_result<T, F>(
    result: Result<T, OrchestratorError>,
    output: &CliOutput,
    json: bool,
    success_msg: F,
) -> anyhow::Result<Option<T>>
where
    F: FnOnce() -> String,
{
    match result {
        Ok(value) => {
            output.success(&success_msg());
            Ok(Some(value))
        }
        Err(OrchestratorError::RunNotFound(name)) => {
            output.error_continue(&format!("Run '{}' not found", name));
            Ok(None)
        }
        Err(OrchestratorError::InvalidOperation(msg)) => {
            output.error_continue(&msg);
            Ok(None)
        }
        Err(e) => {
            if json {
                output.error_continue(&e.to_string());
                Ok(None)
            } else {
                Err(e.into())
            }
        }
    }
}

// =============================================================================
// Time Limit Parsing
// =============================================================================

/// Parse time limit string into minutes.
///
/// Supports various formats:
/// - "30" (minutes)
/// - "30m" (minutes)
/// - "1h" (hours)
/// - "1.5h" (float hours)
/// - "1h30m" (combined)
///
/// Returns an error if the parsed value is less than 1 minute.
///
/// # Example
///
/// ```ignore
/// use crate::cli::helpers::parse_time_limit;
///
/// assert_eq!(parse_time_limit("30").unwrap(), 30);
/// assert_eq!(parse_time_limit("1.5h").unwrap(), 90);
/// assert_eq!(parse_time_limit("1h30m").unwrap(), 90);
/// assert!(parse_time_limit("0").is_err());
/// ```
pub fn parse_time_limit(s: &str) -> Result<i64, String> {
    let s = s.trim().to_lowercase();

    // Check for combined format like "1h30m"
    if s.contains('h') && s.contains('m') {
        let h_pos = s.find('h').unwrap();
        let m_pos = s.find('m').unwrap();

        if h_pos < m_pos {
            let hours_str = &s[..h_pos];
            let minutes_str = &s[h_pos + 1..m_pos];

            let hours: f64 = hours_str
                .parse()
                .map_err(|_| format!("Invalid hours: {}", hours_str))?;
            let minutes: f64 = minutes_str
                .parse()
                .map_err(|_| format!("Invalid minutes: {}", minutes_str))?;

            let total = (hours * 60.0 + minutes) as i64;
            if total < 1 {
                return Err("Time limit must be at least 1 minute".to_string());
            }
            return Ok(total);
        }
    }

    // Check for hours format (supports floats like "1.5h")
    if s.ends_with('h') {
        let num_str = &s[..s.len() - 1];
        let hours: f64 = num_str
            .parse()
            .map_err(|_| format!("Invalid hours: {}", num_str))?;
        let total = (hours * 60.0) as i64;
        if total < 1 {
            return Err("Time limit must be at least 1 minute".to_string());
        }
        return Ok(total);
    }

    // Check for minutes format
    if s.ends_with('m') {
        let num_str = &s[..s.len() - 1];
        let minutes: f64 = num_str
            .parse()
            .map_err(|_| format!("Invalid minutes: {}", num_str))?;
        let total = minutes as i64;
        if total < 1 {
            return Err("Time limit must be at least 1 minute".to_string());
        }
        return Ok(total);
    }

    // Plain number - assume minutes
    let minutes: f64 = s
        .parse()
        .map_err(|_| format!("Invalid time limit: {}", s))?;
    let total = minutes as i64;
    if total < 1 {
        return Err("Time limit must be at least 1 minute".to_string());
    }
    Ok(total)
}

// =============================================================================
// Orchestrator Result Handling with Data
// =============================================================================

/// Handle orchestrator errors with additional success data.
///
/// Like `handle_orchestrator_result` but allows attaching additional data
/// to the success response (useful for JSON output).
///
/// # Example
///
/// ```ignore
/// use crate::cli::helpers::{block_on, get_orchestrator, CliOutput, handle_orchestrator_result_with_data};
///
/// fn deliver_run(name: &str, json: bool) -> anyhow::Result<()> {
///     let output = CliOutput::new(json);
///     let orch = get_orchestrator(None)?;
///
///     handle_orchestrator_result_with_data(
///         block_on(orch.deliver_run(name, None)),
///         &output,
///         json,
///         |branch| format!("Delivered to branch '{}'", branch),
///         |branch| DeliverData { branch: branch.clone() },
///     )
/// }
/// ```
pub fn handle_orchestrator_result_with_data<T, F, D, S>(
    result: Result<T, OrchestratorError>,
    output: &CliOutput,
    json: bool,
    success_msg: F,
    data_fn: D,
) -> anyhow::Result<Option<T>>
where
    F: FnOnce(&T) -> String,
    D: FnOnce(&T) -> S,
    S: Serialize,
{
    match result {
        Ok(value) => {
            let msg = success_msg(&value);
            let data = data_fn(&value);
            output.success_with_data(&msg, data);
            Ok(Some(value))
        }
        Err(OrchestratorError::RunNotFound(name)) => {
            output.error_continue(&format!("Run '{}' not found", name));
            Ok(None)
        }
        Err(OrchestratorError::InvalidOperation(msg)) => {
            output.error_continue(&msg);
            Ok(None)
        }
        Err(e) => {
            if json {
                output.error_continue(&e.to_string());
                Ok(None)
            } else {
                Err(e.into())
            }
        }
    }
}

// =============================================================================
// Worker Scale
// =============================================================================

/// Parsed worker scale configuration.
pub struct WorkerScale {
    pub max: u32,
}

impl WorkerScale {
    /// Parse worker scale from string - just the max worker count.
    /// - "4" -> autoscale up to 4 workers
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();

        // Simple number = max workers
        let max: u32 = s
            .parse()
            .map_err(|_| format!("Invalid worker count: '{}'. Use a number like '4'", s))?;
        if max == 0 {
            return Err("Worker count must be at least 1".to_string());
        }
        Ok(WorkerScale { max })
    }

    /// Initial worker count - always 1, we autoscale from there
    pub fn initial_count(&self) -> u32 {
        1
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_limit_minutes() {
        assert_eq!(parse_time_limit("30").unwrap(), 30);
        assert_eq!(parse_time_limit("30m").unwrap(), 30);
        assert_eq!(parse_time_limit("45m").unwrap(), 45);
    }

    #[test]
    fn test_parse_time_limit_hours() {
        assert_eq!(parse_time_limit("1h").unwrap(), 60);
        assert_eq!(parse_time_limit("2h").unwrap(), 120);
        assert_eq!(parse_time_limit("1.5h").unwrap(), 90);
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
        assert!(parse_time_limit("-5").is_err());
    }

    #[test]
    fn test_parse_time_limit_whitespace() {
        assert_eq!(parse_time_limit("  30  ").unwrap(), 30);
        assert_eq!(parse_time_limit(" 1h ").unwrap(), 60);
    }
}
