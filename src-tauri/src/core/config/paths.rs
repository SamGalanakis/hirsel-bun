//! Convenience path functions for quick access without loading full config.

use std::path::PathBuf;

/// Get the hirsel home directory.
///
/// Checks `HIRSEL_ROOT` env var first, falls back to `~/.hirsel`.
pub fn hirsel_dir() -> PathBuf {
    if let Ok(root) = std::env::var("HIRSEL_ROOT") {
        return PathBuf::from(root);
    }
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".hirsel")
}

/// Get the runs directory (~/.hirsel/runs)
pub fn runs_dir() -> PathBuf {
    hirsel_dir().join("runs")
}

/// Get the path to a specific run
pub fn run_dir(run_name: &str) -> PathBuf {
    runs_dir().join(run_name)
}

/// Check if a run exists
pub fn run_exists(run_name: &str) -> bool {
    run_dir(run_name).exists()
}

/// Get the path to the global hirsel database (~/.hirsel/hirsel.db)
/// This stores global data like Shepherd chat history that shouldn't be in run DBs
pub fn global_db_path() -> PathBuf {
    hirsel_dir().join("hirsel.db")
}

/// Get the assets directory for a project (~/.hirsel/projects/{project_id}/assets)
pub fn project_assets_dir(project_id: i64) -> PathBuf {
    hirsel_dir()
        .join("projects")
        .join(project_id.to_string())
        .join("assets")
}
