pub mod commands;
mod comment_prompt;
mod comment_tools;
mod focus_prompt;
mod history;
mod preview;
mod queries;
mod runtime;
mod search_context;
mod session;
mod shell;
mod spawn;
mod spawn_tools;
mod task_tools;
mod tools;
pub mod types;
mod worker;

use crate::backend::ShepherdChatStore;

pub use commands::{
    archive_thread, create_thread, delete_thread, interrupt_scope_turn, prepare_shepherd_session,
    send_scope_message, send_shepherd_message, send_thread_message, stop_scope_activity,
    SendShepherdMessageResponse,
};
pub use spawn::{
    await_thread, discard_thread, inspect_thread, merge_thread, merge_thread_retry, spawn_thread,
    spawn_thread_batch, AwaitOutcome, MergeResult, SpawnThreadRequest, SpawnedThread,
    ThreadInspection,
};
pub use queries::{
    get_project_threads, get_scope_activity, get_shepherd_activity, get_shepherd_conversation,
    get_shepherd_history, get_thread_activity, get_thread_conversation, ShepherdScopeActivity,
};
pub use session::ShepherdScopeSession;
pub use types::{ShepherdMessageChunk, ShepherdScope, ShepherdTaskFocus};

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
        session_store
            .delete_session(&session.scope_key)
            .await
            .map_err(|error| {
                format!(
                    "failed to delete session '{}': {}",
                    session.scope_key, error
                )
            })?;
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
