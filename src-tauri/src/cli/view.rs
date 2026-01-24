//! View command - display run status
//!
//! Shows a comprehensive view of a run including:
//! - Overall status and timing
//! - Project path and request
//! - Worker status with claimed tasks
//! - Task list with progress
//! - Recent activity history
//! - Summary (for completed runs)
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator};
use crate::core::api_types::{HistoryEntry, RunDetail, Task, TaskStatus, Worker, WorkerStatus};
use std::io::{self, Write};

/// Execute the view command for a run
pub fn execute(run_name: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    execute_with_profile(run_name, None, json)
}

/// Execute the view command for a run with a specific profile
pub fn execute_with_profile(
    run_name: &str,
    profile: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let orch = get_orchestrator(profile)?;

    // Fetch run details, tasks, workers, and history in sequence
    let run_detail = block_on(orch.get_run(run_name))?;
    let tasks = block_on(orch.list_tasks(run_name))?;
    let workers = block_on(orch.list_workers(run_name))?;
    let history = block_on(orch.get_history(run_name, Some(10)))?;

    if json {
        print_json(&run_detail, &tasks, &workers, &history)?;
    } else {
        print_text(&run_detail, &tasks, &workers, &history)?;
    }

    Ok(())
}

/// Print run status as JSON
fn print_json(
    run: &RunDetail,
    tasks: &[Task],
    workers: &[Worker],
    history: &[HistoryEntry],
) -> Result<(), Box<dyn std::error::Error>> {
    // Build worker data with claimed tasks
    let worker_data: Vec<_> = workers
        .iter()
        .map(|w| {
            let claimed_task = tasks
                .iter()
                .find(|t| t.claimed_by.as_ref() == Some(&w.name));
            serde_json::json!({
                "name": w.name,
                "status": format!("{:?}", w.status).to_lowercase(),
                "pid": w.pid,
                "claimed_task": claimed_task.map(|t| &t.id),
            })
        })
        .collect();

    // Build task data
    let task_data: Vec<_> = tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "name": t.description,
                "status": format!("{:?}", t.status).to_lowercase(),
                "claimed_by": t.claimed_by,
            })
        })
        .collect();

    // Build history data
    let history_data: Vec<_> = history
        .iter()
        .rev()
        .map(|h| {
            serde_json::json!({
                "timestamp": h.timestamp,
                "action": h.action,
                "detail": h.detail,
            })
        })
        .collect();

    let tasks_done = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .count();
    let tasks_doing = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Doing)
        .count();

    let workers_active = workers
        .iter()
        .filter(|w| w.status == WorkerStatus::Working)
        .count();

    // Calculate time info if we have started_at and time_limit
    let time_info = if let (Some(_started), Some(limit)) = (&run.started_at, run.time_limit_minutes)
    {
        let elapsed = run.elapsed_minutes;
        let remaining = (limit as f64) - elapsed;
        let pct = (elapsed / limit as f64) * 100.0;
        Some(serde_json::json!({
            "limit_minutes": limit,
            "elapsed_minutes": elapsed.round() as i64,
            "remaining_minutes": remaining.max(0.0).round() as i64,
            "percent_elapsed": pct.round() as i64,
        }))
    } else {
        None
    };

    let output = serde_json::json!({
        "name": run.name,
        "status": format!("{:?}", run.status).to_lowercase(),
        "mode": if run.human_in_the_loop { "hitl" } else { "yolo" },
        "project_path": run.project_path,
        "request": run.request,
        "tasks": {
            "total": tasks.len(),
            "done": tasks_done,
            "doing": tasks_doing,
            "todo": tasks.len() - tasks_done - tasks_doing,
            "list": task_data,
        },
        "workers": {
            "total": run.workers_total,
            "active": workers_active,
            "list": worker_data,
        },
        "time": time_info,
        "history": history_data,
        "summary": run.summary,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print run status as formatted text
fn print_text(
    run: &RunDetail,
    tasks: &[Task],
    workers: &[Worker],
    history: &[HistoryEntry],
) -> Result<(), Box<dyn std::error::Error>> {
    // Header with status
    let status_badge = format_status_badge(&run.status);
    println!();
    println!("  {} {}", run.name, status_badge);
    println!();

    // Project path
    let path_display = run.project_path.as_deref().unwrap_or("-");
    println!("  project  {}", path_display);

    // Time info
    if let Some(limit) = run.time_limit_minutes {
        let elapsed = run.elapsed_minutes as i64;
        let remaining = (limit as i64) - elapsed;
        let pct = if limit > 0 {
            (run.elapsed_minutes / limit as f64 * 100.0) as i64
        } else {
            0
        };
        println!(
            "  time     {}/{}m ({}% elapsed, {}m remaining)",
            elapsed,
            limit,
            pct,
            remaining.max(0)
        );
    }

    // Mode
    let mode = if run.human_in_the_loop {
        "hitl"
    } else {
        "yolo"
    };
    println!("  mode     {}", mode);
    println!();

    // Workers with claimed tasks
    if !workers.is_empty() {
        println!("  workers");

        for worker in workers {
            let status_icon = format_worker_status_icon(&worker.status);
            let claimed_task = tasks
                .iter()
                .find(|t| t.claimed_by.as_ref() == Some(&worker.name));
            let task_str = claimed_task
                .map(|t| {
                    let desc = &t.description;
                    let name = if desc.len() > 25 { &desc[..25] } else { desc };
                    format!(" -> {}", name)
                })
                .unwrap_or_default();

            let waiting_str = if worker.hitl_waiting {
                " (waiting for input)"
            } else {
                ""
            };

            println!(
                "    {} {:<12}{}{}",
                status_icon, worker.name, task_str, waiting_str
            );
        }
        println!();
    }

    // Request
    if let Some(req) = &run.request {
        println!("  request  {}", req);
        println!();
    }

    // Tasks with progress
    if !tasks.is_empty() {
        let tasks_done = tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count();
        let total = tasks.len();

        // Progress bar
        let bar = format_progress(tasks_done, total, 20);
        println!("  tasks    {}", bar);
        println!();

        // List tasks in two columns
        let mut task_iter = tasks.iter().enumerate().peekable();
        while let Some((i, t)) = task_iter.next() {
            let icon = format_task_status_icon(&t.status);
            let desc = &t.description;
            let name = if desc.len() > 20 { &desc[..20] } else { desc };
            let task_text = format!("{} {:<20}", icon, name);

            if i % 2 == 0 {
                print!("    {}", task_text);
                if task_iter.peek().is_none() {
                    println!();
                }
            } else {
                println!("  {}", task_text);
            }
        }
        println!();
    }

    // History
    if !history.is_empty() {
        println!("  {}", "-".repeat(40));
        println!("  history");
        for h in history.iter().rev() {
            let ts = &h.timestamp[..16.min(h.timestamp.len())];
            let detail = h.detail.as_deref().unwrap_or("");
            println!("    {} {}: {}", ts, h.action, detail);
        }
        println!();
    }

    // Summary (for completed runs)
    if let Some(sum) = &run.summary {
        println!("  {}", "-".repeat(40));
        println!("  summary");
        println!("    {}", sum);
        println!();
    }

    // Waiting reason if paused/runaway
    if let Some(reason) = &run.waiting_reason {
        if !reason.is_empty() {
            println!("  Waiting: {}", reason);
            println!();
        }
    }

    println!();

    io::stdout().flush()?;
    Ok(())
}

/// Format a status badge
fn format_status_badge(status: &crate::core::api_types::RunStatus) -> String {
    use crate::core::api_types::RunStatus;
    match status {
        RunStatus::Draft => "[DRAFT]".to_string(),
        RunStatus::Working => "[WORKING]".to_string(),
        RunStatus::Paused => "[PAUSED]".to_string(),
        RunStatus::Failed => "[FAILED]".to_string(),
        RunStatus::Eval => "[eval]".to_string(),
        RunStatus::Done => "[DONE]".to_string(),
        RunStatus::Delivered => "[delivered]".to_string(),
    }
}

/// Format worker status icon
fn format_worker_status_icon(status: &WorkerStatus) -> &'static str {
    match status {
        WorkerStatus::Working => "●",
        WorkerStatus::Awaiting => "◌",
        WorkerStatus::Paused => "◫",
        WorkerStatus::Error => "✗",
    }
}

/// Format task status icon
fn format_task_status_icon(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "○",
        TaskStatus::Doing => "●",
        TaskStatus::Done => "✓",
    }
}

/// Create a progress bar with count
fn format_progress(done: usize, total: usize, width: usize) -> String {
    let pct = if total > 0 {
        (done as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "[{}{}] {}/{}",
        "=".repeat(filled),
        " ".repeat(empty),
        done,
        total
    )
}

/// Create an ASCII progress bar (percentage)
#[cfg(test)]
fn progress_bar(percent: f64, width: usize) -> String {
    let filled = ((percent / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "[{}{}] {:3.0}%",
        "=".repeat(filled),
        " ".repeat(empty),
        percent
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::api_types::RunStatus;

    #[test]
    fn test_progress_bar() {
        assert_eq!(progress_bar(0.0, 10), "[          ]   0%");
        assert_eq!(progress_bar(50.0, 10), "[=====     ]  50%");
        assert_eq!(progress_bar(100.0, 10), "[==========] 100%");
    }

    #[test]
    fn test_format_progress() {
        assert_eq!(format_progress(0, 10, 10), "[          ] 0/10");
        assert_eq!(format_progress(5, 10, 10), "[=====     ] 5/10");
        assert_eq!(format_progress(10, 10, 10), "[==========] 10/10");
    }

    #[test]
    fn test_format_status_badge() {
        assert_eq!(format_status_badge(&RunStatus::Working), "[WORKING]");
        assert_eq!(format_status_badge(&RunStatus::Done), "[DONE]");
        assert_eq!(format_status_badge(&RunStatus::Paused), "[PAUSED]");
    }

    #[test]
    fn test_format_task_status_icon() {
        assert_eq!(format_task_status_icon(&TaskStatus::Todo), "○");
        assert_eq!(format_task_status_icon(&TaskStatus::Doing), "●");
        assert_eq!(format_task_status_icon(&TaskStatus::Done), "✓");
    }
}
