//! Delete command - remove a run and its associated files
//!
//! Removes a hirsel run, killing any running workers and cleaning up
//! worktrees, directories, and database files.

use crate::core::ops::{delete_run, DeleteRunConfig, OpsError};

/// Execute the delete command for a run
pub fn execute(run_name: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Use CLI configuration: cleanup remote, don't delete gyp chat
    let config = DeleteRunConfig::for_cli(run_name);

    match delete_run(config) {
        Ok(result) => {
            if json {
                let output = serde_json::json!({
                    "success": true,
                    "run": result.run_name,
                    "message": format!("Removed: {}", result.run_name),
                    "workers_killed": result.workers_killed,
                    "project_remote_removed": result.project_remote_removed,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                if result.workers_killed > 0 {
                    println!("Killed {} worker(s)", result.workers_killed);
                }
                println!("Removed: {}", result.run_name);
            }
            Ok(())
        }
        Err(OpsError::RunNotFound(name)) => {
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
                Err(Box::new(e))
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
