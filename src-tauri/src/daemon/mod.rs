//! Daemon module for persistent local hirsel server
//!
//! The daemon runs as a background process that handles all run lifecycle:
//! - Worker spawning and management
//! - Eval triggering when workers become inactive
//! - Time limit enforcement
//! - Compaction of learnings
//!
//! The daemon listens on TCP port 19700 (configurable).
//! Local CLI/GUI connects via localhost, remote workers via Docker
//! host or SSH tunnels.

mod client;
mod lifecycle;
mod server;

pub use client::DaemonClient;
pub use server::{start_daemon, DaemonConfig, DEFAULT_TCP_PORT};

use std::path::PathBuf;

/// Get the path to the daemon PID file
pub fn pid_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".hirsel")
        .join("hirsel.pid")
}

/// Check if the daemon is running by testing TCP connectivity
pub fn is_daemon_running() -> bool {
    use std::net::TcpStream;
    use std::time::Duration;

    // Try to connect to localhost on the daemon port
    let addr = format!("127.0.0.1:{}", DEFAULT_TCP_PORT);
    TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(100)).is_ok()
}
