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
        // ==========================================================================
        // Task Management
        // ==========================================================================
        Tool {
            name: "get_task_tree",
            description: "Get the full task hierarchy with status and dependencies. Returns all tasks and evals in the run.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "get_available_tasks",
            description: "Get tasks that are ready to claim: status=todo, not claimed, not blocked. Use this to find work.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "get_my_tasks",
            description: "Get tasks claimed by you. Use this to see what you're currently working on.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "get_task_details",
            description: "Get full details for a specific task including content and dependencies.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task ID to get details for"
                    }
                },
                "required": ["task_id"]
            }),
        },
        Tool {
            name: "claim_task",
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
            name: "complete_task",
            description: "Mark a task as complete. This unblocks dependent tasks.",
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
            name: "add_task",
            description: "Add a new task. Task ID must be lowercase, start with letter, use underscores.",
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
                    }
                },
                "required": ["task_id", "name"]
            }),
        },
        Tool {
            name: "add_eval",
            description: "Create an eval task that validates other tasks. Eval becomes ready when all validated tasks complete.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "eval_id": {
                        "type": "string",
                        "description": "Eval identifier (e.g., 'verify_auth')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Human-readable eval name"
                    },
                    "validates": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of task IDs this eval validates"
                    }
                },
                "required": ["eval_id", "name", "validates"]
            }),
        },
        // ==========================================================================
        // Communication
        // ==========================================================================
        Tool {
            name: "list_contacts",
            description: "List available chat contacts: user (human), group (team), other workers, scribe.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "chat_history",
            description: "Read chat message history. Filter by contact or get all.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "with": {
                        "type": "string",
                        "description": "Contact name to filter: 'user', 'group', 'worker-N', 'scribe'. Omit for all."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum messages to return (default 50)"
                    }
                }
            }),
        },
        Tool {
            name: "chat_send",
            description: "Send a message. Messages to 'user' pause until they reply (if HITL enabled).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Recipient: 'user', 'group', 'worker-N', or 'scribe'"
                    },
                    "message": {
                        "type": "string",
                        "description": "Message content"
                    }
                },
                "required": ["to", "message"]
            }),
        },
        Tool {
            name: "chat_unread",
            description: "Check for new unread messages since session started.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "with": {
                        "type": "string",
                        "description": "Contact to check, or omit for all contacts"
                    }
                }
            }),
        },
        // ==========================================================================
        // Documentation
        // ==========================================================================
        Tool {
            name: "scribe",
            description: "Record a learning or discovery about the codebase. Use for patterns, gotchas, architecture decisions, or anything future workers should know. Learnings are batched and integrated into docs/ by a Scribe agent.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The learning to record (patterns, gotchas, architecture decisions, etc.)"
                    }
                },
                "required": ["content"]
            }),
        },
        Tool {
            name: "read_docs",
            description: "Read the current project documentation maintained by the Scribe. Returns all docs or a specific file. Check docs at task start for accumulated project knowledge.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "Optional: specific file to read (e.g., 'architecture.md'). If omitted, returns all docs."
                    }
                }
            }),
        },
        // ==========================================================================
        // Work Management
        // ==========================================================================
        Tool {
            name: "work_done",
            description: "Signal that all assigned work is complete. Only call this when you have no more tasks to do.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "time_status",
            description: "Get current time limit status for this run. Shows elapsed time, remaining time, percentage progress.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ==========================================================================
        // Eval Operations
        // ==========================================================================
        Tool {
            name: "eval_pass",
            description: "Call this when all evaluation criteria pass. Only available for eval tasks. Marks validated tasks as validated.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "eval_fail",
            description: "Call this when evaluation fails. Only available for eval tasks. Provide feedback explaining what failed and how to fix it. Creates a repair task.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "feedback": {
                        "type": "string",
                        "description": "What failed and how to fix it"
                    }
                },
                "required": ["feedback"]
            }),
        },
    ]
}

/// MCP Server for hirsel workers.
pub struct McpServer {
    runner: WorkerRunner,
    /// Flag to indicate the server should exit after current request
    exit_after_response: bool,
}

impl McpServer {
    /// Create a new MCP server from environment configuration.
    pub fn from_env() -> Result<Self, WorkerError> {
        let config = WorkerConfig::from_env()?;
        let runner = WorkerRunner::new(config)?;
        Ok(Self {
            runner,
            exit_after_response: false,
        })
    }

    /// Create a new MCP server with explicit configuration.
    pub fn new(runner: WorkerRunner) -> Self {
        Self {
            runner,
            exit_after_response: false,
        }
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
            // Task Management
            "get_task_tree" => self.runner.get_task_tree(),
            "get_available_tasks" => self.runner.get_available_tasks(),
            "get_my_tasks" => self.runner.get_my_tasks(),
            "get_task_details" => {
                let task_id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("task_id is required".into()))?;
                self.runner.get_task_details(task_id)
            }
            "claim_task" => {
                let task_id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("task_id is required".into()))?;
                self.runner.task_claim(task_id)
            }
            "complete_task" => {
                let task_id = args.get("task_id").and_then(|v| v.as_str());
                self.runner.task_done(task_id)
            }
            "add_task" => {
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
            "add_eval" => {
                let eval_id = args
                    .get("eval_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("eval_id is required".into()))?;
                let eval_name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("name is required".into()))?;
                let validates: Vec<String> = args
                    .get("validates")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .ok_or_else(|| WorkerError::Config("validates is required".into()))?;

                self.runner.add_eval(eval_id, eval_name, &validates)
            }

            // Communication
            "list_contacts" => self.runner.list_contacts(),
            "chat_history" => {
                let with = args.get("with").and_then(|v| v.as_str());
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|l| l as usize);
                self.runner.chat_history(with, limit)
            }
            "chat_send" => {
                let to = args
                    .get("to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("to is required".into()))?;
                let message = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("message is required".into()))?;
                self.runner.chat_send(to, message)
            }
            "chat_unread" => {
                let with = args.get("with").and_then(|v| v.as_str());
                self.runner.chat_unread(with)
            }

            // Documentation
            "scribe" => {
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("content is required".into()))?;
                self.runner.scribe(content)
            }
            "read_docs" => {
                let file = args.get("file").and_then(|v| v.as_str());
                self.runner.read_docs(file)
            }

            // Work Management
            "work_done" => {
                // Signal to exit after response - worker is done
                self.exit_after_response = true;
                self.runner.work_done()
            }
            "time_status" => self.time_status(),

            // Eval Operations
            "eval_pass" => self.runner.eval_pass(),
            "eval_fail" => {
                let feedback = args
                    .get("feedback")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("feedback is required".into()))?;
                self.runner.eval_fail(feedback)
            }

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
                Err(_) => continue,
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

            // Exit after work_done to signal agent to stop
            if self.exit_after_response {
                // Give Claude CLI time to read the response before we exit
                std::thread::sleep(std::time::Duration::from_millis(500));
                break;
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

        // New tool names
        assert!(names.contains(&"get_task_tree"));
        assert!(names.contains(&"get_available_tasks"));
        assert!(names.contains(&"get_my_tasks"));
        assert!(names.contains(&"get_task_details"));
        assert!(names.contains(&"claim_task"));
        assert!(names.contains(&"complete_task"));
        assert!(names.contains(&"add_task"));
        assert!(names.contains(&"add_eval"));
        assert!(names.contains(&"list_contacts"));
        assert!(names.contains(&"chat_history"));
        assert!(names.contains(&"chat_send"));
        assert!(names.contains(&"chat_unread"));
        assert!(names.contains(&"scribe"));
        assert!(names.contains(&"read_docs"));
        assert!(names.contains(&"work_done"));
        assert!(names.contains(&"time_status"));
        assert!(names.contains(&"eval_pass"));
        assert!(names.contains(&"eval_fail"));
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
