//! Conflict resolver service.
//!
//! For now this module validates/records conflict status but does not perform
//! automatic AI conflict resolution.

mod state;

use std::path::Path;

use thiserror::Error;
use tracing::{info, warn};

pub use state::{ConflictResolution, ConflictResolutionStatus, ConflictResolverState};

/// Error type for conflict resolution operations.
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

pub type ConflictResolverResult<T> = Result<T, ConflictResolverError>;

/// Result of conflict resolution.
#[derive(Debug)]
pub struct ResolutionResult {
    pub files_resolved: usize,
    pub success: bool,
}

/// Service for resolving merge conflicts.
pub struct ConflictResolverService;

impl ConflictResolverService {
    /// Create a new conflict resolver service.
    pub fn new() -> Self {
        Self
    }

    /// Resolve conflicts in the given work directory.
    ///
    /// Current behavior checks whether markers still exist and returns
    /// `MarkersRemain` if unresolved files are detected.
    pub async fn resolve_conflicts(
        &self,
        work_dir: &Path,
        conflicts: Vec<String>,
        _context: &str,
    ) -> ConflictResolverResult<ResolutionResult> {
        if conflicts.is_empty() {
            return Err(ConflictResolverError::NoConflicts);
        }

        info!(
            "Validating {} conflict files in {}",
            conflicts.len(),
            work_dir.display()
        );

        let mut unresolved = 0usize;
        for rel_path in &conflicts {
            let path = work_dir.join(rel_path);
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(
                        "Failed to read conflict candidate {}: {}",
                        path.display(),
                        e
                    );
                    unresolved += 1;
                    continue;
                }
            };

            if content.contains("<<<<<<<")
                || content.contains("=======")
                || content.contains(">>>>>>>")
            {
                unresolved += 1;
            }
        }

        if unresolved > 0 {
            return Err(ConflictResolverError::MarkersRemain);
        }

        Ok(ResolutionResult {
            files_resolved: conflicts.len(),
            success: true,
        })
    }
}
