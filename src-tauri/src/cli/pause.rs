//! Implementation of the `hirsel pause` command.
//!
//! Pauses all workers in a run by sending SIGTERM and updating status.

use crate::core::{is_pid_alive, Config, SQLiteState, Status, WorkerStatus, WorkerUpdate};

/// Run the pause command
pub fn run_pause(run_name: &str, json: bool) -> anyhow::Result<()> {
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

    let db_path = run_dir.join("hirsel.db");
    let state = SQLiteState::new(db_path)?;

    let current_status = state.status()?;

    // Check if run is in a pauseable state
    if !matches!(current_status, Status::Working | Status::Eval) {
        if json {
            println!(
                r#"{{"success": false, "error": "Run is not active (status: {})"}}"#,
                current_status
            );
        } else {
            eprintln!("Run is not active (status: {})", current_status);
        }
        return Ok(());
    }

    let workers = state.get_workers()?;
    let mut paused_count = 0;
    let mut paused_names = Vec::new();

    for worker in &workers {
        // Try to terminate worker process if it has a PID
        if let Some(pid) = worker.pid {
            if is_pid_alive(pid as u32) {
                // Send SIGTERM
                #[cfg(unix)]
                {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                }

                if !json {
                    println!("○ Paused {} (PID {})", worker.name, pid);
                }

                state.update_worker(
                    &worker.name,
                    WorkerUpdate {
                        pid: Some(0), // Clear PID (use 0 as "no pid")
                        status: Some(WorkerStatus::Paused),
                        ..Default::default()
                    },
                )?;
                paused_count += 1;
                paused_names.push(worker.name.clone());
            } else {
                // Process not running but worker record exists
                if !worker.status.is_inactive() {
                    state.update_worker(
                        &worker.name,
                        WorkerUpdate {
                            status: Some(WorkerStatus::Paused),
                            ..Default::default()
                        },
                    )?;
                }
            }
        } else {
            // No PID but worker might be active
            if !worker.status.is_inactive() {
                state.update_worker(
                    &worker.name,
                    WorkerUpdate {
                        status: Some(WorkerStatus::Paused),
                        ..Default::default()
                    },
                )?;
            }
        }
    }

    // Cancel any running evals
    let evals_cancelled = state.cancel_running_evals("Run paused")?;

    // Set run status to paused
    state.set_status(Status::Paused)?;

    if json {
        println!(
            r#"{{"success": true, "paused_workers": {}, "paused_names": {:?}, "evals_cancelled": {}}}"#,
            paused_count, paused_names, evals_cancelled
        );
    } else {
        if evals_cancelled > 0 {
            println!("Cancelled {} running eval(s)", evals_cancelled);
        }
        if paused_count == 0 && evals_cancelled == 0 {
            println!("○ No running workers, but run is now paused");
        } else {
            println!("Paused {} worker(s)", paused_count);
        }
        println!("Resume with: hirsel resume {}", run_name);
    }

    Ok(())
}
