//! Eval system for running automated checks on completed work.
//!
//! The eval system runs scripts to verify work quality, capturing feedback
//! and handling retry logic for failed evals.

mod acp;
mod context;
mod parser;
mod script;
mod types;

// Re-export public types
pub use types::{EvalAcpConfig, EvalAcpResult, EvalConfig, EvalError, EvalResult};

// Re-export context types
pub use context::{EvalContext, EvalFailure};

// Re-export script execution functions
pub use script::{execute_eval_script, run_eval, run_eval_in_tmux};

// Re-export parser functions
pub use parser::{get_eval_script, has_eval_script, parse_eval_script};

// Re-export ACP eval functions
pub use acp::{run_eval_acp, run_eval_from_args};
