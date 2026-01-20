//! Deliver command - create branch in target repo
//!
//! Delivers the work from a hirsel run by creating a new branch
//! in the original project repository. For remote repos, pushes
//! directly to the remote. For local repos, creates a local branch.

use crate::core::{
    config, git, lifecycle::LocalLifecycleManager, state::SQLiteState, state::Status, Files,
};

/// Execute the deliver command for a run
pub fn execute(
    run_name: &str,
    branch: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check run exists
    if !config::run_exists(run_name) {
        return Err(format!("Run '{}' not found", run_name).into());
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Get project path and remote URL
    let project_path_str = state.get_project_path()?;
    let remote_url = state.get_remote_url()?;

    let project_path = project_path_str.as_ref().map(std::path::PathBuf::from);

    let project_path = match project_path {
        Some(p) if p.exists() => p,
        _ => {
            return Err(format!("Project path not found for run '{}'", run_name).into());
        }
    };

    // Find work directory (staging dir has the main git repo)
    let work_dir = run_dir.join("work").join("staging");
    let work_dir = if work_dir.exists() {
        work_dir
    } else {
        // Fallback for legacy runs
        let fallback = run_dir.join("work");
        if fallback.exists() {
            fallback
        } else {
            return Err(format!("Work directory not found for run '{}'", run_name).into());
        }
    };

    // Check for git repo
    if !work_dir.join(".git").exists() {
        return Err("No git repository found in work directory".into());
    }

    // Check for unmerged branches
    let unmerged = git::list_unmerged_branches(&work_dir)?;
    if !unmerged.is_empty() {
        let branch_list = unmerged.join(", ");
        if json {
            let output = serde_json::json!({
                "success": false,
                "error": "unmerged_branches",
                "branches": unmerged,
                "message": format!("Unmerged branches exist: {}", branch_list),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("Unmerged branches exist: {}", branch_list);
            eprintln!("All work must be merged to 'staging' before delivering.");
            eprintln!("Use 'hirsel view {}' to check worker status.", run_name);
        }
        return Ok(());
    }

    // Determine branch name
    let branch_name = branch
        .unwrap_or(&format!("hirsel/{}", run_name))
        .to_string();

    // Deliver based on whether it's a remote or local repo
    let (success, message, is_remote) = if let Some(ref url) = remote_url {
        // Push to remote repository
        let (success, message) = git::push_to_remote(&work_dir, url, &branch_name)?;
        (success, message, true)
    } else {
        // Check if branch already exists in local project repo
        if git::branch_exists(&branch_name, Some(&project_path))? {
            if json {
                let output = serde_json::json!({
                    "success": false,
                    "error": "branch_exists",
                    "branch": branch_name,
                    "message": format!("Branch '{}' already exists", branch_name),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!(
                    "Branch '{}' already exists in project repository",
                    branch_name
                );
                eprintln!("Use --branch to specify a different name, or delete the existing branch first.");
            }
            return Ok(());
        }

        // Push staging as branch to local repo
        let (success, message) =
            git::push_staging_as_branch(&work_dir, &project_path, &branch_name)?;
        (success, message, false)
    };

    if !success {
        if json {
            let output = serde_json::json!({
                "success": false,
                "error": "push_failed",
                "message": message,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("Failed to deliver: {}", message);
        }
        return Ok(());
    }

    // Update run status to delivered
    state.set_status(Status::Delivered)?;

    // Trigger auto-improve if enabled
    if let Ok(lifecycle) = LocalLifecycleManager::new(run_name, run_dir.clone(), vec![]) {
        lifecycle.run_improve();
    }

    // Output result
    if json {
        let mut output = serde_json::json!({
            "success": true,
            "branch": branch_name,
            "message": format!("Delivered to branch '{}'", branch_name),
        });
        if is_remote {
            output["remote_url"] = serde_json::Value::String(remote_url.unwrap_or_default());
        } else {
            output["project_path"] = serde_json::Value::String(project_path.display().to_string());
        }
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        if is_remote {
            let url = remote_url.as_deref().unwrap_or("remote");
            println!("Delivered to branch '{}' on {}", branch_name, url);
            println!();
            println!("The branch has been pushed to the remote repository.");
            println!("Create a pull request to merge the changes.");
        } else {
            println!(
                "Delivered to branch '{}' in {}",
                branch_name,
                project_path.display()
            );
            println!();
            println!("To review: git checkout {}", branch_name);
            println!("To merge:  git checkout main && git merge {}", branch_name);
        }
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
