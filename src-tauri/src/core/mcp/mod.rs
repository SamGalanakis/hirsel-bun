//! Shared MCP (Model Context Protocol) server infrastructure.
//!
//! This module provides common types and utilities for building MCP servers
//! that communicate via JSON-RPC 2.0 over stdin/stdout.
//!
//! ## Usage
//!
//! Implement `McpToolServer` for your server type:
//!
//! ```rust,ignore
//! impl McpToolServer for MyServer {
//!     fn server_name(&self) -> &'static str { "my-mcp-server" }
//!     fn tools(&self) -> Vec<Tool> { get_my_tools() }
//!     fn execute(&mut self, name: &str, args: Value) -> Result<(String, bool), String> {
//!         match name {
//!             "my_tool" => Ok((do_something(), false)),
//!             "exit_tool" => Ok((goodbye(), true)), // Exit after response
//!             _ => Err(format!("Unknown tool: {}", name)),
//!         }
//!     }
//! }
//! ```
//!
//! Then run with `run_mcp_server(&mut server)`.

mod jsonrpc;
mod server;

pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, Tool};
pub use server::{run_mcp_server, McpToolServer};
