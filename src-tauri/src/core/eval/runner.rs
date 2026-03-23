//! Eval runner (lash migration pending).

use super::types::{EvalAcpConfig, EvalAcpResult, EvalError};

/// Run an evaluation using the configured evaluator.
///
/// Legacy eval execution has been removed. The eval runner will be
/// migrated to lash in a follow-up.
pub async fn run_eval(config: EvalAcpConfig) -> Result<EvalAcpResult, EvalError> {
    Err(EvalError::ProcessFailed(format!(
        "Eval runner not migrated to lash yet (run='{}', eval='{}', id={})",
        config.runtime_name, config.eval_name, config.eval_id
    )))
}

/// Entry point used by `hirsel __eval-run`.
pub async fn run_eval_from_args(
    runtime_name: &str,
    _runtime_dir: &str,
    _agent_command_json: &str,
) -> Result<(), EvalError> {
    Err(EvalError::ProcessFailed(format!(
        "Eval runner for '{}' is not available until lash migration is complete",
        runtime_name
    )))
}
