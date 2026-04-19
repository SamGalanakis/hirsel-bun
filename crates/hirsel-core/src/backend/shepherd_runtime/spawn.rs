//! Autonomous thread spawning: create a child thread, provision its
//! workspace copy, kick off a runtime turn seeded with the objective,
//! and let the parent observe + merge + discard it.
//!
//! This is the backend of the `spawn_thread` / `await_thread` /
//! `inspect_thread` / `merge_thread` / `merge_thread_retry` /
//! `discard_thread` tool family. All operations are plain tool calls —
//! no lashlang, no RLM mode.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::backend::runtime_settings::{keys, Defaults, RuntimeSettings};
use crate::backend::shepherd_chat::ShepherdChatStore;
use crate::backend::workspace_copy;
use crate::backend::{ShepherdThread, ShepherdThreadStore, BINDING_KIND_FREE};

use super::commands::{
    is_scope_active, is_scope_queue_nonempty, send_thread_message,
};
use super::types::scope_key as make_scope_key;
use super::types::ShepherdScope;

/// Request shape accepted by the `spawn_thread` tool.
#[derive(Debug, Clone, Deserialize)]
pub struct SpawnThreadRequest {
    pub objective: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub binding_kind: Option<String>,
    #[serde(default)]
    pub binding_data: Option<String>,
}

/// Return shape for a spawn call.
#[derive(Debug, Clone, Serialize)]
pub struct SpawnedThread {
    pub thread_id: String,
    pub title: String,
    pub status: String,
    pub workspace_path: Option<String>,
}

/// Create a child thread, provision its workspace (if `workspace_write`
/// is in the caps), and kick off its first turn with `objective` as the
/// seed user message. Returns immediately; the turn runs in the
/// background and can be observed via [`await_thread`].
pub async fn spawn_thread(
    project_id: i64,
    parent_id: Option<String>,
    request: SpawnThreadRequest,
) -> Result<SpawnedThread, String> {
    let objective = request.objective.trim().to_string();
    if objective.is_empty() {
        return Err("objective is required".to_string());
    }

    let title = request
        .title
        .and_then(|t| {
            let t = t.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .unwrap_or_else(|| summarise_for_title(&objective));

    let binding_kind = request
        .binding_kind
        .unwrap_or_else(|| BINDING_KIND_FREE.to_string());

    let caps = request.capabilities;

    // Provision a workspace copy if this child will write files.
    let (workspace_path, workspace_handle) = if caps.iter().any(|c| c == "workspace_write") {
        let canonical = canonical_workspace_for(project_id).await?;
        let tmp_id = uuid::Uuid::new_v4().to_string();
        let handle = workspace_copy::create_copy(&tmp_id, &canonical)
            .await
            .map_err(|e| format!("failed to create workspace copy: {e}"))?;
        (
            Some(handle.copy_dir.display().to_string()),
            Some(handle),
        )
    } else {
        (None, None)
    };

    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| e.to_string())?;

    let thread = store
        .create_thread_with_config(
            project_id,
            &title,
            &objective,
            &summarise_for_title(&objective),
            workspace_path.as_deref(),
            parent_id,
            caps,
            binding_kind,
            request.binding_data,
        )
        .await
        .map_err(|e| e.to_string())?;

    // If we created a workspace copy under a tmp id, we can now associate
    // it with the real thread id. We keep the on-disk path stable (by id)
    // and update the thread row if needed.
    if let Some(_handle) = workspace_handle.as_ref() {
        // The tmp id dir is fine as-is; we just recorded its absolute path
        // on the thread. A future pass can rename to match the thread id.
    }

    // Fire the first turn. Seeds the objective as the user message.
    send_thread_message(project_id, &thread.id, Some(objective), None)
        .await
        .map_err(|e| format!("failed to start turn: {e}"))?;

    Ok(SpawnedThread {
        thread_id: thread.id,
        title: thread.title,
        status: "running".to_string(),
        workspace_path,
    })
}

/// Spawn multiple children in one call. Fan-out; each child runs
/// concurrently in the background. Returns handles in input order.
pub async fn spawn_thread_batch(
    project_id: i64,
    parent_id: Option<String>,
    requests: Vec<SpawnThreadRequest>,
) -> Result<Vec<SpawnedThread>, String> {
    let max: usize = RuntimeSettings::get_or(
        keys::THREAD_SPAWN_MAX_CHILDREN_PER_TURN,
        Defaults::THREAD_SPAWN_MAX_CHILDREN_PER_TURN,
    )
    .await;
    if requests.len() > max {
        return Err(format!(
            "spawn_thread_batch: {} requests exceeds max {}",
            requests.len(),
            max
        ));
    }
    let mut out = Vec::with_capacity(requests.len());
    for req in requests {
        out.push(spawn_thread(project_id, parent_id.clone(), req).await?);
    }
    Ok(out)
}

/// Outcome of an `await_thread` call.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AwaitOutcome {
    /// Child's turn has completed. `final_output` is the last assistant
    /// message, if any.
    Done {
        thread_id: String,
        final_output: Option<String>,
    },
    /// Turn still running at timeout. Caller can re-await.
    Pending { thread_id: String },
}

/// Poll until the child's turn completes (or `timeout_ms` elapses).
pub async fn await_thread(
    project_id: i64,
    thread_id: String,
    timeout_ms_override: Option<u64>,
) -> Result<AwaitOutcome, String> {
    let timeout_ms: u64 = RuntimeSettings::resolve(
        keys::THREAD_AWAIT_DEFAULT_TIMEOUT_MS,
        timeout_ms_override,
        Defaults::THREAD_AWAIT_DEFAULT_TIMEOUT_MS,
    )
    .await;

    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| e.to_string())?;
    let thread = store
        .get_thread(&thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if thread.project_id != project_id {
        return Err("thread belongs to a different project".to_string());
    }

    let scope = thread_scope(&thread);
    let scope_key = make_scope_key(&scope);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        let active = is_scope_active(&scope_key);
        let queued = is_scope_queue_nonempty(&scope_key);
        if !active && !queued {
            // Turn has fully settled. Read the last assistant message and
            // treat it as the child's final output.
            let final_output = latest_assistant_text(project_id, &scope_key).await?;
            // Persist on the thread for future reads.
            let _ = store
                .get_thread(&thread_id)
                .await
                .map_err(|e| e.to_string())?;
            let _ = update_final_output(&store, &thread_id, final_output.as_deref()).await;
            return Ok(AwaitOutcome::Done {
                thread_id,
                final_output,
            });
        }
        if Instant::now() >= deadline {
            return Ok(AwaitOutcome::Pending { thread_id });
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Structured summary for `inspect_thread`.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadInspection {
    pub thread_id: String,
    pub status: String,
    pub merge_status: String,
    pub workspace_path: Option<String>,
    pub final_output: Option<String>,
    pub parent_id: Option<String>,
    pub diff: Option<workspace_copy::DiffStats>,
}

pub async fn inspect_thread(
    project_id: i64,
    thread_id: String,
) -> Result<ThreadInspection, String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| e.to_string())?;
    let thread = store
        .get_thread(&thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if thread.project_id != project_id {
        return Err("thread belongs to a different project".to_string());
    }
    let diff = if thread.cwd.is_some() {
        match rebuild_copy_handle(project_id, &thread).await {
            Ok(copy) => workspace_copy::inspect(&copy).await.ok(),
            Err(_) => None,
        }
    } else {
        None
    };

    Ok(ThreadInspection {
        thread_id: thread.id.clone(),
        status: thread.status.clone(),
        merge_status: thread.merge_status.clone(),
        workspace_path: thread.cwd.clone(),
        final_output: thread.final_output.clone(),
        parent_id: thread.parent_id.clone(),
        diff,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MergeResult {
    Merged { thread_id: String },
    Conflict {
        thread_id: String,
        files: Vec<String>,
    },
}

pub async fn merge_thread(
    project_id: i64,
    thread_id: String,
) -> Result<MergeResult, String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| e.to_string())?;
    let thread = store
        .get_thread(&thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if thread.project_id != project_id {
        return Err("thread belongs to a different project".to_string());
    }
    if thread.cwd.is_none() {
        return Err("thread has no workspace copy to merge".to_string());
    }

    let copy = rebuild_copy_handle(project_id, &thread).await?;
    let outcome = workspace_copy::merge(&copy).await?;

    let merge_status = match &outcome {
        workspace_copy::MergeOutcome::Merged => "merged",
        workspace_copy::MergeOutcome::Conflict { .. } => "conflict",
    };
    let _ = store
        .set_thread_merge_status(&thread_id, merge_status)
        .await;

    Ok(match outcome {
        workspace_copy::MergeOutcome::Merged => MergeResult::Merged { thread_id },
        workspace_copy::MergeOutcome::Conflict { files } => MergeResult::Conflict {
            thread_id,
            files,
        },
    })
}

pub async fn merge_thread_retry(
    project_id: i64,
    thread_id: String,
) -> Result<MergeResult, String> {
    merge_thread(project_id, thread_id).await
}

pub async fn discard_thread(
    project_id: i64,
    thread_id: String,
) -> Result<(), String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|e| e.to_string())?;
    let thread = store
        .get_thread(&thread_id)
        .await
        .map_err(|e| e.to_string())?;
    if thread.project_id != project_id {
        return Err("thread belongs to a different project".to_string());
    }
    if thread.cwd.is_some() {
        if let Ok(copy) = rebuild_copy_handle(project_id, &thread).await {
            let _ = workspace_copy::discard(&copy).await;
        }
        let _ = store
            .set_thread_workspace_path(&thread_id, None)
            .await;
    }
    let _ = store
        .set_thread_merge_status(&thread_id, "discarded")
        .await;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// internals
// ─────────────────────────────────────────────────────────────────────

async fn canonical_workspace_for(project_id: i64) -> Result<PathBuf, String> {
    let project = crate::backend::ProjectStore::open()
        .await
        .map_err(|e| e.to_string())?
        .get_project(project_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(cwd) = project
        .shepherd_cwd
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        return Ok(PathBuf::from(cwd));
    }
    if let Some(ws) = project
        .workspaces
        .iter()
        .find(|w| w.path.as_deref().map(|p| !p.trim().is_empty()).unwrap_or(false))
    {
        return Ok(PathBuf::from(ws.path.clone().unwrap_or_default()));
    }
    Err("project has no resolvable workspace path".to_string())
}

fn thread_scope(thread: &ShepherdThread) -> ShepherdScope {
    ShepherdScope::Thread {
        project_id: thread.project_id,
        thread_id: thread.id.clone(),
        title: thread.title.clone(),
        focus: None,
    }
}

async fn latest_assistant_text(
    project_id: i64,
    scope_key: &str,
) -> Result<Option<String>, String> {
    let store = ShepherdChatStore::open().await.map_err(|e| e.to_string())?;
    let messages = store
        .get_scope_messages(Some(project_id), Some(scope_key), 32)
        .await
        .map_err(|e| e.to_string())?;
    Ok(messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(|m| extract_text_from_chunks_json(&m.chunks_json))
        .and_then(|t| if t.trim().is_empty() { None } else { Some(t) }))
}

fn extract_text_from_chunks_json(json: &str) -> String {
    // Chunks are serde-serialized ShepherdMessageChunk values. The text
    // variants carry a "text" field. We just concatenate them to form a
    // readable `final_output`. Non-text chunks are skipped.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return String::new();
    };
    let Some(array) = value.as_array() else {
        return String::new();
    };
    let mut out = String::new();
    for chunk in array {
        if let Some(text) = chunk.get("text").and_then(|v| v.as_str()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

async fn update_final_output(
    store: &ShepherdThreadStore,
    thread_id: &str,
    final_output: Option<&str>,
) -> Result<(), String> {
    if let Some(text) = final_output {
        let db = crate::backend::db::global_db().await;
        let now = crate::backend::db::utc_now();
        let text = text.to_string();
        let _ = db
            .query(
                "UPDATE type::record('shepherd_thread', $tid) \
                 SET final_output = $out, updated_at = $now",
            )
            .bind(("tid", thread_id.to_string()))
            .bind(("out", text))
            .bind(("now", now))
            .await;
        let _ = store;
    }
    Ok(())
}

async fn rebuild_copy_handle(
    project_id: i64,
    thread: &ShepherdThread,
) -> Result<workspace_copy::WorkspaceCopy, String> {
    let canonical = canonical_workspace_for(project_id).await?;
    let copy_dir = thread
        .cwd
        .as_ref()
        .ok_or_else(|| "thread has no workspace copy".to_string())?;
    let copy_dir = PathBuf::from(copy_dir);
    // Re-read HEAD so merge has the right base commit.
    let base_commit = tokio::process::Command::new("git")
        .current_dir(&copy_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        });
    Ok(workspace_copy::WorkspaceCopy {
        thread_id: thread.id.clone(),
        canonical,
        copy_dir,
        base_commit,
    })
}

fn summarise_for_title(objective: &str) -> String {
    let cleaned: String = objective
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();
    if cleaned.trim().is_empty() {
        "spawned thread".to_string()
    } else {
        cleaned
    }
}
