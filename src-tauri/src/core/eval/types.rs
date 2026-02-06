//! Eval types and error definitions.

use std::path::PathBuf;
use thiserror::Error;

/// Eval-related errors
#[derive(Error, Debug)]
pub enum EvalError {
    #[error("Run not found: {0}")]
    RunNotFound(String),

    #[error("No eval script configured")]
    NoEvalScript,

    #[error("Eval script not found: {0}")]
    ScriptNotFound(String),

    #[error("Eval already running")]
    AlreadyRunning,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("State error: {0}")]
    State(#[from] crate::core::state::StateError),

    #[error("Timeout after {0} seconds")]
    Timeout(u32),

    #[error("Process failed: {0}")]
    ProcessFailed(String),
}

/// Configuration for running an ACP-based eval.
#[derive(Debug, Clone)]
pub struct EvalAcpConfig {
    pub run_name: String,
    pub eval_name: String,
    pub eval_id: i64,
    pub work_dir: PathBuf,
    pub run_dir: PathBuf,
    pub result_file: PathBuf,
    pub log_file: PathBuf,
    pub timeout_secs: u64,
    pub agent_command: Vec<String>,
}

/// Result of an ACP-based eval.
#[derive(Debug, Clone)]
pub struct EvalAcpResult {
    pub success: bool,
    pub feedback: String,
    pub eval_id: i64,
    pub eval_name: String,
}
