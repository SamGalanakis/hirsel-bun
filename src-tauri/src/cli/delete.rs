//! Delete command - remove a run and its associated files
//!
//! Removes a hirsel run, killing any running workers and cleaning up
//! worktrees, directories, and database files.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator};
use crate::core::orchestrator::OrchestratorError;

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
    let orch = get_orchestrator(profile)?;

    match block_on(orch.delete_run(run_name)) {
        Ok(()) => {
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
        Err(OrchestratorError::RunNotFound(name)) => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "not_found",
                    "message": format!("Run '{}' not found", name),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("Run '{}' not found", name);
            }
            Ok(())
        }
        Err(e) => {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "delete_failed",
                    "message": e.to_string(),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
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
