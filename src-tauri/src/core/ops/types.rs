//! Type definitions for core operations
//!
//! Configuration structs and result types used by the ops module.

/// Configuration for deleting a run
#[derive(Debug, Clone)]
pub struct DeleteRunConfig {
    /// Name of the run to delete
    pub runtime_name: String,
}

impl DeleteRunConfig {
    /// Create a DeleteRunConfig
    pub fn new(runtime_name: impl Into<String>) -> Self {
        Self {
            runtime_name: runtime_name.into(),
        }
    }
}

/// Result of a delete operation
#[derive(Debug, Clone)]
pub struct DeleteRunResult {
    /// Name of the deleted run
    pub runtime_name: String,

    /// Number of workers killed (if any were running)
    pub workers_killed: usize,
}
