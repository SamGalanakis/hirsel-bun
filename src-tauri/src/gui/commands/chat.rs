//! Chat session commands
//!
//! Commands for direct AI chat via ACP (Agent Control Protocol).

use std::sync::Arc;

use crate::core::{
    ChatEvent, ChatSessionConfig, ChatSessionManager, PermissionResponse, UIContext,
};

/// Start a new direct chat session with an AI agent
///
/// Returns the session ID. Events will be emitted via Tauri events.
#[tauri::command]
pub async fn start_chat_session(
    app: tauri::AppHandle,
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    agent_command: Vec<String>,
    working_dir: Option<String>,
    run_name: Option<String>,
    system_prompt: Option<String>,
) -> Result<String, String> {
    use tauri::Emitter;

    let config = ChatSessionConfig {
        agent_command,
        working_dir,
        run_name,
        system_prompt,
    };

    let (session_id, mut event_rx) = chat_manager
        .start_session(config)
        .await
        .map_err(|e| format!("Failed to start chat session: {}", e))?;

    // Spawn task to forward events to frontend
    let session_id_clone = session_id.clone();
    let app_clone = app.clone();
    eprintln!(
        "[FORWARD] Starting event forwarder for session {}",
        session_id
    );
    tokio::spawn(async move {
        eprintln!("[FORWARD] Event forwarder task started");
        while let Some(event) = event_rx.recv().await {
            eprintln!("[FORWARD] Received event: {:?}", event);
            // Emit event to frontend
            match app_clone.emit("chat-event", &event) {
                Ok(_) => eprintln!("[FORWARD] Emitted to frontend"),
                Err(e) => eprintln!("[FORWARD] Emit error: {:?}", e),
            }

            // Check if session ended
            if matches!(event, ChatEvent::SessionEnded { .. }) {
                break;
            }
        }
        eprintln!("[FORWARD] Event forwarder stopped for {}", session_id_clone);
    });

    Ok(session_id)
}

/// Send a message to an active chat session
///
/// The message will be prefixed with UI context (invisible to user).
#[tauri::command]
pub async fn send_chat_message(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    session_id: String,
    content: String,
    context: Option<UIContext>,
) -> Result<(), String> {
    chat_manager
        .send_message(&session_id, content, context)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))
}

/// Respond to a permission request from a chat session
#[tauri::command]
pub async fn respond_chat_permission(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    session_id: String,
    request_id: String,
    option_id: String,
) -> Result<(), String> {
    let response = PermissionResponse {
        request_id,
        option_id,
    };

    chat_manager
        .respond_to_permission(&session_id, response)
        .await
        .map_err(|e| format!("Failed to respond to permission: {}", e))
}

/// Stop an active chat session
#[tauri::command]
pub async fn stop_chat_session(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
    session_id: String,
) -> Result<(), String> {
    chat_manager
        .stop_session(&session_id)
        .await
        .map_err(|e| format!("Failed to stop session: {}", e))
}

/// List active chat sessions
#[tauri::command]
pub async fn list_chat_sessions(
    chat_manager: tauri::State<'_, Arc<ChatSessionManager>>,
) -> Result<Vec<String>, String> {
    Ok(chat_manager.list_sessions().await)
}
