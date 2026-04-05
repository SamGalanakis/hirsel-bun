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

/// Get the project workspace directory (~/.hirsel/workspaces).
pub fn workspaces_dir() -> PathBuf {
    hirsel_dir().join("workspaces")
}

/// Get the path to a specific project workspace.
pub fn workspace_dir(workspace_name: &str) -> PathBuf {
    workspaces_dir().join(workspace_name)
}

/// Get the path to the global Hirsel database directory (~/.hirsel/hirsel.surrealkv)
/// This stores shared Hirsel state such as projects, chat history, and thread metadata.
pub fn global_db_path() -> PathBuf {
    hirsel_dir().join("hirsel.surrealkv")
}

/// Get the assets directory for a project (~/.hirsel/projects/{project_id}/assets)
pub fn project_assets_dir(project_id: i64) -> PathBuf {
    hirsel_dir()
        .join("projects")
        .join(project_id.to_string())
        .join("assets")
}
