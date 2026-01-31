//! Type definitions for core operations
//!
//! Configuration structs and result types used by the ops module.

use std::path::PathBuf;

/// Configuration for deleting a run
#[derive(Debug, Clone)]
pub struct DeleteRunConfig {
    /// Name of the run to delete
    pub run_name: String,
}

impl DeleteRunConfig {
    /// Create a DeleteRunConfig
    pub fn new(run_name: impl Into<String>) -> Self {
        Self {
            run_name: run_name.into(),
        }
    }
}

/// Result of a delete operation
#[derive(Debug, Clone)]
pub struct DeleteRunResult {
    /// Name of the deleted run
    pub run_name: String,

    /// Number of workers killed (if any were running)
    pub workers_killed: usize,
}

/// Configuration for cloning a run
#[derive(Debug, Clone)]
pub struct CloneRunConfig {
    /// Name of the source run to clone from
    pub source_run: String,

    /// Name for the new run
    pub new_name: String,

    /// Whether to copy assets folder
    /// CLI: true (full clone), GUI: true
    pub copy_assets: bool,
}

impl CloneRunConfig {
    /// Create a new CloneRunConfig
    pub fn new(source_run: impl Into<String>, new_name: impl Into<String>) -> Self {
        Self {
            source_run: source_run.into(),
            new_name: new_name.into(),
            copy_assets: true,
        }
    }
}

/// Result of a clone operation
#[derive(Debug, Clone)]
pub struct CloneRunResult {
    /// Name of the source run
    pub source_run: String,

    /// Name of the new run
    pub new_name: String,

    /// Project path (if set in source)
    pub project_path: Option<String>,

    /// Worker scale setting
    pub worker_scale: String,

    /// Time limit in minutes (if set)
    pub time_limit_minutes: Option<i64>,

    /// Human-in-the-loop setting
    pub human_in_the_loop: bool,

    /// Default runner (if set)
    pub default_runner: Option<String>,

    /// The spec content that was cloned
    pub spec_content: String,

    /// Whether eval.md was cloned
    pub has_eval: bool,

    /// Whether assets were copied
    pub assets_copied: bool,

    /// Path to the new run directory
    pub run_dir: PathBuf,
}
