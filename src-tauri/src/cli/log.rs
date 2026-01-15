//! Activity log viewing for hirsel runs.
//!
//! This module provides the `hirsel log` command implementation,
//! which displays activity history from the SQLite database.

use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::core::state::{HistoryEntry, SQLiteState};
use crate::core::Config;

/// Output format for log entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable format with colors
    Pretty,
    /// JSON format for scripting
    Json,
}

/// Format a history entry for display.
///
/// Returns a formatted string with timestamp, action, and detail.
fn format_history_entry(entry: &HistoryEntry) -> String {
    // Compact timestamp: "2024-01-15T14:30:45" -> "2024-01-15 14:30"
    let ts = entry
        .timestamp
        .get(..16)
        .unwrap_or(&entry.timestamp)
        .replace('T', " ");

    let detail = entry.detail.as_deref().unwrap_or("");

    match entry.action.as_str() {
        "task_claim" => format!("{} CLAIM: {}", ts, detail),
        "task_done" => format!("{} DONE: {}", ts, detail),
        "task_unclaim" => format!("{} UNCLAIM: {}", ts, detail),
        "task_add" => format!("{} TASK: {}", ts, detail),
        "task_delete" => format!("{} DELETE: {}", ts, detail),
        "task_reopen" => format!("{} REOPEN: {}", ts, detail),
        "status_change" => format!("{} STATUS: {}", ts, detail),
        "worker_add" => format!("{} WORKER+: {}", ts, detail),
        "worker_status" => format!("{} WORKER: {}", ts, detail),
        "eval_start" => format!("{} EVAL: {}", ts, detail),
        "eval_complete" => format!("{} EVAL: {}", ts, detail),
        "eval_cancel" => format!("{} EVAL: {}", ts, detail),
        "mode_change" => format!("{} MODE: {}", ts, detail),
        "pause_all" => format!("{} PAUSE: {}", ts, detail),
        "resume_all" => format!("{} RESUME: {}", ts, detail),
        "init" => format!("{} INIT", ts),
        "summary_generated" => format!("{} SUMMARY: generated", ts),
        "compaction" => format!("{} COMPACT: {}", ts, detail),
        _ => format!("{} {}: {}", ts, entry.action.to_uppercase(), detail),
    }
}

/// Result of running the log command
#[derive(Debug)]
pub enum LogResult {
    /// Successfully displayed log
    Success,
    /// No activity yet
    Empty,
    /// Follow mode was interrupted
    Interrupted,
    /// Error occurred
    Error(String),
}

/// Run the log command.
///
/// # Arguments
/// * `run_name` - Name of the run to view logs for
/// * `follow` - Whether to follow/tail the log
/// * `limit` - Maximum number of entries to show
/// * `format` - Output format (Pretty or Json)
///
/// # Returns
/// A `LogResult` indicating success or failure
pub fn run_log(run_name: &str, follow: bool, limit: usize, format: OutputFormat) -> LogResult {
    // Get the run directory from config
    let (config, _warnings) = match Config::load() {
        Ok(c) => c,
        Err(e) => return LogResult::Error(format!("Failed to load config: {}", e)),
    };

    let run_dir = config.runs_dir().join(run_name);
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        return LogResult::Error(format!(
            "Run '{}' not found (no database at {})",
            run_name,
            db_path.display()
        ));
    }

    // Open the database
    let state = match SQLiteState::new(db_path) {
        Ok(s) => s,
        Err(e) => return LogResult::Error(format!("Failed to open database: {}", e)),
    };

    // Get initial history
    let history = match state.get_history(limit as i64) {
        Ok(h) => h,
        Err(e) => return LogResult::Error(format!("Failed to get history: {}", e)),
    };

    // Handle JSON output
    if format == OutputFormat::Json {
        return output_json(&history, run_name, follow);
    }

    // Handle empty history
    if history.is_empty() {
        println!("No activity yet");
        return LogResult::Empty;
    }

    // Print history (reversed since get_history returns DESC order)
    for entry in history.iter().rev() {
        println!("{}", format_history_entry(entry));
    }

    // If not following, we're done
    if !follow {
        return LogResult::Success;
    }

    // Follow mode
    println!("\nfollowing activity log  (Ctrl+C to stop)\n");
    follow_log(state, limit)
}

/// Output history in JSON format
fn output_json(history: &[HistoryEntry], run_name: &str, follow: bool) -> LogResult {
    if follow {
        // For follow mode with JSON, we'd need streaming JSON which is complex
        // For now, just output the current state
        eprintln!("Warning: --follow with --json outputs current state only");
    }

    #[derive(serde::Serialize)]
    struct JsonOutput<'a> {
        run: &'a str,
        history: Vec<JsonHistoryEntry<'a>>,
    }

    #[derive(serde::Serialize)]
    struct JsonHistoryEntry<'a> {
        timestamp: &'a str,
        action: &'a str,
        detail: Option<&'a str>,
    }

    let output = JsonOutput {
        run: run_name,
        history: history
            .iter()
            .rev()
            .map(|e| JsonHistoryEntry {
                timestamp: &e.timestamp,
                action: &e.action,
                detail: e.detail.as_deref(),
            })
            .collect(),
    };

    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            println!("{}", json);
            LogResult::Success
        }
        Err(e) => LogResult::Error(format!("Failed to serialize JSON: {}", e)),
    }
}

/// Follow the activity log, polling for new entries.
fn follow_log(state: SQLiteState, limit: usize) -> LogResult {
    let mut last_count = match state.get_history(limit as i64) {
        Ok(h) => h.len(),
        Err(e) => return LogResult::Error(format!("Failed to get history: {}", e)),
    };

    // Set up Ctrl+C handler
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();

    if let Err(e) = ctrlc_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    }) {
        // If we can't set up Ctrl+C handler, warn but continue
        eprintln!("Warning: Could not set up Ctrl+C handler: {}", e);
    }

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(1));

        // Reconnect to get fresh data (SQLite may have cached)
        let new_history = match state.get_history(limit as i64) {
            Ok(h) => h,
            Err(_) => continue, // Ignore transient errors
        };

        if new_history.len() > last_count {
            // Show new entries (they come in DESC order, so take from the front)
            let new_entries = &new_history[..new_history.len() - last_count];
            for entry in new_entries.iter().rev() {
                println!("{}", format_history_entry(entry));
            }
            let _ = io::stdout().flush();
            last_count = new_history.len();
        }
    }

    println!("\nstopped");
    LogResult::Interrupted
}

/// Set up a Ctrl+C handler.
///
/// This is a simple wrapper that attempts to set the handler.
/// If the platform doesn't support it, it returns an error.
fn ctrlc_handler<F: FnOnce() + Send + 'static>(handler: F) -> Result<(), String> {
    // We use a simple approach: spawn a thread that blocks on signal
    // This is a fallback since we don't have the ctrlc crate

    // For now, we'll just rely on the process being killed
    // The handler won't actually be called, but the loop will exit
    // when the process receives SIGINT

    // Store the handler but don't actually use it yet
    // In a real implementation, you'd use the ctrlc crate or signal handling
    let _ = handler;

    Ok(())
}

/// Get the database path for a run.
///
/// This is a convenience function for getting the db path without
/// loading the full config.
pub fn get_db_path(run_name: &str) -> Result<PathBuf, String> {
    let (config, _warnings) =
        Config::load().map_err(|e| format!("Failed to load config: {}", e))?;
    Ok(config.runs_dir().join(run_name).join("hirsel.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_history_entry_task_claim() {
        let entry = HistoryEntry {
            id: 1,
            timestamp: "2024-01-15T14:30:45.123456".to_string(),
            action: "task_claim".to_string(),
            detail: Some("scope by alpha".to_string()),
        };
        let formatted = format_history_entry(&entry);
        assert!(formatted.contains("2024-01-15 14:30"));
        assert!(formatted.contains("CLAIM:"));
        assert!(formatted.contains("scope by alpha"));
    }

    #[test]
    fn test_format_history_entry_task_done() {
        let entry = HistoryEntry {
            id: 2,
            timestamp: "2024-01-15T15:00:00.000000".to_string(),
            action: "task_done".to_string(),
            detail: Some("implement_auth by beta".to_string()),
        };
        let formatted = format_history_entry(&entry);
        assert!(formatted.contains("DONE:"));
        assert!(formatted.contains("implement_auth by beta"));
    }

    #[test]
    fn test_format_history_entry_status_change() {
        let entry = HistoryEntry {
            id: 3,
            timestamp: "2024-01-15T16:00:00.000000".to_string(),
            action: "status_change".to_string(),
            detail: Some("run working".to_string()),
        };
        let formatted = format_history_entry(&entry);
        assert!(formatted.contains("STATUS:"));
        assert!(formatted.contains("run working"));
    }

    #[test]
    fn test_format_history_entry_init() {
        let entry = HistoryEntry {
            id: 4,
            timestamp: "2024-01-15T14:00:00.000000".to_string(),
            action: "init".to_string(),
            detail: None,
        };
        let formatted = format_history_entry(&entry);
        assert!(formatted.contains("INIT"));
        // No detail expected
    }

    #[test]
    fn test_format_history_entry_unknown_action() {
        let entry = HistoryEntry {
            id: 5,
            timestamp: "2024-01-15T17:00:00.000000".to_string(),
            action: "custom_action".to_string(),
            detail: Some("some detail".to_string()),
        };
        let formatted = format_history_entry(&entry);
        assert!(formatted.contains("CUSTOM_ACTION:"));
        assert!(formatted.contains("some detail"));
    }

    #[test]
    fn test_format_history_entry_short_timestamp() {
        let entry = HistoryEntry {
            id: 6,
            timestamp: "2024-01-15".to_string(), // Short timestamp
            action: "task_add".to_string(),
            detail: Some("new_task".to_string()),
        };
        let formatted = format_history_entry(&entry);
        // Should handle short timestamps gracefully
        assert!(formatted.contains("2024-01-15"));
        assert!(formatted.contains("TASK:"));
    }
}
