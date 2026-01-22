//! Gyp context resolution
//!
//! Resolves the correct working directory for Gyp based on workspace configuration.
//! Gyp runs next to the orchestrator - local orchestrator = local Gyp, remote = remote Gyp.

use std::path::PathBuf;

use crate::core::draft::WorkspaceProvider;

/// Context for Gyp execution
///
/// Contains the resolved working directory and optional run information
/// for configuring Gyp sessions.
#[derive(Debug, Clone)]
pub struct GypContext {
    /// The working directory for Gyp to operate in
    pub working_dir: PathBuf,
    /// The run name if associated with a run
    pub run_name: Option<String>,
}

impl GypContext {
    /// Create a GypContext for a specific run
    ///
    /// Uses the workspace provider to resolve the correct working directory.
    /// For local workspaces, this is the filesystem path.
    /// For S3 workspaces, this would be the S3 prefix (though Gyp would need
    /// to handle S3 paths differently in that case).
    ///
    /// # Arguments
    ///
    /// * `run_name` - The name of the run
    /// * `workspace` - The workspace provider to use for path resolution
    pub fn for_run(run_name: &str, workspace: &dyn WorkspaceProvider) -> Self {
        Self {
            working_dir: workspace.workspace_path(run_name),
            run_name: Some(run_name.to_string()),
        }
    }

    /// Create a GypContext without a run (general chat mode)
    ///
    /// Uses the current working directory as the working directory for Gyp.
    pub fn without_run() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_default(),
            run_name: None,
        }
    }

    /// Create a GypContext with an explicit working directory
    ///
    /// Useful when you already know the working directory and don't need
    /// to resolve it from a workspace provider.
    pub fn with_working_dir(working_dir: PathBuf, run_name: Option<String>) -> Self {
        Self {
            working_dir,
            run_name,
        }
    }

    /// Get the working directory as a string
    pub fn working_dir_string(&self) -> String {
        self.working_dir.to_string_lossy().to_string()
    }

    /// Check if this context is associated with a run
    pub fn has_run(&self) -> bool {
        self.run_name.is_some()
    }
}

impl Default for GypContext {
    fn default() -> Self {
        Self::without_run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::draft::LocalWorkspaceProvider;
    use tempfile::TempDir;

    #[test]
    fn test_without_run() {
        let ctx = GypContext::without_run();
        assert!(ctx.run_name.is_none());
        assert!(!ctx.has_run());
    }

    #[test]
    fn test_for_run() {
        let temp = TempDir::new().unwrap();
        let provider = LocalWorkspaceProvider::with_base_dir(temp.path().to_path_buf());

        let ctx = GypContext::for_run("test-run", &provider);

        assert_eq!(ctx.run_name, Some("test-run".to_string()));
        assert!(ctx.has_run());
        assert!(ctx.working_dir.to_string_lossy().contains("test-run"));
        assert!(ctx.working_dir.to_string_lossy().contains("workspace"));
    }

    #[test]
    fn test_with_working_dir() {
        let dir = PathBuf::from("/tmp/test");
        let ctx = GypContext::with_working_dir(dir.clone(), Some("my-run".to_string()));

        assert_eq!(ctx.working_dir, dir);
        assert_eq!(ctx.run_name, Some("my-run".to_string()));
    }
}
