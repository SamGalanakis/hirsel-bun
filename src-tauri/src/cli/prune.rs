//! Prune command - remove all delivered runs
//!
//! Cleans up runs that have been delivered (their work merged to the
//! project repository). This helps keep the runs directory clean.

use crate::cli::helpers::block_on;
use crate::core::{config, state::SQLiteState, state::Status, Files};
use std::fs;

/// Execute the prune command
pub fn execute(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let runs_dir = config::runs_dir();

    if !runs_dir.exists() {
        if json {
            let output = serde_json::json!({
                "success": true,
                "pruned": 0,
                "message": "No runs",
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            println!("No runs");
        }
        return Ok(());
    }

    let mut pruned = 0;
    let mut pruned_runs = Vec::new();

    // Iterate through run directories
    for entry in fs::read_dir(&runs_dir)? {
        let entry = entry?;
        let run_dir = entry.path();

        if !run_dir.is_dir() {
            continue;
        }

        let run_name = match run_dir.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let files = Files::new(&run_dir);
        let db_path = files.db_path();

        if !db_path.exists() {
            continue;
        }

        // Try to read state and check if delivered
        let should_prune = match block_on(SQLiteState::new(&run_name)) {
            Ok(state) => {
                let status = block_on(state.status()).unwrap_or(Status::Draft);
                if status == Status::Delivered {
                    // Kill any lingering worker processes
                    if let Ok(workers) = block_on(state.get_workers()) {
                        for worker in workers {
                            if let Some(pid) = worker.pid {
                                kill_process(pid as u32);
                            }
                        }
                    }

                    // Get project path for worktree cleanup
                    if let Ok(Some(project_path_str)) = block_on(state.get_project_path()) {
                        let project_path = std::path::PathBuf::from(project_path_str);
                        if project_path.exists() {
                            // Remove hirsel_work remote if it exists
                            if let Ok(repo) = git2::Repository::open(&project_path) {
                                let _ = repo.remote_delete("hirsel_work");
                            }
                        }
                    }

                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        };

        if should_prune {
            // Remove the run directory
            if fs::remove_dir_all(&run_dir).is_ok() {
                pruned += 1;
                pruned_runs.push(run_name.clone());
                if !json {
                    println!("  {}", run_name);
                }
            }
        }
    }

    if json {
        let output = serde_json::json!({
            "success": true,
            "pruned": pruned,
            "runs": pruned_runs,
            "message": if pruned == 0 {
                "No delivered runs to prune".to_string()
            } else {
                format!("Pruned {} run(s)", pruned)
            },
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if pruned == 0 {
        println!("No delivered runs to prune");
    } else {
        println!("\nPruned {} run(s)", pruned);
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
