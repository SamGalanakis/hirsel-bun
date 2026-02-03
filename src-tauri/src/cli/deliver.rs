//! Deliver command - create branch in target repo
//!
//! Delivers the work from a hirsel run by creating a new branch
//! in the original project repository. For remote repos, pushes
//! directly to the remote. For local repos, creates a local branch.
//!
//! Uses the Orchestrator trait to support both local and remote modes.

use crate::cli::helpers::{
    block_on, get_orchestrator, handle_orchestrator_result_with_data, CliOutput,
};
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

    let result = handle_orchestrator_result_with_data(
        block_on(orch.deliver_run(run_name, branch.map(|s| s.to_string()))),
        &output,
        json,
        |branch_name| format!("Delivered to branch '{}'", branch_name),
        |branch_name| DeliverData {
            branch: branch_name.clone(),
        },
    )?;

    if result.is_some() && !json {
        println!();
        println!("To cleanup: hirsel delete {}", run_name);
    }

    Ok(())
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
