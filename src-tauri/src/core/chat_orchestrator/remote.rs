//! Remote chat orchestrator implementation
//!
//! Connects to a remote Hirsel server via HTTP/SSE for chat sessions.
//! The agent runs on the remote server where the draft files are located.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::{
    ChatContext, ChatOrchestrator, ChatOrchestratorError, ChatOrchestratorResult,
    PermissionResponseRequest, SendMessageRequest, SessionInfo, StartSessionRequest,
};
use crate::core::chat_session::{ChatEvent, UIContext};
use crate::core::http_client::AuthenticatedClient;

/// Remote chat orchestrator that connects to a Hirsel server
pub struct RemoteChatOrchestrator {
    /// Authenticated HTTP client for API calls
    client: AuthenticatedClient,
    /// Active SSE connections (session_id -> abort handle)
    active_streams: Arc<Mutex<Vec<String>>>,
}

impl RemoteChatOrchestrator {
    /// Create a new remote chat orchestrator
    pub fn new(base_url: String, api_key: String) -> Self {
        let reqwest_client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client: AuthenticatedClient::with_client(reqwest_client, base_url, api_key),
            active_streams: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ChatOrchestrator for RemoteChatOrchestrator {
    async fn start_session(&self, context: ChatContext) -> ChatOrchestratorResult<SessionInfo> {
        let request = StartSessionRequest { context };

        let info: SessionInfo = self
            .client
            .post("/api/gyp/sessions", &request)
            .await
            .map_err(|e| ChatOrchestratorError::Http(format!("Failed to start session: {}", e)))?;

        // Track active session
        {
            let mut streams = self.active_streams.lock().await;
            streams.push(info.session_id.clone());
        }

        Ok(info)
    }

    async fn send_message(
        &self,
        session_id: &str,
        content: &str,
        context: Option<UIContext>,
    ) -> ChatOrchestratorResult<()> {
        let request = SendMessageRequest {
            content: content.to_string(),
            context,
        };

        self.client
            .post_empty(
                &format!("/api/gyp/sessions/{}/messages", session_id),
                &request,
            )
            .await
            .map_err(|e| ChatOrchestratorError::Http(format!("Failed to send message: {}", e)))
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        option_id: &str,
    ) -> ChatOrchestratorResult<()> {
        let request = PermissionResponseRequest {
            request_id: request_id.to_string(),
            option_id: option_id.to_string(),
        };

        self.client
            .post_empty(
                &format!("/api/gyp/sessions/{}/permission", session_id),
                &request,
            )
            .await
            .map_err(|e| {
                ChatOrchestratorError::Http(format!("Failed to respond to permission: {}", e))
            })
    }

    async fn stop_session(&self, session_id: &str) -> ChatOrchestratorResult<()> {
        // Remove from active sessions first
        {
            let mut streams = self.active_streams.lock().await;
            streams.retain(|id| id != session_id);
        }

        self.client
            .delete(&format!("/api/gyp/sessions/{}", session_id))
            .await
            .map_err(|e| ChatOrchestratorError::Http(format!("Failed to stop session: {}", e)))
    }

    async fn subscribe_events(
        &self,
        session_id: &str,
    ) -> ChatOrchestratorResult<BoxStream<'static, ChatEvent>> {
        let url = self
            .client
            .url(&format!("/api/gyp/sessions/{}/events", session_id));
        let auth = self.client.auth_header();

        // Create a new client without timeout for SSE (long-lived connection)
        let sse_client = Client::builder()
            .build()
            .map_err(|e| ChatOrchestratorError::Connection(e.to_string()))?;

        let response = sse_client
            .get(&url)
            .header("Authorization", auth)
            .header("Accept", "text/event-stream")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ChatOrchestratorError::Http(format!(
                "Failed to subscribe to events: {} - {}",
                status, text
            )));
        }

        // Parse SSE stream
        let session_id = session_id.to_string();
        let stream = parse_sse_stream(response, session_id);

        Ok(stream.boxed())
    }

    async fn list_sessions(&self) -> ChatOrchestratorResult<Vec<String>> {
        self.client
            .get("/api/gyp/sessions")
            .await
            .map_err(|e| ChatOrchestratorError::Http(format!("Failed to list sessions: {}", e)))
    }
}

/// Parse an SSE stream from a reqwest response
fn parse_sse_stream(
    response: reqwest::Response,
    session_id: String,
) -> impl Stream<Item = ChatEvent> {
    async_stream::stream! {
        use tokio::io::AsyncBufReadExt;
        use tokio_util::io::StreamReader;
        use tokio_util::bytes::Bytes;

        // Convert response body to AsyncRead
        let byte_stream = response.bytes_stream();
        let mapped_stream = byte_stream.map(|result: Result<Bytes, reqwest::Error>| {
            result.map_err(std::io::Error::other)
        });
        let reader = StreamReader::new(mapped_stream);
        let mut lines = tokio::io::BufReader::new(reader).lines();

        let mut event_type: Option<String> = None;
        let mut data_lines: Vec<String> = Vec::new();

        while let Ok(Some(line)) = lines.next_line().await {
            let line: String = line;
            if line.is_empty() {
                // Empty line = end of event
                if !data_lines.is_empty() {
                    let data = data_lines.join("\n");
                    data_lines.clear();

                    // Parse the event
                    if let Some(event) = parse_sse_event(event_type.as_deref(), &data, &session_id) {
                        yield event;
                    }
                    event_type = None;
                }
                continue;
            }

            if let Some(rest) = line.strip_prefix("event:") {
                event_type = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.trim().to_string());
            }
            // Ignore other lines (comments starting with :, id:, retry:, etc.)
        }

        // Yield SessionEnded if stream ends without it
        debug!("[remote] SSE stream ended for session {}", session_id);
    }
}

/// Parse a single SSE event into a ChatEvent
fn parse_sse_event(event_type: Option<&str>, data: &str, session_id: &str) -> Option<ChatEvent> {
    // Try to parse as JSON directly (the event might already be a ChatEvent)
    if let Ok(event) = serde_json::from_str::<ChatEvent>(data) {
        return Some(event);
    }

    // Otherwise, parse based on event type
    let event_type = event_type.unwrap_or("message");

    match event_type {
        "textDelta" => {
            #[derive(serde::Deserialize)]
            struct TextDelta {
                text: String,
            }
            if let Ok(delta) = serde_json::from_str::<TextDelta>(data) {
                return Some(ChatEvent::TextDelta {
                    session_id: session_id.to_string(),
                    text: delta.text,
                });
            }
        }
        "thinkingDelta" => {
            #[derive(serde::Deserialize)]
            struct ThinkingDelta {
                text: String,
            }
            if let Ok(delta) = serde_json::from_str::<ThinkingDelta>(data) {
                return Some(ChatEvent::ThinkingDelta {
                    session_id: session_id.to_string(),
                    text: delta.text,
                });
            }
        }
        "toolCallStart" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ToolCallStart {
                tool_call_id: String,
                title: String,
                kind: Option<String>,
                input: Option<String>,
            }
            if let Ok(tc) = serde_json::from_str::<ToolCallStart>(data) {
                return Some(ChatEvent::ToolCallStart {
                    session_id: session_id.to_string(),
                    tool_call_id: tc.tool_call_id,
                    title: tc.title,
                    kind: tc.kind,
                    input: tc.input,
                });
            }
        }
        "toolCallUpdate" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ToolCallUpdate {
                tool_call_id: String,
                status: String,
                title: Option<String>,
                output: Option<String>,
            }
            if let Ok(tc) = serde_json::from_str::<ToolCallUpdate>(data) {
                return Some(ChatEvent::ToolCallUpdate {
                    session_id: session_id.to_string(),
                    tool_call_id: tc.tool_call_id,
                    status: tc.status,
                    title: tc.title,
                    output: tc.output,
                });
            }
        }
        "permissionRequest" => {
            #[derive(serde::Deserialize)]
            struct PermReq {
                request: crate::core::chat_session::PendingPermission,
            }
            if let Ok(pr) = serde_json::from_str::<PermReq>(data) {
                return Some(ChatEvent::PermissionRequest {
                    session_id: session_id.to_string(),
                    request: pr.request,
                });
            }
        }
        "messageComplete" => {
            return Some(ChatEvent::MessageComplete {
                session_id: session_id.to_string(),
            });
        }
        "error" => {
            #[derive(serde::Deserialize)]
            struct Err {
                message: String,
            }
            if let Ok(e) = serde_json::from_str::<Err>(data) {
                return Some(ChatEvent::Error {
                    session_id: session_id.to_string(),
                    message: e.message,
                });
            }
        }
        "sessionEnded" => {
            return Some(ChatEvent::SessionEnded {
                session_id: session_id.to_string(),
            });
        }
        _ => {
            warn!("[remote] Unknown SSE event type: {}", event_type);
        }
    }

    None
}
