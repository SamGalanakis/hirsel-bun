//! Deliver command - create branch in target repo
//!
//! Delivers the work from a hirsel run by creating a new branch
//! in the original project repository. This allows easy review
//! and merging of the work.

use crate::core::{config, git, state::SQLiteState, state::Status, Files};

/// Execute the deliver command for a run
pub fn execute(run_name: &str, branch: Option<&str>, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Check run exists
    if !config::run_exists(run_name) {
        return Err(format!("Run '{}' not found", run_name).into());
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Get project path
    let project_path_str = state.get_project_path()?;
    let project_path = project_path_str
        .as_ref()
        .map(|s| std::path::PathBuf::from(s));

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
    let branch_name = branch.unwrap_or(&format!("hirsel/{}", run_name)).to_string();

    // Check if branch already exists in project repo
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
            eprintln!("Branch '{}' already exists in project repository", branch_name);
            eprintln!("Use --branch to specify a different name, or delete the existing branch first.");
        }
        return Ok(());
    }

    // Push staging as branch
    let (success, message) = git::push_staging_as_branch(&work_dir, &project_path, &branch_name)?;

    if !success {
        if json {
            let output = serde_json::json!({
                "success": false,
                "error": "push_failed",
                "message": message,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("Failed to create branch: {}", message);
        }
        return Ok(());
    }

    // Update run status to delivered
    state.set_status(Status::Delivered)?;

    // Output result
    if json {
        let output = serde_json::json!({
            "success": true,
            "branch": branch_name,
            "project_path": project_path.display().to_string(),
            "message": format!("Delivered to branch '{}'", branch_name),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Delivered to branch '{}' in {}", branch_name, project_path.display());
        println!();
        println!("To review: git checkout {}", branch_name);
        println!("To merge:  git checkout main && git merge {}", branch_name);
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
