//! Composed runner for host-local lifecycle management.

use async_trait::async_trait;
use tracing::info;

use super::executor::CommandExecutor;
use super::resource::ResourceManager;
use super::{Runner, RunnerError, RunnerResult, SpawnResult, WorkerHandle, WorkerSpawnConfig};

/// A runner composed of an executor and resource manager.
///
/// This implements the Runner trait by delegating:
/// - `stop` -> resource.stop_command() executed via executor
/// - `is_alive` -> resource.is_alive_command() executed via executor
/// - `spawn` -> Not supported (use LocalRunner for spawning)
///
/// The runner_type is computed from both executor and resource types:
/// - "local" for LocalExecutor + ProcessResource
/// - "docker" for LocalExecutor + DockerResource
pub struct ComposedRunner<E, R>
where
    E: CommandExecutor + 'static,
    R: ResourceManager + 'static,
{
    executor: E,
    resource: R,
}

impl<E, R> ComposedRunner<E, R>
where
    E: CommandExecutor + 'static,
    R: ResourceManager + 'static,
{
    /// Create a new composed runner from an executor and resource manager.
    pub fn new(executor: E, resource: R) -> Self {
        Self { executor, resource }
    }
}

#[async_trait]
impl<E, R> Runner for ComposedRunner<E, R>
where
    E: CommandExecutor + 'static,
    R: ResourceManager + 'static,
{
    /// Spawn is not supported by ComposedRunner.
    ///
    /// Use LocalRunner for spawning workers. ComposedRunner is designed for lifecycle management only.
    async fn spawn(&self, _config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        Err(RunnerError::SpawnFailed(
            "ComposedRunner does not support spawn. Use LocalRunner.".into(),
        ))
    }

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        let (cmd, args) = self.resource.stop_command(&handle.runner_id);
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let output = self.executor.execute(&cmd, &args_ref).await?;

        if output.status.success() {
            info!(
                "Stopped worker {} ({} via {})",
                handle.worker_name,
                self.resource.resource_type(),
                self.executor.executor_type()
            );
        } else {
            // Log but don't fail - worker may already be stopped
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::debug!(
                "Stop command for {} returned non-zero: {}",
                handle.worker_name,
                stderr
            );
        }

        Ok(())
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
        let (cmd, args) = self.resource.is_alive_command(&handle.runner_id);
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        match self.executor.execute(&cmd, &args_ref).await {
            Ok(output) => self.resource.parse_is_alive(&output),
            Err(_) => false,
        }
    }

    fn runner_type(&self) -> &'static str {
        // Compute runner type from executor and resource
        let exec_type = self.executor.executor_type();
        let res_type = self.resource.resource_type();

        match (exec_type, res_type) {
            ("local", "process") => "local",
            ("local", "docker") => "docker",
            _ => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::executor::LocalExecutor;
    use super::super::resource::{DockerResource, ProcessResource};
    use super::*;

    #[test]
    fn test_runner_type_local_process() {
        let runner = ComposedRunner::new(LocalExecutor::new(), ProcessResource::new());
        assert_eq!(runner.runner_type(), "local");
    }

    #[test]
    fn test_runner_type_local_docker() {
        let runner = ComposedRunner::new(LocalExecutor::new(), DockerResource::new());
        assert_eq!(runner.runner_type(), "docker");
    }

    /// Integration test: spawn a docker container and verify stop/is_alive work.
    /// This test requires docker to be running.
    #[tokio::test]
    #[ignore] // Run with: cargo test docker_lifecycle --ignored
    async fn test_docker_lifecycle_integration() {
        use std::process::Command;

        // Start a test container
        let container_name = format!("hirsel-test-{}", std::process::id());
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "--name",
                &container_name,
                "alpine:latest",
                "sleep",
                "3600",
            ])
            .output()
            .expect("Failed to start docker container");

        if !output.status.success() {
            panic!(
                "Failed to start container: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Create composed runner for docker
        let runner = ComposedRunner::new(LocalExecutor::new(), DockerResource::new());

        // Create handle
        let handle = WorkerHandle {
            worker_name: "test-worker".to_string(),
            runner_id: container_id.clone(),
            runner_type: "docker".to_string(),
        };

        // Test is_alive returns true for running container
        assert!(
            runner.is_alive(&handle).await,
            "is_alive should return true for running container"
        );

        // Test stop
        runner.stop(&handle).await.expect("stop should succeed");

        // Give docker a moment to stop the container
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Test is_alive returns false after stop
        assert!(
            !runner.is_alive(&handle).await,
            "is_alive should return false after stop"
        );

        // Cleanup (container should already be removed due to --rm, but just in case)
        let _ = Command::new("docker")
            .args(["rm", "-f", &container_id])
            .output();
    }

    /// Integration test: spawn a process and verify stop/is_alive work.
    #[tokio::test]
    #[ignore] // Run with: cargo test process_lifecycle --ignored
    async fn test_process_lifecycle_integration() {
        use std::process::Command;

        // Start a test process (sleep)
        let mut child = Command::new("sleep")
            .arg("3600")
            .spawn()
            .expect("Failed to start sleep process");

        let pid = child.id();

        // Create composed runner for process
        let runner = ComposedRunner::new(LocalExecutor::new(), ProcessResource::new());

        // Create handle
        let handle = WorkerHandle {
            worker_name: "test-worker".to_string(),
            runner_id: pid.to_string(),
            runner_type: "local".to_string(),
        };

        // Test is_alive returns true for running process
        assert!(
            runner.is_alive(&handle).await,
            "is_alive should return true for running process"
        );

        // Test stop (sends SIGTERM)
        runner.stop(&handle).await.expect("stop should succeed");

        // Wait for process to actually terminate (reap zombie)
        // SIGTERM should make sleep exit, but we need to wait() to reap it
        let _ = child.wait();

        // Test is_alive returns false after stop
        assert!(
            !runner.is_alive(&handle).await,
            "is_alive should return false after stop"
        );
    }
}
