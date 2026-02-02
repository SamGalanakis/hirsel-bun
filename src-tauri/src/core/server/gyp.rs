//! Gyp chat API route handlers
//!
//! Server-side endpoints for remote Gyp chat sessions.
//! These endpoints allow remote clients to interact with Gyp (the AI assistant)
//! running on this server.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures::stream::Stream;
use futures::StreamExt;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info};

use crate::core::chat_orchestrator::{
    PermissionResponseRequest, SendMessageRequest, SessionInfo, StartSessionRequest,
};
use crate::core::chat_session::{ChatEvent, ChatSessionConfig, ChatSessionManager};
use crate::core::credentials::CredentialStore;

/// State for Gyp chat sessions
pub struct GypState {
    /// Session manager for running chat sessions
    pub manager: Arc<ChatSessionManager>,
    /// Event receivers for SSE streaming (session_id -> receiver)
    pub event_receivers: RwLock<HashMap<String, mpsc::UnboundedReceiver<ChatEvent>>>,
}

impl GypState {
    pub fn new() -> Self {
        Self {
            manager: Arc::new(ChatSessionManager::new()),
            event_receivers: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for GypState {
    fn default() -> Self {
        Self::new()
    }
}

/// Error response for Gyp endpoints
#[derive(serde::Serialize)]
pub struct GypErrorResponse {
    pub error: String,
}

/// Gyp error enum
pub enum GypError {
    SessionNotFound(String),
    StartFailed(String),
    SendFailed(String),
    PermissionFailed(String),
    Other(String),
}

/// Convert to HTTP response
impl IntoResponse for GypError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GypError::SessionNotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            GypError::StartFailed(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            GypError::SendFailed(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            GypError::PermissionFailed(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            GypError::Other(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };
        (status, Json(GypErrorResponse { error: message })).into_response()
    }
}

type GypResult<T> = Result<T, GypError>;

// =============================================================================
// Session Management
// =============================================================================

/// Start a new chat session
///
/// POST /api/gyp/sessions
///
/// If credentials are provided in the request, they are stored encrypted for future use.
/// If not provided, previously stored credentials are loaded.
pub async fn start_session(
    State(state): State<Arc<GypState>>,
    Json(body): Json<StartSessionRequest>,
) -> GypResult<Json<SessionInfo>> {
    info!("[gyp] Starting new chat session");

    // Resolve credentials: from request or from storage
    let credentials = match body.context.credentials {
        Some(creds) => {
            // Store for future use (if encryption key is available)
            if let Ok(store) = CredentialStore::open() {
                if let Err(e) = store.store_all(&creds) {
                    debug!("[gyp] Failed to store credentials: {}", e);
                }
            }
            Some(creds)
        }
        None => {
            // Load from store
            CredentialStore::open().ok().map(|store| store.load_all())
        }
    };

    // Validate that we have credentials
    if credentials.as_ref().is_none_or(|c| !c.has_any()) {
        return Err(GypError::Other(
            "No credentials available. Please configure OAuth token or API key.".into(),
        ));
    }

    let config = ChatSessionConfig {
        agent_command: body.context.agent_command,
        working_dir: body.context.working_dir,
        run_name: body.context.run_name,
        system_prompt: body.context.system_prompt,
        credentials,
        mcp_servers: body.context.mcp_servers,
    };

    let (session_id, event_rx) = state
        .manager
        .start_session(config)
        .await
        .map_err(|e| GypError::StartFailed(e.to_string()))?;

    // Store the event receiver for SSE streaming
    {
        let mut receivers = state.event_receivers.write().await;
        receivers.insert(session_id.clone(), event_rx);
    }

    info!("[gyp] Started session: {}", session_id);

    Ok(Json(SessionInfo { session_id }))
}

/// List active sessions
///
/// GET /api/gyp/sessions
pub async fn list_sessions(State(state): State<Arc<GypState>>) -> GypResult<Json<Vec<String>>> {
    let sessions = state.manager.list_sessions().await;
    Ok(Json(sessions))
}

/// Send a message to a session
///
/// POST /api/gyp/sessions/:id/messages
pub async fn send_message(
    State(state): State<Arc<GypState>>,
    Path(session_id): Path<String>,
    Json(body): Json<SendMessageRequest>,
) -> GypResult<StatusCode> {
    debug!("[gyp] Sending message to session {}", session_id);

    state
        .manager
        .send_message(&session_id, body.content, body.context)
        .await
        .map_err(|e| GypError::SendFailed(e.to_string()))?;

    Ok(StatusCode::ACCEPTED)
}

/// Respond to a permission request
///
/// POST /api/gyp/sessions/:id/permission
pub async fn respond_permission(
    State(state): State<Arc<GypState>>,
    Path(session_id): Path<String>,
    Json(body): Json<PermissionResponseRequest>,
) -> GypResult<StatusCode> {
    debug!(
        "[gyp] Permission response for session {}: request={}, option={}",
        session_id, body.request_id, body.option_id
    );

    let response = crate::core::chat_session::PermissionResponse {
        request_id: body.request_id,
        option_id: body.option_id,
    };

    state
        .manager
        .respond_to_permission(&session_id, response)
        .await
        .map_err(|e| GypError::PermissionFailed(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Stop a session
///
/// DELETE /api/gyp/sessions/:id
pub async fn stop_session(
    State(state): State<Arc<GypState>>,
    Path(session_id): Path<String>,
) -> GypResult<StatusCode> {
    info!("[gyp] Stopping session: {}", session_id);

    // Remove event receiver
    {
        let mut receivers = state.event_receivers.write().await;
        receivers.remove(&session_id);
    }

    state
        .manager
        .stop_session(&session_id)
        .await
        .map_err(|e| GypError::Other(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// SSE Events Stream
// =============================================================================

/// Subscribe to session events via SSE
///
/// GET /api/gyp/sessions/:id/events
pub async fn session_events(
    State(state): State<Arc<GypState>>,
    Path(session_id): Path<String>,
) -> GypResult<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>> {
    info!("[gyp] SSE subscription for session: {}", session_id);

    // Take the receiver from the state
    let rx = {
        let mut receivers = state.event_receivers.write().await;
        receivers.remove(&session_id).ok_or_else(|| {
            GypError::SessionNotFound(format!("Session not found: {}", session_id))
        })?
    };

    // Convert to SSE stream
    let stream = UnboundedReceiverStream::new(rx).map(|event| {
        // Serialize the event as JSON
        let event_type = match &event {
            ChatEvent::TextDelta { .. } => "textDelta",
            ChatEvent::ThinkingDelta { .. } => "thinkingDelta",
            ChatEvent::ToolCallStart { .. } => "toolCallStart",
            ChatEvent::ToolCallUpdate { .. } => "toolCallUpdate",
            ChatEvent::PermissionRequest { .. } => "permissionRequest",
            ChatEvent::MessageComplete { .. } => "messageComplete",
            ChatEvent::Error { .. } => "error",
            ChatEvent::SessionEnded { .. } => "sessionEnded",
        };

        let data = serde_json::to_string(&event)
            .unwrap_or_else(|e| format!(r#"{{"error":"serialization failed: {}"}}"#, e));

        Ok(Event::default().event(event_type).data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// =============================================================================
// Asset Management
// =============================================================================

/// Response for asset upload
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUploadResponse {
    pub filename: String,
}

/// Upload an asset file to a run's assets directory
///
/// POST /api/runs/:name/assets
///
/// Accepts multipart form data with a file field.
/// Returns the saved filename (may differ if name conflict).
pub async fn upload_asset(
    Path(run_name): Path<String>,
    mut multipart: axum::extract::Multipart,
) -> GypResult<Json<AssetUploadResponse>> {
    use crate::core::config;
    use crate::core::files::Files;

    info!("[gyp] Asset upload for run: {}", run_name);

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(GypError::Other(format!("Run '{}' not found", run_name)));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    // Create assets directory if it doesn't exist
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| GypError::Other(format!("Failed to create assets directory: {}", e)))?;

    // Process the first field from the multipart form
    let field = multipart
        .next_field()
        .await
        .map_err(|e| GypError::Other(format!("Failed to read multipart field: {}", e)))?
        .ok_or_else(|| GypError::Other("No file uploaded".into()))?;

    // Get filename from the field
    let filename = field
        .file_name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "upload".to_string());

    // Read the file data
    let data = field
        .bytes()
        .await
        .map_err(|e| GypError::Other(format!("Failed to read file data: {}", e)))?;

    // Find a unique filename
    let dest_filename = find_unique_asset_filename(&assets_dir, &filename);
    let dest_path = assets_dir.join(&dest_filename);

    // Write the file
    std::fs::write(&dest_path, &data)
        .map_err(|e| GypError::Other(format!("Failed to write asset: {}", e)))?;

    info!("[gyp] Saved asset: {} -> {}", filename, dest_filename);

    Ok(Json(AssetUploadResponse {
        filename: dest_filename,
    }))
}

/// Find a unique filename in the assets directory
fn find_unique_asset_filename(dir: &std::path::Path, filename: &str) -> String {
    let dest = dir.join(filename);
    if !dest.exists() {
        return filename.to_string();
    }

    let path = std::path::Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let ext = path.extension().and_then(|s| s.to_str());

    let mut counter = 1;
    loop {
        let new_name = match ext {
            Some(e) => format!("{}-{}.{}", stem, counter, e),
            None => format!("{}-{}", stem, counter),
        };

        if !dir.join(&new_name).exists() {
            return new_name;
        }
        counter += 1;
    }
}

/// Get the assets path for a run (for constructing URLs)
///
/// GET /api/runs/:name/assets-path
pub async fn get_assets_path(Path(run_name): Path<String>) -> GypResult<Json<String>> {
    use crate::core::config;
    use crate::core::files::Files;

    let run_dir = config::run_dir(&run_name);
    if !run_dir.exists() {
        return Err(GypError::Other(format!("Run '{}' not found", run_name)));
    }

    let files = Files::new(&run_dir);
    let assets_dir = files.assets();

    Ok(Json(assets_dir.to_string_lossy().to_string()))
}
