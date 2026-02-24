//! Project management module
//!
//! Projects are lightweight outcome containers. Repository linkage and execution
//! settings are route-scoped.
//!
//! All runs belong to a project. Projects enable:
//! - Reusable configuration for multiple runs
//! - Documentation persistence through git history
//! - Organized run management

mod store;
mod types;

pub use store::{ProjectError, ProjectResult, ProjectStore};
pub use types::{CreateProjectRequest, Project, UpdateProjectRequest};
