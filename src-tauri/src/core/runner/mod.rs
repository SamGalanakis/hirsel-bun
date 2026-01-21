//! Runner trait and implementations for spawning workers on different platforms.
//!
//! This module provides a unified interface for spawning worker processes
//! across different execution environments using a Host + Container model:
//!
//! ## Hosts (where compute runs)
//! - `local` - on this machine (contextual: GUI/CLI machine in local mode, orchestrator in remote mode)
//! - `ssh` - remote machine via SSH
//! - `sprite` - Sprites.dev cloud VM
//! - `fly` - Fly.io ephemeral machines
//! - `client` - (remote mode only) SSH back to the GUI/CLI user's machine via Tailscale
//!
//! ## Containers (optional isolation)
//! - `none` (default) - run directly on host
//! - `docker` - run in Docker container (image URI only)
//!
//! ## Constraints
//! - Sprites cannot run Docker (Firecracker limitation)
//! - Fly requires container.image (machines ARE containers)
//! - `client` host only available in remote mode

pub mod composed;
pub mod config;
pub mod executor;
pub mod fly;
pub mod local;
pub mod resource;
pub mod setup;
pub mod sprite;
pub mod ssh;
pub mod types;

// Re-export runner implementations
pub use composed::ComposedRunner;
pub use executor::{CommandExecutor, LocalExecutor, SshExecutor};
pub use fly::FlyRunner;
pub use local::LocalRunner;
pub use resource::{DockerResource, ProcessResource, ResourceManager};
pub use sprite::SpriteRunner;
pub use ssh::SshRunner;

// Re-export types
pub use config::{
    ContainerConfig, FlyHostConfig, HostConfig, HostConfigOrShortcut, RunnerConfig,
    SpriteHostConfig, SshHostConfig,
};
pub use types::{
    OrchestratorMode, Runner, RunnerError, RunnerResult, SpawnResult, WorkerHandle,
    WorkerSpawnConfig,
};

// =============================================================================
// Runner Factory
// =============================================================================

/// Create a runner from configuration.
pub fn create_runner(config: &RunnerConfig) -> Box<dyn Runner> {
    let host = config.host.resolve();
    match host {
        HostConfig::Local | HostConfig::Client => {
            Box::new(LocalRunner::new(config.container.clone()))
        }
        HostConfig::Ssh(ssh_config) => {
            Box::new(SshRunner::new(ssh_config, config.container.clone()))
        }
        HostConfig::Sprite(sprite_config) => {
            if config.container.is_some() {
                tracing::warn!("Sprites do not support containers - ignoring container config");
            }
            Box::new(SpriteRunner::new(sprite_config))
        }
        HostConfig::Fly(fly_config) => {
            let image = config
                .container
                .as_ref()
                .map(|c| c.image.clone())
                .unwrap_or_else(|| {
                    tracing::warn!(
                        "Fly host should have container.image set, using debian:bookworm-slim"
                    );
                    "debian:bookworm-slim".to_string()
                });
            Box::new(FlyRunner::new(fly_config, image))
        }
    }
}

/// Create a lifecycle runner from a runner_type string.
///
/// This factory creates a runner suitable for lifecycle management (stop, is_alive)
/// based on the runner_type stored in the database. It uses the compositional design
/// with CommandExecutor + ResourceManager.
///
/// For SSH-based runner types, an optional SshHostConfig can be provided.
/// If not provided for SSH types, the function returns None.
///
/// Supported runner_types:
/// - "local" -> LocalExecutor + ProcessResource
/// - "docker" -> LocalExecutor + DockerResource
/// - "ssh" -> SshExecutor + ProcessResource (requires ssh_config)
/// - "ssh-docker" -> SshExecutor + DockerResource (requires ssh_config)
/// - "sprite" -> Handled by SpriteRunner (requires config)
/// - "fly" -> Handled by FlyRunner (requires config)
///
/// Returns None if the runner_type is not supported or required config is missing.
pub fn create_lifecycle_runner(
    runner_type: &str,
    ssh_config: Option<&SshHostConfig>,
) -> Option<Box<dyn Runner>> {
    match runner_type {
        "local" | "process" => Some(Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            ProcessResource::new(),
        ))),
        "docker" => Some(Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            DockerResource::new(),
        ))),
        "ssh" => {
            let ssh = ssh_config?;
            Some(Box::new(ComposedRunner::new(
                SshExecutor::new(ssh.address.clone(), ssh.port, ssh.ssh_key.clone()),
                ProcessResource::new(),
            )))
        }
        "ssh-docker" => {
            let ssh = ssh_config?;
            Some(Box::new(ComposedRunner::new(
                SshExecutor::new(ssh.address.clone(), ssh.port, ssh.ssh_key.clone()),
                DockerResource::new(),
            )))
        }
        // Sprite and Fly require full config, not just lifecycle runner
        // For now, return None - these should use their own runners
        "sprite" | "fly" => None,
        _ => None,
    }
}

/// Create a lifecycle runner from a WorkerHandle.
///
/// This is a convenience function that determines the runner type from the handle
/// and creates the appropriate lifecycle runner for local/docker runners.
///
/// For remote runners (ssh, sprite, fly), this returns a local runner as a fallback
/// since we don't have the SSH/API config stored in the handle.
pub fn create_lifecycle_runner_for_handle(handle: &WorkerHandle) -> Box<dyn Runner> {
    match handle.runner_type.as_str() {
        "local" | "process" => Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            ProcessResource::new(),
        )),
        "docker" => Box::new(ComposedRunner::new(
            LocalExecutor::new(),
            DockerResource::new(),
        )),
        // For remote types, we can't recreate the runner without config
        // Return a local runner as fallback (won't work but won't panic)
        _ => {
            tracing::warn!(
                "Cannot create lifecycle runner for remote type '{}', using local fallback",
                handle.runner_type
            );
            Box::new(ComposedRunner::new(
                LocalExecutor::new(),
                ProcessResource::new(),
            ))
        }
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

/// Parse a remote specification like "user@host:count" or "user@host"
///
/// # Returns
/// Tuple of (host, count)
pub fn parse_remote_spec(spec: &str) -> (String, u32) {
    // Check for format: user@host:count
    if let Some(colon_idx) = spec.rfind(':') {
        let count_part = &spec[colon_idx + 1..];
        if let Ok(count) = count_part.parse::<u32>() {
            let host = spec[..colon_idx].to_string();
            return (host, count);
        }
    }

    // Format: user@host (count defaults to 1)
    (spec.to_string(), 1)
}

/// Parse multiple remote specifications
///
/// # Returns
/// List of (host, count) tuples
pub fn parse_remote_specs(specs: &[String]) -> Vec<(String, u32)> {
    specs.iter().map(|s| parse_remote_spec(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_spec_with_count() {
        let (host, count) = parse_remote_spec("user@server.com:3");
        assert_eq!(host, "user@server.com");
        assert_eq!(count, 3);
    }

    #[test]
    fn test_parse_remote_spec_without_count() {
        let (host, count) = parse_remote_spec("user@server.com");
        assert_eq!(host, "user@server.com");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_parse_remote_specs() {
        let specs = vec![
            "user@server1.com:2".to_string(),
            "user@server2.com".to_string(),
        ];
        let parsed = parse_remote_specs(&specs);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], ("user@server1.com".to_string(), 2));
        assert_eq!(parsed[1], ("user@server2.com".to_string(), 1));
    }

    #[test]
    fn test_runner_config_default() {
        let config = RunnerConfig::default();
        assert_eq!(config.host_type(), "local");
        assert!(config.container.is_none());
    }

    #[test]
    fn test_runner_config_local_docker() {
        let config = RunnerConfig::local_docker("rust:latest".to_string());
        assert_eq!(config.host_type(), "local");
        assert!(config.uses_container());
        assert_eq!(config.container.as_ref().unwrap().image, "rust:latest");
    }

    #[test]
    fn test_host_config_shortcut_resolution() {
        let shortcut = HostConfigOrShortcut::Shortcut("local".to_string());
        assert!(matches!(shortcut.resolve(), HostConfig::Local));

        let shortcut = HostConfigOrShortcut::Shortcut("client".to_string());
        assert!(matches!(shortcut.resolve(), HostConfig::Client));
    }

    #[test]
    fn test_ssh_config_default() {
        let config = SshHostConfig::default();
        assert_eq!(config.port, 22);
        assert_eq!(config.work_base, "/tmp/hirsel-remote");
    }

    #[test]
    fn test_sprite_config_default() {
        let config = SpriteHostConfig::default();
        assert!(config.api_token.is_none());
        assert!(config.auto_destroy);
        assert_eq!(config.idle_timeout_secs, 30);
    }

    #[test]
    fn test_runner_config_serialization() {
        let config = RunnerConfig::sprite(SpriteHostConfig {
            api_token: Some("my-secret-token".to_string()),
            checkpoint: Some("hirsel-v1".to_string()),
            auto_destroy: false,
            idle_timeout_secs: 60,
            api_url: "https://api.sprites.dev".to_string(),
            use_file_push: false,
        });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("sprite"));
        assert!(json.contains("my-secret-token"));

        let parsed: RunnerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.host_type(), "sprite");
    }

    #[test]
    fn test_host_compatibility_local_mode() {
        // Local and SSH work with local orchestrator
        // SSH uses reverse tunnel to daemon TCP on localhost:19700
        assert!(HostConfig::Local.is_compatible_with(OrchestratorMode::Local));
        assert!(
            HostConfig::Ssh(SshHostConfig::default()).is_compatible_with(OrchestratorMode::Local)
        );

        // Sprite, Fly, Client require publicly accessible HTTP coordinator
        assert!(!HostConfig::Sprite(SpriteHostConfig::default())
            .is_compatible_with(OrchestratorMode::Local));
        assert!(
            !HostConfig::Fly(FlyHostConfig::default()).is_compatible_with(OrchestratorMode::Local)
        );
        assert!(!HostConfig::Client.is_compatible_with(OrchestratorMode::Local));
    }

    #[test]
    fn test_host_compatibility_remote_mode() {
        // All hosts work with remote orchestrator
        assert!(HostConfig::Local.is_compatible_with(OrchestratorMode::Remote));
        assert!(
            HostConfig::Ssh(SshHostConfig::default()).is_compatible_with(OrchestratorMode::Remote)
        );
        assert!(HostConfig::Sprite(SpriteHostConfig::default())
            .is_compatible_with(OrchestratorMode::Remote));
        assert!(
            HostConfig::Fly(FlyHostConfig::default()).is_compatible_with(OrchestratorMode::Remote)
        );
        assert!(HostConfig::Client.is_compatible_with(OrchestratorMode::Remote));
    }

    #[test]
    fn test_runner_config_validate_for_mode() {
        let local = RunnerConfig::local();
        assert!(local.validate_for_mode(OrchestratorMode::Local).is_ok());
        assert!(local.validate_for_mode(OrchestratorMode::Remote).is_ok());

        // SSH works in both modes (local via reverse tunnel)
        let ssh = RunnerConfig::ssh(SshHostConfig::default());
        assert!(ssh.validate_for_mode(OrchestratorMode::Local).is_ok());
        assert!(ssh.validate_for_mode(OrchestratorMode::Remote).is_ok());

        let fly = RunnerConfig::fly(FlyHostConfig::default(), "debian:latest".to_string());
        assert!(fly.validate_for_mode(OrchestratorMode::Local).is_err());
        assert!(fly.validate_for_mode(OrchestratorMode::Remote).is_ok());

        let sprite = RunnerConfig::sprite(SpriteHostConfig::default());
        assert!(sprite.validate_for_mode(OrchestratorMode::Local).is_err());
        assert!(sprite.validate_for_mode(OrchestratorMode::Remote).is_ok());
    }
}
