//! Agent session path utilities.
//!
//! Helper functions for locating the agent session directory.

use std::path::{Path, PathBuf};

use crate::backend::constants::AGENT_SESSION_DIR;

/// Get the path where session data is stored.
///
/// For local: `~/.codex`
/// For Docker container: `/tmp/home/.codex`
pub fn agent_session_dir(home_override: Option<&Path>) -> PathBuf {
    if let Some(home) = home_override {
        home.join(AGENT_SESSION_DIR)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp/home"))
            .join(AGENT_SESSION_DIR)
    }
}

/// Get the host-side path for a worker's session mount.
///
/// This is where Docker containers should mount from.
pub fn host_session_path(runtime_dir: &Path, worker_name: &str) -> PathBuf {
    runtime_dir.join("agent-sessions").join(worker_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_dir() {
        let dir = agent_session_dir(None);
        assert!(dir.to_string_lossy().ends_with(".codex"));

        let custom = agent_session_dir(Some(Path::new("/custom/home")));
        assert_eq!(custom, PathBuf::from("/custom/home/.codex"));
    }

    #[test]
    fn test_host_session_path() {
        let path = host_session_path(Path::new("/runs/my-run"), "worker-1");
        assert_eq!(path, PathBuf::from("/runs/my-run/agent-sessions/worker-1"));
    }
}
