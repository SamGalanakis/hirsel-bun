//! Project management module
//!
//! Projects are the top-level product object.
//!
//! Each project owns:
//! - one starting point
//! - one central checkout
//! - one shepherd conversation
//! - one canvas document
//! - many visible threads

#[cfg(feature = "host")]
mod prepare;
#[cfg(feature = "host")]
mod store;
mod types;

#[cfg(feature = "host")]
pub use prepare::{
    get_project_runtime_preparation, project_runtime_is_ready, retry_project_runtime_preparation,
    start_project_runtime_preparation,
};
#[cfg(feature = "host")]
pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{
    CreateProjectRequest, Project, ProjectPreparationStatus, ProjectPreparationStep,
    ProjectRuntimePreparation, UpdateProjectRequest,
};
#[cfg(feature = "host")]
pub use types::{ProjectRetainedContext, ProjectSurfaceSnapshot};

pub fn legacy_workspace_name_for_project_id(project_id: i64) -> String {
    format!("project-{}", project_id)
}

pub fn workspace_name_for_project(project: &Project) -> String {
    format!("project-{}-{}", project.id, project.workspace_key)
}
