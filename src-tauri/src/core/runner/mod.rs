//! Runner trait and implementations for single-host worker execution.

pub mod composed;
pub mod config;
pub mod executor;
pub mod local;
pub mod resource;
pub mod types;

pub use composed::ComposedRunner;
pub use executor::{CommandExecutor, LocalExecutor};
pub use local::LocalRunner;
pub use resource::{DockerResource, ProcessResource, ResourceManager};

pub use config::{ContainerConfig, RunnerConfig};
pub use types::{Runner, RunnerError, RunnerResult, SpawnResult, WorkerHandle, WorkerSpawnConfig};

/// Create a runner from configuration.
pub fn create_runner(config: &RunnerConfig) -> Box<dyn Runner> {
    Box::new(LocalRunner::new(config.container.clone()))
}

/// Create a lifecycle runner from a runner_type string.
pub fn create_lifecycle_runner(runner_type: &str) -> Option<Box<dyn Runner>> {
    match runner_type {
        "local" | "process" => Some(Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            ProcessResource::new(),
        ))),
        "docker" => Some(Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            DockerResource::new(),
        ))),
        _ => None,
    }
}

/// Create a lifecycle runner from a worker handle.
pub fn create_lifecycle_runner_for_handle(handle: &WorkerHandle) -> Box<dyn Runner> {
    match handle.runner_type.as_str() {
        "docker" => Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            DockerResource::new(),
        )),
        _ => Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            ProcessResource::new(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_config_default() {
        let config = RunnerConfig::default();
        assert_eq!(config.execution_kind(), "local");
        assert!(config.container.is_none());
    }

    #[test]
    fn test_runner_config_local_docker() {
        let config = RunnerConfig::local_docker("rust:latest".to_string());
        assert_eq!(config.execution_kind(), "docker");
        assert!(config.uses_container());
        assert_eq!(config.container.as_ref().unwrap().image, "rust:latest");
    }
}
