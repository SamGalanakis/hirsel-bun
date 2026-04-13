use std::sync::OnceLock;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

static LIVE_UPDATES: OnceLock<broadcast::Sender<LiveUpdateEvent>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveUpdateKind {
    ProjectChanged,
    ProjectPreparationChanged,
    ProjectSurfaceChanged,
    ProjectHistoryChanged,
    ProjectActivityChanged,
    LibrarianHistoryChanged,
    LibrarianActivityChanged,
    KnowledgeGraphChanged,
    ThreadsChanged,
    ThreadChanged,
    ThreadHistoryChanged,
    ThreadActivityChanged,
    TasksChanged,
    TaskChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveUpdateEvent {
    pub project_id: i64,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub kind: LiveUpdateKind,
    pub timestamp: String,
}

pub fn subscribe() -> broadcast::Receiver<LiveUpdateEvent> {
    sender().subscribe()
}

pub fn publish_project(project_id: i64, kind: LiveUpdateKind) {
    let _ = sender().send(LiveUpdateEvent {
        project_id,
        thread_id: None,
        kind,
        timestamp: Utc::now().to_rfc3339(),
    });
}

pub fn publish_thread(project_id: i64, thread_id: impl Into<String>, kind: LiveUpdateKind) {
    let _ = sender().send(LiveUpdateEvent {
        project_id,
        thread_id: Some(thread_id.into()),
        kind,
        timestamp: Utc::now().to_rfc3339(),
    });
}

pub fn scope_project_id(project_id: Option<i64>) -> Option<i64> {
    project_id
}

pub fn scope_thread_id(scope_key: Option<&str>) -> Option<&str> {
    scope_key.and_then(|scope_key| scope_key.strip_prefix("__thread__:"))
}

pub fn scope_is_librarian(scope_key: Option<&str>) -> bool {
    scope_key
        .map(|scope_key| scope_key.starts_with("__librarian__:"))
        .unwrap_or(false)
}

fn sender() -> &'static broadcast::Sender<LiveUpdateEvent> {
    LIVE_UPDATES.get_or_init(|| {
        let (sender, _) = broadcast::channel(512);
        sender
    })
}
