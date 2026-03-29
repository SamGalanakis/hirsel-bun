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

/// Get the runtime workspace directory (~/.hirsel/runtimes).
pub fn runtimes_dir() -> PathBuf {
    hirsel_dir().join("runtimes")
}

/// Get the path to a specific runtime workspace.
pub fn runtime_dir(runtime_name: &str) -> PathBuf {
    runtimes_dir().join(runtime_name)
}

/// Check if a runtime workspace exists.
pub fn runtime_exists(runtime_name: &str) -> bool {
    runtime_dir(runtime_name).exists()
}

/// Get the path to the global hirsel database (~/.hirsel/hirsel.db)
/// This stores global data like Shepherd chat history that shouldn't be in route runtime DBs.
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
