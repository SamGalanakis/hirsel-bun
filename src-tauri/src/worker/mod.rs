//! Worker subprocess implementation for hirsel.
//!
//! This module implements the worker subprocess that runs inside a tmux session
//! and manages an AI coding agent through the Agent Control Protocol (ACP).
//!
//! ## Worker Lifecycle
//!
//! 1. Worker starts via `hirsel-worker` binary
//! 2. Connects to the run's SQLite database
//! 3. Spawns the AI agent process
//! 4. Sends the initial prompt (spec + tasks)
//! 5. Enters main loop handling:
//!    - Agent responses and tool calls
//!    - Task claim/done/unclaim requests
//!    - Message sending/receiving
//!    - Heartbeat updates
//! 6. Signals completion via work_done

pub mod mcp;
pub mod msg;
pub mod runner;

pub use mcp::{McpServer, run_mcp_server};
pub use msg::{execute_inbox, execute_list, execute_read, execute_send};
pub use msg::{inbox, list, read, send, MsgError, MsgResult};
pub use runner::{WorkerConfig, WorkerError, WorkerRunner};
