//! Board activation service used by Shepherd orchestration.
//!
//! Orchestrates activation flow:
//! 1. Find all nodes with status='draft'
//! 2. Set active draft nodes to pending
//! 3. Create/update project_run, board_version
//! 4. Set run status to working

use tracing::info;

use super::state::DeltaState;
use super::types::*;
use crate::core::names::generate_run_name;

/// Error type for dispatch operations
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("State error: {0}")]
    State(#[from] super::state::DeltaStateError),
    #[error("No draft nodes to dispatch")]
    NoChanges,
    #[error("Run creation failed: {0}")]
    RunCreation(String),
}

pub type DeltaDispatchResult<T> = Result<T, DispatchError>;

/// Service for activating board nodes into an active run.
pub struct DeltaDispatchService {
    state: DeltaState,
}

impl DeltaDispatchService {
    /// Create a new dispatch service for a project route
    pub fn new(project_id: i64, route_id: i64) -> Self {
        Self {
            state: DeltaState::with_route(project_id, route_id),
        }
    }

    /// Activate draft nodes for Shepherd orchestration.
    ///
    /// 1. Find all draft nodes
    /// 2. Set active nodes to pending
    /// 3. Get or create persistent run
    /// 4. Create board version
    /// 5. Set run status to working
    pub async fn dispatch(&self) -> DeltaDispatchResult<DispatchResult> {
        // 1. Find all draft nodes
        let draft_nodes = self.state.get_draft_nodes().await?;

        if draft_nodes.is_empty() {
            return Err(DispatchError::NoChanges);
        }

        info!("Dispatching {} draft nodes", draft_nodes.len());

        let mut feature_count = 0;
        // 2. Activate draft nodes
        for node in &draft_nodes {
            // Set node status to pending
            self.state
                .update_node_status(&node.id, BoardNodeStatus::Pending, None)
                .await?;

            if node.kind == NodeKind::Feature {
                feature_count += 1;
            }
        }

        // 3. Get or create persistent run
        let run = self.get_or_create_run().await?;

        // 4. Create board version
        let node_count = draft_nodes.len();
        let description = format!(
            "Dispatched {} nodes ({} features)",
            node_count, feature_count
        );
        let version = self.state.create_board_version(Some(&description)).await?;

        info!(
            "Created board version v{} for {} nodes",
            version.version_number, node_count
        );

        // 5. Record dispatch time
        self.state.record_dispatch().await?;

        // 6. Update run status to working
        self.state
            .update_project_run_status(ProjectRunStatus::Working)
            .await?;

        Ok(DispatchResult {
            run_name: run.run_name,
            node_count: draft_nodes.len(),
            feature_count,
            plan_task_count: 0,
            version_number: version.version_number,
            version_id: version.id,
        })
    }

    /// Get the existing run or create a new one
    async fn get_or_create_run(&self) -> DeltaDispatchResult<ProjectRun> {
        if let Some(run) = self.state.get_project_run().await? {
            info!("Using existing run: {}", run.run_name);
            return Ok(run);
        }

        let run_name = generate_run_name();
        info!("Creating new persistent run: {}", run_name);

        let run = self.state.create_project_run(&run_name).await?;
        Ok(run)
    }

    /// Mark a node as complete (called when task finishes)
    pub async fn complete_node(
        &self,
        node_id: &str,
        success: bool,
        commit_sha: Option<&str>,
    ) -> DeltaDispatchResult<()> {
        let status = if success {
            BoardNodeStatus::Done
        } else {
            BoardNodeStatus::Failed
        };

        self.state
            .update_node_status(node_id, status, commit_sha)
            .await?;
        info!("Marked node {} as {:?}", node_id, status);

        // Check if all nodes are done to pause the run
        self.check_run_completion().await?;

        Ok(())
    }

    /// Check if all non-draft tasks are done and pause the run
    async fn check_run_completion(&self) -> DeltaDispatchResult<()> {
        self.state.check_project_run_completion().await?;
        Ok(())
    }

    /// Get the board tree
    pub async fn get_tree(&self) -> DeltaDispatchResult<Vec<BoardNodeTree>> {
        Ok(self.state.get_tree().await?)
    }

    /// Get the project run
    pub async fn get_project_run(&self) -> DeltaDispatchResult<Option<ProjectRun>> {
        Ok(self.state.get_project_run().await?)
    }

    /// Get underlying state
    pub fn state(&self) -> &DeltaState {
        &self.state
    }

    /// Get all board versions for this project
    pub async fn get_board_versions(&self) -> DeltaDispatchResult<Vec<BoardVersion>> {
        Ok(self.state.get_board_versions().await?)
    }

    /// Get the latest board version
    pub async fn get_latest_version(&self) -> DeltaDispatchResult<Option<BoardVersion>> {
        Ok(self.state.get_latest_version().await?)
    }

    /// Get a specific board version
    pub async fn get_board_version(&self, id: i64) -> DeltaDispatchResult<BoardVersion> {
        Ok(self.state.get_board_version(id).await?)
    }

    /// Create a delivery for the latest board version
    pub async fn create_delivery(&self, target_branch: &str) -> DeltaDispatchResult<Delivery> {
        let version = self
            .state
            .get_latest_version()
            .await?
            .ok_or_else(|| DispatchError::NoChanges)?;

        Ok(self
            .state
            .create_delivery(version.id, target_branch)
            .await?)
    }

    /// Get current delivery for this project
    pub async fn get_current_delivery(&self) -> DeltaDispatchResult<Option<Delivery>> {
        Ok(self.state.get_current_delivery().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_service_creation() {
        let _service = DeltaDispatchService::new(1, 0);
    }
}
