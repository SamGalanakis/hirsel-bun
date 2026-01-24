//! Chat compaction module for hirsel.
//!
//! This module handles compacting chat threads when they get too long.
//! It summarizes older messages into a single compaction message and
//! removes the old messages.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::core::config::Config;
use crate::core::files::Files;
use crate::core::state::{Message, SQLiteState, StateError, StateResult};

/// Error type for compaction operations
#[derive(Error, Debug)]
pub enum CompactionError {
    #[error("Agent failed: {0}")]
    AgentError(String),
    #[error("Timeout generating summary after {0} seconds")]
    Timeout(u64),
    #[error("State error: {0}")]
    State(#[from] StateError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Compaction not needed")]
    NotNeeded,
}

/// Prompt template for generating compaction summaries
const COMPACTION_PROMPT: &str = r#"You are summarizing a chat thread for an AI coding agent system.

IMPORTANT: This is a text-only task. Do NOT use any tools - no file reads, no terminal commands, no searches. Simply read the messages below and output your summary directly as text.

Below are messages from a shared learnings chat where workers record discoveries, patterns, and gotchas about a codebase.

Summarize ALL the learnings into a concise bullet-point list:
- One bullet per distinct learning
- Be concise (one sentence max per bullet)
- Remove duplicates (keep the most complete version)
- Preserve specific details (file paths, env vars, patterns)
- Group related items if it improves clarity
- Order by importance/usefulness

Output ONLY the bullet points, no preamble or explanation. Do not use any tools.

Messages to summarize:
"#;

/// Check if a thread should be compacted based on message count and size
pub fn should_compact(messages: &[Message], threshold: u32, keep_count: u32) -> bool {
    let keep_count = keep_count as usize;

    if messages.len() <= keep_count {
        return false;
    }

    // Only count characters in messages that would be compacted
    let messages_to_compact = if keep_count > 0 && messages.len() > keep_count {
        &messages[..messages.len() - keep_count]
    } else {
        messages
    };

    let total_chars: usize = messages_to_compact.iter().map(|m| m.content.len()).sum();

    total_chars >= threshold as usize
}

/// Format messages for inclusion in the compaction prompt
pub fn format_messages_for_summary(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|msg| format!("[{}]: {}", msg.sender, msg.content))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate the full compaction prompt from messages
pub fn get_compaction_prompt(messages: &[Message]) -> String {
    format!(
        "{}{}",
        COMPACTION_PROMPT,
        format_messages_for_summary(messages)
    )
}

/// Perform compaction on a thread (synchronous version)
///
/// This function checks if compaction is needed, splits messages,
/// and performs the database operations. It does NOT generate the
/// summary - that must be done externally and passed in.
pub fn compact_thread_with_summary(
    state: &SQLiteState,
    files: &Files,
    thread: &str,
    summary: &str,
    threshold: Option<u32>,
    keep_count: Option<u32>,
) -> StateResult<bool> {
    // Load actual config from file, fall back to defaults if load fails
    let config = Config::load().map(|(c, _)| c).unwrap_or_default();
    let threshold = threshold.unwrap_or(config.compaction_threshold.unwrap_or(10000));
    let keep_count = keep_count.unwrap_or(config.compaction_keep_messages);

    // Get all messages in thread (use large limit)
    let messages = state.get_messages(thread, 100000)?;

    if !should_compact(&messages, threshold, keep_count) {
        return Ok(false);
    }

    info!(
        "Compacting thread '{}': {} messages, keeping {}",
        thread,
        messages.len(),
        keep_count
    );

    let keep_count = keep_count as usize;

    // Split into messages to compact (older) and keep (recent)
    let messages_to_compact = if keep_count > 0 && messages.len() > keep_count {
        &messages[..messages.len() - keep_count]
    } else {
        &messages[..]
    };

    // Build the compaction message
    let compaction_msg = format!(
        "[COMPACTED] Automated summary of {} previous messages.\n\n{}",
        messages_to_compact.len(),
        summary
    );

    // Get the IDs of messages to delete
    let ids_to_delete: Vec<i64> = messages_to_compact.iter().map(|m| m.id).collect();

    // Perform compaction in database
    state.compact_messages(thread, &ids_to_delete, &compaction_msg)?;

    // Rewrite the chat file to match
    let chat_path = files.chat_file(thread);
    if chat_path.exists() {
        // Get fresh messages from DB (now includes compaction message)
        let new_messages = state.get_messages(thread, 100000)?;
        if let Err(e) = rewrite_chat_file(&chat_path, &new_messages) {
            warn!("Failed to rewrite chat file after compaction: {}", e);
        }
    }

    info!(
        "Compacted thread '{}': {} messages → 1 summary",
        thread,
        messages_to_compact.len()
    );

    Ok(true)
}

/// Rewrite a chat file with new messages
fn rewrite_chat_file(path: &Path, messages: &[Message]) -> std::io::Result<()> {
    use std::fs::File;
    use std::io::Write;

    let mut file = File::create(path)?;

    for msg in messages {
        writeln!(file, "## {} ({})", msg.sender, msg.timestamp)?;
        writeln!(file)?;
        writeln!(file, "{}", msg.content)?;
        writeln!(file)?;
    }

    Ok(())
}

/// Default timeout for summary generation (60 seconds)
const SUMMARY_TIMEOUT_SECS: u64 = 60;

// ============================================================================
// Text-only ACP Client for Compaction
// ============================================================================

use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, Implementation, InitializeRequest, KillTerminalCommandRequest,
    KillTerminalCommandResponse, NewSessionRequest, PromptRequest, ProtocolVersion,
    ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionRequest, RequestPermissionResponse, SessionNotification, SessionUpdate,
    TerminalOutputRequest, TerminalOutputResponse, TextContent, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use tokio::sync::Mutex;

/// A minimal ACP client for text-only operations (no tools).
/// All tool requests are denied - we only want text output.
struct TextOnlyClient {
    collected_text: Mutex<String>,
}

impl TextOnlyClient {
    fn new() -> Self {
        Self {
            collected_text: Mutex::new(String::new()),
        }
    }

    async fn get_text(&self) -> String {
        self.collected_text.lock().await.clone()
    }
}

#[async_trait::async_trait(?Send)]
impl Client for TextOnlyClient {
    async fn request_permission(
        &self,
        _args: RequestPermissionRequest,
    ) -> std::result::Result<RequestPermissionResponse, agent_client_protocol::Error> {
        // Deny all permission requests - text only mode
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn session_notification(
        &self,
        args: SessionNotification,
    ) -> std::result::Result<(), agent_client_protocol::Error> {
        // Collect text output from agent
        if let SessionUpdate::AgentMessageChunk(chunk) = &args.update {
            if let ContentBlock::Text(text) = &chunk.content {
                let mut collected = self.collected_text.lock().await;
                collected.push_str(&text.text);
            }
        }
        Ok(())
    }

    async fn read_text_file(
        &self,
        _args: ReadTextFileRequest,
    ) -> std::result::Result<ReadTextFileResponse, agent_client_protocol::Error> {
        // Deny file reads - text only mode
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn write_text_file(
        &self,
        _args: WriteTextFileRequest,
    ) -> std::result::Result<WriteTextFileResponse, agent_client_protocol::Error> {
        // Deny file writes - text only mode
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn create_terminal(
        &self,
        _args: CreateTerminalRequest,
    ) -> std::result::Result<CreateTerminalResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn terminal_output(
        &self,
        _args: TerminalOutputRequest,
    ) -> std::result::Result<TerminalOutputResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn release_terminal(
        &self,
        _args: ReleaseTerminalRequest,
    ) -> std::result::Result<ReleaseTerminalResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: WaitForTerminalExitRequest,
    ) -> std::result::Result<WaitForTerminalExitResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn kill_terminal_command(
        &self,
        _args: KillTerminalCommandRequest,
    ) -> std::result::Result<KillTerminalCommandResponse, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::internal_error())
    }
}

/// Generate a compaction summary using an AI agent via ACP.
///
/// Spawns the configured agent and uses ACP protocol to send a prompt
/// and collect text output. No tools are enabled - text only mode.
pub async fn generate_compaction_summary(
    messages: &[Message],
    project_path: &Path,
    agent_command: &[String],
) -> Result<String, CompactionError> {
    use crate::core::acp::{AcpChild, AcpSpawnConfig};
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

    if messages.is_empty() {
        return Err(CompactionError::AgentError(
            "No messages to summarize".to_string(),
        ));
    }

    if agent_command.is_empty() {
        return Err(CompactionError::AgentError(
            "Empty agent command".to_string(),
        ));
    }

    info!(
        "Generating compaction summary for {} messages via ACP",
        messages.len()
    );

    // Build the prompt with messages to summarize
    let prompt = get_compaction_prompt(messages);

    // Create text-only client
    let client = Arc::new(TextOnlyClient::new());

    // Spawn agent process using AcpChild for automatic cleanup
    let spawn_config = AcpSpawnConfig::new(
        agent_command.to_vec(),
        project_path.to_path_buf(),
        "compaction",
    );
    let mut acp_child = AcpChild::spawn(spawn_config)
        .map_err(|e| CompactionError::AgentError(format!("Failed to spawn agent: {}", e)))?;
    let stdin = acp_child
        .take_stdin()
        .ok_or_else(|| CompactionError::AgentError("Failed to get stdin".to_string()))?;
    let stdout = acp_child
        .take_stdout()
        .ok_or_else(|| CompactionError::AgentError("Failed to get stdout".to_string()))?;

    // Convert to futures-compatible streams
    let stdin_compat = stdin.compat_write();
    let stdout_compat = stdout.compat();

    // Create ACP connection
    let (conn, io_task) =
        ClientSideConnection::new(client.clone(), stdin_compat, stdout_compat, |fut| {
            tokio::task::spawn_local(fut);
        });

    // Spawn IO task
    let io_handle = tokio::task::spawn_local(async move {
        if let Err(e) = io_task.await {
            debug!("ACP IO task ended: {:?}", e);
        }
    });

    // Run with timeout
    let result = timeout(Duration::from_secs(SUMMARY_TIMEOUT_SECS), async {
        // Initialize
        let init_request = InitializeRequest::new(ProtocolVersion::LATEST).client_info(
            Implementation::new("hirsel-compaction", env!("CARGO_PKG_VERSION")),
        );
        conn.initialize(init_request).await?;

        // Create session with NO MCP servers (text-only)
        let session_request = NewSessionRequest::new(project_path.to_string_lossy().to_string());
        let session = conn.new_session(session_request).await?;
        let session_id = session.session_id;

        debug!("ACP session created for compaction: {}", session_id);

        // Send prompt
        let prompt_request = PromptRequest::new(
            session_id,
            vec![ContentBlock::Text(TextContent::new(prompt))],
        );
        conn.prompt(prompt_request).await?;

        Ok::<(), agent_client_protocol::Error>(())
    })
    .await;

    // Clean up - AcpChild handles process group cleanup on drop
    drop(conn);
    let _ = io_handle.await;
    let _ = acp_child.kill().await;

    // Check result
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            return Err(CompactionError::AgentError(format!("ACP error: {}", e)));
        }
        Err(_) => {
            return Err(CompactionError::Timeout(SUMMARY_TIMEOUT_SECS));
        }
    }

    // Get collected text
    let summary = client.get_text().await.trim().to_string();

    if summary.is_empty() {
        return Err(CompactionError::AgentError(
            "Agent returned empty summary".to_string(),
        ));
    }

    info!(
        "Generated compaction summary ({} chars) for {} messages",
        summary.len(),
        messages.len()
    );

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_compact() {
        let messages: Vec<Message> = (0..50)
            .map(|i| Message {
                id: i,
                thread: "test".to_string(),
                sender: "worker".to_string(),
                content: "x".repeat(100),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                waiting: false,
            })
            .collect();

        // 50 messages * 100 chars = 5000 chars
        // With threshold 10000 and keep_count 10, we have 40 messages = 4000 chars to compact
        assert!(!should_compact(&messages, 10000, 10));

        // With threshold 3000, should compact
        assert!(should_compact(&messages, 3000, 10));

        // With too few messages, should not compact
        assert!(!should_compact(&messages[..5], 100, 10));
    }

    #[test]
    fn test_format_messages_for_summary() {
        let messages = vec![
            Message {
                id: 1,
                thread: "test".to_string(),
                sender: "alice".to_string(),
                content: "Hello".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                waiting: false,
            },
            Message {
                id: 2,
                thread: "test".to_string(),
                sender: "bob".to_string(),
                content: "World".to_string(),
                timestamp: "2024-01-01T00:00:01Z".to_string(),
                waiting: false,
            },
        ];

        let formatted = format_messages_for_summary(&messages);
        assert!(formatted.contains("[alice]: Hello"));
        assert!(formatted.contains("[bob]: World"));
    }
}
