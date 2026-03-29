//! Project operations - validate and initialize project directories
//!
//! These operations are shared between CLI and GUI.

use std::fs;
use std::path::Path;
use std::process::Command;

use super::OpsError;

/// Initialize a git repository in the given directory
///
/// This will:
/// 1. Run `git init -b main`
/// 2. Add all existing files with `git add -A`
/// 3. Create an initial commit (allow-empty)
///
/// # Arguments
///
/// * `path` - The directory to initialize as a git repository
///
/// # Returns
///
/// * `Ok(())` - If initialization succeeded
/// * `Err(OpsError)` - If any git command failed
pub fn init_git_repo(path: &Path) -> Result<(), OpsError> {
    // git init -b main
    let output = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(format!(
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    // git add -A (add any existing files)
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(format!(
            "git add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    // git commit -m "Initial commit" --allow-empty
    let output = Command::new("git")
        .args(["commit", "-m", "Initial commit", "--allow-empty"])
        .current_dir(path)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(format!(
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(())
}

/// Check if a path is a git repository root (has .git directory)
pub fn is_git_repo_root(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Ensure a directory exists and is a git repository
///
/// This will:
/// 1. Create the directory if it doesn't exist
/// 2. Initialize git if not already a git repository
///
/// # Arguments
///
/// * `path` - The directory path to ensure exists as a git repo
///
/// # Returns
///
/// * `Ok(())` - If the directory now exists and is a git repo
/// * `Err(OpsError)` - If creation or initialization failed
pub fn ensure_project_directory(path: &Path) -> Result<(), OpsError> {
    // Create directory if it doesn't exist
    if !path.exists() {
        fs::create_dir_all(path)?;
    }

    // Initialize git if not already a repo
    if !is_git_repo_root(path) {
        init_git_repo(path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_git_repo_root_false() {
        let temp = TempDir::new().unwrap();
        assert!(!is_git_repo_root(temp.path()));
    }

    #[test]
    fn test_init_git_repo() {
        let temp = TempDir::new().unwrap();

        // Should succeed
        let result = init_git_repo(temp.path());
        assert!(result.is_ok());

        // Should now be a git repo
        assert!(is_git_repo_root(temp.path()));
    }

    #[test]
    fn test_ensure_project_directory_creates_dir() {
        let temp = TempDir::new().unwrap();
        let new_path = temp.path().join("new_project");

        assert!(!new_path.exists());

        let result = ensure_project_directory(&new_path);
        assert!(result.is_ok());

        assert!(new_path.exists());
        assert!(is_git_repo_root(&new_path));
    }
}
