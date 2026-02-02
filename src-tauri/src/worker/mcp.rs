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

use serde_json::{json, Value};

use super::{WorkerConfig, WorkerError, WorkerRunner};
use crate::core::mcp::{run_mcp_server, McpToolServer, Tool};

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
            description: "Signal task complete and ready for new assignment. Auto-completes your assigned task, then exits. You'll be respawned with a new task if available.",
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

    /// Get time status for the run.
    fn time_status(&self) -> Result<String, WorkerError> {
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
}

impl McpServer {
    /// Execute a tool and return the result.
    /// Returns `Ok((output, should_exit))` or `Err(error_message)`.
    fn execute_tool(&mut self, name: &str, args: Value) -> Result<(String, bool), WorkerError> {
        match name {
            // Task Management
            "get_task_tree" => self.runner.get_task_tree().map(|s| (s, false)),
            "get_available_tasks" => self.runner.get_available_tasks().map(|s| (s, false)),
            "get_my_tasks" => self.runner.get_my_tasks().map(|s| (s, false)),
            "get_task_details" => {
                let task_id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("task_id is required".into()))?;
                self.runner.get_task_details(task_id).map(|s| (s, false))
            }
            "complete_task" => {
                let task_id = args.get("task_id").and_then(|v| v.as_str());
                self.runner.task_done(task_id).map(|s| (s, false))
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
                    .map(|s| (s, false))
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

                self.runner
                    .add_eval(eval_id, eval_name, &validates)
                    .map(|s| (s, false))
            }

            // Communication
            "list_contacts" => self.runner.list_contacts().map(|s| (s, false)),
            "chat_history" => {
                let with = args.get("with").and_then(|v| v.as_str());
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|l| l as usize);
                self.runner.chat_history(with, limit).map(|s| (s, false))
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
                self.runner.chat_send(to, message).map(|s| (s, false))
            }
            "chat_unread" => {
                let with = args.get("with").and_then(|v| v.as_str());
                self.runner.chat_unread(with).map(|s| (s, false))
            }

            // Documentation
            "scribe" => {
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("content is required".into()))?;
                self.runner.scribe(content).map(|s| (s, false))
            }
            "read_docs" => {
                let file = args.get("file").and_then(|v| v.as_str());
                self.runner.read_docs(file).map(|s| (s, false))
            }

            // Work Management
            "work_done" => self.runner.work_done().map(|s| (s, true)), // Exit after work_done
            "time_status" => self.time_status().map(|s| (s, false)),

            // Eval Operations
            "eval_pass" => self.runner.eval_pass().map(|s| (s, false)),
            "eval_fail" => {
                let feedback = args
                    .get("feedback")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("feedback is required".into()))?;
                self.runner.eval_fail(feedback).map(|s| (s, false))
            }

            _ => Err(WorkerError::Config(format!("Unknown tool: {}", name))),
        }
    }
}

impl McpToolServer for McpServer {
    fn server_name(&self) -> &'static str {
        "hirsel-mcp"
    }

    fn tools(&self) -> Vec<Tool> {
        get_tools()
    }

    fn execute(&mut self, name: &str, args: Value) -> Result<(String, bool), String> {
        self.execute_tool(name, args).map_err(|e| e.to_string())
    }
}

/// Run the MCP server from environment configuration.
///
/// This is the main entry point for the MCP server binary.
pub fn run_mcp_server_main() -> Result<(), WorkerError> {
    let mut server = McpServer::from_env()?;
    run_mcp_server(&mut server).map_err(WorkerError::Io)
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
