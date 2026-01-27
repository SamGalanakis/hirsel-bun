//! Summary command - generate or view run summary.
//!
//! The summary command provides:
//! - View existing summary for a completed run
//! - Regenerate summary with --regenerate flag
//! - JSON output for scripting

use crate::core::state::{SQLiteState, StateError, TaskStatus};
use crate::core::{config, Files};
use serde::Serialize;

/// Errors that can occur during summary operations.
#[derive(Debug, thiserror::Error)]
pub enum SummaryError {
    #[error("Run '{0}' not found")]
    RunNotFound(String),

    #[error("State error: {0}")]
    State(#[from] StateError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to serialize: {0}")]
    Serialization(String),
}

/// Summary data structure for JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub run_name: String,
    pub status: String,
    pub tasks_completed: usize,
    pub tasks_total: usize,
    pub workers_used: usize,
    pub summary_text: Option<String>,
    pub has_summary: bool,
}

/// Execute the `hirsel summary` command.
///
/// Shows or generates a summary for a run.
pub fn run_summary(
    run_name: &str,
    regenerate: bool,
    json_output: bool,
) -> Result<String, SummaryError> {
    // Check run exists
    if !config::run_exists(run_name) {
        return Err(SummaryError::RunNotFound(run_name.to_string()));
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path()).map_err(SummaryError::State)?;

    // Get existing summary
    let existing_summary = state.get_summary().map_err(SummaryError::State)?;

    // If regenerate requested or no summary exists, generate one
    let summary_text = if regenerate || existing_summary.is_none() {
        let generated = generate_summary(&state, run_name)?;
        state.set_summary(&generated).map_err(SummaryError::State)?;
        Some(generated)
    } else {
        existing_summary
    };

    // Get stats for output
    let status = state.status().map_err(SummaryError::State)?;
    let tasks = state.get_tasks().map_err(SummaryError::State)?;
    let workers = state.get_workers().map_err(SummaryError::State)?;

    let tasks_completed = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .count();

    if json_output {
        let summary = RunSummary {
            run_name: run_name.to_string(),
            status: status.to_string(),
            tasks_completed,
            tasks_total: tasks.len(),
            workers_used: workers.len(),
            summary_text: summary_text.clone(),
            has_summary: summary_text.is_some(),
        };

        return serde_json::to_string_pretty(&summary)
            .map_err(|e| SummaryError::Serialization(e.to_string()));
    }

    // Text output
    let mut output = String::new();

    output.push_str(&format!("Summary for '{}'\n", run_name));
    output.push_str(&format!("Status: {}\n", status));
    output.push_str(&format!(
        "Tasks: {}/{} completed\n",
        tasks_completed,
        tasks.len()
    ));
    output.push_str(&format!("Workers: {}\n\n", workers.len()));

    match summary_text {
        Some(text) => {
            output.push_str("--- Summary ---\n");
            output.push_str(&text);
            output.push('\n');
        }
        None => {
            output.push_str("No summary available.\n");
            output.push_str("Use --regenerate to generate a summary.\n");
        }
    }

    Ok(output)
}

/// Generate a summary of the run's work.
fn generate_summary(state: &SQLiteState, run_name: &str) -> Result<String, SummaryError> {
    let status = state.status().map_err(SummaryError::State)?;
    let tasks = state.get_tasks().map_err(SummaryError::State)?;
    let workers = state.get_workers().map_err(SummaryError::State)?;
    let history = state.get_history(100).map_err(SummaryError::State)?;
    let request = state.get_request().map_err(SummaryError::State)?;

    let mut summary = String::new();

    // Header
    summary.push_str(&format!("# Run: {}\n\n", run_name));

    // Request/Goal
    if let Some(req) = request {
        summary.push_str("## Goal\n");
        summary.push_str(&req);
        summary.push_str("\n\n");
    }

    // Status
    summary.push_str("## Status\n");
    summary.push_str(&format!("Final status: {}\n\n", status));

    // Task summary
    let tasks_done: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .collect();
    let tasks_todo: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Todo)
        .collect();

    summary.push_str("## Tasks Completed\n");
    if tasks_done.is_empty() {
        summary.push_str("No tasks completed.\n");
    } else {
        for task in &tasks_done {
            let worker = task
                .claimed_by
                .as_ref()
                .map(|w| format!(" (by {})", w))
                .unwrap_or_default();
            summary.push_str(&format!("- {} - {}{}\n", task.id, task.name, worker));
        }
    }
    summary.push('\n');

    // Remaining tasks (if any)
    if !tasks_todo.is_empty() {
        summary.push_str("## Tasks Remaining\n");
        for task in &tasks_todo {
            let blocked = if !task.blocked_by.is_empty() {
                format!(" [blocked by: {}]", task.blocked_by.join(", "))
            } else {
                String::new()
            };
            summary.push_str(&format!("- {} - {}{}\n", task.id, task.name, blocked));
        }
        summary.push('\n');
    }

    // Worker summary
    summary.push_str("## Workers\n");
    for worker in &workers {
        summary.push_str(&format!("- {} ({})\n", worker.name, worker.status.as_str()));
    }
    summary.push('\n');

    // Key events from history
    let key_events: Vec<_> = history
        .iter()
        .filter(|h| {
            matches!(
                h.action.as_str(),
                "init"
                    | "status_change"
                    | "task_done"
                    | "eval_passed"
                    | "eval_failed"
                    | "work_done"
            )
        })
        .collect();

    if !key_events.is_empty() {
        summary.push_str("## Key Events\n");
        for event in key_events.iter().take(20) {
            let detail = event
                .detail
                .as_ref()
                .map(|d| format!(": {}", d))
                .unwrap_or_default();
            summary.push_str(&format!("- {}{}\n", event.action, detail));
        }
    }

    Ok(summary)
}

/// Check if a run has a summary.
pub fn has_summary(run_name: &str) -> Result<bool, SummaryError> {
    if !config::run_exists(run_name) {
        return Err(SummaryError::RunNotFound(run_name.to_string()));
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path()).map_err(SummaryError::State)?;

    let summary = state.get_summary().map_err(SummaryError::State)?;
    Ok(summary.is_some())
}

/// Get summary text for a run.
pub fn get_summary_text(run_name: &str) -> Result<Option<String>, SummaryError> {
    if !config::run_exists(run_name) {
        return Err(SummaryError::RunNotFound(run_name.to_string()));
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path()).map_err(SummaryError::State)?;

    state.get_summary().map_err(SummaryError::State)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_summary_not_found() {
        let result = run_summary("nonexistent-run-xyz", false, false);
        assert!(matches!(result, Err(SummaryError::RunNotFound(_))));
    }

    #[test]
    fn test_has_summary_not_found() {
        let result = has_summary("nonexistent-run-xyz");
        assert!(matches!(result, Err(SummaryError::RunNotFound(_))));
    }

    #[test]
    fn test_get_summary_text_not_found() {
        let result = get_summary_text("nonexistent-run-xyz");
        assert!(matches!(result, Err(SummaryError::RunNotFound(_))));
    }

    #[test]
    fn test_run_summary_struct_serialization() {
        let summary = RunSummary {
            run_name: "test-run".to_string(),
            status: "done".to_string(),
            tasks_completed: 5,
            tasks_total: 10,
            workers_used: 3,
            summary_text: Some("Test summary".to_string()),
            has_summary: true,
        };

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"run_name\":\"test-run\""));
        assert!(json.contains("\"tasks_completed\":5"));
        assert!(json.contains("\"has_summary\":true"));
    }
}
