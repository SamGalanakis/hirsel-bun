//! Implementation of the `hirsel attach` command.
//!
//! Attaches to a worker to view live output using a TUI.

use crate::cli::helpers::block_on;
use crate::cli::tui::AttachTui;
use crate::core::{Config, SQLiteState};

/// Run the attach command
pub fn run_attach(run_name: &str, target: Option<&str>, json: bool) -> anyhow::Result<()> {
    let (config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));
    let run_dir = config.runs_dir().join(run_name);

    if !run_dir.exists() {
        if json {
            println!(
                r#"{{"success": false, "error": "Run '{}' not found"}}"#,
                run_name
            );
        } else {
            eprintln!("Run '{}' not found", run_name);
        }
        return Ok(());
    }

    let state = block_on(SQLiteState::new(run_name))?;

    let workers = block_on(state.get_workers())?;
    let evals = block_on(state.get_evals(10))?;

    if workers.is_empty() && evals.is_empty() {
        if json {
            println!(
                r#"{{"success": false, "error": "No workers or evals for run '{}'"}}"#,
                run_name
            );
        } else {
            eprintln!("No workers or evals for run '{}'", run_name);
        }
        return Ok(());
    }

    // Determine leader (who has "scope" node or first worker)
    // Use board nodes from the delta state if available
    let mut leader: Option<String> = None;
    if let Ok(Some(project_id)) = block_on(state.get_project_id()) {
        use crate::core::delta::DeltaState;
        let route_id = block_on(state.get_route_id()).unwrap_or(0);
        let delta_state = DeltaState::with_route(project_id, route_id);
        if let Ok(nodes) = block_on(delta_state.get_nodes()) {
            for node in &nodes {
                if node.id == "scope" {
                    if let Some(ref claimed_by) = node.claimed_by {
                        leader = Some(claimed_by.clone());
                        break;
                    }
                }
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
                let eval_name = e
                    .eval_name
                    .clone()
                    .unwrap_or_else(|| format!("eval_{}", e.id));
                eval_name == target_name
            });
            if eval_match.is_some() {
                ("eval", target_name.to_string())
            } else {
                if json {
                    println!(
                        r#"{{"success": false, "error": "'{}' not found as worker or eval"}}"#,
                        target_name
                    );
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
                let eval_name = e
                    .eval_name
                    .clone()
                    .unwrap_or_else(|| format!("eval_{}", e.id));
                targets.push(serde_json::json!({
                    "type": "eval",
                    "name": eval_name,
                    "status": e.status.as_str()
                }));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "success": false,
                    "error": "Multiple targets available, specify one",
                    "targets": targets
                }))?
            );
        } else {
            println!("Multiple targets available. Specify one:");
            println!();
            println!("Workers:");
            for w in &workers {
                let leader_mark = if leader.as_ref() == Some(&w.name) {
                    " (leader)"
                } else {
                    ""
                };
                println!("  {} [{}]{}", w.name, w.status, leader_mark);
            }
            if !evals.is_empty() {
                println!();
                println!("Evals:");
                for e in &evals {
                    let eval_name = e
                        .eval_name
                        .clone()
                        .unwrap_or_else(|| format!("eval_{}", e.id));
                    println!("  {} [{}]", eval_name, e.status);
                }
            }
            println!();
            println!("Usage: hirsel attach {} <target>", run_name);
        }
        return Ok(());
    };

    // Handle based on type
    if selection_type == "worker" {
        if json {
            // Can't run TUI in JSON mode
            println!(
                r#"{{"success": true, "worker": "{}", "hint": "Run without --json to view live output"}}"#,
                selection_name
            );
        } else {
            // Launch TUI for worker
            let state = block_on(SQLiteState::new(run_name))?;
            let mut tui = AttachTui::new(run_name.to_string(), selection_name.clone(), state);

            if let Err(e) = tui.run() {
                eprintln!("TUI error: {}", e);
            }
        }
    } else {
        // Eval - show log file path (evals don't have streaming events)
        let log_file = run_dir.join("logs").join(format!("{}.log", selection_name));

        if json {
            if log_file.exists() {
                println!(
                    r#"{{"success": true, "eval": "{}", "log_file": "{}"}}"#,
                    selection_name,
                    log_file.display()
                );
            } else {
                println!(
                    r#"{{"success": false, "error": "Log file not found for eval '{}'"}}"#,
                    selection_name
                );
            }
        } else if log_file.exists() {
            println!("Eval '{}' log file:", selection_name);
            println!("  {}", log_file.display());
            println!();
            println!("View with: tail -f {}", log_file.display());
        } else {
            eprintln!("Log file not found for eval '{}'", selection_name);
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

    let state = block_on(SQLiteState::new(run_name))?;

    let mut targets = Vec::new();

    for worker in block_on(state.get_workers())? {
        targets.push(worker.name);
    }

    for eval in block_on(state.get_evals(10))? {
        let eval_name = eval
            .eval_name
            .unwrap_or_else(|| format!("eval_{}", eval.id));
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
