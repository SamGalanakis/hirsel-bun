use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use lash::tools::StandardShell;
use lash::{ProgressSender, ToolDefinition, ToolParam, ToolProvider, ToolResult};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use super::rpc::{send_server_control_request, ServerControlRequest, ServerToolResultPayload};

#[derive(Clone, Debug)]
enum RoutedShellSession {
    Local {
        inner_session_id: i64,
    },
    Thread {
        project_id: i64,
        thread_id: String,
        inner_session_id: i64,
    },
}

pub(super) struct ShepherdShellToolProvider {
    local_shell: StandardShell,
    default_project_id: Option<i64>,
    sessions: Arc<Mutex<HashMap<i64, RoutedShellSession>>>,
    next_session_id: AtomicI64,
}

impl ShepherdShellToolProvider {
    pub(super) fn new(default_project_id: Option<i64>, workspace_root: Option<PathBuf>) -> Self {
        let local_shell = match workspace_root {
            Some(path) => StandardShell::new().with_cwd(path),
            None => StandardShell::new(),
        };
        Self {
            local_shell,
            default_project_id,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_session_id: AtomicI64::new(1),
        }
    }

    fn resolve_project_id(&self, args: &Value) -> Result<i64, ToolResult> {
        if let Some(project_id) = args
            .get("project_id")
            .and_then(|value| value.as_i64().or_else(|| value.as_u64().map(|n| n as i64)))
        {
            return Ok(project_id);
        }
        self.default_project_id.ok_or_else(|| {
            ToolResult::err_fmt("project_id is required for thread-targeted shell commands")
        })
    }

    fn parse_session_id(args: &Value) -> Result<i64, ToolResult> {
        args.get("session_id")
            .and_then(|value| value.as_i64().or_else(|| value.as_u64().map(|n| n as i64)))
            .ok_or_else(|| ToolResult::err_fmt("Invalid session_id: expected int"))
    }

    fn output_session_id(result: &Value) -> Option<i64> {
        result
            .get("session_id")
            .and_then(|value| value.as_i64().or_else(|| value.as_u64().map(|n| n as i64)))
    }

    fn has_exit_code(result: &Value) -> bool {
        result.get("exit_code").is_some()
    }

    fn decode_remote_shell_result(payload: Result<Option<Value>, String>) -> ToolResult {
        let payload = match payload {
            Ok(Some(payload)) => payload,
            Ok(None) => {
                return ToolResult::err(json!({
                    "error": "server control reply did not include a tool result payload"
                }));
            }
            Err(error) => return ToolResult::err(json!({ "error": error })),
        };

        let payload: ServerToolResultPayload = match serde_json::from_value(payload) {
            Ok(payload) => payload,
            Err(error) => {
                return ToolResult::err(json!({
                    "error": format!("failed to decode tool result payload: {}", error)
                }));
            }
        };

        ToolResult {
            success: payload.success,
            result: payload.result,
            images: vec![],
        }
    }

    fn annotate_result(mut result: ToolResult, target: &RoutedShellSession) -> ToolResult {
        if let Some(object) = result.result.as_object_mut() {
            match target {
                RoutedShellSession::Local { .. } => {
                    object.insert("target_kind".to_string(), json!("shepherd"));
                }
                RoutedShellSession::Thread { thread_id, .. } => {
                    object.insert("target_kind".to_string(), json!("thread"));
                    object.insert("thread_id".to_string(), json!(thread_id));
                }
            }
        }
        result
    }

    fn set_session_id(mut result: ToolResult, session_id: i64) -> ToolResult {
        if let Some(object) = result.result.as_object_mut() {
            if object.contains_key("session_id") {
                object.insert("session_id".to_string(), json!(session_id));
            }
        }
        result
    }

    async fn broker_result(&self, result: ToolResult, target: RoutedShellSession) -> ToolResult {
        if !result.success {
            return result;
        }

        let Some(inner_session_id) = Self::output_session_id(&result.result) else {
            return Self::annotate_result(result, &target);
        };

        let broker_session_id = self.next_session_id.fetch_add(1, Ordering::SeqCst);
        let mut sessions = self.sessions.lock().await;
        let target = match target {
            RoutedShellSession::Local { .. } => RoutedShellSession::Local { inner_session_id },
            RoutedShellSession::Thread {
                project_id,
                thread_id,
                ..
            } => RoutedShellSession::Thread {
                project_id,
                thread_id,
                inner_session_id,
            },
        };
        sessions.insert(broker_session_id, target.clone());
        drop(sessions);

        let mut result = Self::annotate_result(result, &target);
        if let Some(object) = result.result.as_object_mut() {
            object.insert("session_id".to_string(), json!(broker_session_id));
        }
        result
    }

    async fn exec_remote(&self, project_id: i64, thread_id: &str, args: &Value) -> ToolResult {
        if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_err() {
            return ToolResult::err(json!({
                "error": "thread-targeted shell commands require the host runtime control socket"
            }));
        }
        let result = Self::decode_remote_shell_result(
            send_server_control_request(&ServerControlRequest::ExecThreadShell {
                project_id,
                thread_id: thread_id.to_string(),
                args: args.clone(),
            })
            .await,
        );
        self.broker_result(
            result,
            RoutedShellSession::Thread {
                project_id,
                thread_id: thread_id.to_string(),
                inner_session_id: 0,
            },
        )
        .await
    }

    async fn exec_local(&self, args: &Value, progress: Option<&ProgressSender>) -> ToolResult {
        let result = self
            .local_shell
            .execute_streaming("exec_command", args, progress)
            .await;
        self.broker_result(
            result,
            RoutedShellSession::Local {
                inner_session_id: 0,
            },
        )
        .await
    }

    async fn exec_command(&self, args: &Value, progress: Option<&ProgressSender>) -> ToolResult {
        let Some(thread_id) = args.get("thread_id").and_then(|value| value.as_str()) else {
            return self.exec_local(args, progress).await;
        };
        let thread_id = thread_id.trim();
        if thread_id.is_empty() {
            return ToolResult::err_fmt("thread_id cannot be empty");
        }
        let project_id = match self.resolve_project_id(args) {
            Ok(project_id) => project_id,
            Err(error) => return error,
        };
        self.exec_remote(project_id, thread_id, args).await
    }

    async fn write_local(
        &self,
        broker_session_id: i64,
        inner_session_id: i64,
        args: &Value,
        progress: Option<&ProgressSender>,
    ) -> ToolResult {
        let mut forwarded_args = args.clone();
        if let Some(object) = forwarded_args.as_object_mut() {
            object.insert("session_id".to_string(), json!(inner_session_id));
        }
        let result = self
            .local_shell
            .execute_streaming("write_stdin", &forwarded_args, progress)
            .await;
        if !result.success || Self::has_exit_code(&result.result) {
            self.sessions.lock().await.remove(&broker_session_id);
        }
        let result = Self::set_session_id(result, broker_session_id);
        Self::annotate_result(result, &RoutedShellSession::Local { inner_session_id })
    }

    async fn write_remote(
        &self,
        broker_session_id: i64,
        project_id: i64,
        thread_id: &str,
        inner_session_id: i64,
        args: &Value,
    ) -> ToolResult {
        if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_err() {
            self.sessions.lock().await.remove(&broker_session_id);
            return ToolResult::err(json!({
                "error": "thread-targeted shell commands require the host runtime control socket"
            }));
        }
        let mut forwarded_args = args.clone();
        if let Some(object) = forwarded_args.as_object_mut() {
            object.insert("session_id".to_string(), json!(inner_session_id));
        }
        let result = Self::decode_remote_shell_result(
            send_server_control_request(&ServerControlRequest::WriteThreadShell {
                project_id,
                thread_id: thread_id.to_string(),
                args: forwarded_args,
            })
            .await,
        );
        if !result.success || Self::has_exit_code(&result.result) {
            self.sessions.lock().await.remove(&broker_session_id);
        }
        let result = Self::set_session_id(result, broker_session_id);
        Self::annotate_result(
            result,
            &RoutedShellSession::Thread {
                project_id,
                thread_id: thread_id.to_string(),
                inner_session_id,
            },
        )
    }

    async fn write_stdin(&self, args: &Value, progress: Option<&ProgressSender>) -> ToolResult {
        let broker_session_id = match Self::parse_session_id(args) {
            Ok(session_id) => session_id,
            Err(error) => return error,
        };
        let session = {
            let sessions = self.sessions.lock().await;
            sessions.get(&broker_session_id).cloned()
        };
        let Some(session) = session else {
            return ToolResult::err_fmt(format!("Unknown session id {}", broker_session_id));
        };

        match session {
            RoutedShellSession::Local { inner_session_id } => {
                self.write_local(broker_session_id, inner_session_id, args, progress)
                    .await
            }
            RoutedShellSession::Thread {
                project_id,
                thread_id,
                inner_session_id,
            } => {
                self.write_remote(
                    broker_session_id,
                    project_id,
                    &thread_id,
                    inner_session_id,
                    args,
                )
                .await
            }
        }
    }
}

#[async_trait::async_trait]
impl ToolProvider for ShepherdShellToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definitions = self.local_shell.definitions();
        for definition in &mut definitions {
            match definition.name.as_str() {
                "exec_command" => {
                    definition.description.push_str(
                        " Pass `thread_id` to run the command inside that idle thread container instead of shepherd's own workspace.",
                    );
                    definition
                        .params
                        .push(ToolParam::optional("thread_id", "str"));
                }
                "write_stdin" => {
                    definition.description.push_str(
                        " The returned session_id works for both shepherd-local and thread-targeted commands.",
                    );
                }
                _ => {}
            }
        }
        definitions
    }

    async fn execute(&self, name: &str, args: &Value) -> ToolResult {
        self.execute_streaming(name, args, None).await
    }

    async fn execute_streaming(
        &self,
        name: &str,
        args: &Value,
        progress: Option<&ProgressSender>,
    ) -> ToolResult {
        match name {
            "exec_command" => self.exec_command(args, progress).await,
            "write_stdin" => self.write_stdin(args, progress).await,
            _ => ToolResult::err_fmt(format!("Unknown tool: {}", name)),
        }
    }
}
