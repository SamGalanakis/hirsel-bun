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

// Re-export commonly used types
pub use acp::{ACPClientConfig, ACPError, MCPServerConfig, SessionUpdate};
pub use chats::{ChatHeader, ChatMode};
pub use config::*;
pub use files::Files;
