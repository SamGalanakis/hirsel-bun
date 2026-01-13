//! Worker subprocess commands for hirsel-worker.
//!
//! This module provides the implementation for commands that AI agents
//! use inside their worker tmux sessions. These commands interact with
//! the hirsel state through environment variables:
//!
//! - `HIRSEL_RUN_DIR` - Path to the run directory
//! - `HIRSEL_WORKER_NAME` - Name of this worker
//! - `HIRSEL_WORKER_SUBPROCESS` - Marker that we're running as a worker

pub mod msg;

// Re-export commonly used items
pub use msg::{execute_inbox, execute_list, execute_read, execute_send};
pub use msg::{inbox, list, read, send, MsgError, MsgResult};
