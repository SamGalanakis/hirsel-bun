//! Project management module
//!
//! Projects are lightweight outcome containers. Repository linkage and execution
//! settings are route-scoped.
//!
//! All runs belong to a project. Projects enable:
//! - Reusable configuration for multiple runs
//! - Documentation persistence through git history
//! - Organized run management

mod focus;
mod store;
mod types;

pub use focus::validate_project_focus_view_html;
pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{
    CreateProjectRequest, Project, ProjectFocusView, ProjectRetainedContext,
    ProjectSurfaceSnapshot, RouteSummary, UpdateProjectRequest,
};
