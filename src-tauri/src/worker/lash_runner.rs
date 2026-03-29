//! lash-based worker runtime.
//!
//! Embedded lash worker runtime.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use lash::{
    default_context_strategy, default_execution_mode, AgentEvent, AgentStateEnvelope, EventSink,
    HostProfile, InputItem, LashRuntime, PluginHost, RuntimeHostConfig, RuntimeServices,
    SessionPolicy, ToolDefinition, ToolParam, ToolProvider, ToolResult, TurnInput,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::common::{build_worker_prompt, WorkerRunConfig};
use super::runner::{WorkerConfig, WorkerRunner};
use crate::backend::lash_tools::{attach_embedded_mcp_servers, embedded_tool_plugin_factories};
use crate::backend::state::{SQLiteState, ToolCallStatus};
use crate::backend::{llm_provider, Config};

macro_rules! tool_definition {
    ($($field:tt)*) => {
        ToolDefinition {
            $($field)*
            input_schema_override: None,
            output_schema_override: None,
        }
    };
}

struct WorkerToolProvider {
    runner: Arc<WorkerRunner>,
    cancel: CancellationToken,
}

impl WorkerToolProvider {
    fn new(runner: Arc<WorkerRunner>, cancel: CancellationToken) -> Self {
        Self { runner, cancel }
    }

    fn json_strings(args: &serde_json::Value, key: &str) -> Vec<String> {
        args.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn string_arg<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("{} is required", key))
    }

    fn ok_json_or_string(value: String) -> ToolResult {
        match serde_json::from_str::<serde_json::Value>(&value) {
            Ok(parsed) => ToolResult::ok(parsed),
            Err(_) => ToolResult::ok(json!(value)),
        }
    }

    fn from_worker_result(result: crate::worker::runner::WorkerResult<String>) -> ToolResult {
        match result {
            Ok(value) => Self::ok_json_or_string(value),
            Err(e) => ToolResult::err(json!({"error": e.to_string()})),
        }
    }

    fn time_status(&self) -> ToolResult {
        match self.runner.get_time_info() {
            Ok(Some(info)) => ToolResult::ok(json!({
                "time_limit_minutes": info.limit_minutes,
                "elapsed_minutes": info.elapsed_minutes,
                "remaining_minutes": info.remaining_minutes,
                "percent_elapsed": info.percent_elapsed,
                "percent_remaining": 100.0 - info.percent_elapsed,
            })),
            Ok(None) => ToolResult::ok(json!({
                "message": "No time limit set for this run"
            })),
            Err(e) => ToolResult::err(json!({"error": e.to_string()})),
        }
    }
}

#[async_trait::async_trait]
impl ToolProvider for WorkerToolProvider {
    fn definitions(&self) -> Vec<ToolDefinition> {
        vec![
            tool_definition! {
                name: "get_task_tree".into(),
                description: "Get full task tree with status/dependencies".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "get_available_tasks".into(),
                description: "Get tasks ready to work on".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "get_my_tasks".into(),
                description: "Get currently assigned tasks".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "get_task_details".into(),
                description: "Get details for a specific task".into(),
                params: vec![ToolParam::typed("task_id", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "complete_task".into(),
                description: "Complete task and end worker turn".into(),
                params: vec![ToolParam::optional("task_id", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "add_task".into(),
                description: "Add a task".into(),
                params: vec![
                    ToolParam::typed("task_id", "str"),
                    ToolParam::typed("name", "str"),
                    ToolParam::optional("parent", "str"),
                    ToolParam::optional("blocked_by", "list"),
                ],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "add_check".into(),
                description: "Add a check node".into(),
                params: vec![
                    ToolParam::typed("check_id", "str"),
                    ToolParam::typed("name", "str"),
                    ToolParam::optional("parent", "str"),
                    ToolParam::optional("validates", "list"),
                ],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "delete_task".into(),
                description: "Delete worker-created task".into(),
                params: vec![ToolParam::typed("task_id", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "report_progress".into(),
                description: "Report important progress upward to the orchestrator without blocking execution.".into(),
                params: vec![
                    ToolParam::typed("summary", "str"),
                    ToolParam::optional("details", "str"),
                ],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "raise_concern".into(),
                description: "Raise a structured concern to the orchestrator. Use this when you hit a blocker, detect risk, or want review. Set blocking=true if you need a decision before continuing.".into(),
                params: vec![
                    ToolParam::typed("kind", "str"),
                    ToolParam::typed("summary", "str"),
                    ToolParam::optional("details", "str"),
                    ToolParam::optional("severity", "str"),
                    ToolParam::optional("blocking", "bool"),
                ],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "request_decision".into(),
                description: "Escalate a decision that requires orchestrator or user input. This pauses your work until the orchestrator resolves it.".into(),
                params: vec![
                    ToolParam::typed("summary", "str"),
                    ToolParam::optional("details", "str"),
                ],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "scribe".into(),
                description: "Record durable project context".into(),
                params: vec![ToolParam::typed("content", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "read_retained_context".into(),
                description: "Read retained project context".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "work_done".into(),
                description: "Complete work, exit, and return control to the orchestrator".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "time_status".into(),
                description: "Get time limit status".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "check_pass".into(),
                description: "Pass current check".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            tool_definition! {
                name: "check_fail".into(),
                description: "Fail current check with feedback".into(),
                params: vec![ToolParam::typed("feedback", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
        ]
    }

    async fn execute(&self, name: &str, args: &serde_json::Value) -> ToolResult {
        match name {
            "get_task_tree" => Self::from_worker_result(self.runner.get_task_tree()),
            "get_available_tasks" => Self::from_worker_result(self.runner.get_available_tasks()),
            "get_my_tasks" => Self::from_worker_result(self.runner.get_my_tasks()),
            "get_task_details" => match Self::string_arg(args, "task_id") {
                Ok(task_id) => Self::from_worker_result(self.runner.get_task_details(task_id)),
                Err(e) => ToolResult::err(json!({"error": e})),
            },
            "complete_task" => {
                let result = self
                    .runner
                    .task_done(args.get("task_id").and_then(|v| v.as_str()));
                if result.is_ok() {
                    self.cancel.cancel();
                }
                Self::from_worker_result(result)
            }
            "add_task" => {
                let task_id = match Self::string_arg(args, "task_id") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let name = match Self::string_arg(args, "name") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let parent = args.get("parent").and_then(|v| v.as_str());
                let blocked_by = Self::json_strings(args, "blocked_by");
                Self::from_worker_result(self.runner.task_add(task_id, name, parent, &blocked_by))
            }
            "add_check" => {
                let check_id = match Self::string_arg(args, "check_id") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let name = match Self::string_arg(args, "name") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let parent = args.get("parent").and_then(|v| v.as_str());
                let validates = Self::json_strings(args, "validates");
                Self::from_worker_result(self.runner.add_check(check_id, name, parent, &validates))
            }
            "delete_task" => match Self::string_arg(args, "task_id") {
                Ok(task_id) => Self::from_worker_result(self.runner.delete_task(task_id)),
                Err(e) => ToolResult::err(json!({"error": e})),
            },
            "report_progress" => {
                let summary = match Self::string_arg(args, "summary") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let details = args.get("details").and_then(|v| v.as_str());
                Self::from_worker_result(self.runner.report_progress(summary, details))
            }
            "raise_concern" => {
                let kind = match Self::string_arg(args, "kind") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let summary = match Self::string_arg(args, "summary") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let details = args.get("details").and_then(|v| v.as_str());
                let severity = args.get("severity").and_then(|v| v.as_str());
                let blocking = args
                    .get("blocking")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Self::from_worker_result(
                    self.runner
                        .raise_concern(kind, summary, details, severity, blocking),
                )
            }
            "request_decision" => {
                let summary = match Self::string_arg(args, "summary") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let details = args.get("details").and_then(|v| v.as_str());
                Self::from_worker_result(self.runner.request_decision(summary, details))
            }
            "scribe" => match Self::string_arg(args, "content") {
                Ok(content) => Self::from_worker_result(self.runner.scribe(content)),
                Err(e) => ToolResult::err(json!({"error": e})),
            },
            "read_retained_context" => {
                Self::from_worker_result(self.runner.read_retained_context())
            }
            "work_done" => {
                let result = self.runner.task_done(None);
                if result.is_ok() {
                    self.cancel.cancel();
                }
                Self::from_worker_result(result)
            }
            "time_status" => self.time_status(),
            "check_pass" => Self::from_worker_result(self.runner.check_pass()),
            "check_fail" => match Self::string_arg(args, "feedback") {
                Ok(feedback) => Self::from_worker_result(self.runner.check_fail(feedback)),
                Err(e) => ToolResult::err(json!({"error": e})),
            },
            _ => ToolResult::err(json!({"error": format!("Unknown tool: {}", name)})),
        }
    }
}

struct DbEventSink {
    runtime_name: String,
    worker_name: String,
    tool_seq: AtomicU64,
}

impl DbEventSink {
    fn new(runtime_name: String, worker_name: String) -> Self {
        Self {
            runtime_name,
            worker_name,
            tool_seq: AtomicU64::new(1),
        }
    }

    async fn state(&self) -> Option<SQLiteState> {
        match SQLiteState::new(&self.runtime_name).await {
            Ok(state) => Some(state),
            Err(e) => {
                warn!(
                    "[{}] failed to open SQLiteState for events: {}",
                    self.worker_name, e
                );
                None
            }
        }
    }

    fn tool_kind(name: &str) -> Option<&'static str> {
        if matches!(
            name,
            "read_file" | "ls" | "glob" | "grep" | "read_retained_context"
        ) {
            Some("read")
        } else if matches!(
            name,
            "apply_patch" | "add_task" | "add_check" | "delete_task"
        ) {
            Some("edit")
        } else if matches!(name, "exec_command" | "write_stdin" | "shell") {
            Some("execute")
        } else if matches!(
            name,
            "get_task_tree"
                | "get_task_details"
                | "get_available_tasks"
                | "get_my_tasks"
                | "report_progress"
                | "raise_concern"
                | "request_decision"
        ) {
            Some("search")
        } else {
            None
        }
    }
}

#[async_trait::async_trait]
impl EventSink for DbEventSink {
    async fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta { content } => {
                if let Some(state) = self.state().await {
                    let _ = state.insert_text_event(&self.worker_name, &content).await;
                }
            }
            AgentEvent::CodeBlock { code } => {
                if let Some(state) = self.state().await {
                    let _ = state.insert_thought_event(&self.worker_name, &code).await;
                }
            }
            AgentEvent::ToolCall {
                name,
                args,
                result,
                success,
                ..
            } => {
                if let Some(state) = self.state().await {
                    let id = format!(
                        "lash-tool-{}",
                        self.tool_seq.fetch_add(1, Ordering::Relaxed)
                    );
                    let status = if success {
                        ToolCallStatus::Completed
                    } else {
                        ToolCallStatus::Failed
                    };
                    let input = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                    let output =
                        serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());

                    let _ = state
                        .insert_tool_start_event(
                            &self.worker_name,
                            &id,
                            &name,
                            Self::tool_kind(&name),
                            status,
                            Some(&input),
                        )
                        .await;
                    let _ = state
                        .insert_tool_update_event(
                            &self.worker_name,
                            &id,
                            Some(&name),
                            Some(status),
                            Some(&output),
                        )
                        .await;
                }
            }
            AgentEvent::Message { text, kind } => {
                if kind == "final" {
                    if let Some(state) = self.state().await {
                        let _ = state.insert_text_event(&self.worker_name, &text).await;
                    }
                }
            }
            AgentEvent::Error { message, .. } => {
                if let Some(state) = self.state().await {
                    let _ = state
                        .insert_text_event(&self.worker_name, &format!("[error] {}", message))
                        .await;
                }
            }
            AgentEvent::LlmRequest { .. }
            | AgentEvent::LlmResponse { .. }
            | AgentEvent::TokenUsage { .. }
            | AgentEvent::RetryStatus { .. }
            | AgentEvent::InjectedMessagesCommitted { .. }
            | AgentEvent::PluginEvent { .. }
            | AgentEvent::DurableSnapshot { .. }
            | AgentEvent::Prompt { .. }
            | AgentEvent::CodeOutput { .. }
            | AgentEvent::Done => {}
        }
    }
}

pub async fn run_worker(config: WorkerRunConfig) -> anyhow::Result<()> {
    info!(
        "[{}] starting lash worker for run={} in {}",
        config.worker_name,
        config.runtime_name,
        config.work_dir.display()
    );

    let worker_runner = Arc::new(WorkerRunner::new(WorkerConfig::new(
        config.worker_name.clone(),
        config.runtime_name.clone(),
        config.runtime_dir.clone(),
        config.agent_command.clone(),
    ))?);

    let cancel = CancellationToken::new();
    let worker_tools: Arc<dyn ToolProvider> =
        Arc::new(WorkerToolProvider::new(worker_runner, cancel.clone()));

    let (hirsel_config, _) = Config::load().context("failed to load Hirsel config")?;
    let provider = llm_provider::resolve_provider(&hirsel_config)
        .await
        .map_err(anyhow::Error::msg)?;

    let (model, model_variant) = provider
        .default_agent_model("high")
        .map(|(m, variant)| (m.to_string(), variant.map(str::to_string)))
        .unwrap_or_else(|| {
            let model = provider.default_model().to_string();
            let variant = provider.default_model_variant(&model).map(str::to_string);
            (model, variant)
        });
    let execution_mode = default_execution_mode();
    let tavily_api_key = std::env::var("TAVILY_API_KEY").ok();
    let plugin_factories = embedded_tool_plugin_factories(
        "hirsel_worker_tools",
        Arc::clone(&worker_tools),
        tavily_api_key,
    );
    let plugin_host = PluginHost::new(plugin_factories).with_dynamic_tools();
    let root_plugins = plugin_host
        .build_session("root", execution_mode, None)
        .map_err(|e| anyhow::anyhow!("failed to build worker tool session: {}", e))?;
    let dynamic_tools = root_plugins
        .dynamic_tools()
        .ok_or_else(|| anyhow::anyhow!("worker dynamic tool provider was not initialized"))?;
    attach_embedded_mcp_servers(&dynamic_tools, &hirsel_config.mcp_servers)
        .await
        .map_err(anyhow::Error::msg)?;
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::backend::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(config.worker_name.clone()),
        execution_mode,
        context_strategy: default_context_strategy(),
        ..Default::default()
    };
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(config.work_dir.clone()),
        ..RuntimeHostConfig::default()
    };
    let mut runtime = LashRuntime::from_state(
        session_policy.clone(),
        host_config,
        RuntimeServices::new(root_plugins),
        AgentStateEnvelope {
            agent_id: config.worker_name.clone(),
            policy: session_policy,
            ..AgentStateEnvelope::default()
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("failed to create worker lash runtime: {}", e))?;

    let prompt = build_worker_prompt(
        &config.worker_name,
        &config.runtime_name,
        config.teammates.as_deref(),
        &config.work_dir,
        &config.runtime_dir,
        config.assigned_task_id.as_deref(),
        config.is_plan_task,
    );

    let sink = DbEventSink::new(config.runtime_name.clone(), config.worker_name.clone());
    let turn = runtime
        .stream_turn(
            TurnInput {
                items: vec![InputItem::Text { text: prompt }],
                image_blobs: Default::default(),
                mode: None,
            },
            &sink,
            cancel,
        )
        .await
        .map_err(|e| anyhow::anyhow!("failed to run lash worker turn: {}", e))?;

    debug!(
        "[{}] lash worker finished (status={:?}, reason={:?}, assistant_safe_len={})",
        config.worker_name,
        turn.status,
        turn.done_reason,
        turn.assistant_output.safe_text.len()
    );

    Ok(())
}
