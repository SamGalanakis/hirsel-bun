//! lash-based worker runtime.
//!
//! Embedded lash worker runtime.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use lash::plugin::StaticPluginFactory;
use lash::{
    default_context_strategy, default_execution_mode, default_tool_plugin_factories, AgentEvent,
    AgentStateEnvelope, DefaultToolPluginDeps, EventSink, FsInstructionSource, HostProfile,
    InputItem, LashRuntime, PluginHost, PluginSpec, RuntimeHostConfig, RuntimeServices,
    SessionPolicy, ToolDefinition, ToolParam, ToolProvider, ToolResult, TurnInput,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::common::{build_worker_prompt, WorkerRunConfig};
use super::runner::{WorkerConfig, WorkerRunner};
use crate::core::state::{SQLiteState, ToolCallStatus};
use crate::core::{llm_provider, Config};

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
            ToolDefinition {
                name: "get_task_tree".into(),
                description: "Get full task tree with status/dependencies".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "get_available_tasks".into(),
                description: "Get tasks ready to work on".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "get_my_tasks".into(),
                description: "Get currently assigned tasks".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "get_task_details".into(),
                description: "Get details for a specific task".into(),
                params: vec![ToolParam::typed("task_id", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "complete_task".into(),
                description: "Complete task and end worker turn".into(),
                params: vec![ToolParam::optional("task_id", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
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
            ToolDefinition {
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
            ToolDefinition {
                name: "delete_task".into(),
                description: "Delete worker-created task".into(),
                params: vec![ToolParam::typed("task_id", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "list_contacts".into(),
                description: "List message contacts".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "chat_history".into(),
                description: "Read chat history".into(),
                params: vec![
                    ToolParam::optional("with", "str"),
                    ToolParam::optional("limit", "int"),
                ],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "chat_send".into(),
                description: "Send chat message".into(),
                params: vec![
                    ToolParam::typed("to", "str"),
                    ToolParam::typed("message", "str"),
                ],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "chat_unread".into(),
                description: "Get unread chat".into(),
                params: vec![ToolParam::optional("with", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "scribe".into(),
                description: "Record durable project context".into(),
                params: vec![ToolParam::typed("content", "str")],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "read_retained_context".into(),
                description: "Read retained project context".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "work_done".into(),
                description: "Complete work and exit".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "time_status".into(),
                description: "Get time limit status".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
                name: "check_pass".into(),
                description: "Pass current check".into(),
                params: vec![],
                returns: "dict".into(),
                examples: vec![],
                enabled: true,
                injected: true,
            },
            ToolDefinition {
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
            "list_contacts" => Self::from_worker_result(self.runner.list_contacts()),
            "chat_history" => {
                let with = args.get("with").and_then(|v| v.as_str());
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                Self::from_worker_result(self.runner.chat_history(with, limit))
            }
            "chat_send" => {
                let to = match Self::string_arg(args, "to") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                let message = match Self::string_arg(args, "message") {
                    Ok(v) => v,
                    Err(e) => return ToolResult::err(json!({"error": e})),
                };
                Self::from_worker_result(self.runner.chat_send(to, message))
            }
            "chat_unread" => {
                let with = args.get("with").and_then(|v| v.as_str());
                Self::from_worker_result(self.runner.chat_unread(with))
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
    run_name: String,
    worker_name: String,
    tool_seq: AtomicU64,
}

impl DbEventSink {
    fn new(run_name: String, worker_name: String) -> Self {
        Self {
            run_name,
            worker_name,
            tool_seq: AtomicU64::new(1),
        }
    }

    async fn state(&self) -> Option<SQLiteState> {
        match SQLiteState::new(&self.run_name).await {
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
            "get_task_tree" | "get_task_details" | "get_available_tasks" | "get_my_tasks"
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
        config.run_name,
        config.work_dir.display()
    );

    let worker_runner = Arc::new(WorkerRunner::new(WorkerConfig::new(
        config.worker_name.clone(),
        config.run_name.clone(),
        config.run_dir.clone(),
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
    let instruction_source = Arc::new(FsInstructionSource::new());
    let mut plugin_factories = default_tool_plugin_factories(
        execution_mode,
        DefaultToolPluginDeps {
            tavily_api_key: None,
            prompt_bridge: None,
            instruction_source: Some(instruction_source),
        },
    );
    plugin_factories.push(Arc::new(StaticPluginFactory::new(
        "hirsel_worker_tools",
        PluginSpec::new().with_tool_provider(Arc::clone(&worker_tools)),
    )));
    let plugin_host = PluginHost::new(plugin_factories);
    let root_plugins = plugin_host
        .build_session("root", execution_mode, None)
        .map_err(|e| anyhow::anyhow!("failed to build worker tool session: {}", e))?;
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::core::config::get_context_window(&model) as usize),
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
        &config.run_name,
        config.teammates.as_deref(),
        &config.work_dir,
        &config.run_dir,
        config.assigned_task_id.as_deref(),
        config.is_plan_task,
    );

    let sink = DbEventSink::new(config.run_name.clone(), config.worker_name.clone());
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
