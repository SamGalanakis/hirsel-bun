use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{LazyLock, Mutex as StdMutex, OnceLock};

use lash::ToolResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::history::{build_user_chunks, chunks_to_json, save_message, save_message_with_options};
use super::preview::{
    close_preview_forward, close_thread_preview_forwards, list_preview_forwards,
    open_preview_forward,
};
use super::queries::scope_activity;
use super::session::ShepherdSessionStore;
use super::types::{
    scope_key, PreviewForwardInfo, ProjectLoreEntry, ShepherdMessageChunk, ShepherdScope,
    ShepherdTaskFocus, WorkerStreamEvent,
};
use crate::backend::app::ResultExt;
use crate::backend::{
    ShepherdChatMessageOptions, ShepherdChatStore, ShepherdThread, ShepherdThreadStore,
};

// ---------------------------------------------------------------------------
// Active-scope tracking and queued turns
// ---------------------------------------------------------------------------

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

static QUEUED_TURNS: LazyLock<StdMutex<HashMap<String, VecDeque<QueuedTurn>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

fn queue_turn(key: &str, turn: QueuedTurn) {
    if let Ok(mut map) = QUEUED_TURNS.lock() {
        map.entry(key.to_string()).or_default().push_back(turn);
    }
}

fn take_queued_turn(key: &str) -> Option<QueuedTurn> {
    let mut map = QUEUED_TURNS.lock().ok()?;
    let queue = map.get_mut(key)?;
    let turn = queue.pop_front();
    if queue.is_empty() {
        map.remove(key);
    }
    turn
}

// ---------------------------------------------------------------------------
// Public response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendShepherdMessageResponse {
    pub started: bool,
    #[serde(default)]
    pub thread_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Scope storage helpers
// ---------------------------------------------------------------------------

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

fn completed_summary_from_chunks(chunks: &[ShepherdMessageChunk]) -> String {
    let summary = summary_from_chunks(chunks);
    if summary != "No summary available yet." {
        return summary;
    }

    let tool_count = chunks
        .iter()
        .filter(|chunk| matches!(chunk, ShepherdMessageChunk::Tool { .. }))
        .count();
    if tool_count > 0 {
        return if tool_count == 1 {
            "Completed 1 tool action.".to_string()
        } else {
            format!("Completed {tool_count} tool actions.")
        };
    }

    "No summary available yet.".to_string()
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

// ---------------------------------------------------------------------------
// LiveTurnAccumulator
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Scope-active bookkeeping
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Live turn persistence
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Scope state persistence
// ---------------------------------------------------------------------------

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
    }

    let merged_json = serde_json::to_string(&incoming)
        .map_err(|error| format!("failed to serialize shepherd scope state: {}", error))?;
    save_scope_state(scope, &merged_json).await
}

pub(super) async fn load_scope_state_local(
    scope: &ShepherdScope,
) -> Result<Option<String>, String> {
    let (Some(project_id), Some(scope_key)) = scope_storage_ids(scope) else {
        return Ok(None);
    };
    let store = ShepherdChatStore::open().await.str_err()?;
    store
        .get_scope_state(project_id, &scope_key)
        .await
        .str_err()
}

pub(super) async fn load_scope_messages_local(
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

// ---------------------------------------------------------------------------
// Thread helpers
// ---------------------------------------------------------------------------

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
        .update_thread(thread_id, None, None, summary, status, None)
        .await
        .map_err(|error| format!("failed to update thread {}: {}", thread_id, error))
}

fn thread_scope(thread: &ShepherdThread) -> ShepherdScope {
    ShepherdScope::Thread {
        project_id: thread.project_id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        focus: None,
    }
}

// ---------------------------------------------------------------------------
// Turn execution (in-process via worker::run_scope_turn)
// ---------------------------------------------------------------------------

struct CompletedTurnOutcome {
    assistant_chunks: Vec<ShepherdMessageChunk>,
}

async fn run_scope_turn_task(
    scope: ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: i64,
) -> Result<CompletedTurnOutcome, String> {
    let key = scope_key(&scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let mut accumulator = LiveTurnAccumulator::default();
    let result: Result<CompletedTurnOutcome, String> = async {
        store.set_status(&key, "running", None).await
            .map_err(|error| format!("failed to mark session running: {}", error))?;

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);
        let cancel = tokio_util::sync::CancellationToken::new();
        let turn_scope = scope.clone();
        let turn_chunks = user_chunks.clone();
        let turn_cancel = cancel.clone();
        let turn_handle = tokio::spawn(async move {
            super::worker::run_scope_turn(&turn_scope, turn_chunks, focus, Some(user_message_id), event_tx, turn_cancel).await
        });

        while let Some(event) = event_rx.recv().await {
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

        let turn_result = turn_handle.await.map_err(|e| format!("turn task panicked: {e}"))??;
        let mut assistant_chunks = accumulator.finalize(turn_result.assistant_chunks);
        if turn_result.interrupted && !assistant_chunks.iter().any(|chunk| {
            matches!(chunk, ShepherdMessageChunk::Notice { title, content, .. }
                if title.as_deref() == Some("Interrupted") && content == "Stopped manually. This response is partial.")
        }) {
            assistant_chunks.push(interruption_notice_chunk());
        }
        merge_scope_state(&scope, &turn_result.state_json).await?;
        clear_live_turn(&scope).await?;
        store.set_status(&key, "idle", None).await
            .map_err(|error| format!("failed to mark session idle: {}", error))?;
        if !assistant_chunks.is_empty() {
            let assistant_chunks_json = chunks_to_json(&assistant_chunks)?;
            save_message(&scope, "assistant", &assistant_chunks_json).await?;
        }
        if turn_result.interrupted {
            let interrupted_summary = interrupted_summary_from_chunks(&assistant_chunks);
            update_thread_after_turn(&scope, Some(&interrupted_summary), Some("idle")).await?;
        } else {
            update_thread_after_turn(&scope, Some(&turn_result.summary), None).await?;
        }
        Ok(CompletedTurnOutcome { assistant_chunks })
    }.await;

    let result = match result {
        Err(error) if error.contains("Model returned no usable output.") => {
            let assistant_chunks = accumulator.finalize(Vec::new());
            if assistant_chunks.is_empty() {
                Err(error)
            } else {
                clear_live_turn(&scope).await?;
                store.set_status(&key, "idle", None).await
                    .map_err(|e| format!("failed to mark session idle after tool-only turn: {e}"))?;
                let assistant_chunks_json = chunks_to_json(&assistant_chunks)?;
                save_message(&scope, "assistant", &assistant_chunks_json).await?;
                let summary = completed_summary_from_chunks(&assistant_chunks);
                update_thread_after_turn(&scope, Some(&summary), None).await?;
                Ok(CompletedTurnOutcome { assistant_chunks })
            }
        }
        other => other,
    };

    if let Err(error) = &result {
        if is_interruption_error(error) {
            let mut assistant_chunks = accumulator.finalize(Vec::new());
            if !assistant_chunks.iter().any(|chunk| {
                matches!(chunk, ShepherdMessageChunk::Notice { title, content, .. }
                    if title.as_deref() == Some("Interrupted") && content == "Stopped manually. This response is partial.")
            }) {
                assistant_chunks.push(interruption_notice_chunk());
            }
            if !assistant_chunks.is_empty() {
                if let Ok(json) = chunks_to_json(&assistant_chunks) {
                    let _ = save_message(&scope, "assistant", &json).await;
                }
            }
            let _ = clear_live_turn(&scope).await;
            let _ = store.set_status(&key, "idle", None).await;
            let summary = interrupted_summary_from_chunks(&assistant_chunks);
            let _ = update_thread_after_turn(&scope, Some(&summary), Some("idle")).await;
        } else {
            let _ = clear_live_turn(&scope).await;
            let _ = store.set_status(&key, "failed", Some(error)).await;
            let _ = update_thread_after_turn(&scope, Some(error), Some("failed")).await;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Turn spawning and queued dispatch
// ---------------------------------------------------------------------------

fn spawn_scope_turn(
    scope: ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    user_message_id: i64,
) -> Result<(), String> {
    let key = scope_key(&scope);
    mark_scope_active(&key)?;
    tokio::spawn(async move {
        let sync_chunks = user_chunks.clone();
        match run_scope_turn_task(scope.clone(), user_chunks, focus, user_message_id).await {
            Ok(outcome) => {
                if let Some(project_id) = match &scope {
                    ShepherdScope::Shepherd { project_id, .. } => Some(*project_id),
                    ShepherdScope::Thread { project_id, .. } => Some(*project_id),
                    _ => None,
                } {
                    if let Err(error) = crate::backend::librarian::queue_background_sync(
                        project_id,
                        &scope,
                        &sync_chunks,
                        &outcome.assistant_chunks,
                    )
                    .await
                    {
                        tracing::debug!(
                            %error,
                            project_id,
                            "failed to enqueue librarian background sync"
                        );
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, scope = %scope_key(&scope), "shepherd scope turn failed");
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

// ---------------------------------------------------------------------------
// Core dispatch
// ---------------------------------------------------------------------------

async fn dispatch_scope_message_local(
    scope: ShepherdScope,
    user_chunks: Vec<ShepherdMessageChunk>,
    focus: Option<ShepherdTaskFocus>,
    options: &ShepherdChatMessageOptions,
) -> Result<SendShepherdMessageResponse, String> {
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
    let message_id = save_message_with_options(&scope, "user", &chunks_json, options).await?;

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
    if let Err(error) = set_live_turn(&scope, "[]", "running", None).await {
        tracing::warn!(%error, scope = %scope_key(&scope), "failed to seed live turn");
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub(crate) async fn enqueue_librarian_automated_message(
    project_id: i64,
    prompt: String,
    preview_text: String,
) -> Result<(), String> {
    let scope = ShepherdScope::Librarian {
        project_id,
    };
    let options = ShepherdChatMessageOptions::shepherd_sync(preview_text);
    let _ = dispatch_scope_message_local(
        scope,
        vec![ShepherdMessageChunk::Text { content: prompt }],
        None,
        &options,
    )
    .await?;
    Ok(())
}

pub async fn send_scope_message(
    scope: ShepherdScope,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
    focus: Option<ShepherdTaskFocus>,
) -> Result<SendShepherdMessageResponse, String> {
    let user_chunks = build_user_chunks(content, chunks)?;
    dispatch_scope_message_local(scope, user_chunks, focus, &Default::default()).await
}

pub async fn send_shepherd_message(
    project_id: i64,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
) -> Result<SendShepherdMessageResponse, String> {
    send_scope_message(
        ShepherdScope::Shepherd {
            project_id,
            focus: None,
        },
        content,
        chunks,
        None,
    )
    .await
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

pub async fn prepare_shepherd_session(_project_id: i64) -> Result<(), String> {
    Ok(())
}

pub async fn exec_thread_shell(
    _project_id: i64,
    _thread_id: &str,
    _args: Value,
) -> Result<ToolResult, String> {
    Err("thread shell not yet available in lightweight mode".to_string())
}

pub async fn write_thread_shell(
    _project_id: i64,
    _thread_id: &str,
    _args: Value,
) -> Result<ToolResult, String> {
    Err("thread shell not yet available in lightweight mode".to_string())
}

pub async fn forward_thread_port(
    project_id: i64,
    thread_id: &str,
    port: u16,
    protocol: &str,
    label: &str,
) -> Result<PreviewForwardInfo, String> {
    open_preview_forward(project_id, thread_id, port, protocol, label).await
}

pub async fn list_thread_port_forwards(
    project_id: i64,
    thread_id: Option<&str>,
) -> Result<Vec<PreviewForwardInfo>, String> {
    Ok(list_preview_forwards(project_id, thread_id).await)
}

pub async fn close_thread_port_forward(
    forward_id: &str,
) -> Result<Option<PreviewForwardInfo>, String> {
    close_preview_forward(forward_id).await
}

pub async fn create_thread(
    project_id: i64,
    title: &str,
    objective: &str,
) -> Result<ShepherdThread, String> {
    let summary = truncate_copy(objective, 180);
    create_thread_local(project_id, title, objective, &summary).await
}

pub async fn stop_scope_activity(scope: ShepherdScope) -> Result<(), String> {
    let key = scope_key(&scope);
    let store = ShepherdSessionStore::open().await.map_err(|e| format!("{e}"))?;
    let _ = store.delete_session(&key).await;
    Ok(())
}

async fn interrupt_scope_turn_local(scope: &ShepherdScope) -> Result<(), String> {
    let key = scope_key(scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let _ = store.set_status(&key, "interrupting", None).await;
    let _ = update_live_turn_status(scope, "interrupting", None).await;
    Ok(())
}

pub async fn interrupt_scope_turn(scope: ShepherdScope) -> Result<(), String> {
    interrupt_scope_turn_local(&scope).await
}

pub async fn archive_thread(project_id: i64, thread_id: &str) -> Result<(), String> {
    archive_thread_local(project_id, thread_id).await
}

pub async fn delete_thread(project_id: i64, thread_id: &str) -> Result<(), String> {
    delete_thread_local(project_id, thread_id).await
}

// ---------------------------------------------------------------------------
// Thread CRUD
// ---------------------------------------------------------------------------

async fn create_thread_local(
    project_id: i64,
    title: &str,
    objective: &str,
    summary: &str,
) -> Result<ShepherdThread, String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    store
        .create_thread(project_id, title, objective, summary, None)
        .await
        .map_err(|error| format!("failed to create thread '{}': {}", title, error))
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
    let session_key = scope_key(&thread_scope(&thread));
    let session_store = ShepherdSessionStore::open().await.map_err(|e| e.to_string())?;
    let _ = session_store.delete_session(&session_key).await;
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
    let session_key = scope_key(&thread_scope(&thread));
    let session_store = ShepherdSessionStore::open().await.map_err(|e| e.to_string())?;
    let _ = session_store.delete_session(&session_key).await;
    if let Ok(chat_store) = ShepherdChatStore::open().await {
        let _ = chat_store
            .delete_scope_messages(&ShepherdThreadStore::scope_key(&thread.id))
            .await;
        let _ = chat_store
            .clear_scope_state(project_id, &ShepherdThreadStore::scope_key(&thread.id))
            .await;
    }
    store
        .delete_thread(thread_id)
        .await
        .map_err(|e| format!("failed to delete thread {}: {}", thread_id, e))
}

// ---------------------------------------------------------------------------
// Project lore
// ---------------------------------------------------------------------------

pub async fn load_project_lore_local(project_id: i64) -> Result<Vec<ProjectLoreEntry>, String> {
    use surrealdb::types::SurrealValue;

    #[derive(serde::Deserialize, SurrealValue)]
    struct LoreRow {
        node_id: String,
        content: Option<String>,
        label: Option<String>,
    }

    let db = crate::backend::db::global_db().await;
    let result: Result<Vec<LoreRow>, _> = db
        .query("SELECT node_id, label, content FROM kg_node WHERE id[0] = $project_id AND kind = 'lore' ORDER BY updated_at DESC LIMIT 40")
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
