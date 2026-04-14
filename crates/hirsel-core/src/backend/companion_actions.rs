//! In-memory queue of UI actions the Shepherd companion wants the
//! frontend to perform (filter canvas, focus node, center on node, …).
//!
//! Tools push actions into the queue then the frontend drains via
//! `GET /api/projects/{id}/companion/actions`. We also emit a
//! `CompanionAction` live-update event so the frontend knows to drain.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backend::live_updates::{self, LiveUpdateKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionAction {
    /// e.g. "filter_canvas", "focus_node", "center_on_node"
    pub action: String,
    pub payload: Value,
}

static QUEUE: OnceLock<Mutex<HashMap<i64, Vec<CompanionAction>>>> = OnceLock::new();

fn queue() -> &'static Mutex<HashMap<i64, Vec<CompanionAction>>> {
    QUEUE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn enqueue(project_id: i64, action: CompanionAction) {
    if let Ok(mut map) = queue().lock() {
        map.entry(project_id).or_default().push(action);
    }
    live_updates::publish_project(project_id, LiveUpdateKind::CompanionAction);
}

pub fn drain(project_id: i64) -> Vec<CompanionAction> {
    queue()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&project_id))
        .unwrap_or_default()
}
