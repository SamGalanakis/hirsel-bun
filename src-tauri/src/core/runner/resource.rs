//! Resource managers for different types of worker resources.
//!
//! This module provides the "what" part of the compositional runner design.
//! Resources know how to stop/check different types of workers (processes,
//! containers, etc.) but don't know where they're running.

use std::process::Output;

/// Trait for managing different types of worker resources.
///
/// Implementors know what commands to run (the "what") but not where to run them.
/// Each method returns the command and arguments needed to perform an operation.
pub trait ResourceManager: Send + Sync {
    /// Get the command and arguments to stop a resource.
    ///
    /// Returns (command, args) tuple.
    fn stop_command(&self, id: &str) -> (String, Vec<String>);

    /// Get the command and arguments to check if a resource is alive.
    ///
    /// Returns (command, args) tuple. The command should return success (0) if alive.
    fn is_alive_command(&self, id: &str) -> (String, Vec<String>);

    /// Parse the output of is_alive_command to determine if the resource is alive.
    ///
    /// Default implementation checks if the command succeeded.
    fn parse_is_alive(&self, output: &Output) -> bool {
        output.status.success()
    }

    /// Get the resource type name (used for runner_type in database).
    fn resource_type(&self) -> &'static str;
}

/// Process resource manager - manages worker processes by PID.
#[derive(Debug, Clone, Default)]
pub struct ProcessResource;

impl ProcessResource {
    pub fn new() -> Self {
        Self
    }
}

impl ResourceManager for ProcessResource {
    fn stop_command(&self, pid: &str) -> (String, Vec<String>) {
        // Use kill -TERM for graceful shutdown
        ("kill".into(), vec!["-TERM".into(), pid.into()])
    }

    fn is_alive_command(&self, pid: &str) -> (String, Vec<String>) {
        // kill -0 checks if process exists without sending a signal
        ("kill".into(), vec!["-0".into(), pid.into()])
    }

    fn resource_type(&self) -> &'static str {
        "process"
    }
}

/// Docker resource manager - manages worker containers by container ID.
#[derive(Debug, Clone, Default)]
pub struct DockerResource;

impl DockerResource {
    pub fn new() -> Self {
        Self
    }
}

impl ResourceManager for DockerResource {
    fn stop_command(&self, container_id: &str) -> (String, Vec<String>) {
        // docker stop -t 10 gives 10 seconds for graceful shutdown before SIGKILL
        (
            "docker".into(),
            vec!["stop".into(), "-t".into(), "10".into(), container_id.into()],
        )
    }

    fn is_alive_command(&self, container_id: &str) -> (String, Vec<String>) {
        // docker inspect -f '{{.State.Running}}' returns "true" or "false"
        (
            "docker".into(),
            vec![
                "inspect".into(),
                "-f".into(),
                "{{.State.Running}}".into(),
                container_id.into(),
            ],
        )
    }

    fn parse_is_alive(&self, output: &Output) -> bool {
        if !output.status.success() {
            return false;
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        stdout.trim() == "true"
    }

    fn resource_type(&self) -> &'static str {
        "docker"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_stop_command() {
        let resource = ProcessResource::new();
        let (cmd, args) = resource.stop_command("12345");
        assert_eq!(cmd, "kill");
        assert_eq!(args, vec!["-TERM", "12345"]);
    }

    #[test]
    fn test_process_is_alive_command() {
        let resource = ProcessResource::new();
        let (cmd, args) = resource.is_alive_command("12345");
        assert_eq!(cmd, "kill");
        assert_eq!(args, vec!["-0", "12345"]);
    }

    #[test]
    fn test_docker_stop_command() {
        let resource = DockerResource::new();
        let (cmd, args) = resource.stop_command("abc123");
        assert_eq!(cmd, "docker");
        assert_eq!(args, vec!["stop", "-t", "10", "abc123"]);
    }

    #[test]
    fn test_docker_is_alive_command() {
        let resource = DockerResource::new();
        let (cmd, args) = resource.is_alive_command("abc123");
        assert_eq!(cmd, "docker");
        assert_eq!(args, vec!["inspect", "-f", "{{.State.Running}}", "abc123"]);
    }

    #[test]
    fn test_resource_types() {
        assert_eq!(ProcessResource::new().resource_type(), "process");
        assert_eq!(DockerResource::new().resource_type(), "docker");
    }
}
