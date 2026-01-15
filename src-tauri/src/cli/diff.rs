//! CLI diff command - show code changes via git diff
//!
//! Shows the diff between the original project and the hirsel work directory,
//! highlighting what changes have been made during the run.

use std::path::PathBuf;

use crate::core::{git, Config, SQLiteState};

/// Result of running the diff command
#[derive(Debug, serde::Serialize)]
pub struct DiffResult {
    pub run_name: String,
    pub has_changes: bool,
    pub stat: Option<String>,
    pub diff: Option<String>,
}

/// Error type for diff operations
#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("Run '{0}' not found")]
    RunNotFound(String),

    #[error("No workspace found for run '{0}'")]
    NoWorkspace(String),

    #[error("Could not determine project path for run")]
    NoProjectPath,

    #[error("Git error: {0}")]
    GitError(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),
}

/// Run the diff command for a run
///
/// Shows the diff between the original project and the work directory.
/// By default shows the full diff; use `stat_only` for a summary.
pub fn run_diff(run_name: &str, stat_only: bool) -> Result<DiffResult, DiffError> {
    // Load config to get runs directory
    let (config, _warnings) = Config::load().map_err(|e| DiffError::ConfigError(e.to_string()))?;

    let run_dir = config.runs_dir().join(run_name);
    let staging_dir = run_dir.join("work").join("staging");
    let db_path = run_dir.join("hirsel.db");

    // Check if run exists
    if !run_dir.exists() {
        return Err(DiffError::RunNotFound(run_name.to_string()));
    }

    if !staging_dir.exists() {
        return Err(DiffError::NoWorkspace(run_name.to_string()));
    }

    // Get project path from state database
    let project_path = get_project_path(&db_path)?;

    let (stat, diff) = if stat_only {
        let stat = git::get_diff_stat(&project_path, &staging_dir)
            .map_err(|e| DiffError::GitError(e.to_string()))?;
        (stat, None)
    } else {
        let stat = git::get_diff_stat(&project_path, &staging_dir)
            .map_err(|e| DiffError::GitError(e.to_string()))?;
        let diff = git::get_diff(&project_path, &staging_dir)
            .map_err(|e| DiffError::GitError(e.to_string()))?;
        (stat, diff)
    };

    let has_changes = stat.is_some() || diff.is_some();

    Ok(DiffResult {
        run_name: run_name.to_string(),
        has_changes,
        stat,
        diff,
    })
}

/// Get the project path for a run from the database
fn get_project_path(db_path: &PathBuf) -> Result<PathBuf, DiffError> {
    if !db_path.exists() {
        return Err(DiffError::NoProjectPath);
    }

    let state =
        SQLiteState::new(db_path.clone()).map_err(|e| DiffError::DatabaseError(e.to_string()))?;

    let project_path = state
        .get_project_path()
        .map_err(|e| DiffError::DatabaseError(e.to_string()))?;

    project_path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .ok_or(DiffError::NoProjectPath)
}

/// Print diff result to stdout
pub fn print_diff(result: &DiffResult, json: bool) {
    if json {
        print_diff_json(result);
    } else {
        print_diff_text(result);
    }
}

fn print_diff_text(result: &DiffResult) {
    if !result.has_changes {
        println!("No changes in run '{}'", result.run_name);
        return;
    }

    // Print stat summary
    if let Some(ref stat) = result.stat {
        println!("{}", stat);
        println!();
    }

    // Print full diff if available
    if let Some(ref diff) = result.diff {
        println!("{}", diff);
    }
}

fn print_diff_json(result: &DiffResult) {
    let json = serde_json::json!({
        "run_name": result.run_name,
        "has_changes": result.has_changes,
        "stat": result.stat,
        "diff": result.diff,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_error_display() {
        let err = DiffError::RunNotFound("test-run".to_string());
        assert_eq!(err.to_string(), "Run 'test-run' not found");

        let err = DiffError::NoWorkspace("test-run".to_string());
        assert_eq!(err.to_string(), "No workspace found for run 'test-run'");
    }
}
