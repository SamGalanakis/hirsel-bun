//! MCP server trait and runner.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use super::{JsonRpcRequest, JsonRpcResponse, Tool};

/// Trait for implementing an MCP tool server.
///
/// Implement this trait to define the tools and behavior of your MCP server.
/// The server loop is handled by `run_mcp_server`.
pub trait McpToolServer {
    /// Server name for the initialize response.
    fn server_name(&self) -> &'static str;

    /// Server version for the initialize response.
    /// Defaults to the crate version.
    fn server_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Get the list of available tools.
    fn tools(&self) -> Vec<Tool>;

    /// Execute a tool by name with the given arguments.
    ///
    /// Returns `Ok((output, should_exit))` where:
    /// - `output` is the tool result as a string (can be JSON or plain text)
    /// - `should_exit` is true if the server should exit after this response
    ///
    /// Returns `Err(message)` if the tool execution fails.
    fn execute(&mut self, name: &str, args: Value) -> Result<(String, bool), String>;
}

/// Run the MCP server loop for any `McpToolServer` implementation.
///
/// Reads JSON-RPC requests from stdin and writes responses to stdout.
/// The loop exits when stdin closes or when a tool returns `should_exit = true`.
pub fn run_mcp_server<S: McpToolServer>(server: &mut S) -> io::Result<()> {
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

        let (response, should_exit) = handle_request(server, request);

        // Send response FIRST, before any exit actions
        if let Some(response) = response {
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = writeln!(stdout, "{}", json);
                let _ = stdout.flush();
            }
        }

        if should_exit {
            // Give client time to read the response before we exit
            std::thread::sleep(std::time::Duration::from_millis(500));

            // Kill parent process (Claude CLI) AFTER response is sent
            // This ensures clean termination with the response delivered
            #[cfg(unix)]
            {
                let ppid = unsafe { libc::getppid() };
                tracing::info!("MCP server exiting, killing parent (pid={})", ppid);
                unsafe { libc::kill(ppid, libc::SIGTERM) };
            }

            break;
        }
    }

    Ok(())
}

/// Handle a single JSON-RPC request.
fn handle_request<S: McpToolServer>(
    server: &mut S,
    request: JsonRpcRequest,
) -> (Option<JsonRpcResponse>, bool) {
    let id = request.id.clone();

    // Notifications (no id) don't need a response
    if id.is_none() {
        return (None, false);
    }

    match request.method.as_str() {
        "initialize" => {
            let response = JsonRpcResponse::success(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "serverInfo": {
                        "name": server.server_name(),
                        "version": server.server_version()
                    },
                    "capabilities": {
                        "tools": {}
                    }
                }),
            );
            (Some(response), false)
        }
        "tools/list" => {
            let response = JsonRpcResponse::success(id, json!({ "tools": server.tools() }));
            (Some(response), false)
        }
        "tools/call" => {
            let tool_name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));

            match server.execute(tool_name, arguments) {
                Ok((output, should_exit)) => {
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
                    (Some(JsonRpcResponse::success(id, content)), should_exit)
                }
                Err(e) => (Some(JsonRpcResponse::error(id, -32000, e)), false),
            }
        }
        _ => {
            let response =
                JsonRpcResponse::error(id, -32601, format!("Method not found: {}", request.method));
            (Some(response), false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestServer;

    impl McpToolServer for TestServer {
        fn server_name(&self) -> &'static str {
            "test-mcp"
        }

        fn tools(&self) -> Vec<Tool> {
            vec![Tool {
                name: "test_tool",
                description: "A test tool",
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
            }]
        }

        fn execute(&mut self, name: &str, _args: Value) -> Result<(String, bool), String> {
            match name {
                "test_tool" => Ok(("test output".to_string(), false)),
                "exit_tool" => Ok(("goodbye".to_string(), true)),
                _ => Err(format!("Unknown tool: {}", name)),
            }
        }
    }

    #[test]
    fn test_handle_initialize() {
        let mut server = TestServer;
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: json!({}),
            id: Some(json!(1)),
        };

        let (response, should_exit) = handle_request(&mut server, request);
        assert!(!should_exit);
        let response = response.unwrap();
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "test-mcp");
    }

    #[test]
    fn test_handle_tools_list() {
        let mut server = TestServer;
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            method: "tools/list".to_string(),
            params: json!({}),
            id: Some(json!(1)),
        };

        let (response, should_exit) = handle_request(&mut server, request);
        assert!(!should_exit);
        let response = response.unwrap();
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "test_tool");
    }

    #[test]
    fn test_handle_tools_call() {
        let mut server = TestServer;
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            method: "tools/call".to_string(),
            params: json!({
                "name": "test_tool",
                "arguments": {}
            }),
            id: Some(json!(1)),
        };

        let (response, should_exit) = handle_request(&mut server, request);
        assert!(!should_exit);
        let response = response.unwrap();
        assert!(response.error.is_none());
    }

    #[test]
    fn test_handle_exit_tool() {
        let mut server = TestServer;
        let request = JsonRpcRequest {
            _jsonrpc: "2.0".to_string(),
            method: "tools/call".to_string(),
            params: json!({
                "name": "exit_tool",
                "arguments": {}
            }),
            id: Some(json!(1)),
        };

        let (response, should_exit) = handle_request(&mut server, request);
        assert!(should_exit);
        let response = response.unwrap();
        assert!(response.error.is_none());
    }
}
