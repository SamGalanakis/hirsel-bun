//! Librarian event queue — captures user activity signals.
//!
//! Whenever the user creates, edits, or deletes a canvas entity (task,
//! document, goal, decision), we record a `librarian_event` row. A future
//! librarian consumer can drain these to decide what to re-summarize or
//! re-link. For now rows simply accumulate and are exposed to any librarian
//! process that polls the table.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use surrealdb::types::SurrealValue;

use crate::backend::db::{global_db, DbClient};

const EVENT_TABLE: &str = "librarian_event";

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct LibrarianEvent {
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

/// Record a user activity event. Fire-and-forget — errors are logged, not
/// propagated, so a failing event never blocks the user action.
pub async fn record_user_activity(project_id: i64, kind: &str, payload: Value) {
    let db = db().await;
    let res: Result<Option<Value>, _> = db
        .create(EVENT_TABLE)
        .content(json!({
            "project_id": project_id,
            "event_kind": kind,
            "payload": payload,
        }))
        .await;
    if let Err(e) = res {
        tracing::warn!("failed to record librarian event '{kind}': {e}");
    }
}

#[allow(dead_code)]
pub async fn drain_events(project_id: i64) -> Result<Vec<LibrarianEvent>, String> {
    let db = db().await;
    let mut response = db
        .query(
            "LET $events = (SELECT * FROM librarian_event WHERE project_id = $pid ORDER BY created_at ASC);
             DELETE FROM librarian_event WHERE project_id = $pid;
             RETURN $events;",
        )
        .bind(("pid", project_id))
        .await
        .map_err(|e| format!("failed to drain librarian events: {e}"))?;
    let events: Vec<LibrarianEvent> = response.take(2).unwrap_or_default();
    Ok(events)
}
