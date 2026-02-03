//! Implementation of the `hirsel pause` command.
//!
//! Pauses all workers in a run by sending SIGTERM and updating status.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator, handle_orchestrator_result, CliOutput};

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

    let result =
        handle_orchestrator_result(block_on(orch.pause_run(run_name)), &output, json, || {
            format!("Paused run '{}'", run_name)
        })?;

    if result.is_some() && !json {
        println!("Resume with: hirsel resume {}", run_name);
    }

    Ok(())
}
