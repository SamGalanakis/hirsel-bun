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
use tracing::{debug, info, warn};

use super::state::{DeltaState, DeltaStateError};
use super::types::{
    DeltaStatus, DeltaSubmission, DeltaType, LiveNodeStatus, NodeType, ProjectRunStatus,
};
use crate::core::config::global_db_path;
use crate::core::orchestrator::{
    AddDeltaTaskRequest, Orchestrator, OrchestratorError, StartRunRequest,
};
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

        // Build all task requests first (for batch insertion)
        let mut requests = Vec::new();
        let mut submission_map = Vec::new(); // Track submission -> request index

        for submission in &pending {
            // Mark as processing
            self.state
                .update_submission_status(submission.id, DeltaStatus::Processing)?;

            // Generate spec content for the task
            let spec_content = self.generate_task_spec(submission)?;

            info!(
                "Processing delta submission: {} ({:?}) - {}",
                submission.id, submission.delta_type, submission.name
            );
            debug!("Task spec:\n{}", spec_content);

            // Add task to the run via orchestrator
            let task_id = submission
                .live_node_id
                .clone()
                .unwrap_or_else(|| format!("delta-{}", submission.id));

            // Determine task type based on delta node type
            let task_type = self.get_task_type_for_submission(submission)?;

            // Build blocked_by list from the live node
            let mut blocked_by = self.get_blocked_by_for_submission(submission)?;

            // On first dispatch, root work tasks should be blocked by scope
            // This prevents autoscale from spawning workers before leader explores the spec
            // (Eval tasks are already implicitly blocked by their validates targets)
            if is_first_dispatch
                && task_type == "work"
                && blocked_by.as_ref().map(|v| v.is_empty()).unwrap_or(true)
            {
                blocked_by = Some(vec!["scope".to_string()]);
            }

            // Build validates list for eval tasks
            let validates = self.get_validates_for_submission(submission)?;

            let request = AddDeltaTaskRequest {
                task_id: task_id.clone(),
                name: submission.name.clone(),
                content: spec_content,
                parent_id: None,
                blocked_by,
                task_type,
                validates,
                board_task_id: submission.live_node_id.clone(),
                delta_type: Some(submission.delta_type.as_str().to_string()),
                refs: if submission.refs.is_empty() {
                    None
                } else {
                    serde_json::to_string(&submission.refs).ok()
                },
            };

            submission_map.push((submission.id, submission.live_node_id.clone(), task_id));
            requests.push(request);
        }

        // Add all tasks in a single batch with deferred FK constraints
        match orchestrator
            .add_delta_tasks_batch(&run_name, requests)
            .await
        {
            Ok(tasks) => {
                info!(
                    "Added {} tasks to run {} via batch insert",
                    tasks.len(),
                    run_name
                );

                // Update live node statuses to pending
                for (_sub_id, live_node_id, task_id) in &submission_map {
                    if let Some(ref node_id) = live_node_id {
                        if let Err(e) = self.state.update_live_node_status(
                            node_id,
                            LiveNodeStatus::Pending,
                            None,
                        ) {
                            warn!("Failed to update live node {} status: {}", node_id, e);
                        }
                    }
                    info!("Added task {} to run {}", task_id, run_name);
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
                        // Don't fail the whole operation - tasks are in place
                    }
                }

                Ok(submission_map.len())
            }
            Err(e) => {
                // Log at error level AND print to stderr for debugging
                tracing::error!("Failed to add tasks batch to run '{}': {}", run_name, e);
                eprintln!(
                    "[DELTA ERROR] Failed to add tasks batch to run '{}': {}",
                    run_name, e
                );

                // Mark all submissions as failed
                for (sub_id, live_node_id, _task_id) in &submission_map {
                    self.state
                        .update_submission_status(*sub_id, DeltaStatus::Failed)?;

                    if let Some(ref node_id) = live_node_id {
                        let _ = self.state.update_live_node_status(
                            node_id,
                            LiveNodeStatus::Failed,
                            None,
                        );
                    }
                }

                Err(RunnerError::Orchestrator(e.to_string()))
            }
        }
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

    /// Get the task type string for a submission
    fn get_task_type_for_submission(&self, submission: &DeltaSubmission) -> RunnerResult<String> {
        // Check the live node to determine if it's an eval
        if let Some(ref node_id) = submission.live_node_id {
            if let Ok(node) = self.state.get_live_node(node_id) {
                if node.node_type == NodeType::Eval {
                    return Ok("eval".to_string());
                }
            }
        }
        Ok("work".to_string())
    }

    /// Get blocked_by list for submissions
    ///
    /// - Evals: blocked by tasks they validate (implicit from `validates`)
    /// - Tasks: blocked by explicit `blocked_by` list (task-to-task dependencies)
    fn get_blocked_by_for_submission(
        &self,
        submission: &DeltaSubmission,
    ) -> RunnerResult<Option<Vec<String>>> {
        if let Some(ref node_id) = submission.live_node_id {
            if let Ok(node) = self.state.get_live_node(node_id) {
                let blocked_by = if node.node_type == NodeType::Eval {
                    // Evals are implicitly blocked by tasks they validate
                    node.validates.clone()
                } else {
                    // Tasks use explicit blocked_by dependencies
                    node.blocked_by.clone()
                };

                if !blocked_by.is_empty() {
                    return Ok(Some(blocked_by));
                }
            }
        }
        Ok(None)
    }

    /// Get validates list for eval submissions
    fn get_validates_for_submission(
        &self,
        submission: &DeltaSubmission,
    ) -> RunnerResult<Option<Vec<String>>> {
        // For eval nodes, return the list of tasks they validate
        if let Some(ref node_id) = submission.live_node_id {
            if let Ok(node) = self.state.get_live_node(node_id) {
                if node.node_type == NodeType::Eval && !node.validates.is_empty() {
                    return Ok(Some(node.validates.clone()));
                }
            }
        }
        Ok(None)
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
