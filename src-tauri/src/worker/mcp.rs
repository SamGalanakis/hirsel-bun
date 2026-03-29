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
//! HIRSEL_RUNTIME=myrun HIRSEL_WORKER=achilles hirsel-worker mcp
//! ```

use serde_json::{json, Value};

use super::{WorkerConfig, WorkerError, WorkerRunner};
use crate::backend::mcp::{run_mcp_server, McpToolServer, Tool};

/// Get the list of available MCP tools.
fn get_tools() -> Vec<Tool> {
    vec![
        // ==========================================================================
        // Task Management
        // ==========================================================================
        Tool {
            name: "get_task_tree",
            description: "Get the full task hierarchy with status and dependencies. Returns all tasks and checks in the run.",
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
            description: "Mark task complete and exit. Unblocks dependent tasks, then returns control to the orchestrator.",
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
            name: "add_check",
            description: "Create a check (validation node). If validates is provided, writes validated_by on target nodes. Check becomes ready when all validated nodes complete. Parent under the feature for scoped checks; omit parent for global/e2e checks.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "check_id": {
                        "type": "string",
                        "description": "Check identifier (e.g., 'verify_auth')"
                    },
                    "name": {
                        "type": "string",
                        "description": "Human-readable check name"
                    },
                    "parent": {
                        "type": "string",
                        "description": "Parent node ID. Use this to place the check under the feature it validates. Omit for global/e2e checks."
                    },
                    "validates": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Node IDs this check validates (convenience sugar — writes validated_by on each node). Omit for global checks."
                    }
                },
                "required": ["check_id", "name"]
            }),
        },
        Tool {
            name: "delete_task",
            description: "Delete a worker-created task. Cannot delete spec tasks, claimed tasks, or completed tasks.",
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
        // ==========================================================================
        // Orchestrator Coordination
        // ==========================================================================
        Tool {
            name: "report_progress",
            description: "Report important progress upward to the orchestrator without blocking execution.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "Short progress summary"
                    },
                    "details": {
                        "type": "string",
                        "description": "Optional extra context"
                    }
                },
                "required": ["summary"]
            }),
        },
        Tool {
            name: "raise_concern",
            description: "Raise a structured concern to the orchestrator. Set blocking=true if you need a decision before continuing.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "description": "Concern kind, for example blocker, risk, review_needed, or conflict"
                    },
                    "summary": {
                        "type": "string",
                        "description": "Short concern summary"
                    },
                    "details": {
                        "type": "string",
                        "description": "Optional supporting detail"
                    },
                    "severity": {
                        "type": "string",
                        "description": "Optional severity: info, low, medium, high, critical"
                    },
                    "blocking": {
                        "type": "boolean",
                        "description": "Whether the worker must pause until this is resolved"
                    }
                },
                "required": ["kind", "summary"]
            }),
        },
        Tool {
            name: "request_decision",
            description: "Escalate a decision that requires orchestrator or user input. This pauses your work until the orchestrator resolves it.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "Decision needed"
                    },
                    "details": {
                        "type": "string",
                        "description": "Optional context or options"
                    }
                },
                "required": ["summary"]
            }),
        },
        Tool {
            name: "scribe",
            description: "Record durable project context, constraints, or discoveries for project-level condensation.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "Durable context worth retaining across route work."
                    }
                },
                "required": ["content"]
            }),
        },
        Tool {
            name: "read_retained_context",
            description: "Read the current project-level retained context artifact.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ==========================================================================
        // Work Management
        // ==========================================================================
        Tool {
            name: "work_done",
            description: "Signal task complete. Auto-completes your assigned task, then exits so the orchestrator can decide what happens next.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "time_status",
            description: "Get current time limit status for this route runtime. Shows elapsed time, remaining time, and percentage progress.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        // ==========================================================================
        // Check Operations
        // ==========================================================================
        Tool {
            name: "check_pass",
            description: "Call this when all validation criteria pass. Only available for check nodes. Marks validated nodes as validated.",
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "check_fail",
            description: "Call this when validation fails. Only available for check nodes. Provide feedback explaining what failed and how to fix it. Creates a repair task.",
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
                self.runner.task_done(task_id).map(|s| (s, true)) // Exit after completing task
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
            "add_check" => {
                let check_id = args
                    .get("check_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("check_id is required".into()))?;
                let check_name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("name is required".into()))?;
                let parent = args.get("parent").and_then(|v| v.as_str());
                let validates: Vec<String> = args
                    .get("validates")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                self.runner
                    .add_check(check_id, check_name, parent, &validates)
                    .map(|s| (s, false))
            }
            "delete_task" => {
                let task_id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("task_id is required".into()))?;
                self.runner.delete_task(task_id).map(|s| (s, false))
            }

            // Orchestrator coordination
            "report_progress" => {
                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("summary is required".into()))?;
                let details = args.get("details").and_then(|v| v.as_str());
                self.runner
                    .report_progress(summary, details)
                    .map(|s| (s, false))
            }
            "raise_concern" => {
                let kind = args
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("kind is required".into()))?;
                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("summary is required".into()))?;
                let details = args.get("details").and_then(|v| v.as_str());
                let severity = args.get("severity").and_then(|v| v.as_str());
                let blocking = args
                    .get("blocking")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.runner
                    .raise_concern(kind, summary, details, severity, blocking)
                    .map(|s| (s, false))
            }
            "request_decision" => {
                let summary = args
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("summary is required".into()))?;
                let details = args.get("details").and_then(|v| v.as_str());
                self.runner
                    .request_decision(summary, details)
                    .map(|s| (s, false))
            }

            "scribe" => {
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("content is required".into()))?;
                self.runner.scribe(content).map(|s| (s, false))
            }
            "read_retained_context" => self.runner.read_retained_context().map(|s| (s, false)),

            // Work Management
            "work_done" => self.runner.task_done(None).map(|s| (s, true)),
            "time_status" => self.time_status().map(|s| (s, false)),

            // Check Operations
            "check_pass" => self.runner.check_pass().map(|s| (s, false)),
            "check_fail" => {
                let feedback = args
                    .get("feedback")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WorkerError::Config("feedback is required".into()))?;
                self.runner.check_fail(feedback).map(|s| (s, false))
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
        assert!(names.contains(&"add_check"));
        assert!(names.contains(&"delete_task"));
        assert!(names.contains(&"report_progress"));
        assert!(names.contains(&"raise_concern"));
        assert!(names.contains(&"request_decision"));
        assert!(names.contains(&"scribe"));
        assert!(names.contains(&"read_retained_context"));
        assert!(names.contains(&"work_done"));
        assert!(names.contains(&"time_status"));
        assert!(names.contains(&"check_pass"));
        assert!(names.contains(&"check_fail"));
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
