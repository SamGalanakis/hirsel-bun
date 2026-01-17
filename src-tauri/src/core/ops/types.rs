//! Type definitions for core operations
//!
//! Configuration structs and result types used by the ops module.

use std::path::PathBuf;

/// Configuration for deleting a run
#[derive(Debug, Clone)]
pub struct DeleteRunConfig {
    /// Name of the run to delete
    pub run_name: String,

    /// Whether to delete GypChat messages for this run
    /// GUI: true (cleanup chat history), CLI: false (not needed)
    pub delete_gyp_chat: bool,

    /// Whether to remove the hirsel_work remote from the project repository
    /// CLI: true (cleanup remote), GUI: false (not typically needed)
    pub remove_project_remote: bool,
}

impl DeleteRunConfig {
    /// Create a new DeleteRunConfig with all options
    pub fn new(
        run_name: impl Into<String>,
        delete_gyp_chat: bool,
        remove_project_remote: bool,
    ) -> Self {
        Self {
            run_name: run_name.into(),
            delete_gyp_chat,
            remove_project_remote,
        }
    }

    /// Create a config for GUI delete (deletes gyp chat, no remote cleanup)
    pub fn for_gui(run_name: impl Into<String>) -> Self {
        Self {
            run_name: run_name.into(),
            delete_gyp_chat: true,
            remove_project_remote: false,
        }
    }

    /// Create a config for CLI delete (no gyp chat, cleans up remote)
    pub fn for_cli(run_name: impl Into<String>) -> Self {
        Self {
            run_name: run_name.into(),
            delete_gyp_chat: false,
            remove_project_remote: true,
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

    /// Whether GypChat messages were deleted
    pub gyp_chat_deleted: bool,

    /// Whether the project remote was removed
    pub project_remote_removed: bool,
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

    /// Max iterations (if set)
    pub max_iterations: Option<i64>,

    /// The spec content that was cloned
    pub spec_content: String,

    /// Whether eval.md was cloned
    pub has_eval: bool,

    /// Whether assets were copied
    pub assets_copied: bool,

    /// Path to the new run directory
    pub run_dir: PathBuf,
}
