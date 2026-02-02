//! Scribe system for maintaining project documentation.
//!
//! Workers call scribe() to record learnings, which are batched
//! and processed by an ephemeral Scribe agent that maintains docs
//! in the run directory.
//!
//! ## How it works
//!
//! 1. Workers call `scribe(content)` to submit learnings
//! 2. Submissions are stored in the database with status 'pending'
//! 3. A batch timer starts on the first submission (default: 3s)
//! 4. When the timer expires, the daemon spawns a Scribe agent
//! 5. The Scribe agent reads existing docs, integrates learnings, writes updates
//! 6. Workers can call `read_docs()` to get current documentation
//!
//! ## Security Note
//!
//! The Scribe agent currently has full capabilities (read, write, execute).
//! A future improvement would sandbox it to the docs/ directory only.

use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};

use agent_client_protocol::{
    Client, CreateTerminalRequest, CreateTerminalResponse, KillTerminalCommandRequest,
    KillTerminalCommandResponse, PermissionOptionKind, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification, TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};

use crate::core::acp_runner::{AcpAgentRunner, AcpRunnerError};
use crate::core::config::Config;
use crate::core::constants::{SCRIBE_PROMPT, SCRIBE_TIMEOUT_SECS};
use crate::core::files::Files;
use crate::core::state::{SQLiteState, ScribeSubmission, StateError};

/// Error type for scribe operations
#[derive(Error, Debug)]
pub enum ScribeError {
    #[error("Agent failed: {0}")]
    AgentError(String),
    #[error("Timeout processing batch after {0} seconds")]
    Timeout(u64),
    #[error("State error: {0}")]
    State(#[from] StateError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No pending submissions")]
    NoPending,
    #[error("Batch already processing")]
    BatchProcessing,
}

impl From<AcpRunnerError> for ScribeError {
    fn from(err: AcpRunnerError) -> Self {
        match err {
            AcpRunnerError::Timeout(secs) => ScribeError::Timeout(secs),
            AcpRunnerError::Io(e) => ScribeError::Io(e),
            other => ScribeError::AgentError(other.to_string()),
        }
    }
}

/// Result of processing a scribe batch
#[derive(Debug)]
pub struct ScribeBatchResult {
    pub batch_id: i64,
    pub submissions_processed: usize,
    pub success: bool,
}

/// Check if a scribe batch should be processed.
///
/// Returns true if:
/// - There are pending submissions
/// - The batch window has expired (batch_started_at + window < now)
/// - No batch is currently processing
pub fn should_process_batch(state: &SQLiteState, config: &Config) -> bool {
    if !config.scribe_enabled {
        return false;
    }

    // Check if a batch is already processing
    if let Ok(Some(_)) = state.get_processing_scribe_batch() {
        return false;
    }

    // Check if batch timer has expired
    if let Ok(Some(started_at)) = state.get_scribe_batch_started_at() {
        if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(&started_at) {
            let elapsed = chrono::Utc::now().signed_duration_since(start_time);
            let window = chrono::Duration::seconds(config.scribe_batch_window_seconds as i64);
            return elapsed >= window;
        }
    }

    false
}

/// Process pending scribe submissions.
///
/// This function:
/// 1. Gets all pending submissions (including failed with retry_count < 3)
/// 2. Marks them as processing with a batch ID
/// 3. Spawns a Scribe agent to integrate the learnings
/// 4. Marks submissions as done or failed based on result
///
/// Note: Takes ownership of state to avoid Send issues with async boundaries.
pub async fn process_scribe_batch(
    files: &Files,
    _config: &Config,
    agent_command: &[String],
) -> Result<ScribeBatchResult, ScribeError> {
    let db_path = files.db_path();
    let docs_dir = files.docs_dir();

    // Phase 1: Get submissions and mark as processing (sync)
    let (batch_id, submissions) = {
        let state = SQLiteState::new(db_path.clone())?;
        let submissions = state.get_pending_scribe_submissions()?;
        if submissions.is_empty() {
            return Err(ScribeError::NoPending);
        }

        let batch_id = state.next_scribe_batch_id()?;
        let ids: Vec<i64> = submissions.iter().map(|s| s.id).collect();
        state.mark_scribe_processing(&ids, batch_id)?;

        info!(
            "Processing scribe batch {}: {} submissions",
            batch_id,
            submissions.len()
        );

        (batch_id, submissions)
    };

    // Ensure docs directory exists
    files.init_docs()?;

    // Phase 2: Run the scribe agent (async)
    let result = run_scribe_agent(&submissions, docs_dir.as_path(), agent_command).await;

    // Phase 3: Update submission statuses based on result (sync)
    let success = result.is_ok();
    {
        let state = SQLiteState::new(db_path)?;
        state.complete_scribe_batch(batch_id, success)?;
    }

    if let Err(ref e) = result {
        warn!("Scribe batch {} failed: {}", batch_id, e);
    } else {
        info!("Scribe batch {} completed successfully", batch_id);
    }

    Ok(ScribeBatchResult {
        batch_id,
        submissions_processed: submissions.len(),
        success,
    })
}

/// Format submissions for the scribe prompt
fn format_submissions(submissions: &[ScribeSubmission]) -> String {
    submissions
        .iter()
        .map(|s| format!("[{}]: {}", s.worker_name, s.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Build the full scribe prompt
fn build_scribe_prompt(submissions: &[ScribeSubmission]) -> String {
    format!("{}{}", SCRIBE_PROMPT, format_submissions(submissions))
}

/// Run the Scribe agent to process submissions.
///
/// Spawns the agent with full capabilities, working directory set to docs/.
async fn run_scribe_agent(
    submissions: &[ScribeSubmission],
    docs_dir: &Path,
    agent_command: &[String],
) -> Result<(), ScribeError> {
    if submissions.is_empty() {
        return Err(ScribeError::NoPending);
    }

    // Ensure docs directory exists
    std::fs::create_dir_all(docs_dir)?;

    info!(
        "Running Scribe agent to process {} submissions",
        submissions.len()
    );

    let prompt = build_scribe_prompt(submissions);
    let client = Arc::new(ScribeClient::new());

    AcpAgentRunner::new(agent_command.to_vec(), docs_dir, "scribe")
        .timeout_secs(SCRIBE_TIMEOUT_SECS)
        .run(client, prompt)
        .await
        .map_err(ScribeError::from)
}

// ============================================================================
// Scribe ACP Client
// ============================================================================

/// ACP client for the Scribe agent.
///
/// Allows file read/write operations but denies terminal operations.
/// The Scribe agent only needs to read and write documentation files.
struct ScribeClient {}

impl ScribeClient {
    fn new() -> Self {
        Self {}
    }
}

/// Result type for ACP operations
type AcpResult<T> = std::result::Result<T, agent_client_protocol::Error>;

#[async_trait::async_trait(?Send)]
impl Client for ScribeClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> AcpResult<RequestPermissionResponse> {
        // Auto-approve all permission requests
        let option_id = args
            .options
            .iter()
            .find(|o| o.kind == PermissionOptionKind::AllowAlways)
            .or_else(|| {
                args.options
                    .iter()
                    .find(|o| o.kind == PermissionOptionKind::AllowOnce)
            })
            .map(|o| o.option_id.clone())
            .unwrap_or_else(|| "allow".into());

        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
        ))
    }

    async fn session_notification(&self, _args: SessionNotification) -> AcpResult<()> {
        // Accept all notifications silently
        Ok(())
    }

    async fn read_text_file(&self, args: ReadTextFileRequest) -> AcpResult<ReadTextFileResponse> {
        let path = std::path::Path::new(&args.path);
        match std::fs::read_to_string(path) {
            Ok(mut content) => {
                if args.line.is_some() || args.limit.is_some() {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = args.line.map(|l| l as usize).unwrap_or(0);
                    let end = args
                        .limit
                        .map(|l| start + l as usize)
                        .unwrap_or(lines.len());
                    content = lines[start.min(lines.len())..end.min(lines.len())].join("\n");
                }
                Ok(ReadTextFileResponse::new(content))
            }
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn write_text_file(
        &self,
        args: WriteTextFileRequest,
    ) -> AcpResult<WriteTextFileResponse> {
        let path = std::path::Path::new(&args.path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(path, &args.content) {
            Ok(()) => Ok(WriteTextFileResponse::new()),
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    async fn create_terminal(
        &self,
        _args: CreateTerminalRequest,
    ) -> AcpResult<CreateTerminalResponse> {
        // Deny terminal creation - scribe only needs file operations
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn terminal_output(
        &self,
        _args: TerminalOutputRequest,
    ) -> AcpResult<TerminalOutputResponse> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn release_terminal(
        &self,
        _args: ReleaseTerminalRequest,
    ) -> AcpResult<ReleaseTerminalResponse> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: WaitForTerminalExitRequest,
    ) -> AcpResult<WaitForTerminalExitResponse> {
        Err(agent_client_protocol::Error::internal_error())
    }

    async fn kill_terminal_command(
        &self,
        _args: KillTerminalCommandRequest,
    ) -> AcpResult<KillTerminalCommandResponse> {
        Err(agent_client_protocol::Error::internal_error())
    }
}
