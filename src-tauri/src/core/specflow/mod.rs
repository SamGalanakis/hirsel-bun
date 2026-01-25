//! SpecFlow - Spatial canvas for managing project specs, tasks, and evals
//!
//! SpecFlow provides a 2D infinite canvas where project features are laid out as "Islands."
//! Each island contains a Trifecta Grid (Spec | Tasks | Eval) representing the intent,
//! reality, and proof of a feature.
//!
//! # Architecture
//!
//! - Each project has its own `specflow.db` SQLite database
//! - Islands are feature containers positioned on the canvas
//! - Rows within islands form the Trifecta Grid
//! - Wires connect dependent islands
//! - Bookmarks save viewport positions
//!
//! # Usage
//!
//! ```ignore
//! use hirsel::core::specflow::{SpecFlowState, CreateIslandRequest};
//!
//! let state = SpecFlowState::open(project_id)?;
//!
//! // Create an island
//! let island = state.create_island(&CreateIslandRequest {
//!     name: "Authentication".to_string(),
//!     x: 100.0,
//!     y: 100.0,
//!     width: 400.0,
//! })?;
//!
//! // List all islands with their rows
//! let islands = state.list_islands()?;
//! ```

mod state;
mod types;

pub use state::{SpecFlowError, SpecFlowResult, SpecFlowState};
pub use types::{
    Bookmark, CreateIslandRequest, CreateRowRequest, DispatchResult, InitialTask, Island, Row,
    RowEvalStatus, SpecStatus, TaskStatus, UpdateIslandRequest, UpdateRowRequest, Wire,
};
