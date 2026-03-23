//! Scribe batch processing for route and project artifacts.
//!
//! Workers submit durable learnings into run state. The daemon periodically
//! condenses pending submissions into the project's retained context.

use thiserror::Error;
use tracing::{info, warn};

use crate::core::config::Config;
use crate::core::project::ProjectStore;
use crate::core::state::{SQLiteState, StateError};

/// Error type for scribe operations.
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
    #[error("Database error: {0}")]
    Database(String),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    #[error("No pending submissions")]
    NoPending,
    #[error("Batch already processing")]
    BatchProcessing,
}

/// Result of processing a scribe batch.
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
pub async fn should_process_batch(state: &SQLiteState, config: &Config) -> bool {
    if !config.scribe_enabled {
        return false;
    }

    if let Ok(Some(_)) = state.get_processing_scribe_batch().await {
        return false;
    }

    if let Ok(Some(started_at)) = state.get_scribe_batch_started_at().await {
        if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(&started_at) {
            let elapsed = chrono::Utc::now().signed_duration_since(start_time);
            let window = chrono::Duration::seconds(config.scribe_batch_window_seconds as i64);
            return elapsed >= window;
        }
    }

    false
}

/// Process pending scribe submissions for a run.
pub async fn process_scribe_batch(runtime_name: &str) -> Result<ScribeBatchResult, ScribeError> {
    let (project_id, batch_id, submissions) = {
        let state = SQLiteState::new(runtime_name)
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
        let project_id = state
            .get_project_id()
            .await
            .map_err(ScribeError::State)?
            .ok_or_else(|| {
                ScribeError::InvalidPath(format!(
                    "Run '{}' is not linked to a project",
                    runtime_name
                ))
            })?;
        let submissions = state
            .get_pending_scribe_submissions()
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
        if submissions.is_empty() {
            return Err(ScribeError::NoPending);
        }

        let batch_id = state
            .next_scribe_batch_id()
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
        let ids: Vec<i64> = submissions.iter().map(|s| s.id).collect();
        state
            .mark_scribe_processing(&ids, batch_id)
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;

        info!(
            "Processing scribe batch {}: {} submissions",
            batch_id,
            submissions.len()
        );

        (project_id, batch_id, submissions)
    };

    let write_result = async {
        let store = ProjectStore::open()
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
        let existing = store
            .get_project_retained_context(project_id)
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
        let mut markdown = existing.markdown.trim_end().to_string();
        if !markdown.is_empty() {
            markdown.push_str("\n\n");
        }
        markdown.push_str(&format!(
            "## Batch {} - {}\n\n",
            batch_id,
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        ));
        for submission in &submissions {
            markdown.push_str(&format!(
                "- **{}**: {}\n",
                submission.worker_name,
                submission.content.trim()
            ));
        }
        store
            .update_project_retained_context(project_id, &markdown, Some("scribe"))
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
        Ok::<(), ScribeError>(())
    }
    .await;

    let success = write_result.is_ok();
    {
        let state = SQLiteState::new(runtime_name)
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
        state
            .complete_scribe_batch(batch_id, success)
            .await
            .map_err(|e| ScribeError::Database(e.to_string()))?;
    }

    if let Err(e) = write_result {
        warn!("Scribe batch {} failed: {}", batch_id, e);
        return Err(e);
    }

    info!("Scribe batch {} completed successfully", batch_id);

    Ok(ScribeBatchResult {
        batch_id,
        submissions_processed: submissions.len(),
        success: true,
    })
}
