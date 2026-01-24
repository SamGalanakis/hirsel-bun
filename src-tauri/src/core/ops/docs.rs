//! Documentation lifecycle management for hirsel runs.
//!
//! This module handles copying project documentation to the run directory at run start,
//! hiding it from git during the run, and optionally persisting changes back to the
//! workspace on delivery.
//!
//! ## Flow
//!
//! 1. **Run start**: Copy `workspace/docs_path/` to `run_dir/docs/`, mark as skip-worktree,
//!    delete from workspace
//! 2. **During run**: Scribe works on `run_dir/docs/`, workers read via `read_docs` MCP tool
//! 3. **On delivery (persist=true)**: Copy `run_dir/docs/` back to `workspace/docs_path/`, commit
//! 4. **On delivery (persist=false)**: Restore original from git

use std::path::Path;
use std::process::Command;

use super::OpsError;

/// Configuration for setting up docs at run start
pub struct DocsSetupConfig<'a> {
    /// Path to the workspace directory (where the project lives)
    pub workspace_dir: &'a Path,
    /// Path to the run directory (~/.hirsel/runs/<run_name>)
    pub run_dir: &'a Path,
    /// Relative path to docs directory within workspace (e.g., "docs")
    pub docs_path: &'a str,
}

/// Configuration for delivering docs at run end
pub struct DocsDeliveryConfig<'a> {
    /// Path to the workspace directory (where the project lives)
    pub workspace_dir: &'a Path,
    /// Path to the run directory (~/.hirsel/runs/<run_name>)
    pub run_dir: &'a Path,
    /// Relative path to docs directory within workspace (e.g., "docs")
    pub docs_path: &'a str,
    /// Whether to persist scribe changes back to workspace
    pub persist: bool,
}

/// Set up docs for a run: copy to run_dir, hide from git, delete from workspace.
///
/// This function:
/// 1. Copies the workspace docs to the run directory
/// 2. Marks the docs as skip-worktree in git (so they appear unchanged)
/// 3. Deletes the docs from the workspace (scribe works on run_dir copy)
///
/// If the docs directory doesn't exist in the workspace, this is a no-op.
pub fn setup_docs(config: &DocsSetupConfig) -> Result<(), OpsError> {
    let workspace_docs = config.workspace_dir.join(config.docs_path);
    let run_docs = config.run_dir.join("docs");

    // If no docs in workspace, nothing to do - init_docs() will create defaults
    if !workspace_docs.exists() {
        tracing::debug!(
            "No docs directory at {}, skipping docs setup",
            workspace_docs.display()
        );
        return Ok(());
    }

    // Copy workspace docs to run_dir/docs/
    tracing::info!(
        "Copying docs from {} to {}",
        workspace_docs.display(),
        run_docs.display()
    );
    copy_dir_recursive(&workspace_docs, &run_docs)?;

    // Mark docs as skip-worktree so git ignores the deletion
    if let Err(e) = hide_docs_from_git(config.workspace_dir, config.docs_path) {
        tracing::warn!("Failed to mark docs as skip-worktree: {}", e);
        // Continue anyway - not fatal
    }

    // Delete docs from workspace (scribe works on run_dir copy)
    if let Err(e) = std::fs::remove_dir_all(&workspace_docs) {
        tracing::warn!("Failed to remove workspace docs: {}", e);
        // Continue anyway - not fatal
    }

    Ok(())
}

/// Deliver docs at run end: restore or persist based on config.
///
/// If `persist` is true:
/// - Copies run_dir/docs/ back to workspace/docs_path/
/// - Clears skip-worktree flag
/// - Commits the changes
///
/// If `persist` is false:
/// - Restores original docs from git
/// - Clears skip-worktree flag
pub fn deliver_docs(config: &DocsDeliveryConfig) -> Result<(), OpsError> {
    let workspace_docs = config.workspace_dir.join(config.docs_path);
    let run_docs = config.run_dir.join("docs");

    if config.persist {
        // Persist scribe changes: copy from run_dir back to workspace
        tracing::info!(
            "Persisting docs from {} to {}",
            run_docs.display(),
            workspace_docs.display()
        );

        // Ensure parent directory exists
        if let Some(parent) = workspace_docs.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Remove old docs dir if it exists (it shouldn't, we deleted it)
        if workspace_docs.exists() {
            std::fs::remove_dir_all(&workspace_docs)?;
        }

        // Copy run docs to workspace
        if run_docs.exists() {
            copy_dir_recursive(&run_docs, &workspace_docs)?;
        }

        // Clear skip-worktree flag
        if let Err(e) = clear_skip_worktree(config.workspace_dir, config.docs_path) {
            tracing::warn!("Failed to clear skip-worktree: {}", e);
        }

        // Stage and commit the docs changes
        if let Err(e) = commit_docs_changes(config.workspace_dir, config.docs_path) {
            tracing::warn!("Failed to commit docs changes: {}", e);
        }
    } else {
        // Restore original docs from git
        tracing::info!(
            "Restoring original docs from git at {}",
            workspace_docs.display()
        );

        // Checkout original docs from git
        if let Err(e) = restore_docs_from_git(config.workspace_dir, config.docs_path) {
            tracing::warn!("Failed to restore docs from git: {}", e);
        }

        // Clear skip-worktree flag
        if let Err(e) = clear_skip_worktree(config.workspace_dir, config.docs_path) {
            tracing::warn!("Failed to clear skip-worktree: {}", e);
        }
    }

    Ok(())
}

/// Mark docs as skip-worktree so git ignores the deletion.
fn hide_docs_from_git(workspace_dir: &Path, docs_path: &str) -> Result<(), OpsError> {
    // Get list of tracked files in docs path
    let output = Command::new("git")
        .args(["ls-files", docs_path])
        .current_dir(workspace_dir)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let files = String::from_utf8_lossy(&output.stdout);
    let file_list: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();

    if file_list.is_empty() {
        return Ok(());
    }

    // Mark each file as skip-worktree
    let mut args = vec!["update-index", "--skip-worktree"];
    args.extend(file_list.iter());

    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_dir)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Clear skip-worktree flag on docs files.
fn clear_skip_worktree(workspace_dir: &Path, docs_path: &str) -> Result<(), OpsError> {
    // Get list of tracked files in docs path
    let output = Command::new("git")
        .args(["ls-files", docs_path])
        .current_dir(workspace_dir)
        .output()?;

    if !output.status.success() {
        // Not an error - docs might not be tracked
        return Ok(());
    }

    let files = String::from_utf8_lossy(&output.stdout);
    let file_list: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();

    if file_list.is_empty() {
        return Ok(());
    }

    // Clear skip-worktree flag on each file
    let mut args = vec!["update-index", "--no-skip-worktree"];
    args.extend(file_list.iter());

    let output = Command::new("git")
        .args(&args)
        .current_dir(workspace_dir)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Restore docs from git (checkout original version).
fn restore_docs_from_git(workspace_dir: &Path, docs_path: &str) -> Result<(), OpsError> {
    let output = Command::new("git")
        .args(["checkout", "--", docs_path])
        .current_dir(workspace_dir)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

/// Commit docs changes.
fn commit_docs_changes(workspace_dir: &Path, docs_path: &str) -> Result<(), OpsError> {
    // Check if there are any changes to commit
    let output = Command::new("git")
        .args(["diff", "--cached", "--quiet", docs_path])
        .current_dir(workspace_dir)
        .output()?;

    // If diff returns 0, there are no staged changes
    // But we also need to check for unstaged changes
    let has_staged_changes = !output.status.success();

    // Check for unstaged changes
    let output = Command::new("git")
        .args(["diff", "--quiet", docs_path])
        .current_dir(workspace_dir)
        .output()?;

    let has_unstaged_changes = !output.status.success();

    // Check for untracked files
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", docs_path])
        .current_dir(workspace_dir)
        .output()?;

    let has_untracked = !String::from_utf8_lossy(&output.stdout).trim().is_empty();

    if !has_staged_changes && !has_unstaged_changes && !has_untracked {
        tracing::debug!("No docs changes to commit");
        return Ok(());
    }

    // Stage docs changes
    let output = Command::new("git")
        .args(["add", docs_path])
        .current_dir(workspace_dir)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    // Check again if there are staged changes after add
    let output = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(workspace_dir)
        .output()?;

    if output.status.success() {
        tracing::debug!("No staged changes after git add");
        return Ok(());
    }

    // Commit
    let output = Command::new("git")
        .args(["commit", "-m", "docs: update from hirsel run"])
        .current_dir(workspace_dir)
        .output()?;

    if !output.status.success() {
        return Err(OpsError::Git(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    tracing::info!("Committed docs changes");
    Ok(())
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), OpsError> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_git_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .expect("git init failed");

        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .expect("git config failed");

        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .expect("git config failed");
    }

    #[test]
    fn test_copy_dir_recursive() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        // Create source structure
        std::fs::create_dir_all(src.join("subdir")).unwrap();
        std::fs::write(src.join("file1.txt"), "content1").unwrap();
        std::fs::write(src.join("subdir/file2.txt"), "content2").unwrap();

        // Copy
        copy_dir_recursive(&src, &dst).unwrap();

        // Verify
        assert!(dst.join("file1.txt").exists());
        assert!(dst.join("subdir/file2.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dst.join("file1.txt")).unwrap(),
            "content1"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("subdir/file2.txt")).unwrap(),
            "content2"
        );
    }

    #[test]
    fn test_setup_docs_no_docs() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        let run_dir = tmp.path().join("run");

        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&run_dir).unwrap();

        let config = DocsSetupConfig {
            workspace_dir: &workspace,
            run_dir: &run_dir,
            docs_path: "docs",
        };

        // Should succeed even when no docs exist
        setup_docs(&config).unwrap();

        // Run dir docs shouldn't exist (no source to copy from)
        assert!(!run_dir.join("docs").exists());
    }

    #[test]
    fn test_setup_docs_with_docs() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        let run_dir = tmp.path().join("run");

        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&run_dir).unwrap();

        // Create workspace with docs
        std::fs::create_dir_all(workspace.join("docs")).unwrap();
        std::fs::write(workspace.join("docs/readme.md"), "# Docs").unwrap();

        // Initialize git repo
        setup_git_repo(&workspace);

        // Add and commit docs
        Command::new("git")
            .args(["add", "docs"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add docs"])
            .current_dir(&workspace)
            .output()
            .unwrap();

        let config = DocsSetupConfig {
            workspace_dir: &workspace,
            run_dir: &run_dir,
            docs_path: "docs",
        };

        setup_docs(&config).unwrap();

        // Docs should be copied to run_dir
        assert!(run_dir.join("docs/readme.md").exists());
        assert_eq!(
            std::fs::read_to_string(run_dir.join("docs/readme.md")).unwrap(),
            "# Docs"
        );

        // Docs should be removed from workspace
        assert!(!workspace.join("docs").exists());
    }
}
