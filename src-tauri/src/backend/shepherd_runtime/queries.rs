use serde::{Deserialize, Serialize};

use super::history::{load_scope_live_turn, load_scope_messages};
use super::sandbox::scope_key;
use super::session::{ShepherdScopeSession, ShepherdSessionStore};
use super::types::ShepherdScope;
use crate::backend::{ShepherdChatMessage, ShepherdLiveTurn, ShepherdThread, ShepherdThreadStore};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShepherdScopeActivity {
    pub session: Option<ShepherdScopeSession>,
    pub live_turn: Option<ShepherdLiveTurn>,
    pub has_active_turn: bool,
}

pub(super) async fn scope_activity(scope: &ShepherdScope) -> Result<ShepherdScopeActivity, String> {
    let key = scope_key(scope);
    let store = ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let session = store
        .get_session(&key)
        .await
        .map_err(|error| format!("failed to load session: {}", error))?;
    let live_turn = load_scope_live_turn(scope).await?;
    let has_active_turn = session.as_ref().is_some_and(|session| {
        matches!(
            session.status.as_str(),
            "starting_container" | "waiting_for_socket" | "running" | "interrupting"
        )
    }) || live_turn.as_ref().is_some_and(|turn| {
        matches!(
            turn.status.as_str(),
            "starting" | "starting_container" | "waiting_for_socket" | "running" | "interrupting"
        )
    });
    Ok(ShepherdScopeActivity {
        session,
        live_turn,
        has_active_turn,
    })
}

pub async fn get_project_threads(project_id: i64) -> Result<Vec<ShepherdThread>, String> {
    let store = ShepherdThreadStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd thread store: {}", error))?;
    store
        .list_project_threads(project_id)
        .await
        .map_err(|error| format!("failed to load project threads: {}", error))
}

pub async fn get_shepherd_conversation(
    project_id: i64,
) -> Result<Vec<ShepherdChatMessage>, String> {
    load_scope_messages(
        &ShepherdScope::Shepherd {
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

pub async fn get_shepherd_activity(project_id: i64) -> Result<ShepherdScopeActivity, String> {
    scope_activity(&ShepherdScope::Shepherd {
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
        ShepherdScope::Shepherd { .. } | ShepherdScope::Thread { .. } => messages,
        _ => messages.into_iter().take(limit).collect(),
    })
}
