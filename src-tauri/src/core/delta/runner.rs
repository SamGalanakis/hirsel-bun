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
//!    - Ensure run exists (create via orchestrator if first dispatch)
//!    - Add task to the underlying run via orchestrator
//!    - Mark live node as pending (workers will set working)
//! 4. When all submissions complete, pause the project run

use rusqlite::Connection;
use tracing::{info, warn};

use super::state::{DeltaState, DeltaStateError};
use super::types::{
    DeltaStatus, DeltaSubmission, DeltaType, LiveNodeStatus, NodeType, ProjectRunStatus,
};
use crate::core::config::global_db_path;
use crate::core::orchestrator::{Orchestrator, OrchestratorError, StartRunRequest};
use crate::core::project::ProjectStore;

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
    #[error("Project not found: {0}")]
    ProjectNotFound(i64),
    #[error("Orchestrator error: {0}")]
    Orchestrator(String),
}

impl From<OrchestratorError> for RunnerError {
    fn from(e: OrchestratorError) -> Self {
        RunnerError::Orchestrator(e.to_string())
    }
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
    ///
    /// ## First Dispatch
    ///
    /// On first dispatch (run doesn't exist), we:
    /// 1. Create run in draft mode (no workers spawned)
    /// 2. Create a "scope" task that blocks all root tasks
    /// 3. Add scope + all delta tasks in one batch
    /// 4. Spawn workers after batch succeeds
    ///
    /// ## Later Dispatches
    ///
    /// Just add delta tasks to the existing run.
    pub async fn process_pending(&self, orchestrator: &dyn Orchestrator) -> RunnerResult<usize> {
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

        // Ensure the run exists in ~/.hirsel/runs/
        // Returns (run_name, is_first_dispatch)
        let (run_name, is_first_dispatch) =
            self.ensure_run_exists(orchestrator, &run.run_name).await?;

        // Process each submission - live nodes already exist from dispatch,
        // we just need to mark submissions as done and update live node statuses
        let mut processed_count = 0;

        for submission in &pending {
            // Mark as processing
            self.state
                .update_submission_status(submission.id, DeltaStatus::Processing)?;

            info!(
                "Processing delta submission: {} ({:?}) - {}",
                submission.id, submission.delta_type, submission.name
            );

            // Update live node status to pending (workers will claim it)
            if let Some(ref node_id) = submission.live_node_id {
                if let Err(e) =
                    self.state
                        .update_live_node_status(node_id, LiveNodeStatus::Pending, None)
                {
                    warn!("Failed to update live node {} status: {}", node_id, e);
                }
            }

            // Mark submission as done
            self.state
                .update_submission_status(submission.id, DeltaStatus::Done)?;

            processed_count += 1;
        }

        // On first dispatch, spawn leader with scope task
        if is_first_dispatch {
            info!(
                "First dispatch complete, spawning leader with scope task for run '{}'",
                run_name
            );
            if let Err(e) = orchestrator
                .spawn_workers(&run_name, 1, Some("scope".to_string()))
                .await
            {
                tracing::error!("Failed to spawn workers for run '{}': {}", run_name, e);
                // Don't fail the whole operation - live nodes are in place
            }
        }

        Ok(processed_count)
    }

    /// Ensure the actual run directory exists in ~/.hirsel/runs/
    ///
    /// If the run doesn't exist yet, create it via orchestrator.start_run() in draft mode.
    /// This happens on first dispatch for a project.
    ///
    /// Returns (run_name, is_first_dispatch).
    async fn ensure_run_exists(
        &self,
        orchestrator: &dyn Orchestrator,
        run_name: &str,
    ) -> RunnerResult<(String, bool)> {
        // Check if run already exists
        match orchestrator.get_run(run_name).await {
            Ok(_detail) => {
                // Run exists, not first dispatch
                return Ok((run_name.to_string(), false));
            }
            Err(OrchestratorError::RunNotFound(_)) => {
                // Run doesn't exist, need to create it
                info!("Run '{}' not found, creating in draft mode...", run_name);
            }
            Err(e) => {
                // Some other error
                return Err(RunnerError::Orchestrator(format!(
                    "Failed to check run existence: {}",
                    e
                )));
            }
        }

        // Load project to get starting point
        let store = ProjectStore::open().map_err(|e| RunnerError::Orchestrator(e.to_string()))?;
        let project = store
            .get_project(self.project_id)
            .map_err(|_| RunnerError::ProjectNotFound(self.project_id))?;

        // Generate the spec from the root node content
        let spec = self.generate_run_spec()?;

        // Create run - orchestrator will create scope task and block other tasks by it
        let request = StartRunRequest {
            name: run_name.to_string(),
            project_id: self.project_id,
            spec,
            starting_point: Some(project.starting_point.clone()),
            eval: None, // Eval nodes are handled as tasks
            worker_scale: project.worker_scale.as_ref().and_then(|s| s.parse().ok()),
            time_limit_minutes: project.time_limit_minutes,
            human_in_the_loop: Some(project.human_in_the_loop),
            runner: None,
            worker_runners: None,
            tailscale_oauth: None,
        };

        let detail = orchestrator.start_run(request).await?;
        info!(
            "Created draft run '{}' for project {} (workers not spawned yet)",
            detail.name, self.project_id
        );

        Ok((detail.name, true))
    }

    /// Generate the run spec from the root node content
    fn generate_run_spec(&self) -> RunnerResult<String> {
        // Get the root node (project node)
        if let Ok(Some(root_id)) = self.state.get_root_node_id() {
            if let Ok(root) = self.state.get_draft_node(&root_id) {
                if !root.content.is_empty() {
                    return Ok(root.content);
                }
            }
        }

        // Fallback: generate from all task nodes
        let nodes = self.state.get_draft_nodes()?;
        let mut spec = String::new();
        spec.push_str("# Project Scope\n\n");
        spec.push_str("## Tasks\n\n");

        for node in nodes {
            if node.node_type == NodeType::Task {
                spec.push_str(&format!("- **{}**: {}\n", node.name, node.content));
            }
        }

        Ok(spec)
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

    /// Get the project ID
    pub fn project_id(&self) -> i64 {
        self.project_id
    }

    /// Get a reference to the state
    pub fn state(&self) -> &DeltaState {
        &self.state
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
