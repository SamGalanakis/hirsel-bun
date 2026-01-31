//! Implementation of the `hirsel pause` command.
//!
//! Pauses all workers in a run by sending SIGTERM and updating status.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator, CliOutput};
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
    let output = CliOutput::new(json);
    let orch = get_orchestrator(profile)?;

    match block_on(orch.pause_run(run_name)) {
        Ok(()) => {
            output.success(&format!("Paused run '{}'", run_name));
            if !json {
                println!("Resume with: hirsel resume {}", run_name);
            }
            Ok(())
        }
        Err(OrchestratorError::RunNotFound(name)) => {
            output.error_continue(&format!("Run '{}' not found", name));
            Ok(())
        }
        Err(OrchestratorError::InvalidOperation(msg)) => {
            output.error_continue(&msg);
            Ok(())
        }
        Err(e) => {
            if json {
                output.error_continue(&e.to_string());
                Ok(())
            } else {
                Err(e.into())
            }
        }
    }
}
