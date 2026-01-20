//! Daemon module for persistent local hirsel server
//!
//! The daemon runs as a background process that handles all run lifecycle:
//! - Worker spawning and management
//! - Eval triggering when workers become inactive
//! - Time limit enforcement
//! - Compaction of learnings
//!
//! The daemon listens on:
//! - Unix socket (`~/.hirsel/hirsel.sock`) - for local CLI/GUI
//! - TCP (`localhost:19700`) - for SSH reverse tunnels

mod client;
mod lifecycle;
mod server;

pub use client::DaemonClient;
pub use server::{start_daemon, DaemonConfig, DEFAULT_TCP_PORT};

use std::path::PathBuf;

/// Get the path to the daemon socket
pub fn socket_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".hirsel")
        .join("hirsel.sock")
}

/// Get the path to the daemon PID file
pub fn pid_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".hirsel")
        .join("hirsel.pid")
}

/// Check if the daemon is running by testing socket connectivity
pub fn is_daemon_running() -> bool {
    use std::os::unix::net::UnixStream;

    let sock_path = socket_path();
    if !sock_path.exists() {
        return false;
    }

    // Try to connect to the socket
    UnixStream::connect(&sock_path).is_ok()
}
