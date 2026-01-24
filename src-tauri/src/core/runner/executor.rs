//! Command executors for running commands on different hosts.
//!
//! This module provides the "where" part of the compositional runner design.
//! Executors know how to run commands on different hosts (local, SSH, etc.)
//! but don't know what resources to manage.

use async_trait::async_trait;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use super::RunnerError;

/// Result type for executor operations
pub type ExecutorResult<T> = Result<T, RunnerError>;

/// Trait for executing commands on different hosts.
///
/// Implementors know how to run commands (the "where") but not what to run.
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

/// SSH command executor - runs commands on a remote machine via SSH.
#[derive(Debug, Clone)]
pub struct SshExecutor {
    /// SSH host (e.g., "user@server.example.com")
    pub host: String,
    /// SSH port (default: 22)
    pub port: u16,
    /// Path to SSH private key (optional)
    pub ssh_key: Option<String>,
}

impl SshExecutor {
    pub fn new(host: String, port: u16, ssh_key: Option<String>) -> Self {
        Self {
            host,
            port,
            ssh_key,
        }
    }

    /// Build the base SSH command with common options.
    fn build_ssh_cmd(&self) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.args(["-o", "BatchMode=yes"])
            .args(["-o", "StrictHostKeyChecking=accept-new"])
            .args(["-p", &self.port.to_string()]);

        if let Some(ref key) = self.ssh_key {
            let key_path = if key.starts_with("~") {
                dirs::home_dir()
                    .map(|h| h.join(key.strip_prefix("~/").unwrap_or(key)))
                    .unwrap_or_else(|| PathBuf::from(key))
            } else {
                PathBuf::from(key)
            };
            cmd.args(["-i", &key_path.to_string_lossy()]);
        }

        cmd.arg(&self.host);
        cmd
    }
}

#[async_trait]
impl CommandExecutor for SshExecutor {
    async fn execute(&self, cmd: &str, args: &[&str]) -> ExecutorResult<Output> {
        // Build the remote command string
        let remote_cmd = if args.is_empty() {
            cmd.to_string()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };

        let mut ssh_cmd = self.build_ssh_cmd();
        ssh_cmd
            .arg(&remote_cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        ssh_cmd.output().map_err(RunnerError::Io)
    }

    fn executor_type(&self) -> &'static str {
        "ssh"
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

    #[test]
    fn test_ssh_executor_type() {
        let executor = SshExecutor::new("user@host".into(), 22, None);
        assert_eq!(executor.executor_type(), "ssh");
    }
}
