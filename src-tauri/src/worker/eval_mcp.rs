//! Eval MCP Server for hirsel evaluations.
//!
//! This module implements an MCP server that provides eval_pass and eval_fail
//! tools for AI agents running evaluations. The server communicates via
//! JSON-RPC 2.0 over stdin/stdout.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

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

    /// Handle a JSON-RPC request.
    fn handle_request(&mut self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let response = match request.method.as_str() {
            "initialize" => self.handle_initialize(request.id),
            "tools/list" => self.handle_tools_list(request.id),
            "tools/call" => self.handle_tools_call(request.id, request.params),
            _ => JsonRpcResponse::error(
                request.id,
                -32601,
                format!("Method not found: {}", request.method),
            ),
        };

        Some(response)
    }

    fn handle_initialize(&self, id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "hirsel-eval",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {}
                }
            }),
        )
    }

    fn handle_tools_list(&self, id: Option<Value>) -> JsonRpcResponse {
        let tools = vec![
            json!({
                "name": "eval_pass",
                "description": "Call this when all evaluation criteria pass. No parameters needed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }),
            json!({
                "name": "eval_fail",
                "description": "Call this when evaluation fails. You must provide feedback explaining what failed and how to fix it.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "feedback": {
                            "type": "string",
                            "description": "What failed and how to fix it"
                        }
                    },
                    "required": ["feedback"]
                }
            }),
        ];

        JsonRpcResponse::success(id, json!({ "tools": tools }))
    }

    fn handle_tools_call(&mut self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        match tool_name {
            "eval_pass" => {
                eprintln!("EvalMCPServer: eval_pass called");
                let result = EvalResult {
                    success: true,
                    feedback: "All checks passed.".to_string(),
                };
                if let Err(e) = self.write_result(&result) {
                    return JsonRpcResponse::error(id, -32000, format!("Failed to write result: {}", e));
                }
                self.submitted = true;
                JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": "Eval result submitted: PASSED"
                        }]
                    }),
                )
            }
            "eval_fail" => {
                let feedback = arguments
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
                if let Err(e) = self.write_result(&result) {
                    return JsonRpcResponse::error(id, -32000, format!("Failed to write result: {}", e));
                }
                self.submitted = true;
                JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": "Eval result submitted: FAILED"
                        }]
                    }),
                )
            }
            _ => JsonRpcResponse::error(id, -32000, format!("Unknown tool: {}", tool_name)),
        }
    }

    fn write_result(&self, result: &EvalResult) -> std::io::Result<()> {
        let json = serde_json::to_string(result)?;
        fs::write(&self.result_file, json)
    }

    /// Run the MCP server loop, reading from stdin and writing to stdout.
    pub fn run(&mut self) -> std::io::Result<()> {
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
                    let error_response = JsonRpcResponse::error(
                        None,
                        -32700,
                        format!("Parse error: {}", e),
                    );
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

            // Exit after submitting result
            if self.submitted {
                eprintln!("Eval MCP server: result submitted, exiting");
                break;
            }
        }

        Ok(())
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

    eprintln!("Eval MCP server started, result_file={}", result_file.display());

    let mut server = EvalMcpServer::new(result_file);
    if let Err(e) = server.run() {
        eprintln!("Eval MCP server error: {}", e);
        std::process::exit(1);
    }
}
