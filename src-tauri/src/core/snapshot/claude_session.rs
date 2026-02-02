//! Claude session path utilities.
//!
//! Helper functions for locating Claude's session directory.

use std::path::{Path, PathBuf};

use crate::core::constants::CLAUDE_SESSION_DIR;

/// Get the path where Claude stores its session data.
///
/// For local: `~/.claude`
/// For Docker container: `/tmp/home/.claude`
pub fn claude_session_dir(home_override: Option<&Path>) -> PathBuf {
    if let Some(home) = home_override {
        home.join(CLAUDE_SESSION_DIR)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp/home"))
            .join(CLAUDE_SESSION_DIR)
    }
}

/// Get the host-side path for a worker's session mount.
///
/// This is where Docker containers should mount from.
pub fn host_session_path(run_dir: &Path, worker_name: &str) -> PathBuf {
    run_dir.join("agent-sessions").join(worker_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_dir() {
        let dir = claude_session_dir(None);
        assert!(dir.to_string_lossy().ends_with(".claude"));

        let custom = claude_session_dir(Some(Path::new("/custom/home")));
        assert_eq!(custom, PathBuf::from("/custom/home/.claude"));
    }

    #[test]
    fn test_host_session_path() {
        let path = host_session_path(Path::new("/runs/my-run"), "worker-1");
        assert_eq!(path, PathBuf::from("/runs/my-run/agent-sessions/worker-1"));
    }
}
