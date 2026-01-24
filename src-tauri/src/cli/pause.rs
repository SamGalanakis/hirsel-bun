//! Implementation of the `hirsel pause` command.
//!
//! Pauses all workers in a run by sending SIGTERM and updating status.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator};
use crate::core::orchestrator::OrchestratorError;

/// Run the pause command
pub fn run_pause(run_name: &str, json: bool) -> anyhow::Result<()> {
    run_pause_with_profile(run_name, None, json)
}

/// Run the pause command with a specific profile
pub fn run_pause_with_profile(
    run_name: &str,
    profile: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    let orch = get_orchestrator(profile)?;

    match block_on(orch.pause_run(run_name)) {
        Ok(()) => {
            if json {
                println!(
                    r#"{{"success": true, "message": "Run '{}' paused"}}"#,
                    run_name
                );
            } else {
                println!("Paused run '{}'", run_name);
                println!("Resume with: hirsel resume {}", run_name);
            }
            Ok(())
        }
        Err(OrchestratorError::RunNotFound(name)) => {
            if json {
                println!(
                    r#"{{"success": false, "error": "Run '{}' not found"}}"#,
                    name
                );
            } else {
                eprintln!("Run '{}' not found", name);
            }
            Ok(())
        }
        Err(OrchestratorError::InvalidOperation(msg)) => {
            if json {
                println!(r#"{{"success": false, "error": "{}"}}"#, msg);
            } else {
                eprintln!("{}", msg);
            }
            Ok(())
        }
        Err(e) => {
            if json {
                println!(r#"{{"success": false, "error": "{}"}}"#, e);
                Ok(())
            } else {
                Err(e.into())
            }
        }
    }
}
