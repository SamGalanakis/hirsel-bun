//! Deliver command - create branch in target repo
//!
//! Delivers the work from a hirsel run by creating a new branch
//! in the original project repository. For remote repos, pushes
//! directly to the remote. For local repos, creates a local branch.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{block_on, get_orchestrator, CliOutput};
use crate::core::orchestrator::OrchestratorError;
use serde::Serialize;

#[derive(Serialize)]
struct DeliverData {
    branch: String,
}

/// Execute the deliver command for a run
pub fn execute(
    run_name: &str,
    branch: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    execute_with_profile(run_name, branch, None, json)
}

/// Execute the deliver command with a specific profile
pub fn execute_with_profile(
    run_name: &str,
    branch: Option<&str>,
    profile: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = CliOutput::new(json);
    let orch = get_orchestrator(profile)?;

    match block_on(orch.deliver_run(run_name, branch.map(|s| s.to_string()))) {
        Ok(branch_name) => {
            let data = DeliverData {
                branch: branch_name.clone(),
            };
            output.success_with_data(&format!("Delivered to branch '{}'", branch_name), data);
            if !json {
                println!();
                println!("To cleanup: hirsel delete {}", run_name);
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

#[cfg(test)]
mod tests {
    // Note: Integration tests would require a full git setup
    // These are placeholder tests

    #[test]
    fn test_branch_name_default() {
        let run_name = "my-run";
        let branch = format!("hirsel/{}", run_name);
        assert_eq!(branch, "hirsel/my-run");
    }
}
