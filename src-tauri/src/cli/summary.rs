//! Summary command - generate or view run summary.
//!
//! The summary command provides:
//! - View existing summary for a completed run
//! - Regenerate summary with --regenerate flag
//! - JSON output for scripting

use crate::cli::helpers::block_on;
use crate::core::config;
use crate::core::delta::{DeltaState, LiveNodeStatus};
use crate::core::state::{SQLiteState, StateError};
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

    let state = block_on(SQLiteState::new(run_name)).map_err(SummaryError::State)?;

    // Get existing summary
    let existing_summary = block_on(state.get_summary()).map_err(SummaryError::State)?;

    // If regenerate requested or no summary exists, generate one
    let summary_text = if regenerate || existing_summary.is_none() {
        let generated = generate_summary(&state, run_name)?;
        block_on(state.set_summary(&generated)).map_err(SummaryError::State)?;
        Some(generated)
    } else {
        existing_summary
    };

    // Get stats for output
    let status = block_on(state.status()).map_err(SummaryError::State)?;
    let workers = block_on(state.get_workers()).map_err(SummaryError::State)?;

    // Get task stats from live_nodes if available (project run), otherwise fallback to empty
    let (tasks_completed, tasks_total) = if let Ok(Some(project_id)) =
        block_on(state.get_project_id())
    {
        let route_id = block_on(state.get_route_id()).unwrap_or(0);
        let delta_state = DeltaState::with_route(project_id, route_id);
        if let Ok(nodes) = block_on(delta_state.get_live_nodes()) {
            let completed = nodes
                .iter()
                .filter(|n| matches!(n.status, LiveNodeStatus::Done | LiveNodeStatus::Validated))
                .count();
            (completed, nodes.len())
        } else {
            (0, 0)
        }
    } else {
        (0, 0)
    };

    if json_output {
        let summary = RunSummary {
            run_name: run_name.to_string(),
            status: status.to_string(),
            tasks_completed,
            tasks_total,
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
        tasks_completed, tasks_total
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
    use crate::core::delta::LiveNode;

    let status = block_on(state.status()).map_err(SummaryError::State)?;
    let workers = block_on(state.get_workers()).map_err(SummaryError::State)?;
    let history = block_on(state.get_history(100)).map_err(SummaryError::State)?;
    let request = block_on(state.get_request()).map_err(SummaryError::State)?;

    // Get nodes from live_nodes if available
    let nodes: Vec<LiveNode> = if let Ok(Some(project_id)) = block_on(state.get_project_id()) {
        let route_id = block_on(state.get_route_id()).unwrap_or(0);
        let delta_state = DeltaState::with_route(project_id, route_id);
        block_on(delta_state.get_live_nodes()).unwrap_or_default()
    } else {
        vec![]
    };

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

    // Task summary using live_nodes
    let nodes_done: Vec<_> = nodes
        .iter()
        .filter(|n| matches!(n.status, LiveNodeStatus::Done | LiveNodeStatus::Validated))
        .collect();
    let nodes_pending: Vec<_> = nodes
        .iter()
        .filter(|n| n.status == LiveNodeStatus::Pending)
        .collect();

    summary.push_str("## Tasks Completed\n");
    if nodes_done.is_empty() {
        summary.push_str("No tasks completed.\n");
    } else {
        for node in &nodes_done {
            let worker = node
                .claimed_by
                .as_ref()
                .map(|w| format!(" (by {})", w))
                .unwrap_or_default();
            summary.push_str(&format!("- {} - {}{}\n", node.id, node.name, worker));
        }
    }
    summary.push('\n');

    // Remaining tasks (if any)
    if !nodes_pending.is_empty() {
        summary.push_str("## Tasks Remaining\n");
        for node in &nodes_pending {
            let blocked = if !node.blocked_by.is_empty() {
                format!(" [blocked by: {}]", node.blocked_by.join(", "))
            } else {
                String::new()
            };
            summary.push_str(&format!("- {} - {}{}\n", node.id, node.name, blocked));
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

    let state = block_on(SQLiteState::new(run_name)).map_err(SummaryError::State)?;

    let summary = block_on(state.get_summary()).map_err(SummaryError::State)?;
    Ok(summary.is_some())
}

/// Get summary text for a run.
pub fn get_summary_text(run_name: &str) -> Result<Option<String>, SummaryError> {
    if !config::run_exists(run_name) {
        return Err(SummaryError::RunNotFound(run_name.to_string()));
    }

    let state = block_on(SQLiteState::new(run_name)).map_err(SummaryError::State)?;

    block_on(state.get_summary()).map_err(SummaryError::State)
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
