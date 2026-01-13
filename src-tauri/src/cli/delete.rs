//! Delete command - remove a run and its associated files
//!
//! Removes a hirsel run, killing any running workers and cleaning up
//! worktrees, directories, and database files.

use crate::core::{config, state::SQLiteState, Files};
use std::fs;

/// Execute the delete command for a run
pub fn execute(run_name: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let run_dir = config::run_dir(run_name);

    // Check run exists
    if !run_dir.exists() {
        if json {
            let output = serde_json::json!({
                "success": false,
                "error": "not_found",
                "message": format!("Run '{}' not found", run_name),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("Run '{}' not found", run_name);
        }
        return Ok(());
    }

    // Try to get project path and kill workers
    let files = Files::new(&run_dir);
    let project_path = if let Ok(state) = SQLiteState::new(files.db_path()) {
        // Kill any running worker processes
        if let Ok(workers) = state.get_workers() {
            for worker in workers {
                if let Some(pid) = worker.pid {
                    kill_process(pid as u32);
                }
            }
        }

        // Get project path for worktree cleanup
        state.get_project_path().ok().flatten()
    } else {
        None
    };

    // Try to remove worktrees from project repo
    if let Some(project_path_str) = &project_path {
        let project_path = std::path::PathBuf::from(project_path_str);
        if project_path.exists() {
            // Remove hirsel_work remote if it exists
            if let Ok(repo) = git2::Repository::open(&project_path) {
                let _ = repo.remote_delete("hirsel_work");
            }
        }
    }

    // Remove the run directory
    fs::remove_dir_all(&run_dir)?;

    if json {
        let output = serde_json::json!({
            "success": true,
            "run": run_name,
            "message": format!("Removed: {}", run_name),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Removed: {}", run_name);
    }

    Ok(())
}

/// Kill a process by PID (best effort, ignore errors)
fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        // Use kill command on Unix
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
    }

    #[cfg(windows)]
    {
        // On Windows, use taskkill
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_exists() {
        // Basic smoke test - module compiles
        assert!(true);
    }
}
