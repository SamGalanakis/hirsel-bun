//! Implementation of the `hirsel attach` command.
//!
//! Attaches to a worker's tmux session to view live output.

use crate::core::{Config, SQLiteState};
use std::process::Command;

/// Run the attach command
pub fn run_attach(run_name: &str, target: Option<&str>, json: bool) -> anyhow::Result<()> {
    let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
    let run_dir = config.runs_dir().join(run_name);

    if !run_dir.exists() {
        if json {
            println!(r#"{{"success": false, "error": "Run '{}' not found"}}"#, run_name);
        } else {
            eprintln!("Run '{}' not found", run_name);
        }
        return Ok(());
    }

    let db_path = run_dir.join("hirsel.db");
    let state = SQLiteState::new(db_path)?;

    let workers = state.get_workers()?;
    let evals = state.get_evals(10)?;

    if workers.is_empty() && evals.is_empty() {
        if json {
            println!(r#"{{"success": false, "error": "No workers or evals for run '{}'"}}"#, run_name);
        } else {
            eprintln!("No workers or evals for run '{}'", run_name);
        }
        return Ok(());
    }

    // Determine leader (who has "scope" task or first worker)
    let tasks = state.get_tasks()?;
    let mut leader: Option<String> = None;
    for task in &tasks {
        if task.id == "scope" {
            if let Some(ref claimed_by) = task.claimed_by {
                leader = Some(claimed_by.clone());
                break;
            }
        }
    }
    if leader.is_none() && workers.len() > 1 {
        leader = workers.first().map(|w| w.name.clone());
    }

    // Determine what to attach to
    let (selection_type, selection_name) = if let Some(target_name) = target {
        // Check if it's a worker name
        let worker_match = workers.iter().find(|w| w.name == target_name);
        if worker_match.is_some() {
            ("worker", target_name.to_string())
        } else {
            // Check if it's an eval name
            let eval_match = evals.iter().find(|e| {
                let eval_name = e.eval_name.clone().unwrap_or_else(|| format!("eval_{}", e.id));
                eval_name == target_name
            });
            if eval_match.is_some() {
                ("eval", target_name.to_string())
            } else {
                if json {
                    println!(r#"{{"success": false, "error": "'{}' not found as worker or eval"}}"#, target_name);
                } else {
                    eprintln!("'{}' not found as worker or eval", target_name);
                }
                return Ok(());
            }
        }
    } else if workers.len() == 1 && evals.is_empty() {
        // Single worker, no evals - attach directly
        ("worker", workers[0].name.clone())
    } else {
        // List available targets
        if json {
            let mut targets = Vec::new();
            for w in &workers {
                let is_leader = leader.as_ref() == Some(&w.name);
                targets.push(serde_json::json!({
                    "type": "worker",
                    "name": w.name,
                    "status": w.status.as_str(),
                    "is_leader": is_leader
                }));
            }
            for e in &evals {
                let eval_name = e.eval_name.clone().unwrap_or_else(|| format!("eval_{}", e.id));
                targets.push(serde_json::json!({
                    "type": "eval",
                    "name": eval_name,
                    "status": e.status.as_str()
                }));
            }
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "success": false,
                "error": "Multiple targets available, specify one",
                "targets": targets
            }))?);
        } else {
            println!("Multiple targets available. Specify one:");
            println!();
            println!("Workers:");
            for w in &workers {
                let leader_mark = if leader.as_ref() == Some(&w.name) { " (leader)" } else { "" };
                println!("  {} [{}]{}", w.name, w.status, leader_mark);
            }
            if !evals.is_empty() {
                println!();
                println!("Evals:");
                for e in &evals {
                    let eval_name = e.eval_name.clone().unwrap_or_else(|| format!("eval_{}", e.id));
                    println!("  {} [{}]", eval_name, e.status);
                }
            }
            println!();
            println!("Usage: hirsel attach {} <target>", run_name);
        }
        return Ok(());
    };

    // Get tmux session name
    let session_name = format!("hirsel-{}-{}", run_name, selection_name);

    // Check if tmux session exists
    let session_exists = Command::new("tmux")
        .args(["has-session", "-t", &session_name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !session_exists {
        if json {
            println!(
                r#"{{"success": false, "error": "No tmux session '{}' found. {} may not be running."}}"#,
                session_name, selection_name
            );
        } else {
            eprintln!("No tmux session '{}' found.", session_name);
            eprintln!("{} '{}' may not be running.", selection_type, selection_name);
            
            // Check if there's a log file we can tail instead
            let log_file = run_dir.join("logs").join(format!("{}.log", selection_name));
            if log_file.exists() {
                eprintln!();
                eprintln!("You can view the log file with:");
                eprintln!("  tail -f {}", log_file.display());
            }
        }
        return Ok(());
    }

    if json {
        // Can't attach interactively in JSON mode
        println!(
            r#"{{"success": true, "session": "{}", "command": "tmux attach-session -t {}"}}"#,
            session_name, session_name
        );
    } else {
        // Attach to tmux session
        println!("Attaching to {} '{}'...", selection_type, selection_name);
        println!("(Detach with Ctrl-b d)");
        println!();

        let status = Command::new("tmux")
            .args(["attach-session", "-t", &session_name])
            .status();

        match status {
            Ok(s) if s.success() => {
                println!();
                println!("Detached from session.");
            }
            Ok(_) => {
                eprintln!("tmux attach failed");
            }
            Err(e) => {
                eprintln!("Failed to run tmux: {}", e);
            }
        }
    }

    Ok(())
}

/// List available targets for a run (for shell completion)
pub fn list_targets(run_name: &str) -> anyhow::Result<Vec<String>> {
    let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
    let run_dir = config.runs_dir().join(run_name);

    if !run_dir.exists() {
        return Ok(vec![]);
    }

    let db_path = run_dir.join("hirsel.db");
    let state = SQLiteState::new(db_path)?;

    let mut targets = Vec::new();

    for worker in state.get_workers()? {
        targets.push(worker.name);
    }

    for eval in state.get_evals(10)? {
        let eval_name = eval.eval_name.unwrap_or_else(|| format!("eval_{}", eval.id));
        targets.push(eval_name);
    }

    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_targets_nonexistent_run() {
        // Should return empty list for non-existent run
        let targets = list_targets("nonexistent-run-12345").unwrap();
        assert!(targets.is_empty());
    }
}
