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

mod prepare;
mod store;
mod types;

pub use prepare::{
    get_project_runtime_preparation, project_runtime_is_ready, retry_project_runtime_preparation,
    start_project_runtime_preparation,
};
pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{
    CreateProjectRequest, Project, ProjectPreparationStatus, ProjectPreparationStep,
    ProjectRetainedContext, ProjectRuntimePreparation, ProjectSurfaceSnapshot,
    UpdateProjectRequest,
};
