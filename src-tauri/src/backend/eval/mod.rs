//! Eval system for running automated checks on completed work.
//!
//! The eval system runs scripts to verify work quality, capturing feedback
//! and handling retry logic for failed evals.

mod parser;
mod runner;
mod types;

// Re-export public types
pub use types::{EvalAcpConfig, EvalAcpResult, EvalError};

// Re-export parser functions
pub use parser::{get_eval_script, has_eval_script, parse_eval_script};

// Re-export eval execution functions
pub use runner::{run_eval, run_eval_from_args};
