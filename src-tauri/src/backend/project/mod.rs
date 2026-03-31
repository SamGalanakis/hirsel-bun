//! Project management module
//!
//! Projects are the top-level product object.
//!
//! Each project owns:
//! - one starting point
//! - one central checkout
//! - one shepherd conversation
//! - one canvas artifact
//! - many visible threads

mod prepare;
mod store;
mod types;

pub use prepare::{
    ensure_project_runtime_preparation_started, get_project_runtime_preparation,
    project_runtime_is_ready, retry_project_runtime_preparation,
};
pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{
    CreateProjectRequest, Project, ProjectFocusView, ProjectPreparationStep,
    ProjectRetainedContext, ProjectRuntimePreparation, ProjectSurfaceSnapshot,
    UpdateProjectRequest,
};
