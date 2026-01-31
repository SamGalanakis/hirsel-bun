//! Shared ACP Agent Runner
//!
//! Provides a unified interface for running ACP agents. This reduces
//! duplication across scribe, conflict_resolver, compaction, eval, etc.
//!
//! ## Usage
//!
//! ```ignore
//! let runner = AcpAgentRunner::new(agent_command, work_dir, "my-agent")
//!     .timeout_secs(300);
//!
//! let client = Arc::new(MyClient::new());
//! runner.run(client, "Do something").await?;
//! ```

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::time::timeout;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::debug;

use agent_client_protocol::{
    Agent, Client, ClientSideConnection, ContentBlock, Implementation, InitializeRequest,
    NewSessionRequest, PromptRequest, ProtocolVersion, TextContent,
};

use crate::core::acp::{AcpChild, AcpSpawnConfig};

/// Error type for ACP agent operations
#[derive(Error, Debug)]
pub enum AcpRunnerError {
    #[error("Failed to spawn agent: {0}")]
    SpawnFailed(String),
    #[error("Agent error: {0}")]
    AgentError(String),
    #[error("Timeout after {0} seconds")]
    Timeout(u64),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Default timeout for agent operations (5 minutes)
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Unified ACP agent runner
///
/// Handles all the boilerplate of spawning an ACP agent:
/// - Process spawning via AcpChild
/// - Connection setup with ClientSideConnection
/// - Initialize/NewSession/Prompt sequence
/// - Timeout handling
/// - Cleanup
pub struct AcpAgentRunner {
    agent_command: Vec<String>,
    work_dir: std::path::PathBuf,
    name: String,
    timeout_secs: u64,
}

impl AcpAgentRunner {
    /// Create a new agent runner
    pub fn new(
        agent_command: Vec<String>,
        work_dir: impl AsRef<Path>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            agent_command,
            work_dir: work_dir.as_ref().to_path_buf(),
            name: name.into(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Set the timeout in seconds
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Run the agent with a prompt
    ///
    /// This must be called from within a LocalSet context because
    /// ACP uses spawn_local internally.
    pub async fn run<C: Client + 'static>(
        &self,
        client: Arc<C>,
        prompt: impl Into<String>,
    ) -> Result<(), AcpRunnerError> {
        let prompt = prompt.into();

        if self.agent_command.is_empty() {
            return Err(AcpRunnerError::SpawnFailed(
                "Empty agent command".to_string(),
            ));
        }

        // Spawn agent process
        let spawn_config = AcpSpawnConfig::new(
            self.agent_command.clone(),
            self.work_dir.clone(),
            &self.name,
        );
        let mut acp_child = AcpChild::spawn(spawn_config)
            .map_err(|e| AcpRunnerError::SpawnFailed(format!("Failed to spawn agent: {}", e)))?;

        let stdin = acp_child
            .take_stdin()
            .ok_or_else(|| AcpRunnerError::SpawnFailed("Failed to get stdin".to_string()))?;
        let stdout = acp_child
            .take_stdout()
            .ok_or_else(|| AcpRunnerError::SpawnFailed("Failed to get stdout".to_string()))?;

        // Create ACP connection
        let (conn, io_task) =
            ClientSideConnection::new(client, stdin.compat_write(), stdout.compat(), |fut| {
                tokio::task::spawn_local(fut);
            });

        // Spawn IO task
        let io_handle = tokio::task::spawn_local(async move {
            if let Err(e) = io_task.await {
                debug!("ACP IO task ended: {:?}", e);
            }
        });

        // Run with timeout
        let client_name = format!("hirsel-{}", self.name);
        let session_dir = self.work_dir.to_string_lossy().to_string();

        let result = timeout(Duration::from_secs(self.timeout_secs), async {
            // Initialize
            let init_request = InitializeRequest::new(ProtocolVersion::LATEST)
                .client_info(Implementation::new(&client_name, env!("CARGO_PKG_VERSION")));
            conn.initialize(init_request).await?;

            // Create session
            let session_request = NewSessionRequest::new(session_dir);
            let session = conn.new_session(session_request).await?;
            let session_id = session.session_id;

            debug!("ACP session created for {}: {}", self.name, session_id);

            // Send prompt
            let prompt_request = PromptRequest::new(
                session_id,
                vec![ContentBlock::Text(TextContent::new(prompt))],
            );
            conn.prompt(prompt_request).await?;

            Ok::<(), agent_client_protocol::Error>(())
        })
        .await;

        // Clean up
        drop(conn);
        let _ = io_handle.await;
        let _ = acp_child.kill().await;

        // Check result
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(AcpRunnerError::AgentError(format!("ACP error: {}", e))),
            Err(_) => Err(AcpRunnerError::Timeout(self.timeout_secs)),
        }
    }

    /// Run the agent in a LocalSet context
    ///
    /// This is a convenience method that wraps the run() call in the
    /// required spawn_blocking + LocalSet pattern for use from non-local contexts.
    pub async fn run_in_local_set<C: Client + Send + Sync + 'static>(
        self,
        client: Arc<C>,
        prompt: impl Into<String> + Send + 'static,
    ) -> Result<(), AcpRunnerError> {
        let prompt = prompt.into();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| {
                    AcpRunnerError::AgentError(format!("Failed to create runtime: {}", e))
                })?;

            rt.block_on(async {
                tokio::task::LocalSet::new()
                    .run_until(self.run(client, prompt))
                    .await
            })
        })
        .await
        .map_err(|e| AcpRunnerError::AgentError(format!("Join error: {}", e)))?
    }
}
