//! MCP (Model Context Protocol) server for hirsel workers.
//!
//! This module implements an MCP server that exposes hirsel worker commands
//! as tools that can be used by AI agents. The server communicates via
//! JSON-RPC 2.0 over stdin/stdout.
//!
//! ## Protocol
//!
//! The server handles three JSON-RPC methods:
//! - `initialize` - Returns server capabilities
//! - `tools/list` - Returns available tool definitions
//! - `tools/call` - Executes a tool and returns the result
//!
//! ## Usage
//!
//! The MCP server is started by setting up the worker environment and running:
//! ```bash
//! HIRSEL_RUN=myrun HIRSEL_WORKER=achilles hirsel-worker mcp
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use super::{WorkerConfig, WorkerError, WorkerRunner};

/// JSON-RPC 2.0 request structure.
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    id: Option<Value>,
}

/// JSON-RPC 2.0 response structure.
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: Option<Value>,
}

/// JSON-RPC 2.0 error structure.
#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        }
    }

    fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
            id,
        }
    }
}

/// MCP tool definition.
#[derive(Debug, Serialize)]
struct Tool {
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

/// Get the list of available MCP tools.
fn get_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "task_list",
            description: "List all tasks for the current run",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "task_add",
            description: "Add a new task. Task ID must be lowercase, start with letter, use underscores. Will warn if similar tasks exist.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task identifier (e.g., 'implement_auth')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Human-readable task name"
                    },
                    "parent": {
                        "type": "string",
                        "description": "Parent task ID for hierarchical organization"
                    },
                    "blocked_by": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of task IDs that must complete before this task can start"
                    },
                    "confirm": {
                        "type": "boolean",
                        "description": "Set to true to bypass similar task warning and force creation"
                    }
                },
                "required": ["task_id", "name"]
            }),
        },
        Tool {
            name: "task_claim",
            description: "Claim a task to work on. Only one task can be claimed at a time.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task ID to claim"
                    }
                },
                "required": ["task_id"]
            }),
        },
        Tool {
            name: "task_done",
            description: "Mark the currently claimed task as complete. Optionally specify task_id.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task ID (optional, uses claimed task if omitted)"
                    }
                }
            }),
        },
        Tool {
            name: "task_unclaim",
            description: "Release a claimed task without completing it.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task ID (optional, uses claimed task if omitted)"
                    }
                }
            }),
        },
        Tool {
            name: "task_delete",
            description: "Delete a task and all its children. Use this to remove duplicate tasks, tasks that no longer make sense, or to restructure your plan. Cannot delete tasks that are currently claimed.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task ID to delete"
                    }
                },
                "required": ["task_id"]
            }),
        },
        Tool {
            name: "msg_send",
            description: "Send a message to a thread. Use wait=true to pause and wait for a reply.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "thread": {
                        "type": "string",
                        "description": "Thread name (e.g., 'user', 'group')"
                    },
                    "message": {
                        "type": "string",
                        "description": "Message content"
                    },
                    "wait": {
                        "type": "boolean",
                        "description": "If true, pause execution until user replies"
                    }
                },
                "required": ["thread", "message"]
            }),
        },
        Tool {
            name: "msg_read",
            description: "Read new messages from a thread (or all threads if none specified).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "thread": {
                        "type": "string",
                        "description": "Thread name (optional, reads all if omitted)"
                    }
                }
            }),
        },
        Tool {
            name: "msg_list",
            description: "List available message threads.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "msg_inbox",
            description: "Check inbox for new messages since session started.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "task_await",
            description: "Wait for tasks to become available. Use this if you're a worker waiting for the leader to assign tasks.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "work_done",
            description: "Signal that all assigned work is complete. Only call this when you have no more tasks to do.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "force": {
                        "type": "boolean",
                        "description": "Skip checks for unclaimed tasks and unmerged branches"
                    }
                }
            }),
        },
        Tool {
            name: "time_status",
            description: "Get current time limit status for this run. Shows elapsed time, remaining time, percentage progress, and time spent on your current task.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

/// MCP Server for hirsel workers.
pub struct McpServer {
    runner: WorkerRunner,
}

impl McpServer {
    /// Create a new MCP server from environment configuration.
    pub fn from_env() -> Result<Self, WorkerError> {
        let config = WorkerConfig::from_env()?;
        let runner = WorkerRunner::new(config)?;
        Ok(Self { runner })
    }

    /// Create a new MCP server with explicit configuration.
    pub fn new(runner: WorkerRunner) -> Self {
        Self { runner }
    }

    /// Handle a JSON-RPC request and return a response.
    fn handle_request(&mut self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = request.id.clone();

        // Notifications (no id) don't need a response
        id.as_ref()?;

        let response = match request.method.as_str() {
            "initialize" => self.handle_initialize(id),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, request.params),
            _ => {
                JsonRpcResponse::error(id, -32601, format!("Method not found: {}", request.method))
            }
        };

        Some(response)
    }

    fn handle_initialize(&self, id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "hirsel-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {}
                }
            }),
        )
    }

    fn handle_tools_list(&self, id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(id, json!({ "tools": get_tools() }))
    }

    fn handle_tools_call(&mut self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = self.execute_tool(tool_name, arguments);

        match result {
            Ok(output) => {
                // Parse output as JSON if possible, otherwise wrap as text
                let content = match serde_json::from_str::<Value>(&output) {
                    Ok(json_output) => json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&json_output).unwrap_or(output)
                        }]
                    }),
                    Err(_) => json!({
                        "content": [{
                            "type": "text",
                            "text": output
                        }]
                    }),
                };
                JsonRpcResponse::success(id, content)
            }
            Err(e) => JsonRpcResponse::error(id, -32000, e.to_string()),
        }
    }

    fn execute_tool(&mut self, name: &str, args: Value) -> Result<String, WorkerError> {
        match name {
            "task_list" => self.runner.task_list(),
            "task_add" => {
                let task_id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("task_id is required".into()))?;
                let task_name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("name is required".into()))?;
                let parent = args.get("parent").and_then(|v| v.as_str());
                let blocked_by: Vec<String> = args
                    .get("blocked_by")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                self.runner
                    .task_add(task_id, task_name, parent, &blocked_by)
            }
            "task_claim" => {
                let task_id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("task_id is required".into()))?;
                self.runner.task_claim(task_id)
            }
            "task_done" => {
                let task_id = args.get("task_id").and_then(|v| v.as_str());
                self.runner.task_done(task_id)
            }
            "task_unclaim" => {
                let task_id = args.get("task_id").and_then(|v| v.as_str());
                self.runner.task_unclaim(task_id)
            }
            "task_delete" => {
                let task_id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("task_id is required".into()))?;
                self.runner.task_delete(task_id)
            }
            "task_await" => self.runner.task_await(),
            "msg_send" => {
                let thread = args
                    .get("thread")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("thread is required".into()))?;
                let message = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("message is required".into()))?;
                let wait = args.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);
                self.runner.msg_send(thread, message, wait)
            }
            "msg_read" => {
                let thread = args.get("thread").and_then(|v| v.as_str());
                self.runner.msg_read(thread)
            }
            "msg_list" => self.runner.msg_list(),
            "msg_inbox" => self.runner.msg_inbox(),
            "work_done" => self.runner.work_done(),
            "time_status" => self.time_status(),
            _ => Err(WorkerError::Config(format!("Unknown tool: {}", name))),
        }
    }

    /// Get time status for the run.
    fn time_status(&self) -> Result<String, WorkerError> {
        // Get time info from state
        let time_info = self.runner.get_time_info()?;

        match time_info {
            Some(info) => {
                let elapsed_min = info.elapsed_minutes;
                let remaining_min = info.remaining_minutes;
                let limit_min = info.limit_minutes;
                let pct_elapsed = info.percent_elapsed;

                Ok(json!({
                    "time_limit_minutes": limit_min,
                    "elapsed_minutes": elapsed_min,
                    "remaining_minutes": remaining_min,
                    "percent_elapsed": pct_elapsed,
                    "percent_remaining": 100.0 - pct_elapsed,
                    "message": format!(
                        "Time: {:.1}/{} min ({:.1}% elapsed, {:.1} min remaining)",
                        elapsed_min, limit_min, pct_elapsed, remaining_min
                    )
                })
                .to_string())
            }
            None => Ok(json!({
                "message": "No time limit set for this run."
            })
            .to_string()),
        }
    }

    /// Run the MCP server loop, reading from stdin and writing to stdout.
    pub fn run(&mut self) -> Result<(), WorkerError> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut stdout = stdout.lock();

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Error reading stdin: {}", e);
                    continue;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    let error_response =
                        JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e));
                    if let Ok(json) = serde_json::to_string(&error_response) {
                        let _ = writeln!(stdout, "{}", json);
                        let _ = stdout.flush();
                    }
                    continue;
                }
            };

            if let Some(response) = self.handle_request(request) {
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = writeln!(stdout, "{}", json);
                    let _ = stdout.flush();
                }
            }
        }

        Ok(())
    }
}

/// Run the MCP server from environment configuration.
///
/// This is the main entry point for the MCP server binary.
pub fn run_mcp_server() -> Result<(), WorkerError> {
    let mut server = McpServer::from_env()?;
    server.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tools_has_all_required() {
        let tools = get_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();

        assert!(names.contains(&"task_list"));
        assert!(names.contains(&"task_add"));
        assert!(names.contains(&"task_claim"));
        assert!(names.contains(&"task_done"));
        assert!(names.contains(&"task_unclaim"));
        assert!(names.contains(&"task_delete"));
        assert!(names.contains(&"task_await"));
        assert!(names.contains(&"msg_send"));
        assert!(names.contains(&"msg_read"));
        assert!(names.contains(&"msg_list"));
        assert!(names.contains(&"msg_inbox"));
        assert!(names.contains(&"work_done"));
        assert!(names.contains(&"time_status"));
    }

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(Some(json!(1)), json!({"result": "ok"}));
        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let response = JsonRpcResponse::error(Some(json!(1)), -32000, "Test error");
        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        assert_eq!(response.error.as_ref().unwrap().code, -32000);
    }

    #[test]
    fn test_tool_schemas_are_valid_json() {
        let tools = get_tools();
        for tool in tools {
            // Verify input_schema is a valid JSON object
            assert!(tool.input_schema.is_object());
            // Verify it has a type field
            assert!(tool.input_schema.get("type").is_some());
        }
    }
}
