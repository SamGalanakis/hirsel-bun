//! Delta dispatch service
//!
//! Orchestrates the delta dispatch flow:
//! 1. Compute diff between draft and live trees
//! 2. Generate delta tasks (implement, modify, revert)
//! 3. Get or create persistent run
//! 4. Add delta tasks to run
//! 5. Update live tree
//! 6. Resume run

use tracing::info;

use super::generator::{DeltaGenerator, GeneratorError};
use super::state::{DeltaState, DeltaStateError};
use super::types::*;
use crate::core::names::generate_run_name;

/// Error type for dispatch operations
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("State error: {0}")]
    State(#[from] DeltaStateError),
    #[error("Generator error: {0}")]
    Generator(#[from] GeneratorError),
    #[error("No changes to dispatch")]
    NoChanges,
    #[error("Run creation failed: {0}")]
    RunCreation(String),
}

pub type DeltaDispatchResult<T> = Result<T, DispatchError>;

/// Service for dispatching delta tasks to runs
pub struct DeltaDispatchService {
    #[allow(dead_code)]
    project_id: i64,
    state: DeltaState,
    generator: DeltaGenerator,
}

impl DeltaDispatchService {
    /// Create a new dispatch service for a project
    pub fn new(project_id: i64) -> Self {
        Self {
            project_id,
            state: DeltaState::new(project_id),
            generator: DeltaGenerator::new(project_id),
        }
    }

    /// Execute a full dispatch
    ///
    /// 1. Compute diff between draft and live
    /// 2. Generate delta tasks
    /// 3. Get or create persistent run
    /// 4. Create delta submissions in DB
    /// 5. Create board version
    /// 6. Update live tree to reflect new/modified nodes
    /// 7. Return dispatch result
    pub fn dispatch(&self) -> DeltaDispatchResult<DispatchResult> {
        // 1. Get the diff
        let diff = self.generator.get_diff()?;

        if diff.is_empty() {
            return Err(DispatchError::NoChanges);
        }

        info!(
            "Dispatching delta: {} new, {} modified, {} deleted",
            diff.new_nodes.len(),
            diff.modified_nodes.len(),
            diff.deleted_nodes.len()
        );

        // 2. Generate delta tasks
        let tasks = self.generator.generate_tasks()?;

        // 3. Get or create persistent run
        let run = self.get_or_create_run()?;

        // 4. Create delta submissions
        let batch_id = self.state.next_batch_id()?;
        let submissions = self.state.create_delta_submissions(&tasks, batch_id)?;

        info!(
            "Created {} delta submissions in batch {}",
            submissions.len(),
            batch_id
        );

        // 5. Create board version for this dispatch
        let description = Some(diff.summary());
        let version = self
            .state
            .create_board_version(batch_id, description.as_deref())?;

        info!(
            "Created board version v{} (batch {})",
            version.version_number, batch_id
        );

        // 6. Update live tree
        self.sync_live_tree(&diff)?;

        // 7. Record dispatch time
        self.state.record_dispatch()?;

        // 8. Update run status to working
        self.state
            .update_project_run_status(ProjectRunStatus::Working)?;

        Ok(DispatchResult {
            run_name: run.run_name,
            batch_id,
            delta_count: submissions.len(),
            diff_summary: diff.summary(),
            version_number: version.version_number,
            version_id: version.id,
        })
    }

    /// Get the existing run or create a new one
    fn get_or_create_run(&self) -> DeltaDispatchResult<ProjectRun> {
        if let Some(run) = self.state.get_project_run()? {
            info!("Using existing run: {}", run.run_name);
            return Ok(run);
        }

        // Create new run
        let run_name = generate_run_name();
        info!("Creating new persistent run: {}", run_name);

        let run = self.state.create_project_run(&run_name)?;
        Ok(run)
    }

    /// Sync live tree to reflect dispatch
    ///
    /// - Create live nodes for new draft nodes (excluding project nodes - UI-only)
    /// - Update live nodes for modified draft nodes
    /// - Mark deleted nodes in live (don't delete yet - revert task will handle)
    fn sync_live_tree(&self, diff: &TreeDiff) -> DeltaDispatchResult<()> {
        // Create live nodes for new draft nodes
        for new_node in &diff.new_nodes {
            // Get the full draft node
            if let Ok(draft) = self.state.get_draft_node(&new_node.id) {
                self.state.create_live_node_from_draft(&draft)?;
                info!("Created live node: {}", new_node.id);
            }
        }

        // Update live nodes for modified draft nodes
        for modified in &diff.modified_nodes {
            if let Ok(draft) = self.state.get_draft_node(&modified.draft_node.id) {
                // Reset status to pending since content changed
                self.state.update_live_node_from_draft(&draft)?;
                self.state
                    .update_live_node_status(&draft.id, LiveNodeStatus::Pending, None)?;
                info!("Updated live node: {}", draft.id);
            }
        }

        // For deleted nodes, we don't remove them from live yet
        // The revert task will handle cleanup when it completes

        Ok(())
    }

    /// Preview what would be dispatched without actually dispatching
    pub fn preview(&self) -> DeltaDispatchResult<DispatchPreview> {
        let diff = self.generator.get_diff()?;
        let tasks = if diff.is_empty() {
            vec![]
        } else {
            self.generator.generate_tasks().unwrap_or_default()
        };

        Ok(DispatchPreview {
            diff,
            tasks,
            has_existing_run: self.state.get_project_run()?.is_some(),
        })
    }

    /// Get the current diff
    pub fn get_diff(&self) -> DeltaDispatchResult<TreeDiff> {
        Ok(self.generator.get_diff()?)
    }

    /// Get diff summary for display
    pub fn get_diff_summary(&self) -> DeltaDispatchResult<String> {
        Ok(self.generator.get_diff_summary()?)
    }

    /// Get the project run
    pub fn get_project_run(&self) -> DeltaDispatchResult<Option<ProjectRun>> {
        Ok(self.state.get_project_run()?)
    }

    /// Mark live node as complete (called when task finishes)
    pub fn complete_live_node(
        &self,
        node_id: &str,
        success: bool,
        commit_sha: Option<&str>,
    ) -> DeltaDispatchResult<()> {
        let status = if success {
            LiveNodeStatus::Done
        } else {
            LiveNodeStatus::Failed
        };

        self.state
            .update_live_node_status(node_id, status, commit_sha)?;
        info!("Marked live node {} as {:?}", node_id, status);

        // Check if all nodes are done to pause the run
        self.check_run_completion()?;

        Ok(())
    }

    /// Handle revert completion - remove the live node
    pub fn complete_revert(&self, node_id: &str) -> DeltaDispatchResult<()> {
        self.state.delete_live_node(node_id)?;
        info!("Deleted reverted live node: {}", node_id);

        self.check_run_completion()?;

        Ok(())
    }

    /// Check if all tasks are done and pause the run
    fn check_run_completion(&self) -> DeltaDispatchResult<()> {
        let live_nodes = self.state.get_live_nodes()?;

        let all_done = live_nodes
            .iter()
            .all(|n| n.status == LiveNodeStatus::Done || n.status == LiveNodeStatus::Failed);

        if all_done && !live_nodes.is_empty() {
            info!("All live nodes complete, pausing run");
            self.state
                .update_project_run_status(ProjectRunStatus::Paused)?;
        }

        Ok(())
    }

    /// Get draft tree
    pub fn get_draft_tree(&self) -> DeltaDispatchResult<Vec<DraftNodeTree>> {
        Ok(self.state.get_draft_tree()?)
    }

    /// Get live tree
    pub fn get_live_tree(&self) -> DeltaDispatchResult<Vec<LiveNodeTree>> {
        Ok(self.state.get_live_tree()?)
    }

    /// Get underlying state
    pub fn state(&self) -> &DeltaState {
        &self.state
    }

    /// Get all board versions for this project
    pub fn get_board_versions(&self) -> DeltaDispatchResult<Vec<BoardVersion>> {
        Ok(self.state.get_board_versions()?)
    }

    /// Get the latest board version
    pub fn get_latest_version(&self) -> DeltaDispatchResult<Option<BoardVersion>> {
        Ok(self.state.get_latest_version()?)
    }

    /// Get a specific board version
    pub fn get_board_version(&self, id: i64) -> DeltaDispatchResult<BoardVersion> {
        Ok(self.state.get_board_version(id)?)
    }

    /// Create a delivery for the latest board version
    pub fn create_delivery(&self, target_branch: &str) -> DeltaDispatchResult<Delivery> {
        let version = self
            .state
            .get_latest_version()?
            .ok_or_else(|| DispatchError::NoChanges)?;

        Ok(self.state.create_delivery(version.id, target_branch)?)
    }

    /// Get current delivery for this project
    pub fn get_current_delivery(&self) -> DeltaDispatchResult<Option<Delivery>> {
        Ok(self.state.get_current_delivery()?)
    }
}

/// Preview of what would be dispatched
#[derive(Debug, Clone)]
pub struct DispatchPreview {
    pub diff: TreeDiff,
    pub tasks: Vec<DeltaTask>,
    pub has_existing_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_service_creation() {
        let service = DeltaDispatchService::new(1);
        assert_eq!(service.project_id, 1);
    }
}
