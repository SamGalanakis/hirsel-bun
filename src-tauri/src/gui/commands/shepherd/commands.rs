use std::collections::HashMap;
use std::sync::Arc;

use lash::{
    default_context_strategy, default_execution_mode, AgentStateEnvelope, EventSink, ExecutionMode,
    HostProfile, InputItem, LashRuntime, OutputState, PluginHost, RuntimeHostConfig,
    RuntimeServices, SessionPolicy, ToolProvider, TurnInput, TurnStatus,
};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::history::{
    build_runtime_messages, build_user_chunks, chunks_to_json, clear_scope_messages,
    decode_png_images, load_scope_messages, save_message, validate_chunks, RUNTIME_HISTORY_LIMIT,
};
use super::runtime::{
    build_assistant_chunks, build_scope, build_user_turn_text, looks_like_runtime_traceback,
    resolve_runtime_cwd, resolve_scope_project_id, resolve_scope_workspace,
    shepherd_prompt_overrides, AssistantDraft, ShepherdEvent, ShepherdLashSink,
};
use super::session::{sessions, ShepherdSession};
use super::tools::ShepherdToolProvider;
use super::types::{
    ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus, StartShepherdSessionRequest,
    StartShepherdSessionResponse,
};
use crate::core::credentials::CredentialStore;
use crate::core::{llm_provider, ShepherdChatMessage};
use crate::lash_tools::{attach_embedded_mcp_servers, embedded_tool_plugin_factories};

struct SilentLashSink;

#[async_trait::async_trait]
impl EventSink for SilentLashSink {
    async fn emit(&self, _event: lash::AgentEvent) {}
}

/// Start a Shepherd session.
#[tauri::command]
pub async fn start_shepherd_session(
    request: StartShepherdSessionRequest,
) -> Result<StartShepherdSessionResponse, String> {
    let scope = build_scope(request);
    let session_id = uuid::Uuid::new_v4().to_string();

    sessions()
        .lock()
        .map_err(|_| "failed to lock Shepherd session map".to_string())?
        .insert(
            session_id.clone(),
            ShepherdSession {
                scope: scope.clone(),
                active_turn: None,
                runtime: None,
            },
        );

    Ok(StartShepherdSessionResponse { session_id, scope })
}

async fn load_tavily_api_key() -> Option<String> {
    match CredentialStore::open().await {
        Ok(store) => store.load("tavily_api_key").await.ok(),
        Err(_) => None,
    }
}

async fn build_runtime_services(
    app: &tauri::AppHandle,
    default_project_id: Option<i64>,
    agent_id: &str,
    execution_mode: ExecutionMode,
) -> Result<RuntimeServices, String> {
    let tools: Arc<dyn ToolProvider> =
        Arc::new(ShepherdToolProvider::new(app.clone(), default_project_id));
    let (hirsel_config, _) =
        crate::core::config::Config::load().map_err(|e| format!("failed to load config: {}", e))?;
    let plugin_factories = embedded_tool_plugin_factories(
        "hirsel_shepherd_tools",
        Arc::clone(&tools),
        load_tavily_api_key().await,
    );
    let plugin_host = PluginHost::new(plugin_factories).with_dynamic_tools();
    let root_plugins = plugin_host
        .build_session(agent_id, execution_mode, None)
        .map_err(|e| format!("failed to build shepherd tool session: {}", e))?;
    let dynamic_tools = root_plugins
        .dynamic_tools()
        .ok_or_else(|| "shepherd dynamic tool provider was not initialized".to_string())?;
    attach_embedded_mcp_servers(&dynamic_tools, &hirsel_config.mcp_servers).await?;
    Ok(RuntimeServices::new(root_plugins))
}

async fn create_runtime_from_history(
    app: &tauri::AppHandle,
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &std::path::Path,
    history: &[ShepherdChatMessage],
) -> Result<LashRuntime, String> {
    let (hirsel_config, _) =
        crate::core::config::Config::load().map_err(|e| format!("failed to load config: {}", e))?;
    let provider = llm_provider::resolve_provider(&hirsel_config).await?;
    let (model, model_variant) = provider
        .default_agent_model("high")
        .map(|(m, variant)| (m.to_string(), variant.map(str::to_string)))
        .unwrap_or_else(|| {
            let model = provider.default_model().to_string();
            let variant = provider.default_model_variant(&model).map(str::to_string);
            (model, variant)
        });
    let execution_mode = default_execution_mode();
    let context_strategy = default_context_strategy();
    let session_policy = SessionPolicy {
        model: model.clone(),
        provider,
        max_context_tokens: Some(crate::core::config::get_context_window(&model) as usize),
        model_variant,
        session_id: Some(runtime_id.to_string()),
        execution_mode,
        context_strategy,
        ..Default::default()
    };
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd),
        ..RuntimeHostConfig::default()
    };
    let state = AgentStateEnvelope {
        agent_id: format!("shepherd-{}", runtime_id),
        policy: session_policy.clone(),
        messages: build_runtime_messages(history),
        ..AgentStateEnvelope::default()
    };
    let services = build_runtime_services(
        app,
        scope_project_id,
        &state.agent_id,
        session_policy.execution_mode,
    )
    .await?;

    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create shepherd lash runtime: {}", e))
}

async fn create_runtime_from_state(
    app: &tauri::AppHandle,
    runtime_id: &str,
    scope: &ShepherdScope,
    focus: Option<&ShepherdTaskFocus>,
    scope_project_id: Option<i64>,
    cwd: &std::path::Path,
    mut state: AgentStateEnvelope,
) -> Result<LashRuntime, String> {
    state.agent_id = format!("shepherd-{}", runtime_id);
    state.policy.session_id = Some(runtime_id.to_string());
    let session_policy = state.policy.clone();
    let host_config = RuntimeHostConfig {
        host_profile: HostProfile::Embedded,
        base_dir: Some(cwd.to_path_buf()),
        prompt_overrides: shepherd_prompt_overrides(scope, focus, cwd),
        ..RuntimeHostConfig::default()
    };
    let services = build_runtime_services(
        app,
        scope_project_id,
        &state.agent_id,
        session_policy.execution_mode,
    )
    .await?;

    LashRuntime::from_state(session_policy, host_config, services, state)
        .await
        .map_err(|e| format!("failed to create shepherd branch runtime: {}", e))
}

fn truncate_internal_note(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

async fn run_private_branch_turn(
    app: &tauri::AppHandle,
    parent_session_id: &str,
    parent_scope: &ShepherdScope,
    parent_focus: Option<ShepherdTaskFocus>,
    user_turn_text: &str,
    user_images_png: &[Vec<u8>],
    cancel: tokio_util::sync::CancellationToken,
    parent_state: AgentStateEnvelope,
) -> Result<Option<String>, String> {
    let ShepherdScope::Project {
        project_id,
        workspace_path,
        ..
    } = parent_scope
    else {
        return Ok(None);
    };

    let branch_id = uuid::Uuid::new_v4().to_string();
    let branch_scope = ShepherdScope::Branch {
        project_id: *project_id,
        branch_id: branch_id.clone(),
        parent_session_id: parent_session_id.to_string(),
        goal: "Think through the latest user request, inspect Hirsel state, use tools if needed, and return a private conclusion for the channel.".to_string(),
        workspace_path: workspace_path.clone(),
        focus: parent_focus,
    };
    let cwd = resolve_runtime_cwd(resolve_scope_workspace(&branch_scope).await);
    let scope_project_id = resolve_scope_project_id(&branch_scope).await;
    let mut branch_runtime = create_runtime_from_state(
        app,
        &branch_id,
        &branch_scope,
        match &branch_scope {
            ShepherdScope::Branch { focus, .. } => focus.as_ref(),
            _ => None,
        },
        scope_project_id,
        &cwd,
        parent_state,
    )
    .await?;

    let mut turn_items = vec![InputItem::Text {
        text: user_turn_text.to_string(),
    }];
    let mut image_blobs = HashMap::new();
    for (idx, bytes) in user_images_png.iter().enumerate() {
        let id = format!("branch-image-{}", idx + 1);
        turn_items.push(InputItem::ImageRef { id: id.clone() });
        image_blobs.insert(id, bytes.clone());
    }

    let turn = branch_runtime
        .stream_turn(
            TurnInput {
                items: turn_items,
                image_blobs,
                mode: None,
            },
            &SilentLashSink,
            cancel,
        )
        .await
        .map_err(|e| format!("failed to run private shepherd branch: {}", e))?;

    let conclusion =
        super::runtime::ShepherdLashSink::sanitize_assistant_text(&turn.assistant_output.safe_text);
    if conclusion.trim().is_empty() || looks_like_runtime_traceback(&conclusion) {
        return Ok(None);
    }

    Ok(Some(truncate_internal_note(&conclusion, 4000)))
}

/// Send a message to Shepherd and stream a lash response.
async fn run_shepherd_turn(
    app: tauri::AppHandle,
    session_id: String,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
    persist_input_message: bool,
) -> Result<(), String> {
    let (scope, cancel, mut session_runtime) = {
        let mut guard = sessions()
            .lock()
            .map_err(|_| "failed to lock Shepherd session map".to_string())?;
        let session = guard
            .get_mut(&session_id)
            .ok_or_else(|| format!("Unknown Shepherd session: {}", session_id))?;

        if session.active_turn.is_some() {
            return Err("Shepherd session already has an active turn".to_string());
        }

        let cancel = tokio_util::sync::CancellationToken::new();
        session.active_turn = Some(cancel.clone());
        (session.scope.clone(), cancel, session.runtime.take())
    };

    let result = async {
        let focus = focus.or_else(|| match &scope {
            ShepherdScope::Project { focus, .. } | ShepherdScope::Branch { focus, .. } => {
                focus.clone()
            }
            _ => None,
        });

        let user_chunks = build_user_chunks(content, chunks)?;
        let user_chunks_json = chunks_to_json(&user_chunks)?;
        let user_images_png = decode_png_images(&user_chunks)?;
        let user_turn_text = build_user_turn_text(&user_chunks);

        let history = if session_runtime.is_none() {
            load_scope_messages(&scope, RUNTIME_HISTORY_LIMIT).await?
        } else {
            Vec::new()
        };
        let cwd = resolve_runtime_cwd(resolve_scope_workspace(&scope).await);
        let scope_project_id = resolve_scope_project_id(&scope).await;

        if session_runtime.is_none() {
            session_runtime = Some(
                create_runtime_from_history(
                    &app,
                    &session_id,
                    &scope,
                    focus.as_ref(),
                    scope_project_id,
                    &cwd,
                    &history,
                )
                .await?,
            );
        }

        let runtime = session_runtime
            .as_mut()
            .ok_or_else(|| "failed to initialize shepherd runtime".to_string())?;

        if persist_input_message {
            save_message(&scope, "user", &user_chunks_json).await?;
        }

        let branch_conclusion = if persist_input_message {
            let parent_state = runtime.export_state();
            match run_private_branch_turn(
                &app,
                &session_id,
                &scope,
                focus.clone(),
                &user_turn_text,
                &user_images_png,
                cancel.clone(),
                parent_state,
            )
            .await
            {
                Ok(conclusion) => conclusion,
                Err(error) => {
                    warn!("failed to run private shepherd branch: {}", error);
                    None
                }
            }
        } else {
            None
        };

        let draft = Arc::new(Mutex::new(AssistantDraft::default()));
        let sink = ShepherdLashSink::new(app.clone(), session_id.clone(), draft.clone());

        let mut turn_items = Vec::new();
        if let Some(branch_conclusion) = branch_conclusion.as_ref() {
            turn_items.push(InputItem::Text {
                text: format!(
                    "Private branch conclusion for this turn. Use it as internal reasoning context only. Do not mention branching, hidden analysis, or this note to the user.\n\n{}",
                    branch_conclusion
                ),
            });
        }
        turn_items.push(InputItem::Text {
            text: user_turn_text.clone(),
        });
        let mut image_blobs: HashMap<String, Vec<u8>> = HashMap::new();
        for (idx, bytes) in user_images_png.into_iter().enumerate() {
            let id = format!("image-{}", idx + 1);
            turn_items.push(InputItem::ImageRef { id: id.clone() });
            image_blobs.insert(id, bytes);
        }

        let mut turn = runtime
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
        let mut recovered_empty_output = false;
        let mut recovered_runtime_traceback = false;

        loop {
            info!(
                "shepherd lash turn complete: session={}, status={:?}, reason={:?}, output_state={:?}, safe_len={}, raw_len={}, errors={}",
                session_id,
                turn.status,
                turn.done_reason,
                turn.assistant_output.state,
                turn.assistant_output.safe_text.len(),
                turn.assistant_output.raw_text.len(),
                turn.errors.len()
            );

            let mut final_draft = draft.lock().await;
            let streamed_text = ShepherdLashSink::sanitize_assistant_text(final_draft.text.trim());
            let assembled_text =
                ShepherdLashSink::sanitize_assistant_text(&turn.assistant_output.safe_text);
            let runtime_output =
                ShepherdLashSink::sanitize_assistant_text(&final_draft.runtime_output);
            let final_text = if !assembled_text.trim().is_empty() {
                assembled_text.trim().to_string()
            } else if !streamed_text.trim().is_empty() {
                streamed_text
            } else {
                runtime_output.trim().to_string()
            };

            if !final_text.is_empty() {
                app.emit(
                    "shepherd-event",
                    (
                        &session_id,
                        &ShepherdEvent::TextDelta {
                            session_id: session_id.clone(),
                            text: final_text.clone(),
                        },
                    ),
                )
                .map_err(|e| format!("failed to emit shepherd text delta: {}", e))?;
                final_draft.text = final_text.clone();
            }
            info!(
                "shepherd draft summary: session={}, text_len={}, final_len={}, errored={}",
                session_id,
                final_draft.text.len(),
                final_text.len(),
                final_draft.errored
            );

            if matches!(turn.status, TurnStatus::Failed)
                && final_text.is_empty()
                && final_draft.tools.is_empty()
            {
                let message = turn
                    .errors
                    .first()
                    .map(|issue| issue.message.clone())
                    .unwrap_or_else(|| "Shepherd turn failed".to_string());
                final_draft.errored = true;
                drop(final_draft);
                app.emit(
                    "shepherd-event",
                    (
                        &session_id,
                        &ShepherdEvent::Error {
                            session_id: session_id.clone(),
                            message,
                        },
                    ),
                )
                .map_err(|e| format!("failed to emit shepherd error event: {}", e))?;
                return Ok(());
            }

            if matches!(turn.status, TurnStatus::Interrupted) {
                drop(final_draft);
                return Ok(());
            }

            if !recovered_runtime_traceback
                && matches!(turn.status, TurnStatus::Completed)
                && (matches!(turn.assistant_output.state, OutputState::TracebackOnly)
                    || looks_like_runtime_traceback(&final_text))
            {
                warn!(
                    "shepherd runtime traceback surfaced as assistant text; running one recovery pass: session={}, output_state={:?}",
                    session_id,
                    turn.assistant_output.state
                );
                *final_draft = AssistantDraft::default();
                drop(final_draft);
                recovered_runtime_traceback = true;
                turn = runtime
                    .stream_turn(
                        TurnInput {
                            items: vec![InputItem::Text {
                                text: "The previous attempt surfaced an internal runtime traceback. Answer the user's most recent message directly in plain language with no code blocks, no repl execution, and no traceback text.".to_string(),
                            }],
                            image_blobs: HashMap::new(),
                            mode: None,
                        },
                        &sink,
                        cancel.clone(),
                    )
                    .await
                    .map_err(|e| format!("failed to run shepherd traceback recovery turn: {}", e))?;
                continue;
            }

            if !recovered_empty_output
                && matches!(turn.status, TurnStatus::Completed)
                && final_text.is_empty()
                && final_draft.tools.is_empty()
            {
                warn!(
                    "shepherd empty output on completed turn; running one recovery pass: session={}, output_state={:?}, raw_assistant_output={:?}, runtime_output={:?}",
                    session_id,
                    turn.assistant_output.state,
                    turn.assistant_output.raw_text,
                    final_draft.runtime_output
                );
                *final_draft = AssistantDraft::default();
                drop(final_draft);
                recovered_empty_output = true;
                turn = runtime
                    .stream_turn(
                        TurnInput {
                            items: vec![InputItem::Text {
                                text: "Respond directly to the user's most recent message in plain language. Provide a complete, non-empty answer.".to_string(),
                            }],
                            image_blobs: HashMap::new(),
                            mode: None,
                        },
                        &sink,
                        cancel.clone(),
                    )
                    .await
                    .map_err(|e| format!("failed to run shepherd recovery turn: {}", e))?;
                continue;
            }

            if final_text.is_empty() && final_draft.tools.is_empty() {
                warn!(
                    "shepherd empty sanitized output: session={}, output_state={:?}, raw_assistant_output={:?}, runtime_output={:?}",
                    session_id,
                    turn.assistant_output.state,
                    turn.assistant_output.raw_text,
                    final_draft.runtime_output
                );
                let message = "Shepherd returned no user-visible output for this turn.".to_string();
                drop(final_draft);
                app.emit(
                    "shepherd-event",
                    (
                        &session_id,
                        &ShepherdEvent::Error {
                            session_id: session_id.clone(),
                            message,
                        },
                    ),
                )
                .map_err(|e| format!("failed to emit shepherd error event: {}", e))?;
                return Ok(());
            }

            if final_draft.errored && final_text.is_empty() && final_draft.tools.is_empty() {
                return Ok(());
            }

            let assistant_chunks = build_assistant_chunks(&final_draft, &final_text);
            if !assistant_chunks.is_empty() {
                let assistant_chunks_json = chunks_to_json(&assistant_chunks)?;
                save_message(&scope, "assistant", &assistant_chunks_json).await?;
            }

            drop(final_draft);
            break;
        }

        app.emit(
            "shepherd-event",
            (
                &session_id,
                &ShepherdEvent::MessageComplete {
                    session_id: session_id.clone(),
                },
            ),
        )
        .map_err(|e| format!("failed to emit shepherd complete event: {}", e))?;

        Ok(())
    }
    .await;

    if let Ok(mut guard) = sessions().lock() {
        if let Some(session) = guard.get_mut(&session_id) {
            session.active_turn = None;
            if let Some(runtime) = session_runtime.take() {
                session.runtime = Some(runtime);
            }
        }
    }

    result
}

/// Send a user-visible message to Shepherd and stream a lash response.
#[tauri::command]
pub async fn send_shepherd_message(
    app: tauri::AppHandle,
    session_id: String,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
) -> Result<(), String> {
    run_shepherd_turn(app, session_id, content, chunks, focus, true).await
}

/// Run a hidden background prompt against the active Shepherd session.
#[tauri::command]
pub async fn run_shepherd_background_prompt(
    app: tauri::AppHandle,
    session_id: String,
    content: String,
    focus: Option<ShepherdTaskFocus>,
) -> Result<(), String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("background prompt content is empty".to_string());
    }

    run_shepherd_turn(
        app,
        session_id,
        Some(trimmed.to_string()),
        None,
        focus,
        false,
    )
    .await
}

/// Cancel the active turn without destroying the session.
///
/// The session and its LashRuntime stay alive so the user can immediately
/// send another message.
#[tauri::command]
pub async fn cancel_shepherd_turn(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let cancelled = {
        let mut guard = sessions()
            .lock()
            .map_err(|_| "failed to lock Shepherd session map".to_string())?;

        let session = guard
            .get_mut(&session_id)
            .ok_or_else(|| format!("Unknown Shepherd session: {}", session_id))?;

        session.active_turn.take()
    };

    if let Some(cancel) = cancelled {
        cancel.cancel();
    }

    // Emit MessageComplete so the frontend finalizes the streaming message
    let event = ShepherdEvent::MessageComplete {
        session_id: session_id.clone(),
    };
    let _ = app.emit("shepherd-event", (&session_id, &event));

    Ok(())
}

/// Stop an active Shepherd session.
#[tauri::command]
pub async fn stop_shepherd_session(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<(), String> {
    let cancelled = sessions()
        .lock()
        .map_err(|_| "failed to lock Shepherd session map".to_string())?
        .remove(&session_id)
        .and_then(|s| s.active_turn);

    if let Some(cancel) = cancelled {
        cancel.cancel();
    }

    let ended = ShepherdEvent::SessionEnded {
        session_id: session_id.clone(),
    };
    app.emit("shepherd-event", (&session_id, &ended))
        .map_err(|e| format!("failed to emit shepherd session end event: {}", e))?;
    Ok(())
}

/// List active Shepherd sessions.
#[tauri::command]
pub async fn list_shepherd_sessions() -> Result<Vec<String>, String> {
    let guard = sessions()
        .lock()
        .map_err(|_| "failed to lock Shepherd session map".to_string())?;
    Ok(guard.keys().cloned().collect())
}

/// Get Shepherd chat history for the requested scope.
#[tauri::command]
pub async fn get_shepherd_history(
    scope: ShepherdScope,
    limit: usize,
) -> Result<Vec<ShepherdChatMessage>, String> {
    let messages = load_scope_messages(&scope, limit).await?;

    Ok(match scope {
        ShepherdScope::Project { .. } => messages,
        _ => messages.into_iter().take(limit).collect(),
    })
}

/// Clear Shepherd history for the requested scope.
#[tauri::command]
pub async fn clear_shepherd_history(scope: ShepherdScope) -> Result<(), String> {
    clear_scope_messages(&scope).await
}

/// Save a Shepherd message chunk payload.
#[tauri::command]
pub async fn save_shepherd_message(
    scope: ShepherdScope,
    role: String,
    chunks_json: String,
) -> Result<i64, String> {
    let chunks: Vec<ShepherdMessageChunk> =
        serde_json::from_str(&chunks_json).map_err(|e| format!("invalid chunk payload: {}", e))?;
    validate_chunks(&chunks)?;
    let normalized = chunks_to_json(&chunks)?;
    save_message(&scope, &role, &normalized).await
}
