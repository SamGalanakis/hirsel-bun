//! Local chat orchestrator implementation
//!
//! Wraps the ChatSessionManager to provide the ChatOrchestrator interface
//! for local (same-process) chat sessions.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{mpsc, RwLock};

use super::{
    ChatContext, ChatOrchestrator, ChatOrchestratorError, ChatOrchestratorResult, SessionInfo,
};
use crate::core::chat_session::{ChatEvent, ChatSessionManager, UIContext};

/// Local chat orchestrator that wraps ChatSessionManager
pub struct LocalChatOrchestrator {
    /// The underlying session manager
    manager: Arc<ChatSessionManager>,
    /// Event receivers for each session (for subscribe_events)
    /// These are created during start_session and consumed by subscribe_events
    pending_receivers: Arc<RwLock<HashMap<String, mpsc::UnboundedReceiver<ChatEvent>>>>,
}

impl LocalChatOrchestrator {
    /// Create a new local chat orchestrator
    pub fn new() -> Self {
        Self {
            manager: Arc::new(ChatSessionManager::new()),
            pending_receivers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create from an existing ChatSessionManager
    pub fn from_manager(manager: Arc<ChatSessionManager>) -> Self {
        Self {
            manager,
            pending_receivers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get a reference to the underlying manager
    pub fn manager(&self) -> &Arc<ChatSessionManager> {
        &self.manager
    }
}

impl Default for LocalChatOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatOrchestrator for LocalChatOrchestrator {
    async fn start_session(&self, context: ChatContext) -> ChatOrchestratorResult<SessionInfo> {
        let config = context.into();

        let (session_id, event_rx) = self.manager.start_session(config).await?;

        // Store the receiver for subscribe_events
        {
            let mut receivers = self.pending_receivers.write().await;
            receivers.insert(session_id.clone(), event_rx);
        }

        Ok(SessionInfo { session_id })
    }

    async fn send_message(
        &self,
        session_id: &str,
        content: &str,
        context: Option<UIContext>,
    ) -> ChatOrchestratorResult<()> {
        self.manager
            .send_message(session_id, content.to_string(), context)
            .await?;
        Ok(())
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        option_id: &str,
    ) -> ChatOrchestratorResult<()> {
        let response = crate::core::chat_session::PermissionResponse {
            request_id: request_id.to_string(),
            option_id: option_id.to_string(),
        };
        self.manager
            .respond_to_permission(session_id, response)
            .await?;
        Ok(())
    }

    async fn stop_session(&self, session_id: &str) -> ChatOrchestratorResult<()> {
        // Clean up pending receiver if any
        {
            let mut receivers = self.pending_receivers.write().await;
            receivers.remove(session_id);
        }

        self.manager.stop_session(session_id).await?;
        Ok(())
    }

    async fn subscribe_events(
        &self,
        session_id: &str,
    ) -> ChatOrchestratorResult<BoxStream<'static, ChatEvent>> {
        // Take the receiver from pending_receivers
        let rx = {
            let mut receivers = self.pending_receivers.write().await;
            receivers.remove(session_id).ok_or_else(|| {
                ChatOrchestratorError::SessionNotFound(format!(
                    "No event receiver for session {}. Was subscribe_events already called?",
                    session_id
                ))
            })?
        };

        // Convert to a stream
        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        Ok(stream.boxed())
    }

    async fn list_sessions(&self) -> ChatOrchestratorResult<Vec<String>> {
        Ok(self.manager.list_sessions().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_orchestrator_creation() {
        let orchestrator = LocalChatOrchestrator::new();
        let sessions = orchestrator.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }
}
