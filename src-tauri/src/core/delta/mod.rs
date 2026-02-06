//! Delta dispatch module
//!
//! Implements the unified board with delta-based dispatch:
//! - Draft tree: user edits freely
//! - Live tree: dispatched/working state
//! - Delta dispatch: diff trees, generate tasks, update run
//!
//! ## Architecture
//!
//! ```text
//! DeltaDispatchService
//!   ├── DeltaGenerator (creates delta tasks from diff)
//!   │   └── DiffService (computes diff between trees)
//!   └── DeltaState (DB operations for draft/live/submissions)
//! ```
//!
//! ## Flow
//!
//! 1. User edits draft tree (via UI or Gyp)
//! 2. User clicks "Dispatch"
//! 3. System computes diff (draft vs live)
//! 4. Delta generator creates tasks (implement/modify/revert)
//! 5. Tasks added to persistent project run
//! 6. Live tree synced to match draft
//! 7. Run resumes work

mod diff;
mod dispatch;
mod export;
mod generator;
mod runner;
mod state;
pub mod types;

pub use diff::DiffService;
pub use dispatch::{DeltaDispatchResult, DeltaDispatchService, DispatchError, DispatchPreview};
pub use export::{DeltaExporter, ExportError, ExportResult, SyncResult};
pub use generator::{DeltaGenerator, GeneratorError, GeneratorResult};
pub use runner::{list_working_project_runs, DeltaRunner, RunnerError, RunnerResult};
pub use state::{bump_generation, get_generation, DeltaState, DeltaStateError, DeltaStateResult};
pub use types::*;
