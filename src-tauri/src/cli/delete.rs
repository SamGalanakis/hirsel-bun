//! Delete command - remove a run and its associated files
//!
//! Removes a hirsel run, killing any running workers and cleaning up
//! worktrees, directories, and database files.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{
    block_on, get_orchestrator, handle_orchestrator_result_with_data, CliOutput,
};
use serde::Serialize;

#[derive(Serialize)]
struct DeleteData {
    run: String,
}

/// Execute the delete command for a run
pub fn execute(run_name: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    execute_with_profile(run_name, None, json)
}

/// Execute the delete command with a specific profile
pub fn execute_with_profile(
    run_name: &str,
    profile: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = CliOutput::new(json);
    let orch = get_orchestrator(profile)?;

    handle_orchestrator_result_with_data(
        block_on(orch.delete_run(run_name)),
        &output,
        json,
        |_| format!("Removed: {}", run_name),
        |_| DeleteData {
            run: run_name.to_string(),
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_exists() {
        // Basic smoke test - module compiles
        assert!(true);
    }
}
