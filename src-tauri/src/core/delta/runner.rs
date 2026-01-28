//! Delta runner for background project run processing
//!
//! Processes pending delta submissions by converting them to run tasks
//! and dispatching to workers via the orchestrator.
//!
//! ## Flow
//!
//! 1. Daemon polls working project_runs
//! 2. DeltaRunner processes pending delta_submissions
//! 3. For each submission:
//!    - Generate spec content from draft node
//!    - Add task to the underlying run
//!    - Mark live node as working
//! 4. When all submissions complete, pause the project run

use rusqlite::Connection;
use tracing::{debug, info, warn};

use super::state::{DeltaState, DeltaStateError};
use super::types::{DeltaStatus, DeltaSubmission, DeltaType, LiveNodeStatus, ProjectRunStatus};
use crate::core::config::global_db_path;

/// Error type for runner operations
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("State error: {0}")]
    State(#[from] DeltaStateError),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("No project run found")]
    NoProjectRun,
    #[error("Run not in working state")]
    RunNotWorking,
}

pub type RunnerResult<T> = Result<T, RunnerError>;

/// Delta runner for processing submissions
pub struct DeltaRunner {
    project_id: i64,
    state: DeltaState,
}

impl DeltaRunner {
    /// Create a new runner for a project
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            state: DeltaState::new(project_id),
        }
    }

    /// Process pending delta submissions
    ///
    /// This is called by the daemon during its polling loop.
    /// Returns the number of submissions processed.
    pub fn process_pending(&self) -> RunnerResult<usize> {
        // Get project run
        let run = self
            .state
            .get_project_run()?
            .ok_or(RunnerError::NoProjectRun)?;

        if run.status != ProjectRunStatus::Working {
            return Err(RunnerError::RunNotWorking);
        }

        // Get pending submissions from all batches
        let pending = self.get_all_pending_submissions()?;

        if pending.is_empty() {
            // No pending work - check if we should pause the run
            self.check_completion()?;
            return Ok(0);
        }

        let mut processed = 0;
        for submission in pending {
            // Mark as processing
            self.state
                .update_submission_status(submission.id, DeltaStatus::Processing)?;

            // Mark the corresponding live node as working
            if let Some(ref node_id) = submission.live_node_id {
                if let Err(e) =
                    self.state
                        .update_live_node_status(node_id, LiveNodeStatus::Working, None)
                {
                    warn!("Failed to update live node {} status: {}", node_id, e);
                }
            }

            // Generate spec content for the task
            let spec_content = self.generate_task_spec(&submission)?;

            // Log what we're processing (actual task dispatch will be handled
            // by connecting to the run system)
            info!(
                "Processing delta submission: {} ({:?}) - {}",
                submission.id, submission.delta_type, submission.name
            );
            debug!("Task spec:\n{}", spec_content);

            // For now, mark as done immediately
            // The full integration will:
            // 1. Add task to run's task queue
            // 2. Let workers pick it up
            // 3. Mark done/failed based on worker outcome
            self.state
                .update_submission_status(submission.id, DeltaStatus::Done)?;

            // Update live node status
            if let Some(ref node_id) = submission.live_node_id {
                let status = LiveNodeStatus::Done;
                if let Err(e) = self.state.update_live_node_status(node_id, status, None) {
                    warn!(
                        "Failed to update live node {} to {:?}: {}",
                        node_id, status, e
                    );
                }
            }

            processed += 1;
        }

        // Check if all done after processing
        self.check_completion()?;

        Ok(processed)
    }

    /// Get all pending submissions for this project
    fn get_all_pending_submissions(&self) -> RunnerResult<Vec<DeltaSubmission>> {
        let db = self.open_db()?;
        let mut stmt = db.prepare(
            "SELECT id, project_id, batch_id, delta_type, draft_node_id, live_node_id,
                    name, description, priority, status, refs, created_at, processed_at
             FROM delta_submissions
             WHERE project_id = ?1 AND status = 'pending'
             ORDER BY priority DESC, batch_id, id
             LIMIT 10", // Process in batches
        )?;

        let submissions = stmt
            .query_map([self.project_id], |row| {
                let refs_json: String = row.get("refs")?;
                let refs = serde_json::from_str(&refs_json).unwrap_or_default();
                let delta_type_str: String = row.get("delta_type")?;

                Ok(DeltaSubmission {
                    id: row.get("id")?,
                    project_id: row.get("project_id")?,
                    batch_id: row.get("batch_id")?,
                    delta_type: DeltaType::from_str(&delta_type_str)
                        .unwrap_or(DeltaType::Implement),
                    draft_node_id: row.get("draft_node_id")?,
                    live_node_id: row.get("live_node_id")?,
                    name: row.get("name")?,
                    description: row.get("description")?,
                    priority: row.get("priority")?,
                    status: DeltaStatus::Pending,
                    refs,
                    created_at: row.get("created_at")?,
                    processed_at: row.get("processed_at")?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(submissions)
    }

    /// Generate spec content for a delta task
    fn generate_task_spec(&self, submission: &DeltaSubmission) -> RunnerResult<String> {
        let mut spec = String::new();

        // Header based on delta type
        let action = match submission.delta_type {
            DeltaType::Implement => "Implement",
            DeltaType::Modify => "Modify",
            DeltaType::Revert => "Revert",
        };

        spec.push_str(&format!("# {} {}\n\n", action, submission.name));

        // Description
        spec.push_str(&submission.description);
        spec.push_str("\n\n");

        // Include context from draft node if available
        if let Some(ref draft_id) = submission.draft_node_id {
            if let Ok(node) = self.state.get_draft_node(draft_id) {
                if !node.content.is_empty() {
                    spec.push_str("## Context\n\n");
                    spec.push_str(&node.content);
                    spec.push_str("\n\n");
                }
            }
        }

        // Include references
        if !submission.refs.is_empty() {
            spec.push_str("## References\n\n");
            for reference in &submission.refs {
                spec.push_str(&format!(
                    "- **{}**: `{}`{}\n",
                    reference.ref_type,
                    reference.value,
                    reference
                        .description
                        .as_ref()
                        .map(|d| format!(" - {}", d))
                        .unwrap_or_default()
                ));
            }
            spec.push('\n');
        }

        Ok(spec)
    }

    /// Check if all work is complete and pause the run if so
    fn check_completion(&self) -> RunnerResult<()> {
        // Check for any pending or processing submissions
        let db = self.open_db()?;
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM delta_submissions
             WHERE project_id = ?1 AND status IN ('pending', 'processing')",
            [self.project_id],
            |row| row.get(0),
        )?;

        if count == 0 {
            // All done - pause the run
            info!(
                "All delta submissions complete for project {}, pausing run",
                self.project_id
            );
            self.state
                .update_project_run_status(ProjectRunStatus::Paused)?;
        }

        Ok(())
    }

    /// Wake up a paused run when new dispatch happens
    ///
    /// This is called after dispatch() creates new submissions
    pub fn wake_up(&self) -> RunnerResult<()> {
        let run = self
            .state
            .get_project_run()?
            .ok_or(RunnerError::NoProjectRun)?;

        if run.status == ProjectRunStatus::Paused {
            info!("Waking up paused run for project {}", self.project_id);
            self.state
                .update_project_run_status(ProjectRunStatus::Working)?;
        }

        Ok(())
    }

    /// Complete a submission (called when underlying task finishes)
    pub fn complete_submission(
        &self,
        submission_id: i64,
        success: bool,
        commit_sha: Option<&str>,
    ) -> RunnerResult<()> {
        let status = if success {
            DeltaStatus::Done
        } else {
            DeltaStatus::Failed
        };

        self.state.update_submission_status(submission_id, status)?;

        // Get submission to update live node
        let db = self.open_db()?;
        let live_node_id: Option<String> = db
            .query_row(
                "SELECT live_node_id FROM delta_submissions WHERE id = ?1",
                [submission_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        if let Some(node_id) = live_node_id {
            let node_status = if success {
                LiveNodeStatus::Done
            } else {
                LiveNodeStatus::Failed
            };
            self.state
                .update_live_node_status(&node_id, node_status, commit_sha)?;
        }

        // Check completion
        self.check_completion()?;

        Ok(())
    }

    /// Open database connection
    fn open_db(&self) -> RunnerResult<Connection> {
        let db = Connection::open(global_db_path())?;
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        Ok(db)
    }
}

/// Get all working project runs (used by daemon)
pub fn list_working_project_runs() -> RunnerResult<Vec<(i64, String)>> {
    let db = Connection::open(global_db_path())?;
    db.busy_timeout(std::time::Duration::from_secs(30))?;
    db.pragma_update(None, "journal_mode", "WAL")?;

    let mut stmt =
        db.prepare("SELECT project_id, run_name FROM project_runs WHERE status = 'working'")?;

    let runs = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(runs)
}
