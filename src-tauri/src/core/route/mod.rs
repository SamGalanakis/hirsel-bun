//! Route management for parallel project exploration
//!
//! Routes allow users to fork from any board version to explore different approaches.
//! Each route owns its own repos, settings, board tree, docs, workspace, and run controls.

mod files;
mod store;
mod types;

pub use files::RouteFiles;
pub use store::{RouteError, RouteResult, RouteStore};
pub use types::*;
