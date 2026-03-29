//! Eval MCP Server for hirsel evaluations.
//!
//! This module implements an MCP server that provides eval_pass and eval_fail
//! tools for AI agents running evaluations. The server communicates via
//! JSON-RPC 2.0 over stdin/stdout.

use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

use crate::backend::mcp::{run_mcp_server, McpToolServer, Tool};

/// Eval result written to the result file.
#[derive(Debug, Serialize)]
struct EvalResult {
    success: bool,
    feedback: String,
}

/// MCP server for eval pass/fail tools.
pub struct EvalMcpServer {
    result_file: PathBuf,
    submitted: bool,
}

impl EvalMcpServer {
    /// Create a new EvalMcpServer.
    pub fn new(result_file: PathBuf) -> Self {
        Self {
            result_file,
            submitted: false,
        }
    }

    fn write_result(&self, result: &EvalResult) -> std::io::Result<()> {
        let json = serde_json::to_string(result)?;
        fs::write(&self.result_file, json)
    }
}

impl McpToolServer for EvalMcpServer {
    fn server_name(&self) -> &'static str {
        "hirsel-eval"
    }

    fn server_version(&self) -> &'static str {
        "1.0.0"
    }

    fn tools(&self) -> Vec<Tool> {
        vec![
            Tool {
                name: "eval_pass",
                description: "Call this when all evaluation criteria pass. No parameters needed.",
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            Tool {
                name: "eval_fail",
                description: "Call this when evaluation fails. You must provide feedback explaining what failed and how to fix it.",
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

    fn execute(&mut self, name: &str, args: Value) -> Result<(String, bool), String> {
        match name {
            "eval_pass" => {
                eprintln!("EvalMCPServer: eval_pass called");
                let result = EvalResult {
                    success: true,
                    feedback: "All checks passed.".to_string(),
                };
                self.write_result(&result)
                    .map_err(|e| format!("Failed to write result: {}", e))?;
                self.submitted = true;
                Ok(("Eval result submitted: PASSED".to_string(), true))
            }
            "eval_fail" => {
                let feedback = args
                    .get("feedback")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No feedback provided")
                    .to_string();
                eprintln!("EvalMCPServer: eval_fail called");
                eprintln!("EvalMCPServer: feedback={:.200}...", feedback);
                let result = EvalResult {
                    success: false,
                    feedback,
                };
                self.write_result(&result)
                    .map_err(|e| format!("Failed to write result: {}", e))?;
                self.submitted = true;
                Ok(("Eval result submitted: FAILED".to_string(), true))
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }
}

/// Entry point for hirsel-eval-mcp binary.
pub fn run_eval_mcp_server() {
    let result_file = match std::env::var("HIRSEL_EVAL_RESULT_FILE") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            let error_response = json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {
                    "code": -32000,
                    "message": "HIRSEL_EVAL_RESULT_FILE must be set"
                }
            });
            println!("{}", error_response);
            eprintln!("HIRSEL_EVAL_RESULT_FILE not set, exiting");
            std::process::exit(1);
        }
    };

    eprintln!(
        "Eval MCP server started, result_file={}",
        result_file.display()
    );

    let mut server = EvalMcpServer::new(result_file);
    if let Err(e) = run_mcp_server(&mut server) {
        eprintln!("Eval MCP server error: {}", e);
        std::process::exit(1);
    }
}
