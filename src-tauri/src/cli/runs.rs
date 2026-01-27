//! Implementation of the `hirsel runs` command.
//!
//! Lists all runs with their status, task progress, and worker counts.
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator};
use crate::core::api_types::RunSummary;
use serde::Serialize;

/// Run summary for display (maps from RunSummary)
#[derive(Debug, Clone, Serialize)]
pub struct RunInfo {
    pub name: String,
    pub status: String,
    pub tasks_done: usize,
    pub tasks_total: usize,
    pub workers_active: usize,
    pub workers_total: usize,
    pub elapsed_minutes: Option<f64>,
    pub time_limit_minutes: Option<i64>,
}

impl From<RunSummary> for RunInfo {
    fn from(s: RunSummary) -> Self {
        Self {
            name: s.name,
            status: format!("{:?}", s.status).to_lowercase(),
            tasks_done: s.tasks_done as usize,
            tasks_total: s.tasks_total as usize,
            workers_active: s.workers_active as usize,
            workers_total: s.workers_total as usize,
            elapsed_minutes: Some(s.elapsed_minutes),
            time_limit_minutes: s.time_limit_minutes.map(|v| v as i64),
        }
    }
}

/// List all runs using the Orchestrator trait
///
/// This supports both local and remote modes through the profile parameter
/// in the CLI args.
pub fn list_runs(json: bool) -> anyhow::Result<()> {
    list_runs_with_profile(None, json)
}

/// List all runs with a specific profile
pub fn list_runs_with_profile(profile: Option<&str>, json: bool) -> anyhow::Result<()> {
    let orch = get_orchestrator(profile)?;
    let run_summaries = block_on(orch.list_runs())?;

    let runs: Vec<RunInfo> = run_summaries.into_iter().map(RunInfo::from).collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&runs)?);
    } else if runs.is_empty() {
        println!("No runs found.");
        println!();
        println!("Start a new run with:");
        println!("  hirsel go <run-name> <spec-file>");
    } else {
        print_runs_table(&runs);
    }

    Ok(())
}

/// Print runs as a formatted table
fn print_runs_table(runs: &[RunInfo]) {
    // Calculate column widths
    let name_width = runs.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let status_width = runs
        .iter()
        .map(|r| r.status.len())
        .max()
        .unwrap_or(6)
        .max(6);

    // Print header
    println!(
        "{:<name_width$}  {:<status_width$}  {:>10}  {:>9}  {:>8}",
        "NAME",
        "STATUS",
        "TASKS",
        "WORKERS",
        "TIME",
        name_width = name_width,
        status_width = status_width,
    );

    // Print separator
    println!(
        "{:-<name_width$}  {:-<status_width$}  {:->10}  {:->9}  {:->8}",
        "",
        "",
        "",
        "",
        "",
        name_width = name_width,
        status_width = status_width,
    );

    // Print rows
    for run in runs {
        let tasks_str = format!("{}/{}", run.tasks_done, run.tasks_total);
        let workers_str = format!("{}/{}", run.workers_active, run.workers_total);

        let time_str = match (run.elapsed_minutes, run.time_limit_minutes) {
            (Some(elapsed), Some(limit)) => {
                format!("{}m/{}m", elapsed as i64, limit)
            }
            (Some(elapsed), None) => format!("{}m", elapsed as i64),
            _ => "-".to_string(),
        };

        // Apply status formatting
        let status_display = format_status(&run.status);

        println!(
            "{:<name_width$}  {:<status_width$}  {:>10}  {:>9}  {:>8}",
            run.name,
            status_display,
            tasks_str,
            workers_str,
            time_str,
            name_width = name_width,
            status_width = status_width,
        );
    }
}

/// Format status with visual indicators
fn format_status(status: &str) -> String {
    match status {
        "working" => format!("● {}", status),
        "done" | "delivered" | "merged" => format!("✓ {}", status),
        "paused" | "waiting" | "timed_out" => format!("⏸ {}", status),
        "runaway" | "eval_failed" => format!("⚠ {}", status),
        "eval" => format!("◐ {}", status),
        _ => format!("○ {}", status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_status() {
        assert!(format_status("working").contains("●"));
        assert!(format_status("done").contains("✓"));
        assert!(format_status("paused").contains("⏸"));
        assert!(format_status("runaway").contains("⚠"));
        assert!(format_status("idle").contains("○"));
    }

    #[test]
    fn test_run_info_from_summary() {
        use crate::core::api_types::{RunStatus, RunSummary};

        let summary = RunSummary {
            name: "test-run".to_string(),
            status: RunStatus::Working,
            tasks_done: 2,
            tasks_total: 5,
            workers_active: 1,
            workers_total: 2,
            elapsed_minutes: 10.5,
            time_limit_minutes: Some(60),
            has_unread_messages: false,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            project_id: None,
            project_name: None,
        };

        let info = RunInfo::from(summary);
        assert_eq!(info.name, "test-run");
        assert_eq!(info.status, "working");
        assert_eq!(info.tasks_done, 2);
        assert_eq!(info.tasks_total, 5);
        assert_eq!(info.workers_active, 1);
        assert_eq!(info.workers_total, 2);
        assert_eq!(info.elapsed_minutes, Some(10.5));
        assert_eq!(info.time_limit_minutes, Some(60));
    }
}
