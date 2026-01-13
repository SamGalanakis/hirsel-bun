//! Core modules for hirsel - shared between CLI and GUI
//!
//! These modules implement the business logic for hirsel, including:
//! - State management (SQLite)
//! - Configuration
//! - Git operations
//! - Chat/messaging system
//! - File utilities
//! - ACP client

pub mod acp;
pub mod chats;
pub mod config;
pub mod files;
pub mod git;
pub mod state;
pub mod workers;

// Re-export commonly used types
pub use acp::{ACPClientConfig, ACPError, MCPServerConfig, SessionUpdate};
pub use chats::{ChatHeader, ChatMode};
pub use config::*;
pub use files::Files;
pub use state::*;
pub use workers::{
    spawn_worker, pause_all_workers, resume_awaiting_workers, check_worker_heartbeats,
    update_worker_heartbeat, get_agent_command, is_pid_alive,
    WorkerError, WorkerResult, WorkerSpawnConfig, SpawnResult,
};
