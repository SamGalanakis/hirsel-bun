//! Implementation of the `hirsel runs` command.
//!
//! Lists all runs with their status, task progress, and worker counts.

use crate::core::{Config, SQLiteState, Status, TaskStatus, WorkerStatus};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

/// Run summary for display
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

/// List all runs
pub fn list_runs(json: bool) -> anyhow::Result<()> {
    let (config, _warnings) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
    let runs_dir = config.runs_dir();

    let mut runs: Vec<RunInfo> = Vec::new();

    if runs_dir.exists() {
        let mut entries: Vec<_> = fs::read_dir(&runs_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        // Sort by modification time (newest first)
        entries.sort_by(|a, b| {
            let a_time = a.metadata().and_then(|m| m.modified()).ok();
            let b_time = b.metadata().and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        for entry in entries {
            let run_name = entry.file_name().to_string_lossy().to_string();
            let run_dir = entry.path();

            if let Some(info) = get_run_info(&run_name, &run_dir) {
                runs.push(info);
            }
        }
    }

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

/// Get info for a single run
fn get_run_info(name: &str, run_dir: &PathBuf) -> Option<RunInfo> {
    let db_path = run_dir.join("hirsel.db");

    if !db_path.exists() {
        // Run directory exists but no database - might be incomplete
        return Some(RunInfo {
            name: name.to_string(),
            status: "unknown".to_string(),
            tasks_done: 0,
            tasks_total: 0,
            workers_active: 0,
            workers_total: 0,
            elapsed_minutes: None,
            time_limit_minutes: None,
        });
    }

    let state = SQLiteState::new(db_path).ok()?;

    let status = state.status().unwrap_or(Status::Idle);
    let tasks = state.get_tasks().unwrap_or_default();
    let workers = state.get_workers().unwrap_or_default();

    let tasks_done = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .count();
    let tasks_total = tasks.len();

    let workers_active = workers
        .iter()
        .filter(|w| matches!(w.status, WorkerStatus::Working | WorkerStatus::Waiting))
        .count();
    let workers_total = workers.len();

    let time_info = state.get_time_info().ok().flatten();
    let elapsed_minutes = time_info.as_ref().map(|t| t.elapsed_minutes);
    let time_limit_minutes = time_info.as_ref().map(|t| t.limit_minutes);

    Some(RunInfo {
        name: name.to_string(),
        status: status.to_string(),
        tasks_done,
        tasks_total,
        workers_active,
        workers_total,
        elapsed_minutes,
        time_limit_minutes,
    })
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
    use tempfile::TempDir;

    #[test]
    fn test_get_run_info_no_db() {
        let temp_dir = TempDir::new().unwrap();
        let run_dir = temp_dir.path().to_path_buf();

        let info = get_run_info("test-run", &run_dir).unwrap();
        assert_eq!(info.name, "test-run");
        assert_eq!(info.status, "unknown");
    }

    #[test]
    fn test_get_run_info_with_db() {
        let temp_dir = TempDir::new().unwrap();
        let run_dir = temp_dir.path().to_path_buf();
        let db_path = run_dir.join("hirsel.db");

        // Create a state with some data
        let state = SQLiteState::new(db_path).unwrap();
        state.init_state(None).unwrap();
        state.set_status(Status::Working).unwrap();
        state.add_task("task1", "Task 1", None, None).unwrap();
        state.add_task("task2", "Task 2", None, None).unwrap();

        let info = get_run_info("test-run", &run_dir).unwrap();
        assert_eq!(info.name, "test-run");
        assert_eq!(info.status, "working");
        assert_eq!(info.tasks_total, 2);
        assert_eq!(info.tasks_done, 0);
    }

    #[test]
    fn test_format_status() {
        assert!(format_status("working").contains("●"));
        assert!(format_status("done").contains("✓"));
        assert!(format_status("paused").contains("⏸"));
        assert!(format_status("runaway").contains("⚠"));
        assert!(format_status("idle").contains("○"));
    }
}
