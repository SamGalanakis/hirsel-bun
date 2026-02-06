//! Delivery module - publishes run changes to git branches/PRs
//!
//! ## Architecture
//!
//! ```text
//! DeliveryOrchestrator (main entry point)
//!   ├── WorkspaceLocation (local path or coordinator)
//!   ├── GitOperations (git commands)
//!   └── ForgeProvider (GitHub API)
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! let orchestrator = DeliveryOrchestrator::for_project(project_id).await?;
//! let push_result = orchestrator.push_branch(None, None)?;
//! let pr = orchestrator.create_pr("main", "Title", "Body").await?;
//! ```

mod git_ops;
mod orchestrator;
mod workspace;

pub use git_ops::{delivery_branch_name, pr_body, pr_title, GitOperations, PushResult};
pub use orchestrator::{DeliveryOrchestrator, DeliveryState, DeliveryStatus};
pub use workspace::{
    resolve_workspace, resolve_workspace_for_project, WorkspaceError, WorkspaceLocation,
    WorkspaceResult,
};

use thiserror::Error;

/// Error type for delivery operations
#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("Git error: {0}")]
    Git(String),
    #[error("GitHub error: {0}")]
    GitHub(#[from] crate::core::github::GitHubError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Run not found: {0}")]
    RunNotFound(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Merge conflicts detected")]
    MergeConflicts,
    #[error("No remote configured")]
    NoRemote,
    #[error("Not a GitHub repository")]
    NotGitHub,
    #[error("Conflict markers remain in files: {0:?}")]
    ConflictMarkersRemain(Vec<String>),
    #[error("Conflict resolution failed: {0}")]
    ConflictResolutionFailed(String),
}

pub type DeliveryResult<T> = Result<T, DeliveryError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delivery_branch_name() {
        assert_eq!(delivery_branch_name("jolly-parrot"), "hirsel/jolly-parrot");
    }

    #[test]
    fn test_pr_title() {
        assert_eq!(
            pr_title("jolly-parrot", Some("Add user auth")),
            "Add user auth"
        );
        assert_eq!(pr_title("jolly-parrot", None), "Changes from jolly-parrot");
        assert_eq!(
            pr_title("jolly-parrot", Some("")),
            "Changes from jolly-parrot"
        );
    }

    #[test]
    fn test_pr_body() {
        let body = pr_body(
            "jolly-parrot",
            &["build-api".to_string(), "user-endpoints".to_string()],
            &["api-test".to_string()],
        );

        assert!(body.contains("## Run: jolly-parrot"));
        assert!(body.contains("### Tasks"));
        assert!(body.contains("- build-api"));
        assert!(body.contains("### Evaluations"));
        assert!(body.contains("- api-test"));
    }
}
