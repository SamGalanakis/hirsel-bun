//! Project management module
//!
//! Projects are context containers — persistent knowledge, conversations,
//! threads, and a list of workspaces (local dirs or remote repos).

mod store;
mod types;

pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{
    CreateProjectRequest, Project, ProjectWorkspaceEntry, UpdateProjectRequest,
};
pub use types::{ProjectRetainedContext, ProjectSurfaceSnapshot};
