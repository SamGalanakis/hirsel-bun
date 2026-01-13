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

/// Update the amendments footer in a spec file.
///
/// Amendments are appended to the spec with a special section that can be
/// updated without affecting the main spec content.
pub fn update_spec_amendments(
    run_dir: &Path,
    amendments: &[Amendment],
) -> Result<(), SpecError> {
    let files = Files::new(run_dir);
    let spec_path = files.spec();

    if !spec_path.exists() {
        return Err(SpecError::NotFound(run_dir.to_path_buf()));
    }

    let mut content = std::fs::read_to_string(&spec_path)
        .map_err(|e| SpecError::ReadError(e.to_string()))?;

    // Remove existing amendments section if present
    let marker = "\n---\n\n## Amendments\n";
    if let Some(idx) = content.find(marker) {
        content.truncate(idx);
    }

    // Build amendments section
    if !amendments.is_empty() {
        content.push_str(marker);
        for a in amendments {
            // Format timestamp as YYYY-MM-DD HH:MM
            let timestamp = &a.timestamp[..16].replace('T', " ");
            content.push_str(&format!("\n- **#{}** ({}): {}\n", a.id, timestamp, a.message));
        }
    }

    std::fs::write(&spec_path, content).map_err(|e| SpecError::WriteError(e.to_string()))?;

    Ok(())
}

/// An amendment to a spec.
#[derive(Debug, Clone)]
pub struct Amendment {
    pub id: i64,
    pub timestamp: String,
    pub message: String,
}

/// Errors that can occur during spec operations.
#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("No spec.md found for run at '{0}'")]
    NotFound(std::path::PathBuf),

    #[error("Failed to read spec: {0}")]
    ReadError(String),

    #[error("Failed to write spec: {0}")]
    WriteError(String),
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

    #[test]
    fn test_update_amendments_empty() {
        let tmp = TempDir::new().unwrap();
        let spec_path = tmp.path().join("spec.md");
        std::fs::write(&spec_path, "# My Spec\n\nDo the thing.").unwrap();

        update_spec_amendments(tmp.path(), &[]).unwrap();

        let content = std::fs::read_to_string(&spec_path).unwrap();
        assert!(!content.contains("## Amendments"));
    }

    #[test]
    fn test_update_amendments() {
        let tmp = TempDir::new().unwrap();
        let spec_path = tmp.path().join("spec.md");
        std::fs::write(&spec_path, "# My Spec\n\nDo the thing.").unwrap();

        let amendments = vec![
            Amendment {
                id: 1,
                timestamp: "2024-01-15T10:30:00".to_string(),
                message: "Added error handling".to_string(),
            },
            Amendment {
                id: 2,
                timestamp: "2024-01-15T11:00:00".to_string(),
                message: "Fixed typo".to_string(),
            },
        ];

        update_spec_amendments(tmp.path(), &amendments).unwrap();

        let content = std::fs::read_to_string(&spec_path).unwrap();
        assert!(content.contains("## Amendments"));
        assert!(content.contains("**#1** (2024-01-15 10:30)"));
        assert!(content.contains("Added error handling"));
        assert!(content.contains("**#2** (2024-01-15 11:00)"));
        assert!(content.contains("Fixed typo"));
    }

    #[test]
    fn test_update_amendments_replaces_existing() {
        let tmp = TempDir::new().unwrap();
        let spec_path = tmp.path().join("spec.md");
        let initial_content = r#"# My Spec

Do the thing.

---

## Amendments

- **#1** (2024-01-14 09:00): Old amendment
"#;
        std::fs::write(&spec_path, initial_content).unwrap();

        let amendments = vec![Amendment {
            id: 2,
            timestamp: "2024-01-15T10:30:00".to_string(),
            message: "New amendment".to_string(),
        }];

        update_spec_amendments(tmp.path(), &amendments).unwrap();

        let content = std::fs::read_to_string(&spec_path).unwrap();
        assert!(!content.contains("Old amendment"));
        assert!(content.contains("New amendment"));
        assert!(content.contains("**#2**"));
    }
}
