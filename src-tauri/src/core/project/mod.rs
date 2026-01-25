//! Project management module
//!
//! Projects are lightweight configuration containers that define a starting point
//! (git repo + branch, local folder, or greenfield) and default settings for runs.
//!
//! All runs belong to a project. Projects enable:
//! - Reusable configuration for multiple runs
//! - Documentation persistence through git history
//! - Organized run management

mod store;
mod types;

pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{CreateProjectRequest, Project, UpdateProjectRequest};
