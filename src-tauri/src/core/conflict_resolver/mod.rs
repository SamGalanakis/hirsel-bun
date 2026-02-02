//! Conflict Resolver Service
//!
//! Spawns an ACP agent to resolve git merge conflicts during delivery.
//! Follows the same pattern as the Scribe service.
//!
//! ## How it works
//!
//! 1. Delivery service starts a merge that has conflicts
//! 2. ConflictResolverService is invoked with the list of conflicting files
//! 3. An ACP agent is spawned in the work directory
//! 4. Agent receives context about the run and conflicting files
//! 5. Agent resolves each conflict (removes markers, makes semantic choices)
//! 6. Service verifies no conflict markers remain
//! 7. Delivery service completes the merge

mod client;
mod state;

use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};

use crate::core::acp_runner::{AcpAgentRunner, AcpRunnerError};
use crate::core::constants::{RESOLUTION_TIMEOUT_SECS, RESOLVER_PROMPT};
pub use client::ConflictResolverClient;
pub use state::{ConflictResolution, ConflictResolutionStatus, ConflictResolverState};

/// Error type for conflict resolution operations
#[derive(Error, Debug)]
pub enum ConflictResolverError {
    #[error("Agent failed: {0}")]
    AgentError(String),
    #[error("Timeout resolving conflicts after {0} seconds")]
    Timeout(u64),
    #[error("State error: {0}")]
    State(#[from] state::StateError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No conflicts to resolve")]
    NoConflicts,
    #[error("Conflict markers remain after resolution")]
    MarkersRemain,
}

impl From<AcpRunnerError> for ConflictResolverError {
    fn from(err: AcpRunnerError) -> Self {
        match err {
            AcpRunnerError::Timeout(secs) => ConflictResolverError::Timeout(secs),
            AcpRunnerError::Io(e) => ConflictResolverError::Io(e),
            other => ConflictResolverError::AgentError(other.to_string()),
        }
    }
}

pub type ConflictResolverResult<T> = Result<T, ConflictResolverError>;

/// Result of conflict resolution
#[derive(Debug)]
pub struct ResolutionResult {
    pub files_resolved: usize,
    pub success: bool,
}

/// Service for resolving merge conflicts via AI agent
pub struct ConflictResolverService {
    agent_command: Vec<String>,
}

impl ConflictResolverService {
    /// Create a new conflict resolver service
    pub fn new(agent_command: Vec<String>) -> Self {
        Self { agent_command }
    }

    /// Resolve conflicts in the given work directory
    ///
    /// This is the main entry point. It spawns an ACP agent that will
    /// read the conflicting files, understand the conflicts, and write
    /// resolved versions.
    pub async fn resolve_conflicts(
        &self,
        work_dir: &Path,
        conflicts: Vec<String>,
        context: &str,
    ) -> ConflictResolverResult<ResolutionResult> {
        if conflicts.is_empty() {
            return Err(ConflictResolverError::NoConflicts);
        }

        if self.agent_command.is_empty() {
            return Err(ConflictResolverError::AgentError(
                "Empty agent command".to_string(),
            ));
        }

        info!(
            "Resolving {} conflicts in {}",
            conflicts.len(),
            work_dir.display()
        );

        // Build the prompt
        let prompt = self.build_prompt(&conflicts, context);

        // Run the agent
        let result = self.run_agent(work_dir, &prompt).await;

        match result {
            Ok(()) => {
                info!("Conflict resolution completed successfully");
                Ok(ResolutionResult {
                    files_resolved: conflicts.len(),
                    success: true,
                })
            }
            Err(e) => {
                warn!("Conflict resolution failed: {}", e);
                Err(e)
            }
        }
    }

    /// Build the prompt for the agent
    fn build_prompt(&self, conflicts: &[String], context: &str) -> String {
        let files_list = conflicts
            .iter()
            .map(|f| format!("- {}", f))
            .collect::<Vec<_>>()
            .join("\n");

        RESOLVER_PROMPT
            .replace("{files}", &files_list)
            .replace("{context}", context)
    }

    /// Run the ACP agent to resolve conflicts
    async fn run_agent(&self, work_dir: &Path, prompt: &str) -> ConflictResolverResult<()> {
        let client = Arc::new(ConflictResolverClient::new(work_dir.to_path_buf()));

        AcpAgentRunner::new(self.agent_command.clone(), work_dir, "conflict-resolver")
            .timeout_secs(RESOLUTION_TIMEOUT_SECS)
            .run(client, prompt.to_string())
            .await
            .map_err(ConflictResolverError::from)
    }
}
