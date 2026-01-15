//! View command - display run status
//!
//! Shows a comprehensive view of a run including:
//! - Overall status and timing
//! - Project path and request
//! - Worker status with claimed tasks
//! - Task list with progress
//! - Recent activity history
//! - Summary (for completed runs)

use crate::core::{config, state::SQLiteState, Files};
use std::io::{self, Write};

/// Execute the view command for a run
pub fn execute(run_name: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Check run exists
    if !config::run_exists(run_name) {
        return Err(format!("Run '{}' not found", run_name).into());
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    if json {
        print_json(&state, run_name, &files)?;
    } else {
        print_text(&state, &files, run_name)?;
    }

    Ok(())
}

/// Print run status as JSON
fn print_json(
    state: &SQLiteState,
    run_name: &str,
    files: &Files,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::state::TaskStatus;

    let status = state.status()?;
    let tasks = state.get_tasks()?;
    let workers = state.get_workers()?;
    let time_info = state.get_time_info()?;
    let hitl = state.get_human_in_the_loop()?;
    let project_path = state.get_project_path()?;
    let request = state.get_request()?;
    let history = state.get_history(10)?;
    let summary = state.get_summary()?;

    // Build worker data with claimed tasks
    let worker_data: Vec<_> = workers
        .iter()
        .map(|w| {
            let claimed_task = tasks
                .iter()
                .find(|t| t.claimed_by.as_ref() == Some(&w.name));
            serde_json::json!({
                "name": w.name,
                "status": w.status.as_str(),
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
                "name": t.name,
                "status": t.status.as_str(),
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

    let output = serde_json::json!({
        "name": run_name,
        "status": status.as_str(),
        "mode": if hitl { "hitl" } else { "yolo" },
        "project_path": project_path,
        "request": request,
        "dir": files.run_dir().to_string_lossy(),
        "tasks": {
            "total": tasks.len(),
            "done": tasks_done,
            "doing": tasks_doing,
            "todo": tasks.len() - tasks_done - tasks_doing,
            "list": task_data,
        },
        "workers": {
            "total": workers.len(),
            "active": workers.iter().filter(|w| !w.status.is_inactive()).count(),
            "list": worker_data,
        },
        "time": time_info.map(|t| serde_json::json!({
            "limit_minutes": t.limit_minutes,
            "elapsed_minutes": t.elapsed_minutes.round() as i64,
            "remaining_minutes": t.remaining_minutes.round() as i64,
            "percent_elapsed": t.percent_elapsed.round() as i64,
        })),
        "history": history_data,
        "summary": summary,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Print run status as formatted text
fn print_text(
    state: &SQLiteState,
    files: &Files,
    run_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::state::TaskStatus;

    let status = state.status()?;
    let tasks = state.get_tasks()?;
    let workers = state.get_workers()?;
    let time_info = state.get_time_info()?;
    let hitl = state.get_human_in_the_loop()?;
    let project_path = state.get_project_path()?;
    let request = state.get_request()?;
    let history = state.get_history(10)?;
    let summary = state.get_summary()?;

    // Header with status
    let status_badge = format_status_badge(&status);
    println!();
    println!("  {} {}", run_name, status_badge);
    println!();

    // Project path
    let path_display = project_path.as_deref().unwrap_or("-");
    println!("  project  {}", path_display);

    // Time info
    if let Some(time) = &time_info {
        let elapsed = time.elapsed_minutes as i64;
        let limit = time.limit_minutes;
        let remaining = time.remaining_minutes as i64;
        let pct = time.percent_elapsed as i64;
        println!(
            "  time     {}/{}m ({}% elapsed, {}m remaining)",
            elapsed, limit, pct, remaining
        );
    }

    // Mode
    let mode = if hitl { "hitl" } else { "yolo" };
    println!("  mode     {}", mode);
    println!();

    // Workers with claimed tasks
    if !workers.is_empty() {
        println!("  workers");

        for worker in &workers {
            let status_icon = format_worker_status_icon(&worker.status);
            let claimed_task = tasks
                .iter()
                .find(|t| t.claimed_by.as_ref() == Some(&worker.name));
            let task_str = claimed_task
                .map(|t| {
                    let name = if t.name.len() > 25 {
                        &t.name[..25]
                    } else {
                        &t.name
                    };
                    format!(" -> {}", name)
                })
                .unwrap_or_default();

            let waiting_str = if worker.status == crate::core::state::WorkerStatus::Waiting {
                " (waiting)"
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
    if let Some(req) = &request {
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
            let name = if t.name.len() > 20 {
                &t.name[..20]
            } else {
                &t.name
            };
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
    if let Some(sum) = &summary {
        println!("  {}", "-".repeat(40));
        println!("  summary");
        println!("    {}", sum);
        println!();
    }

    // Waiting reason if paused/runaway
    if let Ok(Some(reason)) = state.get_waiting_reason() {
        if !reason.is_empty() {
            println!("  Waiting: {}", reason);
            println!();
        }
    }

    // Spec file info
    if files.spec().exists() {
        println!("  Spec: {}", files.spec().display());
    }
    println!("  Dir:  {}", files.run_dir().display());
    println!();

    io::stdout().flush()?;
    Ok(())
}

/// Format a status badge
fn format_status_badge(status: &crate::core::state::Status) -> String {
    use crate::core::state::Status;
    match status {
        Status::Draft => "[DRAFT]".to_string(),
        Status::Idle => "[idle]".to_string(),
        Status::Working => "[WORKING]".to_string(),
        Status::Paused => "[PAUSED]".to_string(),
        Status::Runaway => "[RUNAWAY!]".to_string(),
        Status::TimedOut => "[TIMED OUT]".to_string(),
        Status::Eval => "[eval]".to_string(),
        Status::EvalFailed => "[eval FAILED]".to_string(),
        Status::Waiting => "[waiting]".to_string(),
        Status::Done => "[DONE]".to_string(),
        Status::Delivered => "[delivered]".to_string(),
        Status::Merged => "[merged]".to_string(),
    }
}

/// Format worker status icon
fn format_worker_status_icon(status: &crate::core::state::WorkerStatus) -> &'static str {
    use crate::core::state::WorkerStatus;
    match status {
        WorkerStatus::Idle => "○",
        WorkerStatus::Working => "●",
        WorkerStatus::Waiting => "◐",
        WorkerStatus::Awaiting => "◌",
        WorkerStatus::Paused => "◫",
        WorkerStatus::Error => "✗",
    }
}

/// Format task status icon
fn format_task_status_icon(status: &crate::core::state::TaskStatus) -> &'static str {
    use crate::core::state::TaskStatus;
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
        use crate::core::state::Status;
        assert_eq!(format_status_badge(&Status::Working), "[WORKING]");
        assert_eq!(format_status_badge(&Status::Done), "[DONE]");
        assert_eq!(format_status_badge(&Status::Paused), "[PAUSED]");
    }

    #[test]
    fn test_format_task_status_icon() {
        use crate::core::state::TaskStatus;
        assert_eq!(format_task_status_icon(&TaskStatus::Todo), "○");
        assert_eq!(format_task_status_icon(&TaskStatus::Doing), "●");
        assert_eq!(format_task_status_icon(&TaskStatus::Done), "✓");
    }
}
