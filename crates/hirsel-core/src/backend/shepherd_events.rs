use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, DbClient};
use crate::backend::shepherd_runtime::types::{ShepherdMessageChunk, ShepherdScope};

const EVENT_TABLE: &str = "shepherd_event";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct ShepherdEvent {
    pub project_id: i64,
    pub event_kind: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub created_at: Option<String>,
}

async fn db() -> &'static DbClient {
    global_db().await
}

pub async fn insert_event(project_id: i64, kind: &str, payload: Value) -> Result<(), String> {
    let db = db().await;
    let _: Option<Value> = db
        .create(EVENT_TABLE)
        .content(json!({
            "project_id": project_id,
            "event_kind": kind,
            "payload": payload,
        }))
        .await
        .map_err(|e| format!("failed to insert shepherd event: {e}"))?;
    Ok(())
}

pub async fn drain_events(project_id: i64) -> Result<Vec<ShepherdEvent>, String> {
    let db = db().await;
    let mut response = db
        .query(
            "LET $events = (SELECT * FROM shepherd_event WHERE project_id = $pid ORDER BY created_at ASC);
             DELETE FROM shepherd_event WHERE project_id = $pid;
             RETURN $events;",
        )
        .bind(("pid", project_id))
        .await
        .map_err(|e| format!("failed to drain shepherd events: {e}"))?;

    let events: Vec<ShepherdEvent> = response.take(2).unwrap_or_default();
    Ok(events)
}

pub fn format_event_batch(events: &[ShepherdEvent]) -> String {
    if events.is_empty() {
        return String::new();
    }
    let mut lines = vec![format!("## Events since last turn ({})\n", events.len())];
    for event in events {
        let kind = &event.event_kind;
        let payload = &event.payload;
        let line = match kind.as_str() {
            "thread_completed" => {
                let title = payload
                    .get("thread_title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let summary = payload
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                format!("[thread_completed] Thread \"{title}\" finished.\nSummary: {summary}")
            }
            "thread_blocked" => {
                let title = payload
                    .get("thread_title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let summary = payload
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                format!("[thread_blocked] Thread \"{title}\" is blocked.\nSummary: {summary}")
            }
            "thread_failed" => {
                let title = payload
                    .get("thread_title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let summary = payload
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                format!("[thread_failed] Thread \"{title}\" failed.\nSummary: {summary}")
            }
            "thread_started" => {
                let title = payload
                    .get("thread_title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                format!("[thread_started] Thread \"{title}\" started running.")
            }
            "user_thread_message" => {
                let title = payload
                    .get("thread_title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let content = payload
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                format!(
                    "[user_thread_message] User messaged thread \"{title}\" directly.\nContent: \"{content}\""
                )
            }
            _ => {
                format!("[{kind}] {}", serde_json::to_string(payload).unwrap_or_default())
            }
        };
        lines.push(line);
        lines.push(String::new());
    }
    lines.join("\n")
}

pub async fn dispatch_shepherd_event_batch(project_id: i64) -> Result<(), String> {
    let events = drain_events(project_id).await?;
    if events.is_empty() {
        return Ok(());
    }
    let batch_text = format_event_batch(&events);
    let preview = format!("Event batch: {} events", events.len());

    let scope = ShepherdScope::Shepherd {
        project_id,
        focus: None,
    };
    let chunks = vec![ShepherdMessageChunk::Text {
        content: batch_text,
    }];
    let options =
        crate::backend::shepherd_chat::ShepherdChatMessageOptions::event_batch(preview);

    crate::backend::shepherd_runtime::commands::dispatch_scope_message_local(
        scope, chunks, None, &options,
    )
    .await
    .map(|_| ())
}
