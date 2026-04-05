pub mod commands;
mod history;
mod preview;
mod queries;
mod rpc;
mod runtime;
mod sandbox;
mod session;
mod shell;
mod tools;
pub mod types;
mod worker;

use crate::backend::ShepherdChatStore;

pub use commands::{
    archive_thread, create_thread, delete_thread, interrupt_scope_turn, prepare_shepherd_session,
    promote_thread, send_scope_message, send_shepherd_message, send_thread_message,
    start_server_control_listener, stop_scope_activity, PromoteThreadResponse,
    SendShepherdMessageResponse,
};
pub use queries::{
    get_project_threads, get_scope_activity, get_shepherd_activity, get_shepherd_conversation,
    get_shepherd_history, get_thread_activity, get_thread_conversation, ShepherdScopeActivity,
};
pub use session::ShepherdScopeSession;
pub use types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};
pub use worker::serve_worker_session;

pub async fn scrub_stale_startup_state() -> Result<(), String> {
    let session_store = session::ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let cleared_sessions = session_store
        .clear_stale_startup_state()
        .await
        .map_err(|error| format!("failed to clear stale shepherd sessions: {}", error))?;

    let chat_store = ShepherdChatStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd chat store: {}", error))?;
    let cleared_live_turns = chat_store
        .clear_all_live_turns()
        .await
        .map_err(|error| format!("failed to clear stale live turns: {}", error))?;

    if cleared_sessions > 0 || cleared_live_turns > 0 {
        tracing::info!(
            cleared_sessions,
            cleared_live_turns,
            "Cleared stale shepherd startup state"
        );
    }

    Ok(())
}

pub async fn reset_all_scope_sessions() -> Result<(), String> {
    commands::clear_all_runtime_tracking();

    let session_store = session::ShepherdSessionStore::open()
        .await
        .map_err(|error| format!("failed to open session store: {}", error))?;
    let sessions = session_store
        .list_sessions()
        .await
        .map_err(|error| format!("failed to list shepherd sessions: {}", error))?;

    for session in sessions {
        let scope: ShepherdScope = serde_json::from_str(&session.scope_json).map_err(|error| {
            format!(
                "failed to deserialize stored scope '{}' for reset: {}",
                session.scope_key, error
            )
        })?;
        sandbox::stop_scope_session(&scope).await?;
    }

    let chat_store = ShepherdChatStore::open()
        .await
        .map_err(|error| format!("failed to open shepherd chat store: {}", error))?;
    chat_store
        .clear_all_live_turns()
        .await
        .map_err(|error| format!("failed to clear live turns during reset: {}", error))?;

    Ok(())
}

pub async fn ensure_scope_session_ready(
    scope: &ShepherdScope,
) -> Result<ShepherdScopeSession, String> {
    sandbox::ensure_scope_session(scope).await
}
