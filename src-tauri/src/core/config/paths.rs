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

/// Get all run names by listing the runs directory
pub fn list_runs() -> std::io::Result<Vec<String>> {
    let runs = runs_dir();
    if !runs.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in std::fs::read_dir(runs)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                // Skip hidden directories
                if !name.starts_with('.') {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Check if a run exists
pub fn run_exists(run_name: &str) -> bool {
    run_dir(run_name).exists()
}

/// Get the path to the global hirsel database (~/.hirsel/hirsel.db)
/// This stores global data like Gyp chat history that shouldn't be in run DBs
pub fn global_db_path() -> PathBuf {
    hirsel_dir().join("hirsel.db")
}
