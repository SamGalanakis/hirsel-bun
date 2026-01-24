//! Daemon module for persistent local hirsel server
//!
//! The daemon runs as a background process that handles all run lifecycle:
//! - Worker spawning and management
//! - Eval triggering when workers become inactive
//! - Time limit enforcement
//! - Compaction of learnings
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
