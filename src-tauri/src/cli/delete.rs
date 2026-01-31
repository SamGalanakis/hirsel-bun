//! Delete command - remove a run and its associated files
//!
//! Removes a hirsel run, killing any running workers and cleaning up
//! worktrees, directories, and database files.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator, CliOutput};
use crate::core::orchestrator::OrchestratorError;
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

    match block_on(orch.delete_run(run_name)) {
        Ok(()) => {
            let data = DeleteData {
                run: run_name.to_string(),
            };
            output.success_with_data(&format!("Removed: {}", run_name), data);
            Ok(())
        }
        Err(OrchestratorError::RunNotFound(name)) => {
            output.error_continue(&format!("Run '{}' not found", name));
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_exists() {
        // Basic smoke test - module compiles
        assert!(true);
    }
}
