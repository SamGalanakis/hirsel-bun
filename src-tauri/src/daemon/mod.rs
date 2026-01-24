//! Daemon module for persistent local hirsel server
//!
//! The daemon runs as a background process that handles all run lifecycle:
//! - Worker spawning and management
//! - Eval triggering when workers become inactive
//! - Time limit enforcement
//! - Scribe batch processing
//!
//! The daemon listens on a TCP port (default 19700, configurable via HIRSEL_DAEMON_PORT).
//! Local CLI/GUI connects via localhost, remote workers via Docker host or SSH tunnels.

mod client;
mod lifecycle;
mod server;

pub use client::DaemonClient;
pub use server::{start_daemon, DaemonConfig, DEFAULT_TCP_PORT};

use crate::core::config::paths::hirsel_dir;
use std::path::PathBuf;

/// Environment variable for daemon port
pub const DAEMON_PORT_ENV: &str = "HIRSEL_DAEMON_PORT";

/// Get the configured daemon port (from env var or default)
pub fn get_daemon_port() -> u16 {
    std::env::var(DAEMON_PORT_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_TCP_PORT)
}

/// Get the path to the daemon PID file
pub fn pid_path() -> PathBuf {
    hirsel_dir().join("hirsel.pid")
}

/// Check if the daemon is running by testing TCP connectivity
pub fn is_daemon_running() -> bool {
    is_daemon_running_on_port(get_daemon_port())
}

/// Check if a daemon is running on a specific port
pub fn is_daemon_running_on_port(port: u16) -> bool {
    use std::net::TcpStream;
    use std::time::Duration;

    let addr = format!("127.0.0.1:{}", port);
    TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(100)).is_ok()
}

/// Daemon info from PID file
pub struct DaemonInfo {
    pub pid: u32,
    pub binary_path: Option<String>,
}

/// Read daemon info from PID file
pub fn read_daemon_info() -> Option<DaemonInfo> {
    let content = std::fs::read_to_string(pid_path()).ok()?;
    let mut lines = content.lines();
    let pid: u32 = lines.next()?.parse().ok()?;
    let binary_path = lines.next().map(String::from);
    Some(DaemonInfo { pid, binary_path })
}

/// Check if the running daemon's binary matches the current binary
pub fn is_daemon_binary_current() -> bool {
    let Some(info) = read_daemon_info() else {
        return true; // No PID file, assume current
    };
    let Some(daemon_binary) = info.binary_path else {
        return true; // Old format without binary path, assume current
    };
    let Ok(current_binary) = std::env::current_exe() else {
        return true; // Can't determine current binary, assume current
    };
    daemon_binary == current_binary.display().to_string()
}

/// Kill the daemon process by PID
pub fn kill_daemon() -> bool {
    let Some(info) = read_daemon_info() else {
        return false;
    };

    #[cfg(unix)]
    {
        use std::process::Command;
        if Command::new("kill")
            .args(["-TERM", &info.pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            // Wait a bit for graceful shutdown
            std::thread::sleep(std::time::Duration::from_millis(500));
            return true;
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix, just return false - manual restart required
        let _ = info;
    }

    false
}
