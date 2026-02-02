//! Chat orchestrator abstraction for remote/local Gyp coordination
//!
//! This module provides the `ChatOrchestrator` trait that abstracts chat session
//! operations for Gyp (the AI assistant for draft editing). It has two implementations:
//! - `LocalChatOrchestrator`: Direct calls to ChatSessionManager (default)
//! - `RemoteChatOrchestrator`: HTTP/SSE calls to a remote Hirsel server

mod local;
mod remote;

pub use local::LocalChatOrchestrator;
pub use remote::RemoteChatOrchestrator;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::chat_session::{ChatEvent, ChatSessionConfig, PermissionResponse, UIContext};
use crate::core::config::Config;
use crate::core::credentials::ForwardedCredentials;

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug, Error)]
pub enum ChatOrchestratorError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session already exists: {0}")]
    SessionExists(String),

    #[error("Failed to start session: {0}")]
    StartFailed(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("SSE stream error: {0}")]
    StreamError(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Unknown profile: {0}")]
    UnknownProfile(String),

    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for ChatOrchestratorError {
    fn from(e: reqwest::Error) -> Self {
        ChatOrchestratorError::Http(e.to_string())
    }
}

impl From<serde_json::Error> for ChatOrchestratorError {
    fn from(e: serde_json::Error) -> Self {
        ChatOrchestratorError::Serialization(e.to_string())
    }
}

impl From<crate::core::chat_session::ChatSessionError> for ChatOrchestratorError {
    fn from(e: crate::core::chat_session::ChatSessionError) -> Self {
        ChatOrchestratorError::Other(e.to_string())
    }
}

pub type ChatOrchestratorResult<T> = Result<T, ChatOrchestratorError>;

// =============================================================================
// DTOs for API communication
// =============================================================================

/// MCP server configuration for chat sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMcpServer {
    /// Server name
    pub name: String,
    /// Command and args (first element is command, rest are args)
    pub command: Vec<String>,
    /// Environment variables as key-value pairs
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// Context for starting a chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContext {
    /// Agent command (e.g., ["claude", "acp"])
    pub agent_command: Vec<String>,
    /// Working directory for the agent
    pub working_dir: Option<String>,
    /// Current run name (for hirsel MCP access)
    pub run_name: Option<String>,
    /// System prompt to prepend
    pub system_prompt: Option<String>,
    /// Credentials to forward to the agent process
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<ForwardedCredentials>,
    /// MCP servers to configure for this session
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<ChatMcpServer>,
}

impl From<ChatContext> for ChatSessionConfig {
    fn from(ctx: ChatContext) -> Self {
        ChatSessionConfig {
            agent_command: ctx.agent_command,
            working_dir: ctx.working_dir,
            run_name: ctx.run_name,
            system_prompt: ctx.system_prompt,
            credentials: ctx.credentials,
            mcp_servers: ctx.mcp_servers,
        }
    }
}

impl From<ChatSessionConfig> for ChatContext {
    fn from(cfg: ChatSessionConfig) -> Self {
        ChatContext {
            agent_command: cfg.agent_command,
            working_dir: cfg.working_dir,
            run_name: cfg.run_name,
            system_prompt: cfg.system_prompt,
            credentials: cfg.credentials,
            mcp_servers: cfg.mcp_servers,
        }
    }
}

/// Information about a started session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
}

/// Send message request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub content: String,
    pub context: Option<UIContext>,
}

/// Permission response request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponseRequest {
    pub request_id: String,
    pub option_id: String,
}

impl From<PermissionResponseRequest> for PermissionResponse {
    fn from(req: PermissionResponseRequest) -> Self {
        PermissionResponse {
            request_id: req.request_id,
            option_id: req.option_id,
        }
    }
}

/// Start session request (for server API)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSessionRequest {
    pub context: ChatContext,
}

// =============================================================================
// ChatOrchestrator Trait
// =============================================================================

/// High-level chat orchestrator trait for managing Gyp chat sessions
///
/// This trait abstracts the chat session layer, allowing both local and remote
/// implementations. The GUI uses this trait to perform all chat operations.
#[async_trait]
pub trait ChatOrchestrator: Send + Sync {
    /// Start a new chat session
    ///
    /// Returns session info. Events are received via `subscribe_events`.
    async fn start_session(&self, context: ChatContext) -> ChatOrchestratorResult<SessionInfo>;

    /// Send a message to a chat session
    ///
    /// The message will be prefixed with UI context if provided.
    async fn send_message(
        &self,
        session_id: &str,
        content: &str,
        context: Option<UIContext>,
    ) -> ChatOrchestratorResult<()>;

    /// Respond to a permission request
    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        option_id: &str,
    ) -> ChatOrchestratorResult<()>;

    /// Stop a chat session
    async fn stop_session(&self, session_id: &str) -> ChatOrchestratorResult<()>;

    /// Subscribe to chat events for a session
    ///
    /// Returns a stream of ChatEvents. The stream ends when the session ends.
    async fn subscribe_events(
        &self,
        session_id: &str,
    ) -> ChatOrchestratorResult<BoxStream<'static, ChatEvent>>;

    /// List active session IDs
    async fn list_sessions(&self) -> ChatOrchestratorResult<Vec<String>>;
}

// =============================================================================
// Factory Function
// =============================================================================

/// Create a chat orchestrator based on the profile configuration
///
/// If no profile is specified, uses the default profile from config.
/// Returns a LocalChatOrchestrator for local mode, RemoteChatOrchestrator for remote.
pub fn create_chat_orchestrator(
    profile: Option<&str>,
) -> ChatOrchestratorResult<Box<dyn ChatOrchestrator>> {
    use crate::core::config::OrchestratorMode;

    let (config, _) = Config::load().map_err(|e| ChatOrchestratorError::Config(e.to_string()))?;

    let profile_name = profile.unwrap_or(&config.default_profile);

    let profile_config = config
        .profiles
        .get(profile_name)
        .ok_or_else(|| ChatOrchestratorError::UnknownProfile(profile_name.to_string()))?;

    match profile_config.mode {
        OrchestratorMode::Local => Ok(Box::new(LocalChatOrchestrator::new())),
        OrchestratorMode::Remote => {
            let url = profile_config.url.as_ref().ok_or_else(|| {
                ChatOrchestratorError::Config("Missing URL for remote profile".into())
            })?;
            let key = profile_config.api_key.as_ref().ok_or_else(|| {
                ChatOrchestratorError::Config("Missing API key for remote profile".into())
            })?;
            Ok(Box::new(RemoteChatOrchestrator::new(
                url.clone(),
                key.clone(),
            )))
        }
    }
}

/// Create a local chat orchestrator directly (bypasses profile resolution)
pub fn create_local_chat_orchestrator() -> LocalChatOrchestrator {
    LocalChatOrchestrator::new()
}
