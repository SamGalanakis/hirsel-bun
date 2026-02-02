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
    pub binary_mtime: Option<u64>,
}

/// Check if a PID belongs to a hirsel process (Unix only)
///
/// Returns true if we can confirm it's a hirsel process, or if we can't determine
/// (non-Unix platforms, permission issues). Returns false only if we can confirm
/// it's NOT a hirsel process.
#[cfg(unix)]
fn is_hirsel_process(pid: u32) -> bool {
    // Check /proc/PID/exe symlink to see what binary the process is running
    let proc_exe = format!("/proc/{}/exe", pid);
    match std::fs::read_link(&proc_exe) {
        Ok(exe_path) => {
            let exe_str = exe_path.to_string_lossy();
            // Check if the executable name contains "hirsel"
            exe_str.contains("hirsel")
        }
        Err(e) => {
            // ENOENT means process doesn't exist
            // EACCES means we can't read it (different user) - assume it could be hirsel
            // Other errors - be conservative and assume it could be hirsel
            if e.kind() == std::io::ErrorKind::NotFound {
                false // Process doesn't exist
            } else {
                true // Can't determine, assume it might be hirsel
            }
        }
    }
}

#[cfg(not(unix))]
fn is_hirsel_process(_pid: u32) -> bool {
    // On non-Unix, we can't verify - assume it could be hirsel
    true
}

/// Check if a PID is alive (process exists)
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    // kill(pid, 0) checks if process exists without sending a signal
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn is_process_alive(_pid: u32) -> bool {
    // On non-Unix, we can't check - assume it's alive
    true
}

/// Clean up stale PID file if the process is dead or not hirsel
fn cleanup_stale_pid_file() {
    let pid_file = pid_path();
    if pid_file.exists() {
        if let Some(info) = read_daemon_info_raw() {
            if !is_process_alive(info.pid) || !is_hirsel_process(info.pid) {
                tracing::debug!(
                    "Cleaning up stale PID file (pid={}, alive={}, hirsel={})",
                    info.pid,
                    is_process_alive(info.pid),
                    is_hirsel_process(info.pid)
                );
                let _ = std::fs::remove_file(&pid_file);
            }
        }
    }
}

/// Read daemon info from PID file without validation (internal use)
fn read_daemon_info_raw() -> Option<DaemonInfo> {
    let content = std::fs::read_to_string(pid_path()).ok()?;
    let mut lines = content.lines();
    let pid: u32 = lines.next()?.parse().ok()?;
    let binary_path = lines.next().map(String::from);
    let binary_mtime = lines.next().and_then(|s| s.parse().ok());
    Some(DaemonInfo {
        pid,
        binary_path,
        binary_mtime,
    })
}

/// Read daemon info from PID file, cleaning up stale files
///
/// Returns None if no valid PID file exists or if the PID belongs to a
/// dead/non-hirsel process (in which case the stale file is removed).
pub fn read_daemon_info() -> Option<DaemonInfo> {
    let info = read_daemon_info_raw()?;

    // Validate that the PID is alive and belongs to hirsel
    if !is_process_alive(info.pid) {
        tracing::debug!("PID {} from PID file is not alive, cleaning up", info.pid);
        let _ = std::fs::remove_file(pid_path());
        return None;
    }

    if !is_hirsel_process(info.pid) {
        tracing::warn!(
            "PID {} from PID file is not a hirsel process, cleaning up stale PID file",
            info.pid
        );
        let _ = std::fs::remove_file(pid_path());
        return None;
    }

    Some(info)
}

/// Check if the running daemon's binary matches the current binary
pub fn is_daemon_binary_current() -> bool {
    let Some(info) = read_daemon_info() else {
        return true; // No valid PID file, assume current
    };
    let Some(daemon_binary) = info.binary_path else {
        return true; // Old format without binary path, assume current
    };
    let Ok(current_binary) = std::env::current_exe() else {
        return true; // Can't determine current binary, assume current
    };

    // Path must match
    if daemon_binary != current_binary.display().to_string() {
        return false;
    }

    // If we have stored mtime, verify file hasn't changed (catches same-path rebuilds)
    if let Some(stored_mtime) = info.binary_mtime {
        let current_mtime = std::fs::metadata(&current_binary)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if stored_mtime != current_mtime {
            return false; // Binary was rebuilt
        }
    }

    true
}

/// Kill the daemon process by PID
///
/// Only kills if we can verify the PID belongs to a hirsel process.
/// Cleans up stale PID files if the process is dead or not hirsel.
pub fn kill_daemon() -> bool {
    // Clean up any stale PID file first
    cleanup_stale_pid_file();

    let Some(info) = read_daemon_info() else {
        return false; // No valid daemon to kill
    };

    #[cfg(unix)]
    {
        use std::process::Command;

        // Double-check it's a hirsel process before killing
        if !is_hirsel_process(info.pid) {
            tracing::warn!("Refusing to kill PID {} - not a hirsel process", info.pid);
            let _ = std::fs::remove_file(pid_path());
            return false;
        }

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
