//! Chat session commands
//!
//! Commands for direct AI chat via ACP (Agent Control Protocol).
//! Supports both local (same-process) and remote (HTTP/SSE) chat sessions
//! through the ChatOrchestrator abstraction.

use std::sync::Arc;

use futures::StreamExt;

use super::ResultExt;
use crate::core::{
    create_chat_orchestrator, create_workspace_provider, get_local_oauth_credentials, ChatContext,
    ChatEvent, ChatOrchestrator, ChatSessionManager, LocalChatOrchestrator, UIContext,
};

/// Manages chat orchestrators for different profiles
pub struct ChatOrchestratorManager {
    /// The local orchestrator (for local mode)
    local: Arc<LocalChatOrchestrator>,
}

impl ChatOrchestratorManager {
    pub fn new() -> Self {
        Self {
            local: Arc::new(LocalChatOrchestrator::new()),
        }
    }

    /// Create from an existing ChatSessionManager (for backwards compatibility)
    pub fn from_manager(manager: Arc<ChatSessionManager>) -> Self {
        Self {
            local: Arc::new(LocalChatOrchestrator::from_manager(manager)),
        }
    }

    /// Get the appropriate orchestrator for the given profile
    pub async fn get_orchestrator(
        &self,
        profile: Option<&str>,
    ) -> Result<Arc<dyn ChatOrchestrator>, String> {
        // For no profile or "local", use the local orchestrator
        if profile.is_none() || profile == Some("local") {
            return Ok(self.local.clone());
        }

        // For other profiles, check if we have a cached remote orchestrator
        // or create a new one
        let orchestrator =
            create_chat_orchestrator(profile).context("Failed to create orchestrator")?;
        Ok(Arc::from(orchestrator))
    }

    /// Get the local orchestrator's underlying manager
    pub fn local_manager(&self) -> &Arc<ChatSessionManager> {
        self.local.manager()
    }
}

impl Default for ChatOrchestratorManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Start a new direct chat session with an AI agent
///
/// Returns the session ID. Events will be emitted via Tauri events.
///
/// The `profile` parameter determines whether to use local or remote mode.
/// If not specified, uses local mode.
///
/// For remote profiles, local OAuth credentials are automatically read
/// from ~/.claude/.credentials.json and forwarded to the remote server.
#[tracing::instrument(skip(app, orchestrator_manager))]
#[tauri::command]
pub async fn start_chat_session(
    app: tauri::AppHandle,
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    agent_command: Vec<String>,
    working_dir: Option<String>,
    run_name: Option<String>,
    system_prompt: Option<String>,
    profile: Option<String>,
) -> Result<String, String> {
    use tauri::Emitter;

    // Read local credentials for remote profiles
    let credentials = if profile.as_deref() != Some("local") && profile.is_some() {
        get_local_oauth_credentials()
    } else {
        None
    };

    // Resolve working directory via workspace provider if we have a run
    let resolved_working_dir = if let Some(ref name) = run_name {
        let workspace = create_workspace_provider(profile.as_deref());
        Some(workspace.workspace_path(name).to_string_lossy().to_string())
    } else {
        working_dir
    };

    let context = ChatContext {
        agent_command,
        working_dir: resolved_working_dir,
        run_name,
        system_prompt,
        credentials,
        mcp_servers: vec![],
    };

    // Get the appropriate orchestrator
    let orchestrator = orchestrator_manager
        .get_orchestrator(profile.as_deref())
        .await?;

    // Start the session
    let session_info = orchestrator
        .start_session(context)
        .await
        .context("Failed to start chat session")?;

    let session_id = session_info.session_id.clone();

    // Subscribe to events and forward to frontend
    let mut event_stream = orchestrator
        .subscribe_events(&session_id)
        .await
        .context("Failed to subscribe to events")?;

    let session_id_clone = session_id.clone();
    let app_clone = app.clone();
    tracing::info!("[chat] Starting event forwarder for session {}", session_id);

    tokio::spawn(async move {
        tracing::debug!("[chat] Event forwarder task started");
        while let Some(event) = event_stream.next().await {
            tracing::debug!("[chat] Received event: {:?}", event);

            // Emit event to frontend
            if let Err(e) = app_clone.emit("chat-event", &event) {
                tracing::error!("[chat] Emit error: {:?}", e);
            }

            // Check if session ended
            if matches!(event, ChatEvent::SessionEnded { .. }) {
                break;
            }
        }
        tracing::info!("[chat] Event forwarder stopped for {}", session_id_clone);
    });

    Ok(session_id)
}

/// Send a message to an active chat session
///
/// The message will be prefixed with UI context (invisible to user).
#[tracing::instrument(skip(orchestrator_manager, context))]
#[tauri::command]
pub async fn send_chat_message(
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    session_id: String,
    content: String,
    context: Option<UIContext>,
    profile: Option<String>,
) -> Result<(), String> {
    let orchestrator = orchestrator_manager
        .get_orchestrator(profile.as_deref())
        .await?;

    orchestrator
        .send_message(&session_id, &content, context)
        .await
        .context("Failed to send message")
}

/// Respond to a permission request from a chat session
#[tracing::instrument(skip(orchestrator_manager))]
#[tauri::command]
pub async fn respond_chat_permission(
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    session_id: String,
    request_id: String,
    option_id: String,
    profile: Option<String>,
) -> Result<(), String> {
    let orchestrator = orchestrator_manager
        .get_orchestrator(profile.as_deref())
        .await?;

    orchestrator
        .respond_permission(&session_id, &request_id, &option_id)
        .await
        .context("Failed to respond to permission")
}

/// Stop an active chat session
#[tracing::instrument(skip(orchestrator_manager))]
#[tauri::command]
pub async fn stop_chat_session(
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    session_id: String,
    profile: Option<String>,
) -> Result<(), String> {
    let orchestrator = orchestrator_manager
        .get_orchestrator(profile.as_deref())
        .await?;

    orchestrator
        .stop_session(&session_id)
        .await
        .context("Failed to stop session")
}

/// List active chat sessions
#[tracing::instrument(skip(orchestrator_manager))]
#[tauri::command]
pub async fn list_chat_sessions(
    orchestrator_manager: tauri::State<'_, Arc<ChatOrchestratorManager>>,
    profile: Option<String>,
) -> Result<Vec<String>, String> {
    let orchestrator = orchestrator_manager
        .get_orchestrator(profile.as_deref())
        .await?;

    orchestrator
        .list_sessions()
        .await
        .context("Failed to list sessions")
}
