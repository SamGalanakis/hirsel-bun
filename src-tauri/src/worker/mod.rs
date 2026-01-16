//! Worker subprocess implementation for hirsel.
//!
//! This module implements the worker subprocess that manages an AI coding agent
//! through the Agent Control Protocol (ACP).
//!
//! ## Worker Lifecycle
//!
//! 1. Worker starts via `hirsel worker run` command
//! 2. Connects to the run's SQLite database
//! 3. Spawns the AI agent process
//! 4. Sends the initial prompt (spec + tasks)
//! 5. Enters main loop handling:
//!    - Agent responses and tool calls
//!    - Task claim/done/unclaim requests
//!    - Message sending/receiving
//!    - Heartbeat updates
//! 6. Signals completion via work_done

pub mod acp_client;
pub mod eval_mcp;
pub mod http_state;
pub mod mcp;
pub mod msg;
pub mod remote_runner;
pub mod runner;

pub use acp_client::{run_acp_worker, WorkerRunConfig};
pub use eval_mcp::{run_eval_mcp_server, EvalMcpServer};
pub use mcp::{run_mcp_server, McpServer};
pub use msg::{execute_inbox, execute_list, execute_read, execute_send};
pub use msg::{inbox, list, read, send, MsgError, MsgResult};
pub use remote_runner::run_remote_worker;
pub use runner::{WorkerConfig, WorkerError, WorkerRunner};
