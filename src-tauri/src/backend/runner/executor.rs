//! Command executors for local runner lifecycle operations.

use async_trait::async_trait;
use std::process::{Command, Output, Stdio};

use super::RunnerError;

/// Result type for executor operations
pub type ExecutorResult<T> = Result<T, RunnerError>;

/// Trait for executing host-local lifecycle commands.
#[async_trait]
pub trait CommandExecutor: Send + Sync {
    /// Execute a command with arguments and return the output.
    async fn execute(&self, cmd: &str, args: &[&str]) -> ExecutorResult<Output>;

    /// Execute a command and return whether it succeeded (exit code 0).
    async fn execute_success(&self, cmd: &str, args: &[&str]) -> bool {
        self.execute(cmd, args)
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get the executor type name.
    fn executor_type(&self) -> &'static str;
}

/// Local command executor - runs commands directly on this machine.
#[derive(Debug, Clone, Default)]
pub struct LocalExecutor;

impl LocalExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CommandExecutor for LocalExecutor {
    async fn execute(&self, cmd: &str, args: &[&str]) -> ExecutorResult<Output> {
        Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(RunnerError::Io)
    }

    fn executor_type(&self) -> &'static str {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_executor_type() {
        let executor = LocalExecutor::new();
        assert_eq!(executor.executor_type(), "local");
    }
}
