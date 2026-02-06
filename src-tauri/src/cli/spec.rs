//! Spec management commands.
//!
//! Provides functionality for viewing and managing run specifications.

use std::path::Path;

use crate::core::Files;

/// Execute the `hirsel spec <run_name>` command.
///
/// Prints the path to the spec.md file for the given run.
/// This is useful for scripting and integration with other tools.
pub fn run_spec(run_dir: &Path) -> Result<String, SpecError> {
    let files = Files::new(run_dir);
    let spec_path = files.spec();

    if !spec_path.exists() {
        return Err(SpecError::NotFound(run_dir.to_path_buf()));
    }

    Ok(spec_path.to_string_lossy().to_string())
}

/// Read the spec content for a run.
pub fn read_spec(run_dir: &Path) -> Result<String, SpecError> {
    let files = Files::new(run_dir);
    let spec_path = files.spec();

    if !spec_path.exists() {
        return Err(SpecError::NotFound(run_dir.to_path_buf()));
    }

    std::fs::read_to_string(&spec_path).map_err(|e| SpecError::ReadError(e.to_string()))
}

/// Errors that can occur during spec operations.
#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("No spec.md found for run at '{0}'")]
    NotFound(std::path::PathBuf),

    #[error("Failed to read spec: {0}")]
    ReadError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_run_spec_not_found() {
        let tmp = TempDir::new().unwrap();
        let result = run_spec(tmp.path());
        assert!(matches!(result, Err(SpecError::NotFound(_))));
    }

    #[test]
    fn test_run_spec_success() {
        let tmp = TempDir::new().unwrap();
        let spec_path = tmp.path().join("spec.md");
        std::fs::write(&spec_path, "# My Spec\n\nDo the thing.").unwrap();

        let result = run_spec(tmp.path()).unwrap();
        assert!(result.ends_with("spec.md"));
    }

    #[test]
    fn test_read_spec() {
        let tmp = TempDir::new().unwrap();
        let spec_path = tmp.path().join("spec.md");
        std::fs::write(&spec_path, "# My Spec\n\nDo the thing.").unwrap();

        let content = read_spec(tmp.path()).unwrap();
        assert!(content.contains("My Spec"));
        assert!(content.contains("Do the thing"));
    }
}
