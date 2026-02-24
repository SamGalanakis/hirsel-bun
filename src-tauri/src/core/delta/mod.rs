//! Board module
//!
//! Implements the unified board tree with spec/task/eval nodes:
//! - Single board_nodes table with kind + status lifecycle
//! - Dispatch: flip spec statuses + create plan tasks
//! - Plan workers handle spec->task decomposition
//!
//! ## Flow
//!
//! 1. User creates spec nodes (via UI or Shepherd)
//! 2. User clicks "Dispatch"
//! 3. Spec nodes set to pending, plan tasks created per spec
//! 4. Plan workers decompose specs into implementation tasks + evals
//! 5. Workers execute tasks, evals validate
//! 6. Delivery pushes completed work

mod dispatch;
mod export;
mod state;
pub mod types;

pub use dispatch::{DeltaDispatchResult, DeltaDispatchService, DispatchError};
pub use export::{DeltaExporter, ExportError, ExportResult, SyncResult};
pub use state::{
    bump_generation, get_generation, list_working_project_runs, update_project_run_status_by_name,
    DeltaState, DeltaStateError, DeltaStateResult,
};
pub use types::*;
