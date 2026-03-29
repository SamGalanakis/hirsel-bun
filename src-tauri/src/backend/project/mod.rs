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

mod focus;
mod store;
mod types;

pub use focus::validate_project_focus_view_html;
pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{
    CreateProjectRequest, Project, ProjectFocusView, ProjectRetainedContext,
    ProjectSurfaceSnapshot, UpdateProjectRequest,
};
