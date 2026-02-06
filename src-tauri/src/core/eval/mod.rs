//! Eval system for running automated checks on completed work.
//!
//! The eval system runs scripts to verify work quality, capturing feedback
//! and handling retry logic for failed evals.

mod acp;
mod context;
mod parser;
mod types;

// Re-export public types
pub use types::{EvalAcpConfig, EvalAcpResult, EvalError};

// Re-export context types
pub use context::{EvalContext, EvalFailure};

// Re-export parser functions
pub use parser::{get_eval_script, has_eval_script, parse_eval_script};

// Re-export ACP eval functions
pub use acp::{run_eval_acp, run_eval_from_args};
