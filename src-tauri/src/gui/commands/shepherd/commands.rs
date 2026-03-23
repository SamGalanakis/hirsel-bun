use std::collections::HashMap;
use std::sync::Arc;

use lash::{
    default_context_strategy, default_execution_mode, AgentStateEnvelope, HostProfile, InputItem,
    OutputState, PluginHost, RuntimeHostConfig, RuntimeServices, SessionPolicy, ToolProvider,
    TurnInput, TurnStatus,
};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::history::{
    build_runtime_messages, build_user_chunks, chunks_to_json, clear_scope_messages,
    decode_png_images, load_scope_messages, save_message, validate_chunks, RUNTIME_HISTORY_LIMIT,
};
use super::runtime::{
    build_assistant_chunks, build_scope, build_user_turn_text, load_shepherd_provider,
    looks_like_runtime_traceback, resolve_runtime_cwd, resolve_scope_project_id,
    resolve_scope_workspace, shepherd_prompt_overrides, AssistantDraft, ShepherdEvent,
    ShepherdLashSink,
};
use super::session::{sessions, ShepherdSession};
use super::tools::ShepherdToolProvider;
use super::types::{
    ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus, StartShepherdSessionRequest,
    StartShepherdSessionResponse,
};
use crate::core::credentials::CredentialStore;
use crate::core::ShepherdChatMessage;
use crate::lash_tools::embedded_tool_plugin_factories;

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

/// Send a message to Shepherd and stream a lash response.
#[tauri::command]
pub async fn send_shepherd_message(
    app: tauri::AppHandle,
    session_id: String,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
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
            ShepherdScope::Project { focus, .. } => focus.clone(),
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
        let prompt_overrides = shepherd_prompt_overrides(&scope, focus.as_ref(), &cwd);

        if session_runtime.is_none() {
            let tools: Arc<dyn ToolProvider> =
                Arc::new(ShepherdToolProvider::new(app.clone(), scope_project_id));
            let provider = load_shepherd_provider().await?;
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
            let tavily_api_key = match CredentialStore::open().await {
                Ok(store) => store.load("tavily_api_key").await.ok(),
                Err(_) => None,
            };
            let plugin_factories =
                embedded_tool_plugin_factories("hirsel_shepherd_tools", Arc::clone(&tools), tavily_api_key);
            let plugin_host = PluginHost::new(plugin_factories);
            let root_plugins = plugin_host
                .build_session("root", execution_mode, None)
                .map_err(|e| format!("failed to build shepherd tool session: {}", e))?;
            let session_policy = SessionPolicy {
                model: model.clone(),
                provider,
                max_context_tokens: Some(crate::core::config::get_context_window(&model) as usize),
                model_variant,
                session_id: Some(session_id.clone()),
                execution_mode,
                context_strategy,
                ..Default::default()
            };
            let host_config = RuntimeHostConfig {
                host_profile: HostProfile::Embedded,
                base_dir: Some(cwd.clone()),
                prompt_overrides,
                ..RuntimeHostConfig::default()
            };
            let state = AgentStateEnvelope {
                agent_id: format!("shepherd-{}", session_id),
                policy: session_policy.clone(),
                messages: build_runtime_messages(&history),
                ..AgentStateEnvelope::default()
            };

            let runtime = lash::LashRuntime::from_state(
                session_policy,
                host_config,
                RuntimeServices::new(root_plugins),
                state,
            )
            .await
            .map_err(|e| format!("failed to create shepherd lash runtime: {}", e))?;
            session_runtime = Some(runtime);
        }

        let runtime = session_runtime
            .as_mut()
            .ok_or_else(|| "failed to initialize shepherd runtime".to_string())?;

        save_message(&scope, "user", &user_chunks_json).await?;

        let draft = Arc::new(Mutex::new(AssistantDraft::default()));
        let sink = ShepherdLashSink::new(app.clone(), session_id.clone(), draft.clone());

        let mut turn_items = vec![InputItem::Text {
            text: user_turn_text.clone(),
        }];
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
