use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex as StdMutex, OnceLock};

use lash::ToolResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::UnixListener;

use super::history::{build_user_chunks, chunks_to_json, save_message};
use super::preview::{
    close_preview_forward, close_thread_preview_forwards, list_preview_forwards,
    open_preview_forward,
};
use super::queries::{get_thread_activity, scope_activity};
use super::rpc::{
    connect_worker_socket, read_json_line, send_server_control_request, server_control_socket_path,
    write_json_line, PreviewForwardInfo, ProjectLoreEntry, ServerControlReply,
    ServerControlRequest, ServerToolResultPayload, WorkerReply, WorkerRequest, WorkerStreamEvent,
};
use super::sandbox::{
    current_server_control_socket_path, enrich_scope_disconnect_error, ensure_scope_session,
    scope_key, stop_scope_session, validate_scope_runtime,
};
use super::session::ShepherdSessionStore;
use super::tools::{execute_librarian_server_tool_local, execute_shepherd_server_tool_local};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::app::ResultExt;
use crate::backend::git::{promote_thread_checkout, GitError};
use crate::backend::{
    ensure_project_workspace, prepare_thread_checkout, ShepherdChatStore, ShepherdThread,
    ShepherdThreadStore,
};

static ACTIVE_SCOPE_TURNS: OnceLock<StdMutex<HashSet<String>>> = OnceLock::new();

fn active_scope_turns() -> &'static StdMutex<HashSet<String>> {
    ACTIVE_SCOPE_TURNS.get_or_init(|| StdMutex::new(HashSet::new()))
}

struct QueuedTurn {
    scope: ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: i64,
}

static QUEUED_TURNS: LazyLock<StdMutex<HashMap<String, QueuedTurn>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

fn queue_turn(key: &str, turn: QueuedTurn) {
    if let Ok(mut map) = QUEUED_TURNS.lock() {
        map.insert(key.to_string(), turn);
    }
}

fn take_queued_turn(key: &str) -> Option<QueuedTurn> {
    QUEUED_TURNS.lock().ok()?.remove(key)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendShepherdMessageResponse {
    pub started: bool,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteThreadResponse {
    pub promoted: bool,
    pub thread_id: String,
    pub central_head: String,
    pub changed_files: Vec<String>,
    pub env_changed: bool,
}

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

fn summary_from_chunks(chunks: &[ShepherdMessageChunk]) -> String {
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

fn interruption_notice_chunk() -> ShepherdMessageChunk {
    ShepherdMessageChunk::Notice {
        tone: "warning".to_string(),
        title: Some("Interrupted".to_string()),
        content: "Stopped manually. This response is partial.".to_string(),
    }
}

fn is_interruption_error(error: &str) -> bool {
    error.to_ascii_lowercase().contains("interrupt")
}

fn interrupted_summary_from_chunks(chunks: &[ShepherdMessageChunk]) -> String {
    let summary = summary_from_chunks(chunks);
    if summary == "No summary available yet." {
        "Interrupted by user.".to_string()
    } else {
        format!("{summary} (interrupted)")
    }
}

fn truncate_copy(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
}

#[derive(Default)]
struct LiveTurnAccumulator {
    chunks: Vec<ShepherdMessageChunk>,
    /// Set after a non-text event so the next TextDelta starts a fresh chunk
    /// instead of appending to the previous one.
    needs_text_break: bool,
}

impl LiveTurnAccumulator {
    fn push_text(&mut self, content: String) {
        if content.is_empty() {
            return;
        }
        if !self.needs_text_break {
            if let Some(ShepherdMessageChunk::Text { content: existing }) = self.chunks.last_mut() {
                existing.push_str(&content);
                return;
            }
        }
        self.needs_text_break = false;
        self.chunks.push(ShepherdMessageChunk::Text { content });
    }

    fn apply(&mut self, event: WorkerStreamEvent) {
        match event {
            WorkerStreamEvent::TextDelta { content } => {
                self.push_text(content);
            }
            WorkerStreamEvent::DurableSnapshot { .. } => {}
            WorkerStreamEvent::Tool {
                id,
                title,
                kind,
                status,
                input,
                output,
            } => {
                self.needs_text_break = true;
                if let Some(chunk) = self.chunks.iter_mut().find(|chunk| {
                    matches!(chunk, ShepherdMessageChunk::Tool { id: existing_id, .. } if existing_id == &id)
                }) {
                    *chunk = ShepherdMessageChunk::Tool {
                        id,
                        title,
                        kind,
                        status,
                        input,
                        output,
                    };
                } else {
                    self.chunks.push(ShepherdMessageChunk::Tool {
                        id,
                        title,
                        kind,
                        status,
                        input,
                        output,
                    });
                }
            }
            WorkerStreamEvent::Message { text, kind } => {
                self.needs_text_break = true;
                if kind == "final" {
                    self.chunks
                        .push(ShepherdMessageChunk::Text { content: text });
                }
            }
            WorkerStreamEvent::Error { message } => {
                let _ = message;
                self.needs_text_break = true;
            }
        }
    }

    fn chunks_json(&self) -> Result<Option<String>, String> {
        if self.chunks.is_empty() {
            return Ok(None);
        }
        chunks_to_json(&self.chunks).map(Some)
    }

    fn finalize(&mut self, fallback: Vec<ShepherdMessageChunk>) -> Vec<ShepherdMessageChunk> {
        if self.chunks.is_empty() {
            fallback
        } else {
            std::mem::take(&mut self.chunks)
        }
    }
}

fn mark_scope_active(scope_key: &str) -> Result<(), String> {
    let mut active = active_scope_turns()
        .lock()
        .map_err(|_| "failed to lock active scope set".to_string())?;
    if active.contains(scope_key) {
        return Err("This conversation is already running.".to_string());
    }
    active.insert(scope_key.to_string());
    Ok(())
}

fn clear_scope_active(scope_key: &str) {
    if let Ok(mut active) = active_scope_turns().lock() {
        active.remove(scope_key);
    }
}

pub(crate) fn clear_all_runtime_tracking() {
    if let Ok(mut active) = active_scope_turns().lock() {
        active.clear();
    }
    if let Ok(mut queued) = QUEUED_TURNS.lock() {
        queued.clear();
    }
}

async fn set_live_turn(
    scope: &ShepherdScope,
    chunks_json: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let (project_id, scope_key) = scope_storage_ids(scope);
    let Some(scope_key) = scope_key else {
        return Ok(());
    };
    let store = ShepherdChatStore::open().await.str_err()?;
    store
        .save_live_turn(
            project_id,
            &scope_key,
            "assistant",
            chunks_json,
            status,
            error,
        )
        .await
        .str_err()
}

async fn update_live_turn_status(
    scope: &ShepherdScope,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let (project_id, scope_key) = scope_storage_ids(scope);
    let Some(scope_key) = scope_key else {
        return Ok(());
    };
    let store = ShepherdChatStore::open().await.str_err()?;
    let chunks_json = store
        .get_live_turn(project_id, &scope_key)
        .await
        .str_err()?
        .map(|turn| turn.chunks_json)
        .unwrap_or_else(|| "[]".to_string());
    store
        .save_live_turn(
            project_id,
            &scope_key,
            "assistant",
            &chunks_json,
            status,
            error,
        )
        .await
        .str_err()
}

async fn clear_live_turn(scope: &ShepherdScope) -> Result<(), String> {
    let (project_id, scope_key) = scope_storage_ids(scope);
    let Some(scope_key) = scope_key else {
        return Ok(());
    };
    let store = ShepherdChatStore::open().await.str_err()?;
    store
        .clear_live_turn(project_id, &scope_key)
        .await
        .str_err()
}

async fn save_scope_state(scope: &ShepherdScope, state_json: &str) -> Result<(), String> {
    let (Some(project_id), Some(scope_key)) = scope_storage_ids(scope) else {
        return Ok(());
    };
    let store = ShepherdChatStore::open().await.str_err()?;
    store
        .save_scope_state(project_id, &scope_key, state_json)
        .await
        .str_err()
}

async fn merge_scope_state(scope: &ShepherdScope, state_json: &str) -> Result<(), String> {
    let mut incoming: lash::SessionStateEnvelope = serde_json::from_str(state_json)
        .map_err(|error| format!("failed to deserialize shepherd scope state: {}", error))?;

    if let Some(existing_json) = load_scope_state_local(scope).await? {
        let existing: lash::SessionStateEnvelope =
            serde_json::from_str(&existing_json).map_err(|error| {
                format!(
                    "failed to deserialize persisted shepherd scope state: {}",
                    error
                )
            })?;

        incoming.session_id = existing.session_id;
        if incoming.policy == lash::SessionPolicy::default() {
            incoming.policy = existing.policy;
        }
        if incoming.token_usage.total() == 0 {
            incoming.token_usage = existing.token_usage;
        }
        if incoming.last_prompt_usage.is_none() {
            incoming.last_prompt_usage = existing.last_prompt_usage;
        }
        if incoming.task_state.is_none() {
            incoming.task_state = existing.task_state;
        }
        if incoming.replay_manifest.is_none() {
            incoming.replay_manifest = existing.replay_manifest;
        }
        if incoming.plugin_snapshot.is_none() {
            incoming.plugin_snapshot = existing.plugin_snapshot;
        }
        if incoming.repl_snapshot.is_none() {
            incoming.repl_snapshot = existing.repl_snapshot;
        }
    }

    let merged_json = serde_json::to_string(&incoming)
        .map_err(|error| format!("failed to serialize shepherd scope state: {}", error))?;
    save_scope_state(scope, &merged_json).await
}

async fn load_scope_state_local(scope: &ShepherdScope) -> Result<Option<String>, String> {
    let (Some(project_id), Some(scope_key)) = scope_storage_ids(scope) else {
        return Ok(None);
    };
    let store = ShepherdChatStore::open().await.str_err()?;
    store
        .get_scope_state(project_id, &scope_key)
        .await
        .str_err()
}

async fn load_scope_messages_local(
    scope: &ShepherdScope,
    limit: usize,
    skip_message_id: Option<i64>,
) -> Result<Vec<crate::backend::ShepherdChatMessage>, String> {
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

async fn update_thread_after_turn(
    scope: &ShepherdScope,
    summary: Option<&str>,
    status: Option<&str>,
) -> Result<(), String> {
    let ShepherdScope::Thread { thread_id, .. } = scope else {
        return Ok(());
    };
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    store
        .update_thread(thread_id, None, None, summary, status, None, None)
        .await
        .map_err(|error| format!("failed to update thread {}: {}", thread_id, error))
}

async fn run_scope_turn_task(
    scope: ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: i64,
) -> Result<(), String> {
    let scope_key = scope_key(&scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let mut accumulator = LiveTurnAccumulator::default();
    let result = async {
        let session = ensure_scope_session(&scope).await?;
        store
            .set_status(&scope_key, "running", None)
            .await
            .map_err(|error| format!("failed to mark session running: {}", error))?;

        let stream = connect_worker_socket(std::path::Path::new(&session.socket_path)).await?;
        let (read_half, mut write_half) = stream.into_split();
        write_json_line(
            &mut write_half,
            &WorkerRequest::RunTurn {
                user_chunks: user_chunks.clone(),
                focus,
                user_message_id: Some(user_message_id),
            },
        )
        .await?;
        let mut reader = BufReader::new(read_half);
        loop {
            let reply: WorkerReply = read_json_line(&mut reader).await?;
            match reply {
                WorkerReply::Accepted => {}
                WorkerReply::Event { event } => {
                    match event {
                        WorkerStreamEvent::DurableSnapshot { state_json } => {
                            merge_scope_state(&scope, &state_json).await?;
                        }
                        other => accumulator.apply(other),
                    }
                    if let Some(chunks_json) = accumulator.chunks_json()? {
                        set_live_turn(&scope, &chunks_json, "running", None).await?;
                    }
                }
                WorkerReply::Finished {
                    assistant_chunks,
                    state_json,
                    summary,
                    interrupted,
                } => {
                    let mut assistant_chunks = accumulator.finalize(assistant_chunks);
                    if interrupted
                        && !assistant_chunks.iter().any(|chunk| {
                            matches!(chunk, ShepherdMessageChunk::Notice { title, content, .. }
                                if title.as_deref() == Some("Interrupted")
                                && content == "Stopped manually. This response is partial.")
                        })
                    {
                        assistant_chunks.push(interruption_notice_chunk());
                    }
                    let assistant_chunks_json = chunks_to_json(&assistant_chunks)?;
                    save_message(&scope, "assistant", &assistant_chunks_json).await?;
                    merge_scope_state(&scope, &state_json).await?;
                    clear_live_turn(&scope).await?;
                    store
                        .set_status(&scope_key, "idle", None)
                        .await
                        .map_err(|error| format!("failed to mark session idle: {}", error))?;
                    if interrupted {
                        let interrupted_summary =
                            interrupted_summary_from_chunks(&assistant_chunks);
                        update_thread_after_turn(&scope, Some(&interrupted_summary), Some("idle"))
                            .await?;
                    } else {
                        update_thread_after_turn(&scope, Some(&summary), None).await?;
                    }
                    break Ok(());
                }
                WorkerReply::Error { message } => {
                    break Err(message);
                }
                WorkerReply::Pong
                | WorkerReply::Status { .. }
                | WorkerReply::ToolResult { .. }
                | WorkerReply::ProxyHttpResponse(_) => {}
            }
        }
    }
    .await;

    let result = match result {
        Err(error) if error.contains("rpc connection closed") => {
            Err(enrich_scope_disconnect_error(&scope, &error).await)
        }
        other => other,
    };

    if let Err(error) = &result {
        if is_interruption_error(error) {
            let mut assistant_chunks = accumulator.finalize(Vec::new());
            if !assistant_chunks.iter().any(|chunk| {
                matches!(chunk, ShepherdMessageChunk::Notice { title, content, .. }
                    if title.as_deref() == Some("Interrupted")
                    && content == "Stopped manually. This response is partial.")
            }) {
                assistant_chunks.push(interruption_notice_chunk());
                if let Ok(assistant_chunks_json) = chunks_to_json(&assistant_chunks) {
                    let _ = save_message(&scope, "assistant", &assistant_chunks_json).await;
                }
            }
            let _ = clear_live_turn(&scope).await;
            let _ = store.set_status(&scope_key, "idle", None).await;
            let summary = interrupted_summary_from_chunks(&assistant_chunks);
            let _ = update_thread_after_turn(&scope, Some(&summary), Some("idle")).await;
        } else {
            let _ = clear_live_turn(&scope).await;
            let _ = store.set_status(&scope_key, "failed", Some(error)).await;
            let _ = update_thread_after_turn(&scope, Some(error), Some("failed")).await;
        }
    }
    clear_scope_active(&scope_key);
    result
}

fn spawn_scope_turn(
    scope: ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: i64,
) -> Result<(), String> {
    let key = scope_key(&scope);
    mark_scope_active(&key)?;
    tokio::spawn(async move {
        let turn_ok = match run_scope_turn_task(scope.clone(), user_chunks, focus, user_message_id)
            .await
        {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, scope = %scope_key(&scope), "shepherd scope turn failed");
                false
            }
        };

        // After a successful shepherd or thread turn, trigger the librarian
        // to update the knowledge graph and refresh the canvas.
        if turn_ok {
            if let Some(project_id) = match &scope {
                ShepherdScope::Shepherd { project_id, .. } => Some(*project_id),
                ShepherdScope::Thread { project_id, .. } => Some(*project_id),
                _ => None, // Don't trigger for librarian turns (avoid loop)
            } {
                let scope_label = scope_key(&scope);
                if let Err(error) = crate::backend::librarian::trigger_librarian_after_turn(
                    project_id,
                    &scope_label,
                    "turn completed",
                )
                .await
                {
                    tracing::debug!(%error, "failed to trigger librarian after turn");
                }
            }
        }

        clear_scope_active(&key);
        dispatch_queued_turn(&key);
    });
    Ok(())
}

fn dispatch_queued_turn(key: &str) {
    if let Some(queued) = take_queued_turn(key) {
        let QueuedTurn {
            scope,
            user_chunks,
            focus,
            user_message_id,
        } = queued;
        if let Err(error) = spawn_scope_turn(scope.clone(), user_chunks, focus, user_message_id) {
            tracing::warn!(%error, scope = %scope_key(&scope), "failed to dispatch queued turn");
        }
    }
}

async fn dispatch_scope_message_local(
    scope: ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
) -> Result<SendShepherdMessageResponse, String> {
    validate_scope_runtime(&scope).await?;
    let key = scope_key(&scope);
    let is_active = {
        let activity = scope_activity(&scope).await?;
        activity.has_active_turn
            || active_scope_turns()
                .lock()
                .map_err(|_| "failed to lock active scope set".to_string())?
                .contains(&key)
    };

    let chunks_json = chunks_to_json(&user_chunks)?;
    let message_id = save_message(&scope, "user", &chunks_json).await?;

    if is_active {
        queue_turn(
            &key,
            QueuedTurn {
                scope: scope.clone(),
                user_chunks,
                focus,
                user_message_id: message_id,
            },
        );
        return Ok(SendShepherdMessageResponse {
            started: false,
            thread_id: match scope {
                ShepherdScope::Thread { thread_id, .. } => Some(thread_id),
                _ => None,
            },
        });
    }

    if let ShepherdScope::Thread { thread_id, .. } = &scope {
        let summary = summary_from_chunks(&user_chunks);
        let _ = update_thread_after_turn(&scope, Some(&summary), Some("running")).await;
        if let Ok(store) = ShepherdThreadStore::open().await {
            let _ = store.touch_thread(thread_id).await;
        }
    }
    if let Err(error) = set_live_turn(&scope, "[]", "starting", None).await {
        tracing::warn!(%error, scope = %scope_key(&scope), "failed to seed starting live turn");
    }
    if let Err(error) = spawn_scope_turn(scope.clone(), user_chunks, focus, message_id) {
        let _ = clear_live_turn(&scope).await;
        return Err(error);
    }
    Ok(SendShepherdMessageResponse {
        started: true,
        thread_id: match scope {
            ShepherdScope::Thread { thread_id, .. } => Some(thread_id),
            _ => None,
        },
    })
}

pub async fn send_scope_message(
    scope: ShepherdScope,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
) -> Result<SendShepherdMessageResponse, String> {
    let user_chunks = build_user_chunks(content, chunks)?;
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::DispatchScopeMessage {
            scope: scope.clone(),
            user_chunks,
            focus,
        })
        .await?;
        let payload = payload.unwrap_or_else(|| json!({ "started": true }));
        return serde_json::from_value(payload)
            .map_err(|error| format!("failed to decode server control reply: {}", error));
    }
    dispatch_scope_message_local(scope, user_chunks, focus).await
}

pub async fn send_shepherd_message(
    project_id: i64,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
) -> Result<SendShepherdMessageResponse, String> {
    send_scope_message(
        ShepherdScope::Shepherd {
            project_id,
            workspace_path: None,
            focus: None,
        },
        content,
        chunks,
        None,
    )
    .await
}

pub async fn prepare_shepherd_session(project_id: i64) -> Result<(), String> {
    let scope = ShepherdScope::Shepherd {
        project_id,
        workspace_path: None,
        focus: None,
    };
    validate_scope_runtime(&scope).await?;
    ensure_scope_session(&scope).await.map(|_| ())
}

pub async fn send_thread_message(
    project_id: i64,
    thread_id: &str,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
) -> Result<SendShepherdMessageResponse, String> {
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    let thread = thread_store
        .get_thread(thread_id)
        .await
        .map_err(|e| format!("failed to load thread {}: {}", thread_id, e))?;
    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }
    send_scope_message(thread_scope(&thread), content, chunks, None).await
}

pub async fn exec_thread_shell(
    project_id: i64,
    thread_id: &str,
    args: Value,
) -> Result<ToolResult, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        return decode_server_tool_result(
            send_server_control_request(&ServerControlRequest::ExecThreadShell {
                project_id,
                thread_id: thread_id.to_string(),
                args,
            })
            .await?,
        );
    }
    thread_worker_tool_call_local(project_id, thread_id, WorkerRequest::ExecShell { args }).await
}

pub async fn write_thread_shell(
    project_id: i64,
    thread_id: &str,
    args: Value,
) -> Result<ToolResult, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        return decode_server_tool_result(
            send_server_control_request(&ServerControlRequest::WriteThreadShell {
                project_id,
                thread_id: thread_id.to_string(),
                args,
            })
            .await?,
        );
    }
    thread_worker_tool_call_local(project_id, thread_id, WorkerRequest::WriteShell { args }).await
}

pub async fn forward_thread_port(
    project_id: i64,
    thread_id: &str,
    port: u16,
    protocol: &str,
    label: &str,
) -> Result<PreviewForwardInfo, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::ForwardThreadPort {
            project_id,
            thread_id: thread_id.to_string(),
            port,
            protocol: protocol.to_string(),
            label: label.to_string(),
        })
        .await?
        .ok_or_else(|| "server control reply did not include a preview payload".to_string())?;
        return serde_json::from_value(payload)
            .map_err(|error| format!("failed to decode preview payload: {}", error));
    }
    open_preview_forward(project_id, thread_id, port, protocol, label).await
}

pub async fn list_thread_port_forwards(
    project_id: i64,
    thread_id: Option<&str>,
) -> Result<Vec<PreviewForwardInfo>, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::ListThreadPortForwards {
            project_id,
            thread_id: thread_id.map(str::to_string),
        })
        .await?
        .ok_or_else(|| "server control reply did not include a preview list payload".to_string())?;
        return serde_json::from_value(payload)
            .map_err(|error| format!("failed to decode preview list payload: {}", error));
    }
    Ok(list_preview_forwards(project_id, thread_id).await)
}

pub async fn close_thread_port_forward(
    forward_id: &str,
) -> Result<Option<PreviewForwardInfo>, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::ClosePortForward {
            forward_id: forward_id.to_string(),
        })
        .await?;
        return payload
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| format!("failed to decode preview close payload: {}", error));
    }
    close_preview_forward(forward_id).await
}

async fn create_thread_local(
    project_id: i64,
    title: &str,
    objective: &str,
    summary: &str,
) -> Result<ShepherdThread, String> {
    let (workspace_path, checkout_name) = prepare_thread_checkout(project_id, title).await?;
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    store
        .create_thread(
            project_id,
            title,
            objective,
            summary,
            Some(&workspace_path),
            Some(&checkout_name),
        )
        .await
        .map_err(|error| format!("failed to create thread '{}': {}", title, error))
}

pub async fn create_thread(
    project_id: i64,
    title: &str,
    objective: &str,
) -> Result<ShepherdThread, String> {
    let summary = truncate_copy(objective, 180);
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::CreateThread {
            project_id,
            title: title.to_string(),
            objective: objective.to_string(),
            summary,
        })
        .await?
        .ok_or_else(|| "server control reply did not include a thread payload".to_string())?;
        return serde_json::from_value(payload)
            .map_err(|error| format!("failed to decode created thread: {}", error));
    }
    create_thread_local(project_id, title, objective, &summary).await
}

pub async fn promote_thread(
    project_id: i64,
    thread_id: &str,
) -> Result<PromoteThreadResponse, String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        let payload = send_server_control_request(&ServerControlRequest::PromoteThread {
            project_id,
            thread_id: thread_id.to_string(),
        })
        .await?
        .ok_or_else(|| "server control reply did not include a promotion payload".to_string())?;
        return serde_json::from_value(payload)
            .map_err(|error| format!("failed to decode promotion response: {}", error));
    }
    promote_thread_local(project_id, thread_id).await
}

fn thread_scope(thread: &ShepherdThread) -> ShepherdScope {
    ShepherdScope::Thread {
        project_id: thread.project_id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        workspace_path: None,
        focus: None,
    }
}

fn decode_server_tool_result(payload: Option<Value>) -> Result<ToolResult, String> {
    let payload = payload
        .ok_or_else(|| "server control reply did not include a tool result payload".to_string())?;
    let payload: ServerToolResultPayload = serde_json::from_value(payload)
        .map_err(|error| format!("failed to decode tool result payload: {}", error))?;
    Ok(ToolResult {
        success: payload.success,
        result: payload.result,
        images: vec![],
    })
}

async fn thread_worker_tool_call_local(
    project_id: i64,
    thread_id: &str,
    request: WorkerRequest,
) -> Result<ToolResult, String> {
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    let thread = thread_store
        .get_thread(thread_id)
        .await
        .map_err(|error| format!("failed to load thread {}: {}", thread_id, error))?;
    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }
    let session = ensure_scope_session(&thread_scope(&thread)).await?;
    let thread_scope = thread_scope(&thread);
    let stream = match connect_worker_socket(std::path::Path::new(&session.socket_path)).await {
        Ok(stream) => stream,
        Err(error) => return Err(enrich_scope_disconnect_error(&thread_scope, &error).await),
    };
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(&mut write_half, &request).await?;
    let mut reader = BufReader::new(read_half);
    let reply = match read_json_line::<_, WorkerReply>(&mut reader).await {
        Ok(reply) => reply,
        Err(error) => return Err(enrich_scope_disconnect_error(&thread_scope, &error).await),
    };
    match reply {
        WorkerReply::ToolResult { success, result } => Ok(ToolResult {
            success,
            result,
            images: vec![],
        }),
        WorkerReply::Error { message } => Err(message),
        other => Err(format!("unexpected worker reply: {:?}", other)),
    }
}

async fn promote_thread_local(
    project_id: i64,
    thread_id: &str,
) -> Result<PromoteThreadResponse, String> {
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    let thread = thread_store
        .get_thread(thread_id)
        .await
        .map_err(|error| format!("failed to load thread {}: {}", thread_id, error))?;
    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }
    let activity = get_thread_activity(project_id, &thread.id, &thread.title).await?;
    if activity.has_active_turn {
        return Err(format!(
            "thread '{}' is still running; wait for it to finish before promoting",
            thread.title
        ));
    }

    let checkout_path = thread
        .workspace_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| format!("thread {} has no workspace path", thread_id))?;
    let checkout_name = thread
        .checkout_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("thread {} has no checkout name", thread_id))?;
    let workspace = ensure_project_workspace(project_id).await?;

    let promote = promote_thread_checkout(
        workspace.central_dir.as_path(),
        checkout_path.as_path(),
        checkout_name,
    )
    .map_err(|error| match error {
        GitError::MergeConflict(files) => {
            format!("promotion hit merge conflicts in: {}", files.join(", "))
        }
        other => other.to_string(),
    })?;

    if promote.promoted {
        let summary = if promote.changed_files.is_empty() {
            "Promoted to central.".to_string()
        } else {
            format!(
                "Promoted to central: {}",
                truncate_copy(&promote.changed_files.join(", "), 180)
            )
        };
        let _ = thread_store
            .update_thread(
                &thread.id,
                None,
                None,
                Some(&summary),
                Some("done"),
                None,
                None,
            )
            .await;
    }

    Ok(PromoteThreadResponse {
        promoted: promote.promoted,
        thread_id: thread.id,
        central_head: promote.central_head,
        changed_files: promote.changed_files,
        env_changed: promote.env_changed,
    })
}

async fn archive_thread_local(project_id: i64, thread_id: &str) -> Result<(), String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    let thread = store
        .get_thread(thread_id)
        .await
        .map_err(|e| format!("failed to load thread {}: {}", thread_id, e))?;
    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }
    close_thread_preview_forwards(project_id, thread_id).await;
    stop_scope_session(&thread_scope(&thread)).await?;
    store
        .archive_thread(thread_id)
        .await
        .map_err(|e| format!("failed to archive thread {}: {}", thread_id, e))
}

async fn delete_thread_local(project_id: i64, thread_id: &str) -> Result<(), String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    let thread = store
        .get_thread(thread_id)
        .await
        .map_err(|e| format!("failed to load thread {}: {}", thread_id, e))?;
    if thread.project_id != project_id {
        return Err(format!(
            "thread {} does not belong to project {}",
            thread_id, project_id
        ));
    }
    close_thread_preview_forwards(project_id, thread_id).await;
    stop_scope_session(&thread_scope(&thread)).await?;
    if let Ok(chat_store) = ShepherdChatStore::open().await {
        let _ = chat_store
            .delete_scope_messages(&ShepherdThreadStore::scope_key(&thread.id))
            .await;
        let _ = chat_store
            .clear_scope_state(project_id, &ShepherdThreadStore::scope_key(&thread.id))
            .await;
    }
    if let Some(workspace_path) = thread
        .workspace_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        let workspace = PathBuf::from(workspace_path);
        if workspace.exists() {
            let _ = std::fs::remove_dir_all(&workspace);
        }
    }
    store
        .delete_thread(thread_id)
        .await
        .map_err(|e| format!("failed to delete thread {}: {}", thread_id, e))
}

async fn handle_server_control_request(
    request: ServerControlRequest,
) -> Result<Option<serde_json::Value>, String> {
    match request {
        ServerControlRequest::DispatchScopeMessage {
            scope,
            user_chunks,
            focus,
        } => {
            let response = dispatch_scope_message_local(scope, user_chunks, focus).await?;
            Ok(Some(serde_json::to_value(response).map_err(|error| {
                format!("failed to encode response: {}", error)
            })?))
        }
        ServerControlRequest::CreateThread {
            project_id,
            title,
            objective,
            summary,
        } => {
            let thread = create_thread_local(project_id, &title, &objective, &summary).await?;
            Ok(Some(serde_json::to_value(thread).map_err(|error| {
                format!("failed to encode thread: {}", error)
            })?))
        }
        ServerControlRequest::InterruptScopeTurn { scope } => {
            interrupt_scope_turn_local(&scope).await?;
            Ok(None)
        }
        ServerControlRequest::StopScopeSession { scope } => {
            stop_scope_session(&scope).await?;
            Ok(None)
        }
        ServerControlRequest::ExecThreadShell {
            project_id,
            thread_id,
            args,
        } => {
            let result = thread_worker_tool_call_local(
                project_id,
                &thread_id,
                WorkerRequest::ExecShell { args },
            )
            .await?;
            Ok(Some(
                serde_json::to_value(ServerToolResultPayload {
                    success: result.success,
                    result: result.result,
                })
                .map_err(|error| format!("failed to encode shell result payload: {}", error))?,
            ))
        }
        ServerControlRequest::WriteThreadShell {
            project_id,
            thread_id,
            args,
        } => {
            let result = thread_worker_tool_call_local(
                project_id,
                &thread_id,
                WorkerRequest::WriteShell { args },
            )
            .await?;
            Ok(Some(
                serde_json::to_value(ServerToolResultPayload {
                    success: result.success,
                    result: result.result,
                })
                .map_err(|error| format!("failed to encode shell result payload: {}", error))?,
            ))
        }
        ServerControlRequest::ArchiveThread {
            project_id,
            thread_id,
        } => {
            archive_thread_local(project_id, &thread_id).await?;
            Ok(None)
        }
        ServerControlRequest::PromoteThread {
            project_id,
            thread_id,
        } => {
            let response = promote_thread_local(project_id, &thread_id).await?;
            Ok(Some(serde_json::to_value(response).map_err(|error| {
                format!("failed to encode promotion response: {}", error)
            })?))
        }
        ServerControlRequest::ForwardThreadPort {
            project_id,
            thread_id,
            port,
            protocol,
            label,
        } => {
            let response =
                open_preview_forward(project_id, &thread_id, port, &protocol, &label).await?;
            Ok(Some(serde_json::to_value(response).map_err(|error| {
                format!("failed to encode preview response: {}", error)
            })?))
        }
        ServerControlRequest::ListThreadPortForwards {
            project_id,
            thread_id,
        } => Ok(Some(
            serde_json::to_value(list_preview_forwards(project_id, thread_id.as_deref()).await)
                .map_err(|error| format!("failed to encode preview list: {}", error))?,
        )),
        ServerControlRequest::ClosePortForward { forward_id } => {
            Ok(close_preview_forward(&forward_id)
                .await?
                .map(serde_json::to_value)
                .transpose()
                .map_err(|error| format!("failed to encode preview close response: {}", error))?)
        }
        ServerControlRequest::DeleteThread {
            project_id,
            thread_id,
        } => {
            delete_thread_local(project_id, &thread_id).await?;
            Ok(None)
        }
        ServerControlRequest::LoadScopeMessages {
            scope,
            limit,
            skip_message_id,
        } => Ok(Some(
            serde_json::to_value(load_scope_messages_local(&scope, limit, skip_message_id).await?)
                .map_err(|error| format!("failed to encode scope messages: {}", error))?,
        )),
        ServerControlRequest::LoadScopeState { scope } => Ok(Some(
            serde_json::to_value(load_scope_state_local(&scope).await?)
                .map_err(|error| format!("failed to encode scope state: {}", error))?,
        )),
        ServerControlRequest::LoadLlmSettings => Ok(Some(
            serde_json::to_value(
                crate::backend::AppSettingsStore::open()
                    .await
                    .map_err(|error| format!("failed to open app settings store: {}", error))?
                    .load_llm_settings()
                    .await
                    .map_err(|error| format!("failed to load llm settings: {}", error))?,
            )
            .map_err(|error| format!("failed to encode llm settings: {}", error))?,
        )),
        ServerControlRequest::LoadProjectLore { project_id } => Ok(Some(
            serde_json::to_value(load_project_lore_local(project_id).await?)
                .map_err(|error| format!("failed to encode project lore: {}", error))?,
        )),
        ServerControlRequest::ExecuteShepherdTool {
            project_id,
            name,
            args,
        } => {
            let result = execute_shepherd_server_tool_local(project_id, &name, &args).await;
            Ok(Some(
                serde_json::to_value(ServerToolResultPayload {
                    success: result.success,
                    result: result.result,
                })
                .map_err(|error| format!("failed to encode tool result payload: {}", error))?,
            ))
        }
        ServerControlRequest::ExecuteLibrarianTool {
            project_id,
            name,
            args,
        } => {
            let result = execute_librarian_server_tool_local(project_id, &name, &args).await;
            Ok(Some(
                serde_json::to_value(ServerToolResultPayload {
                    success: result.success,
                    result: result.result,
                })
                .map_err(|error| format!("failed to encode tool result payload: {}", error))?,
            ))
        }
    }
}

async fn load_project_lore_local(project_id: i64) -> Result<Vec<ProjectLoreEntry>, String> {
    use surrealdb::types::SurrealValue;

    #[derive(serde::Deserialize, SurrealValue)]
    struct LoreRow {
        node_id: String,
        content: Option<String>,
        label: Option<String>,
    }

    let db = crate::backend::db::global_db().await;
    let result: Result<Vec<LoreRow>, _> = db
        .query("SELECT node_id, label, content FROM kg_node WHERE project_id = $project_id AND kind = 'lore' ORDER BY updated_at DESC LIMIT 40")
        .bind(("project_id", project_id))
        .await
        .and_then(|mut r| r.take(0));
    let rows = result.map_err(|error| format!("failed to load project lore: {}", error))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let text = row
                .content
                .filter(|s| !s.trim().is_empty())
                .or_else(|| row.label.filter(|s| !s.trim().is_empty()))?;
            Some(ProjectLoreEntry {
                node_id: row.node_id,
                text,
            })
        })
        .collect())
}

pub async fn start_server_control_listener() -> Result<(), String> {
    let socket_path = current_server_control_socket_path();
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("failed to create server control socket dir: {}", error))?;
    }
    if socket_path.exists() {
        let _ = tokio::fs::remove_file(&socket_path).await;
    }
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| format!("failed to bind server control socket: {}", error))?;
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    tokio::spawn(async move {
                        let (read_half, mut write_half) = stream.into_split();
                        let mut reader = BufReader::new(read_half);
                        let reply =
                            match read_json_line::<_, ServerControlRequest>(&mut reader).await {
                                Ok(request) => match handle_server_control_request(request).await {
                                    Ok(payload) => ServerControlReply::Ok { payload },
                                    Err(message) => ServerControlReply::Error { message },
                                },
                                Err(message) => ServerControlReply::Error { message },
                            };
                        let _ = write_json_line(&mut write_half, &reply).await;
                    });
                }
                Err(error) => {
                    tracing::warn!(%error, socket = %server_control_socket_path().display(), "server control socket accept failed");
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
        }
    });
    Ok(())
}

pub async fn stop_scope_activity(scope: ShepherdScope) -> Result<(), String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        send_server_control_request(&ServerControlRequest::StopScopeSession { scope }).await?;
        return Ok(());
    }
    stop_scope_session(&scope).await
}

async fn interrupt_scope_turn_local(scope: &ShepherdScope) -> Result<(), String> {
    let key = scope_key(scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    if let Some(session) = store
        .get_session(&key)
        .await
        .map_err(|error| format!("failed to load session: {}", error))?
    {
        let socket_path = std::path::Path::new(&session.socket_path);
        if socket_path.exists() {
            if let Ok(stream) = connect_worker_socket(socket_path).await {
                let (_, mut write_half) = stream.into_split();
                let _ = write_json_line(&mut write_half, &WorkerRequest::Interrupt).await;
            }
        }
        let _ = store.set_status(&key, "interrupting", None).await;
        let _ = update_live_turn_status(scope, "interrupting", None).await;
    }
    Ok(())
}

pub async fn interrupt_scope_turn(scope: ShepherdScope) -> Result<(), String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        send_server_control_request(&ServerControlRequest::InterruptScopeTurn { scope }).await?;
        return Ok(());
    }
    interrupt_scope_turn_local(&scope).await
}

pub async fn archive_thread(project_id: i64, thread_id: &str) -> Result<(), String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        send_server_control_request(&ServerControlRequest::ArchiveThread {
            project_id,
            thread_id: thread_id.to_string(),
        })
        .await?;
        return Ok(());
    }
    archive_thread_local(project_id, thread_id).await
}

pub async fn delete_thread(project_id: i64, thread_id: &str) -> Result<(), String> {
    if std::env::var("HIRSEL_SERVER_RPC_SOCKET").is_ok() {
        send_server_control_request(&ServerControlRequest::DeleteThread {
            project_id,
            thread_id: thread_id.to_string(),
        })
        .await?;
        return Ok(());
    }
    delete_thread_local(project_id, thread_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::app;
    use crate::backend::config::testing::TestEnv;
    use crate::backend::draft::StartingPoint;
    use git2::{Repository, Signature};
    use std::path::Path;
    use tempfile::TempDir;

    const TEST_FLAKE: &str = r#"
{
  description = "hirsel shepherd/thread smoke";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = builtins.currentSystem;
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          bash
          coreutils
          findutils
          gawk
          git
          gnugrep
          gnused
          jq
          ripgrep
          xz
        ];
      };
    };
}
"#;

    fn create_local_source_repo() -> TempDir {
        let dir = TempDir::new().expect("create temp repo");
        let repo = Repository::init(dir.path()).expect("init repo");
        std::fs::write(dir.path().join("README.md"), "# smoke\n").expect("write README");

        let mut index = repo.index().expect("index");
        index
            .add_path(Path::new("README.md"))
            .expect("add path to index");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let sig = Signature::now("hirsel", "hirsel@test").expect("signature");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("initial commit");
        dir
    }

    #[tokio::test]
    #[ignore = "smoke test requiring docker and the local worker image"]
    async fn create_thread_over_server_control_persists_host_workspace_path() {
        let _env = TestEnv::builder()
            .with_config(
                r#"
[sandbox]
image = "hirsel-worker:local"
"#,
            )
            .build();

        let source = create_local_source_repo();
        let project = app::create_project(
            "smoke".to_string(),
            StartingPoint::LocalFolder {
                path: source.path().display().to_string(),
            },
            None,
            None,
            None,
        )
        .await
        .expect("create project");

        let workspace = crate::backend::ensure_project_workspace(project.id)
            .await
            .expect("materialize central workspace");
        std::fs::write(workspace.central_dir.join("flake.nix"), TEST_FLAKE).expect("write flake");

        start_server_control_listener()
            .await
            .expect("start server control listener");

        let socket_path = current_server_control_socket_path();
        std::env::set_var("HIRSEL_SERVER_RPC_SOCKET", &socket_path);
        let thread = create_thread(project.id, "Smoke thread", "Read only smoke task")
            .await
            .expect("create thread through server control");
        std::env::remove_var("HIRSEL_SERVER_RPC_SOCKET");

        let workspace_path = thread.workspace_path.expect("thread workspace path");
        let host_root = std::env::var("HIRSEL_ROOT").expect("HIRSEL_ROOT");
        assert!(
            workspace_path.starts_with(&host_root),
            "expected host workspace path under {host_root}, got {workspace_path}"
        );
        assert!(
            !workspace_path.starts_with("/hirsel/"),
            "thread stored container-local workspace path: {workspace_path}"
        );
        assert!(
            Path::new(&workspace_path).exists(),
            "thread workspace path does not exist on host: {workspace_path}"
        );
    }
}
