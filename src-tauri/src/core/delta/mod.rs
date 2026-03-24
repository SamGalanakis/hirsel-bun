//! Route work-tree state.
//!
//! This is the persisted tree used by the backend-first Hirsel runtime:
//! work items, route runtime metadata, versions, and delivery state.

mod state;
pub mod types;

pub use state::{bump_generation, list_working_route_runtimes, DeltaState, DeltaStateError};
pub use types::*;
