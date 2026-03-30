use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex as StdMutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::BufReader;
use tokio::net::UnixListener;

use super::history::{
    build_user_chunks, chunks_to_json, load_scope_live_turn, load_scope_messages, save_message,
};
use super::rpc::{
    connect_worker_socket, read_json_line, send_server_control_request, server_control_socket_path,
    write_json_line, ServerControlReply, ServerControlRequest, WorkerReply, WorkerRequest,
    WorkerStreamEvent,
};
use super::sandbox::{
    current_server_control_socket_path, ensure_scope_session, scope_key, stop_scope_session,
    validate_scope_runtime,
};
use super::session::{ShepherdScopeSession, ShepherdSessionStore};
use super::types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
use crate::backend::app::ResultExt;
use crate::backend::ProjectStore;
use crate::backend::{
    prepare_thread_checkout, ShepherdChatMessage, ShepherdChatStore, ShepherdLiveTurn,
    ShepherdThread, ShepherdThreadStore,
};

const PROJECT_SURVEY_THREAD_TITLE: &str = "Project survey";

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

pub fn has_queued_turn(scope: &ShepherdScope) -> bool {
    let key = scope_key(scope);
    QUEUED_TURNS
        .lock()
        .ok()
        .is_some_and(|map| map.contains_key(&key))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendShepherdMessageResponse {
    pub started: bool,
    #[serde(default)]
    pub queued: bool,
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdScopeActivity {
    pub session: Option<ShepherdScopeSession>,
    pub live_turn: Option<ShepherdLiveTurn>,
    pub has_active_turn: bool,
}

fn scope_storage_ids(scope: &ShepherdScope) -> (Option<i64>, Option<String>) {
    match scope {
        ShepherdScope::General => (None, None),
        ShepherdScope::Project { project_id, .. } => (
            Some(*project_id),
            Some(ShepherdChatStore::project_scope_key(*project_id)),
        ),
        ShepherdScope::Thread {
            project_id,
            thread_id,
            ..
        } => (
            Some(*project_id),
            Some(ShepherdThreadStore::scope_key(thread_id)),
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
}

impl LiveTurnAccumulator {
    fn apply(&mut self, event: WorkerStreamEvent) {
        match event {
            WorkerStreamEvent::TextDelta { content } => {
                if let Some(ShepherdMessageChunk::Text { content: existing }) =
                    self.chunks.last_mut()
                {
                    existing.push_str(&content);
                } else {
                    self.chunks.push(ShepherdMessageChunk::Text { content });
                }
            }
            WorkerStreamEvent::Tool {
                id,
                title,
                kind,
                status,
                input,
                output,
            } => {
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
                if kind == "final" || kind == "tool_output" {
                    if let Some(ShepherdMessageChunk::Text { content }) = self
                        .chunks
                        .iter_mut()
                        .rev()
                        .find(|chunk| matches!(chunk, ShepherdMessageChunk::Text { .. }))
                    {
                        if !content.ends_with('\n') && !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str(&text);
                    } else {
                        self.chunks.push(ShepherdMessageChunk::Text { content: text });
                    }
                }
            }
            WorkerStreamEvent::Error { message } => {
                self.chunks.push(ShepherdMessageChunk::Tool {
                    id: "worker-error".to_string(),
                    title: "Worker Error".to_string(),
                    kind: Some("error".to_string()),
                    status: "failed".to_string(),
                    input: None,
                    output: Some(message),
                });
            }
        }
    }

    fn chunks_json(&self) -> Result<Option<String>, String> {
        if self.chunks.is_empty() {
            return Ok(None);
        }
        chunks_to_json(&self.chunks).map(Some)
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

async fn save_system_error(scope: &ShepherdScope, error: &str) -> Result<(), String> {
    let chunks = vec![ShepherdMessageChunk::Text {
        content: error.trim().to_string(),
    }];
    let chunks_json = chunks_to_json(&chunks)?;
    save_message(scope, "system", &chunks_json).await?;
    Ok(())
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

async fn scope_activity(scope: &ShepherdScope) -> Result<ShepherdScopeActivity, String> {
    let key = scope_key(scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let session = store
        .get_session(&key)
        .await
        .map_err(|error| format!("failed to load session: {}", error))?;
    let live_turn = load_scope_live_turn(scope).await?;
    let has_active_turn = session
        .as_ref()
        .is_some_and(|session| matches!(session.status.as_str(), "starting" | "running"));
    Ok(ShepherdScopeActivity {
        session,
        live_turn,
        has_active_turn,
    })
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
        let mut accumulator = LiveTurnAccumulator::default();
        loop {
            let reply: WorkerReply = read_json_line(&mut reader).await?;
            match reply {
                WorkerReply::Accepted => {}
                WorkerReply::Event { event } => {
                    accumulator.apply(event);
                    if let Some(chunks_json) = accumulator.chunks_json()? {
                        set_live_turn(&scope, &chunks_json, "running", None).await?;
                    }
                }
                WorkerReply::Finished {
                    assistant_chunks,
                    state_json,
                    summary,
                } => {
                    let assistant_chunks_json = chunks_to_json(&assistant_chunks)?;
                    save_message(&scope, "assistant", &assistant_chunks_json).await?;
                    save_scope_state(&scope, &state_json).await?;
                    clear_live_turn(&scope).await?;
                    store
                        .set_status(&scope_key, "idle", None)
                        .await
                        .map_err(|error| format!("failed to mark session idle: {}", error))?;
                    update_thread_after_turn(&scope, Some(&summary), None).await?;
                    break Ok(());
                }
                WorkerReply::Error { message } => {
                    break Err(message);
                }
                WorkerReply::Pong | WorkerReply::Status { .. } => {}
            }
        }
    }
    .await;

    if let Err(error) = &result {
        let _ = clear_live_turn(&scope).await;
        if error.contains("interrupted") {
            let _ = store.set_status(&scope_key, "idle", None).await;
        } else {
            let _ = store.set_status(&scope_key, "failed", Some(error)).await;
            let _ = update_thread_after_turn(&scope, Some(error), Some("failed")).await;
            let _ = save_system_error(&scope, error).await;
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
        if let Err(error) =
            run_scope_turn_task(scope.clone(), user_chunks, focus, user_message_id).await
        {
            tracing::warn!(%error, scope = %scope_key(&scope), "shepherd scope turn failed");
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
            queued: true,
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
    spawn_scope_turn(scope.clone(), user_chunks, focus, message_id)?;
    Ok(SendShepherdMessageResponse {
        started: true,
        queued: false,
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

pub async fn send_project_message(
    project_id: i64,
    content: Option<String>,
    chunks: Option<Vec<ShepherdMessageChunk>>,
) -> Result<SendShepherdMessageResponse, String> {
    send_scope_message(
        ShepherdScope::Project {
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

pub async fn prepare_project_scope_session(project_id: i64) -> Result<(), String> {
    let scope = ShepherdScope::Project {
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

pub async fn get_project_threads(project_id: i64) -> Result<Vec<ShepherdThread>, String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    store
        .list_project_threads(project_id)
        .await
        .map_err(|e| format!("failed to load project threads: {}", e))
}

pub async fn get_project_conversation(project_id: i64) -> Result<Vec<ShepherdChatMessage>, String> {
    load_scope_messages(
        &ShepherdScope::Project {
            project_id,
            workspace_path: None,
            focus: None,
        },
        100,
    )
    .await
}

pub async fn get_thread_conversation(
    project_id: i64,
    thread_id: &str,
    title: &str,
    limit: usize,
) -> Result<Vec<ShepherdChatMessage>, String> {
    load_scope_messages(
        &ShepherdScope::Thread {
            project_id,
            thread_id: thread_id.to_string(),
            title: title.to_string(),
            workspace_path: None,
            focus: None,
        },
        limit,
    )
    .await
}

pub async fn get_scope_activity(scope: ShepherdScope) -> Result<ShepherdScopeActivity, String> {
    scope_activity(&scope).await
}

pub async fn get_project_activity(project_id: i64) -> Result<ShepherdScopeActivity, String> {
    scope_activity(&ShepherdScope::Project {
        project_id,
        workspace_path: None,
        focus: None,
    })
    .await
}

pub async fn get_thread_activity(
    project_id: i64,
    thread_id: &str,
    title: &str,
) -> Result<ShepherdScopeActivity, String> {
    scope_activity(&ShepherdScope::Thread {
        project_id,
        thread_id: thread_id.to_string(),
        title: title.to_string(),
        workspace_path: None,
        focus: None,
    })
    .await
}

pub async fn get_shepherd_history(
    scope: ShepherdScope,
    limit: usize,
) -> Result<Vec<ShepherdChatMessage>, String> {
    let messages = load_scope_messages(&scope, limit).await?;
    Ok(match scope {
        ShepherdScope::Project { .. } | ShepherdScope::Thread { .. } => messages,
        _ => messages.into_iter().take(limit).collect(),
    })
}

fn project_survey_objective(project_name: &str) -> String {
    format!(
        "Survey the `{project_name}` project from the shepherd container, refresh the canvas and retained context when stale, and summarize the current project picture."
    )
}

fn project_survey_prompt(project_name: &str) -> String {
    format!(
        "Survey the `{project_name}` codebase from your shepherd container. Read the workspace, refresh the canvas HTML artifact if it is stale or incomplete, refresh retained context if it is stale, and summarize the architecture, pressure points, and next useful threads. Use `update_plan` so the thread card stays legible."
    )
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

pub async fn launch_project_survey_thread(project_id: i64) -> Result<ShepherdThread, String> {
    let project_store = ProjectStore::open()
        .await
        .map_err(|e| format!("failed to open project store: {}", e))?;
    let project = project_store
        .get_project(project_id)
        .await
        .map_err(|e| format!("failed to load project {}: {}", project_id, e))?;
    let thread_store = ShepherdThreadStore::open()
        .await
        .map_err(|e| format!("failed to open shepherd thread store: {}", e))?;
    let objective = project_survey_objective(&project.name);
    let summary = "Surveying the project and refreshing the shared picture.".to_string();

    let thread = match thread_store
        .find_project_thread_by_title(project_id, PROJECT_SURVEY_THREAD_TITLE)
        .await
        .map_err(|e| format!("failed to look up project survey thread: {}", e))?
    {
        Some(existing) => {
            let needs_workspace = existing
                .workspace_path
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
                || existing
                    .checkout_name
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty());
            if needs_workspace {
                let (workspace_path, checkout_name) =
                    prepare_thread_checkout(project_id, PROJECT_SURVEY_THREAD_TITLE).await?;
                thread_store
                    .update_thread(
                        &existing.id,
                        Some(PROJECT_SURVEY_THREAD_TITLE),
                        Some(&objective),
                        Some(&summary),
                        Some("running"),
                        Some(Some(&workspace_path)),
                        Some(Some(&checkout_name)),
                    )
                    .await
                    .map_err(|e| format!("failed to refresh survey thread: {}", e))?;
            } else {
                thread_store
                    .update_thread(
                        &existing.id,
                        Some(PROJECT_SURVEY_THREAD_TITLE),
                        Some(&objective),
                        Some(&summary),
                        Some("running"),
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| format!("failed to refresh survey thread: {}", e))?;
            }
            thread_store
                .get_thread(&existing.id)
                .await
                .map_err(|e| format!("failed to reload survey thread: {}", e))?
        }
        None => {
            let (workspace_path, checkout_name) =
                prepare_thread_checkout(project_id, PROJECT_SURVEY_THREAD_TITLE).await?;
            thread_store
                .create_thread(
                    project_id,
                    PROJECT_SURVEY_THREAD_TITLE,
                    &objective,
                    &summary,
                    Some(&workspace_path),
                    Some(&checkout_name),
                )
                .await
                .map_err(|e| format!("failed to create survey thread: {}", e))?
        }
    };

    let activity = get_thread_activity(project_id, &thread.id, &thread.title).await?;
    if activity.has_active_turn {
        return Ok(thread);
    }

    send_scope_message(
        thread_scope(&thread),
        Some(project_survey_prompt(&project.name)),
        None,
        None,
    )
    .await?;

    Ok(thread)
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
        ServerControlRequest::ArchiveThread {
            project_id,
            thread_id,
        } => {
            archive_thread_local(project_id, &thread_id).await?;
            Ok(None)
        }
        ServerControlRequest::DeleteThread {
            project_id,
            thread_id,
        } => {
            delete_thread_local(project_id, &thread_id).await?;
            Ok(None)
        }
    }
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
