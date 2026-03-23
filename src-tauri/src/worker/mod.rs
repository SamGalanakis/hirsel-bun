//! Worker subprocess implementation for hirsel.
//!
//! This module implements the worker subprocess that manages an AI coding agent
//! through an embedded lash runtime.
//!
//! ## Worker Lifecycle
//!
//! 1. Worker starts via the hidden `hirsel __worker-runtime` command
//! 2. Connects to the route runtime's SQLite database
//! 3. Spawns the AI agent process
//! 4. Sends the initial prompt for its assigned work
//! 5. Enters main loop handling:
//!    - Agent responses and tool calls
//!    - Task claim/done/unclaim requests
//!    - Progress reports and concerns to the orchestrator
//!    - Heartbeat updates
//! 6. Signals completion via work_done

pub mod common;
pub mod eval_mcp;
pub mod lash_runner;
pub mod mcp;
pub mod runner;

pub use common::{build_worker_prompt, WorkerRunConfig};
pub use eval_mcp::{run_eval_mcp_server, EvalMcpServer};
pub use lash_runner::run_worker;
pub use mcp::{run_mcp_server_main as run_mcp_server, McpServer};
pub use runner::{WorkerConfig, WorkerError, WorkerRunner};
