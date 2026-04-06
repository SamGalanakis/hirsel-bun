use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use lash::tools::StandardShell;
use lash::tools::UpdatePlanTool;
use lash::{
    default_context_strategy, default_execution_mode, AgentEvent, AgentStateEnvelope, EventSink,
    ExecutionMode, HostProfile, InputItem, LashRuntime, PluginError, PluginFactory, PluginHost,
    PluginRegistrar, PluginSessionContext, PluginSnapshotMeta, PromptContribution,
    RuntimeHostConfig, RuntimeServices, SessionPlugin, SessionPolicy, SnapshotReader,
    SnapshotWriter, ToolProvider, TurnInput,
};
use serde_json::Value;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use super::history::{build_runtime_messages, decode_png_images, RUNTIME_HISTORY_LIMIT};
use super::rpc::{
    read_json_line, send_server_control_request, write_json_line, ProxyHttpRequest,
    ProxyHttpResponse, ServerControlRequest, WorkerReply, WorkerRequest, WorkerStreamEvent,
};
use super::runtime::{
    build_user_turn_text, resolve_scope_project_id, resolve_scope_workspace,
    shepherd_prompt_overrides,
};
use super::shell::ShepherdShellToolProvider;
use super::tools::{LibrarianToolProvider, ShepherdToolProvider};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::app::ResultExt;
use crate::backend::lash_tools::{
    attach_embedded_mcp_servers, embedded_tool_plugin_factories, EmbeddedCustomToolPlugin,
    EmbeddedToolPreset,
};
use crate::backend::llm_provider;
use crate::backend::{ShepherdChatMessage, ShepherdChatStore, ShepherdThreadStore};

fn scope_storage_ids(scope: &ShepherdScope) -> (Option<i64>, Option<String>) {
    match scope {
        ShepherdScope::General => (None, None),
        ShepherdScope::Shepherd { project_id, .. } => (
            Some(*project_id),
            Some(ShepherdChatStore::shepherd_scope_key(*project_id)),
        ),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => (
            Some(*project_id),
            Some(ShepherdThreadStore::scope_key(thread_id)),
        ),
        ShepherdScope::Librarian { project_id, .. } => (
            Some(*project_id),
            Some(ShepherdChatStore::librarian_scope_key(*project_id)),
        ),
    }
}

fn tool_title_kind(name: &str) -> (String, Option<String>) {
    match name {
        "list_threads" => ("Threads".to_string(), Some("search".to_string())),
        "create_thread" => ("Create Thread".to_string(), Some("execute".to_string())),
        "rename_thread" => ("Rename Thread".to_string(), Some("edit".to_string())),
        "set_thread_status" => ("Thread Status".to_string(), Some("edit".to_string())),
        "archive_thread" => ("Archive Thread".to_string(), Some("execute".to_string())),
        "promote_thread" => ("Promote Thread".to_string(), Some("execute".to_string())),
        "delete_thread" => ("Delete Thread".to_string(), Some("execute".to_string())),
        "send_thread_message" => ("Message Thread".to_string(), Some("execute".to_string())),
        "read_thread_updates" => ("Thread Updates".to_string(), Some("search".to_string())),
        "forward_port" => ("Forward Port".to_string(), Some("execute".to_string())),
        "list_port_forwards" => ("Port Forwards".to_string(), Some("search".to_string())),
        "close_port_forward" => (
            "Close Port Forward".to_string(),
            Some("execute".to_string()),
        ),
        "read_project_retained_context" => {
            ("Retained Context".to_string(), Some("read".to_string()))
        }
        "update_project_retained_context" => (
            "Retained Context Update".to_string(),
            Some("edit".to_string()),
        ),
        "graph_surql" => (
            "Knowledge Graph Query".to_string(),
            Some("execute".to_string()),
        ),
        "edit_graph_node_text" => (
            "Knowledge Graph Text Patch".to_string(),
            Some("edit".to_string()),
        ),
        "update_plan" => ("Plan Update".to_string(), Some("edit".to_string())),
        _ => (name.to_string(), None),
    }
}

fn sanitize_assistant_text(text: &str) -> String {
    let out = text.replace("</repl>", "").replace("<repl>", "");
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let repl_only = trimmed.contains('<')
        && trimmed.chars().all(|ch| {
            matches!(
                ch.to_ascii_lowercase(),
                '<' | '>' | '/' | 'r' | 'e' | 'p' | 'l' | ' '
            )
        });
    if repl_only {
        String::new()
    } else {
        out
    }
}

fn result_summary_from_chunks(chunks: &[ShepherdMessageChunk]) -> String {
    let joined = chunks
        .iter()
        .filter_map(|chunk| match chunk {
            ShepherdMessageChunk::Text { content } => Some(content.trim()),
            _ => None,
        })
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        return "No summary available yet.".to_string();
    }
    let mut summary = trimmed.chars().take(240).collect::<String>();
    if trimmed.chars().count() > 240 {
        summary.push_str("...");
    }
    summary
}

fn plan_tracker_prompt_contributions() -> Vec<PromptContribution> {
    vec![PromptContribution::guidance(
        "### `update_plan`\nUse `update_plan` for substantial multi-step work. Keep the plan short and concrete, maintain exactly one `in_progress` step, and mark steps completed as soon as they are done.",
    )]
}

struct EmbeddedPlanTrackerPluginFactory;

impl PluginFactory for EmbeddedPlanTrackerPluginFactory {
    fn id(&self) -> &'static str {
        "plan_tracker"
    }

    fn build(&self, _ctx: &PluginSessionContext) -> Result<Arc<dyn SessionPlugin>, PluginError> {
        Ok(Arc::new(EmbeddedPlanTrackerPlugin {
            tools: Arc::new(UpdatePlanTool::default()),
        }))
    }
}

struct EmbeddedPlanTrackerPlugin {
    tools: Arc<UpdatePlanTool>,
}

impl SessionPlugin for EmbeddedPlanTrackerPlugin {
    fn id(&self) -> &'static str {
        "plan_tracker"
    }

    fn register(&self, reg: &mut PluginRegistrar) -> Result<(), PluginError> {
        reg.tools()
            .provider(Arc::clone(&self.tools) as Arc<dyn ToolProvider>)?;
        reg.prompt().contribute(Arc::new(|_ctx| {
            Box::pin(async move { Ok(plan_tracker_prompt_contributions()) })
        }));
        Ok(())
    }

    fn snapshot(
        &self,
        _writer: &mut dyn SnapshotWriter,
    ) -> Result<PluginSnapshotMeta, PluginError> {
        let snapshot = self
            .tools
            .snapshot()
            .map_err(|err| PluginError::Snapshot(err.to_string()))?;
        Ok(PluginSnapshotMeta {
            plugin_id: self.id().to_string(),
            plugin_version: self.version().to_string(),
            state: Some(
                serde_json::to_value(snapshot)
                    .map_err(|err| PluginError::Snapshot(err.to_string()))?,
            ),
        })
    }

    fn restore(
        &self,
        meta: &PluginSnapshotMeta,
        _reader: &dyn SnapshotReader,
    ) -> Result<(), PluginError> {
        let snapshot = meta
            .state
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|err| PluginError::Snapshot(err.to_string()))?
            .unwrap_or_default();
        self.tools
            .restore(snapshot)
            .map_err(PluginError::Snapshot)?;
        Ok(())
    }
}

async fn build_runtime_services(
    scope: &ShepherdScope,
    default_project_id: Option<i64>,
    workspace_root: Option<PathBuf>,
    agent_id: &str,
    execution_mode: ExecutionMode,
) -> Result<RuntimeServices, String> {
    let tavily_api_key = std::env::var("TAVILY_API_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let shell_tools: Arc<dyn ToolProvider> = match scope {
        ShepherdScope::Shepherd { .. } => Arc::new(ShepherdShellToolProvider::new(
            default_project_id,
            workspace_root.clone(),
        )),
        _ => Arc::new(match workspace_root.as_ref() {
            Some(path) => StandardShell::new().with_cwd(path.clone()),
            None => StandardShell::new(),
        }),
    };
    let (tool_preset, custom_tool_plugin) = match scope {
        ShepherdScope::General => (EmbeddedToolPreset::General, None),
        ShepherdScope::Shepherd { .. } => (
            EmbeddedToolPreset::Shepherd,
            Some(EmbeddedCustomToolPlugin {
                id: "hirsel_shepherd_tools",
                provider: Arc::new(ShepherdToolProvider::new(
                    None,
                    default_project_id,
                    workspace_root.clone(),
                )) as Arc<dyn ToolProvider>,
            }),
        ),
        ShepherdScope::Thread { .. } => (EmbeddedToolPreset::Thread, None),
        ShepherdScope::Librarian { .. } => (
            EmbeddedToolPreset::Librarian,
            Some(EmbeddedCustomToolPlugin {
                id: "hirsel_librarian_tools",
                provider: Arc::new(LibrarianToolProvider::new(
                    None,
                    default_project_id,
                    workspace_root.clone(),
                )) as Arc<dyn ToolProvider>,
            }),
        ),
    };
    let mut plugin_factories = embedded_tool_plugin_factories(
        tool_preset,
        custom_tool_plugin,
        shell_tools,
        tavily_api_key,
    );
    if matches!(scope, ShepherdScope::Shepherd { .. }) {
        plugin_factories.push(Arc::new(EmbeddedPlanTrackerPluginFactory));
    }
    let plugin_host = PluginHost::new(plugin_factories).with_dynamic_tools();
    let root_plugins = plugin_host
        .build_session(agent_id, execution_mode, None)
        .map_err(|e| format!("failed to build shepherd tool session: {}", e))?;
    let dynamic_tools = root_plugins
        .dynamic_tools()
        .ok_or_else(|| "shepherd dynamic tool provider was not initialized".to_string())?;
    let (hirsel_config, _) = crate::backend::config::Config::load()
        .map_err(|e| format!("failed to load config: {}", e))?;
    attach_embedded_mcp_servers(&dynamic_tools, &hirsel_config.mcp_servers).await?;
    Ok(RuntimeServices::new(root_plugins))
}

async fn load_scope_state(scope: &ShepherdScope) -> Result<Option<AgentStateEnvelope>, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::LoadScopeState {
            scope: scope.clone(),
        })
        .await?;
        let state_json: Option<String> = serde_json::from_value(payload.unwrap_or(Value::Null))
            .map_err(|e| format!("failed to decode shepherd scope state payload: {}", e))?;
        return state_json
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| format!("failed to deserialize shepherd scope state: {}", e))
            })
            .transpose();
    }

    let (Some(project_id), Some(scope_key)) = scope_storage_ids(scope) else {
        return Ok(None);
    };
    let store = ShepherdChatStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd chat store: {}", e))?;
    let Some(state_json) = store
        .get_scope_state(project_id, &scope_key)
        .await
        .map_err(|e| format!("failed to load shepherd scope state: {}", e))?
    else {
        return Ok(None);
    };

    serde_json::from_str(&state_json)
        .map(Some)
        .map_err(|e| format!("failed to deserialize shepherd scope state: {}", e))
}

async fn load_scope_messages(
    scope: &ShepherdScope,
    limit: usize,
    skip_message_id: Option<i64>,
) -> Result<Vec<ShepherdChatMessage>, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::LoadScopeMessages {
            scope: scope.clone(),
            limit,
            skip_message_id,
        })
        .await?;
        return serde_json::from_value(payload.unwrap_or_else(|| Value::Array(Vec::new())))
            .map_err(|e| format!("failed to decode shepherd scope messages payload: {}", e));
    }

    let store = ShepherdChatStore::open().await.str_err()?;
    let mut messages = match scope {
        ShepherdScope::General => store.get_messages(None).await.str_err()?,
        ShepherdScope::Shepherd { project_id, .. } => store
            .get_scope_messages(
                Some(*project_id),
                Some(&ShepherdChatStore::shepherd_scope_key(*project_id)),
                limit,
            )
            .await
            .str_err()?,
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => store
            .get_scope_messages(
                Some(*project_id),
                Some(&ShepherdChatStore::thread_scope_key(thread_id)),
                limit,
            )
            .await
            .str_err()?,
        ShepherdScope::Librarian { project_id, .. } => store
            .get_scope_messages(
                Some(*project_id),
                Some(&ShepherdChatStore::librarian_scope_key(*project_id)),
                limit,
            )
            .await
            .str_err()?,
    };
    if let Some(skip_id) = skip_message_id {
        messages.retain(|message| message.id != skip_id);
    }
    Ok(messages)
}

async fn create_runtime_from_history(
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &Path,
    history: &[ShepherdChatMessage],
) -> Result<LashRuntime, String> {
    let settings = crate::backend::AppSettingsStore::open()
        .await
        .map_err(|e| format!("failed to open app settings store: {}", e))?
        .load_llm_settings()
        .await
        .map_err(|e| format!("failed to load llm settings: {}", e))?;
    let provider = llm_provider::resolve_provider(&settings).await?;
    let role = match scope {
        ShepherdScope::Shepherd { .. } => llm_provider::RuntimeModelRole::Shepherd,
        ShepherdScope::Thread { .. } => llm_provider::RuntimeModelRole::Thread,
        ShepherdScope::Librarian { .. } => llm_provider::RuntimeModelRole::Librarian,
        ShepherdScope::General => llm_provider::RuntimeModelRole::Shepherd,
    };
    let (model, model_variant) = llm_provider::resolve_model_for_role(&settings, &provider, role);
    let execution_mode = default_execution_mode();
    let context_strategy = default_context_strategy();
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::backend::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(runtime_id.to_string()),
        execution_mode,
        context_strategy,
        ..Default::default()
    };
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd).await,
        ..RuntimeHostConfig::default()
    };
    let state = AgentStateEnvelope {
        agent_id: format!("shepherd-{}", runtime_id),
        policy: session_policy.clone(),
        messages: build_runtime_messages(history),
        ..AgentStateEnvelope::default()
    };
    let services = build_runtime_services(
        scope,
        scope_project_id,
        Some(cwd.to_path_buf()),
        &state.agent_id,
        session_policy.execution_mode,
    )
    .await?;

    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create shepherd lash runtime: {}", e))
}

async fn create_runtime_from_state(
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &Path,
    mut state: AgentStateEnvelope,
) -> Result<LashRuntime, String> {
    state.agent_id = format!("shepherd-{}", runtime_id);
    state.policy.session_id = Some(runtime_id.to_string());
    let session_policy = state.policy.clone();
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd).await,
        ..RuntimeHostConfig::default()
    };
    let services = build_runtime_services(
        scope,
        scope_project_id,
        Some(cwd.to_path_buf()),
        &state.agent_id,
        session_policy.execution_mode,
    )
    .await?;

    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create shepherd runtime: {}", e))
}

async fn emit_stream_event(
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    event: WorkerStreamEvent,
) {
    let mut guard = writer.lock().await;
    let _ = write_json_line(&mut *guard, &WorkerReply::Event { event }).await;
}

struct StreamingRpcSink {
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    snapshot_template: AgentStateEnvelope,
}

#[async_trait::async_trait]
impl EventSink for StreamingRpcSink {
    async fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta { content } => {
                emit_stream_event(&self.writer, WorkerStreamEvent::TextDelta { content }).await;
            }
            AgentEvent::DurableSnapshot { snapshot } => {
                let mut state = self.snapshot_template.clone();
                state.messages = snapshot.messages;
                state.tool_calls = snapshot.tool_calls;
                state.iteration = snapshot.iteration;
                match serde_json::to_string(&state) {
                    Ok(state_json) => {
                        emit_stream_event(
                            &self.writer,
                            WorkerStreamEvent::DurableSnapshot { state_json },
                        )
                        .await;
                    }
                    Err(error) => {
                        emit_stream_event(
                            &self.writer,
                            WorkerStreamEvent::Error {
                                message: format!("failed to serialize durable snapshot: {}", error),
                            },
                        )
                        .await;
                    }
                }
            }
            AgentEvent::ToolCall {
                call_id,
                name,
                args,
                result,
                success,
                ..
            } => {
                let (title, kind) = tool_title_kind(&name);
                emit_stream_event(
                    &self.writer,
                    WorkerStreamEvent::Tool {
                        id: call_id.unwrap_or_else(|| name.clone()),
                        title,
                        kind,
                        status: if success {
                            "completed".to_string()
                        } else {
                            "failed".to_string()
                        },
                        input: serde_json::to_string(&args).ok(),
                        output: serde_json::to_string(&result).ok(),
                    },
                )
                .await;
            }
            AgentEvent::Message { text, kind } => {
                emit_stream_event(&self.writer, WorkerStreamEvent::Message { text, kind }).await;
            }
            AgentEvent::Error { message, .. } => {
                emit_stream_event(&self.writer, WorkerStreamEvent::Error { message }).await;
            }
            _ => {}
        }
    }
}

async fn run_scope_turn(
    scope: &ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: Option<i64>,
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    cancel: CancellationToken,
) -> Result<(Vec<ShepherdMessageChunk>, String, String, bool), String> {
    let focus = focus.or_else(|| match scope {
        ShepherdScope::Shepherd { focus, .. } | ShepherdScope::Thread { focus, .. } => {
            focus.clone()
        }
        _ => None,
    });
    let user_images_png = decode_png_images(&user_chunks)?;
    let user_turn_text = build_user_turn_text(scope, &user_chunks).await?;
    let history = load_scope_messages(scope, RUNTIME_HISTORY_LIMIT, user_message_id).await?;
    let cwd = resolve_scope_workspace(scope).await?;
    let scope_project_id = resolve_scope_project_id(scope).await;

    let runtime = if let Some(state) = load_scope_state(scope).await? {
        create_runtime_from_state(
            "worker-session",
            scope,
            focus.as_ref(),
            scope_project_id,
            &cwd,
            state,
        )
        .await?
    } else {
        create_runtime_from_history(
            "worker-session",
            scope,
            focus.as_ref(),
            scope_project_id,
            &cwd,
            &history,
        )
        .await?
    };

    let mut turn_items = Vec::new();
    turn_items.push(InputItem::Text {
        text: user_turn_text,
    });
    let mut image_blobs = std::collections::HashMap::new();
    for (idx, bytes) in user_images_png.into_iter().enumerate() {
        let id = format!("image-{}", idx + 1);
        turn_items.push(InputItem::ImageRef { id: id.clone() });
        image_blobs.insert(id, bytes);
    }

    let mut runtime = runtime;
    let sink = StreamingRpcSink {
        writer,
        snapshot_template: runtime.export_state(),
    };
    let turn = runtime
        .stream_turn(
            TurnInput {
                items: turn_items,
                image_blobs,
                mode: None,
            },
            &sink,
            cancel.clone(),
        )
        .await
        .map_err(|e| format!("failed to run shepherd turn: {}", e))?;

    let final_text = sanitize_assistant_text(&turn.assistant_output.safe_text);
    let mut assistant_chunks = Vec::new();
    if !final_text.trim().is_empty() {
        assistant_chunks.push(ShepherdMessageChunk::Text {
            content: final_text,
        });
    }
    assistant_chunks.extend(turn.tool_calls.iter().enumerate().map(|(idx, record)| {
        let (title, kind) = tool_title_kind(&record.tool);
        ShepherdMessageChunk::Tool {
            id: record
                .call_id
                .clone()
                .unwrap_or_else(|| format!("tool-{}", idx + 1)),
            title,
            kind,
            status: if record.success {
                "completed".to_string()
            } else {
                "failed".to_string()
            },
            input: serde_json::to_string(&record.args).ok(),
            output: serde_json::to_string(&record.result).ok(),
        }
    }));

    if assistant_chunks.is_empty() {
        return Err("Shepherd returned no user-visible output for this turn.".to_string());
    }

    let state_json = serde_json::to_string(&runtime.export_state())
        .map_err(|e| format!("failed to serialize shepherd scope state: {}", e))?;
    let summary = result_summary_from_chunks(&assistant_chunks);
    Ok((assistant_chunks, state_json, summary, cancel.is_cancelled()))
}

#[derive(Clone)]
struct WorkerControl {
    status: Arc<RwLock<String>>,
    cancel: Arc<Mutex<Option<CancellationToken>>>,
    shell: Arc<StandardShell>,
}

impl WorkerControl {
    fn new(cwd: PathBuf) -> Self {
        Self {
            status: Arc::new(RwLock::new(String::new())),
            cancel: Arc::new(Mutex::new(None)),
            shell: Arc::new(StandardShell::new().with_cwd(cwd)),
        }
    }

    async fn status(&self) -> String {
        self.status.read().await.clone()
    }

    async fn set_status(&self, value: &str) {
        *self.status.write().await = value.to_string();
    }

    async fn turn_is_running(&self) -> bool {
        self.cancel.lock().await.is_some()
    }
}

fn filter_proxy_headers(headers: &[(String, String)]) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "connection"
                | "host"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "content-length"
        ) {
            continue;
        }
        let Ok(header_name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) else {
            continue;
        };
        map.append(header_name, header_value);
    }
    map
}

async fn proxy_http_request(
    port: u16,
    protocol: &str,
    request: ProxyHttpRequest,
) -> Result<ProxyHttpResponse, String> {
    let scheme = match protocol {
        "http" | "https" => protocol,
        other => return Err(format!("unsupported preview protocol '{}'", other)),
    };
    let body = base64::engine::general_purpose::STANDARD
        .decode(request.body_base64)
        .map_err(|error| format!("failed to decode preview request body: {}", error))?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .map_err(|error| format!("failed to build preview client: {}", error))?;
    let url = format!("{scheme}://127.0.0.1:{port}{}", request.path_and_query);
    let method = reqwest::Method::from_bytes(request.method.as_bytes())
        .map_err(|error| format!("invalid preview method '{}': {}", request.method, error))?;
    let response = client
        .request(method, &url)
        .headers(filter_proxy_headers(&request.headers))
        .body(body)
        .send()
        .await
        .map_err(|error| format!("preview request to {} failed: {}", url, error))?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            let lower = name.as_str().to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "connection"
                    | "keep-alive"
                    | "proxy-authenticate"
                    | "proxy-authorization"
                    | "te"
                    | "trailer"
                    | "transfer-encoding"
                    | "upgrade"
                    | "content-length"
            ) {
                return None;
            }
            let value = value.to_str().ok()?.to_string();
            Some((name.as_str().to_string(), value))
        })
        .collect::<Vec<_>>();
    let body = response
        .bytes()
        .await
        .map_err(|error| format!("failed to read preview response body: {}", error))?;
    Ok(ProxyHttpResponse {
        status,
        headers,
        body_base64: base64::engine::general_purpose::STANDARD.encode(body),
    })
}

async fn handle_connection(
    scope: ShepherdScope,
    control: WorkerControl,
    stream: UnixStream,
) -> Result<(), String> {
    let (read_half, write_half) = stream.into_split();
    let writer = Arc::new(Mutex::new(write_half));
    let mut reader = BufReader::new(read_half);
    let request: WorkerRequest = read_json_line(&mut reader).await?;
    match request {
        WorkerRequest::Ping => {
            let mut guard = writer.lock().await;
            write_json_line(&mut *guard, &WorkerReply::Pong).await?;
        }
        WorkerRequest::Status => {
            let status = control.status().await;
            let mut guard = writer.lock().await;
            write_json_line(&mut *guard, &WorkerReply::Status { status }).await?;
        }
        WorkerRequest::Interrupt => {
            if let Some(token) = control.cancel.lock().await.as_ref() {
                token.cancel();
            }
            let mut guard = writer.lock().await;
            write_json_line(
                &mut *guard,
                &WorkerReply::Status {
                    status: "interrupting".to_string(),
                },
            )
            .await?;
        }
        WorkerRequest::ExecShell { args } => {
            if control.turn_is_running().await {
                let mut guard = writer.lock().await;
                write_json_line(
                    &mut *guard,
                    &WorkerReply::Error {
                        message: "This worker is already running a turn.".to_string(),
                    },
                )
                .await?;
                return Ok(());
            }
            let result = control
                .shell
                .execute_streaming("exec_command", &args, None)
                .await;
            let mut guard = writer.lock().await;
            write_json_line(
                &mut *guard,
                &WorkerReply::ToolResult {
                    success: result.success,
                    result: result.result,
                },
            )
            .await?;
        }
        WorkerRequest::WriteShell { args } => {
            if control.turn_is_running().await {
                let mut guard = writer.lock().await;
                write_json_line(
                    &mut *guard,
                    &WorkerReply::Error {
                        message: "This worker is already running a turn.".to_string(),
                    },
                )
                .await?;
                return Ok(());
            }
            let result = control
                .shell
                .execute_streaming("write_stdin", &args, None)
                .await;
            let mut guard = writer.lock().await;
            write_json_line(
                &mut *guard,
                &WorkerReply::ToolResult {
                    success: result.success,
                    result: result.result,
                },
            )
            .await?;
        }
        WorkerRequest::ProxyHttp {
            port,
            protocol,
            request,
        } => {
            let response = proxy_http_request(port, &protocol, request).await?;
            let mut guard = writer.lock().await;
            write_json_line(&mut *guard, &WorkerReply::ProxyHttpResponse(response)).await?;
        }
        WorkerRequest::RunTurn {
            user_chunks,
            focus,
            user_message_id,
        } => {
            {
                let mut cancel = control.cancel.lock().await;
                if cancel.is_some() {
                    let mut guard = writer.lock().await;
                    write_json_line(
                        &mut *guard,
                        &WorkerReply::Error {
                            message: "This worker is already running a turn.".to_string(),
                        },
                    )
                    .await?;
                    return Ok(());
                }
                *cancel = Some(CancellationToken::new());
            }
            control.set_status("running").await;
            {
                let mut guard = writer.lock().await;
                write_json_line(&mut *guard, &WorkerReply::Accepted).await?;
            }
            let cancel = control
                .cancel
                .lock()
                .await
                .as_ref()
                .cloned()
                .expect("cancel token should exist");
            let result = run_scope_turn(
                &scope,
                user_chunks,
                focus,
                user_message_id,
                Arc::clone(&writer),
                cancel,
            )
            .await;
            *control.cancel.lock().await = None;
            match result {
                Ok((assistant_chunks, state_json, summary, interrupted)) => {
                    control.set_status("idle").await;
                    let mut guard = writer.lock().await;
                    write_json_line(
                        &mut *guard,
                        &WorkerReply::Finished {
                            assistant_chunks,
                            state_json,
                            summary,
                            interrupted,
                        },
                    )
                    .await?;
                }
                Err(error) => {
                    control.set_status("failed").await;
                    let mut guard = writer.lock().await;
                    write_json_line(&mut *guard, &WorkerReply::Error { message: error }).await?;
                }
            }
        }
    }
    Ok(())
}

pub async fn serve_worker_session(scope_file: &Path, socket_path: &Path) -> Result<(), String> {
    let bytes = tokio::fs::read(scope_file)
        .await
        .map_err(|error| format!("failed to read scope file: {}", error))?;
    let scope: ShepherdScope = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid shepherd scope json: {}", error))?;

    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("failed to create worker socket dir: {}", error))?;
    }
    if socket_path.exists() {
        let _ = tokio::fs::remove_file(socket_path).await;
    }

    let listener = UnixListener::bind(socket_path)
        .map_err(|error| format!("failed to bind worker socket: {}", error))?;
    let control = WorkerControl::new(resolve_scope_workspace(&scope).await?);
    control.set_status("idle").await;

    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|error| format!("failed to accept worker socket connection: {}", error))?;
        let scope = scope.clone();
        let control = control.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(scope, control, stream).await {
                tracing::warn!(%error, "worker rpc request failed");
            }
        });
    }
}
