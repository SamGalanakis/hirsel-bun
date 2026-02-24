//! SSH runner implementation - spawns workers on remote machines via SSH.
//!
//! This runner connects to remote machines via SSH and spawns worker processes.
//! It uses reverse SSH tunnels to allow workers to connect back to the coordinator.

use async_trait::async_trait;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{debug, error, info};

use super::setup::{self, WorkerSetupConfig};
use super::{
    ContainerConfig, Runner, RunnerError, RunnerResult, SpawnResult, SshHostConfig, WorkerHandle,
    WorkerSpawnConfig,
};

/// SSH runner - spawns workers on remote machines via SSH.
/// Optionally supports running workers in Docker containers on the remote host.
pub struct SshRunner {
    config: SshHostConfig,
    /// Port on remote that tunnels back to coordinator
    tunnel_port: Option<u16>,
    /// Optional container configuration for running workers in Docker
    container: Option<ContainerConfig>,
}

impl SshRunner {
    /// Create a new SSH runner
    pub fn new(config: SshHostConfig, container: Option<ContainerConfig>) -> Self {
        Self {
            config,
            tunnel_port: None,
            container,
        }
    }

    /// Create a new SSH runner without container support (bare host)
    pub fn new_bare(config: SshHostConfig) -> Self {
        Self::new(config, None)
    }

    /// Create a new SSH runner with a tunnel port
    pub fn with_tunnel_port(config: SshHostConfig, tunnel_port: u16) -> Self {
        Self {
            config,
            tunnel_port: Some(tunnel_port),
            container: None,
        }
    }

    /// Set the tunnel port
    pub fn set_tunnel_port(&mut self, port: u16) {
        self.tunnel_port = Some(port);
    }

    /// Set the container configuration
    pub fn set_container(&mut self, container: Option<ContainerConfig>) {
        self.container = container;
    }

    /// Build the base SSH command
    fn build_ssh_cmd(&self) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.args(["-o", "BatchMode=yes"])
            .args(["-o", "StrictHostKeyChecking=accept-new"])
            .args(["-p", &self.config.port.to_string()]);

        if let Some(ref key) = self.config.ssh_key {
            let key_path = if key.starts_with("~") {
                dirs::home_dir()
                    .map(|h| h.join(key.strip_prefix("~/").unwrap_or(key)))
                    .unwrap_or_else(|| PathBuf::from(key))
            } else {
                PathBuf::from(key)
            };
            cmd.args(["-i", &key_path.to_string_lossy()]);
        }

        cmd.arg(&self.config.address);
        cmd
    }

    /// Run a command on the remote machine
    fn run_ssh_command(&self, script: &str, _timeout: Duration) -> RunnerResult<bool> {
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(script);

        debug!("Running SSH command on {}", self.config.address);

        let output = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(RunnerError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("SSH command failed: {}", stderr);
            return Ok(false);
        }

        Ok(true)
    }

    /// Spawn a background process on remote and return its PID
    fn spawn_remote_process(&self, script: &str) -> RunnerResult<u32> {
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(script);

        let output = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(RunnerError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RunnerError::SpawnFailed(stderr.to_string()));
        }

        // Script echoes PID
        let stdout = String::from_utf8_lossy(&output.stdout);
        let pid_str = stdout.trim().lines().last().unwrap_or("");

        pid_str
            .parse::<u32>()
            .map_err(|_| RunnerError::SpawnFailed("Invalid PID response".to_string()))
    }

    /// Spawn a docker container on remote and return container ID
    fn spawn_remote_docker(&self, script: &str) -> RunnerResult<String> {
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(script);

        let output = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(RunnerError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RunnerError::SpawnFailed(stderr.to_string()));
        }

        // Script echoes container ID
        let stdout = String::from_utf8_lossy(&output.stdout);
        let container_id = stdout.trim().lines().last().unwrap_or("").to_string();

        if container_id.is_empty() {
            return Err(RunnerError::SpawnFailed(
                "No container ID returned".to_string(),
            ));
        }

        Ok(container_id)
    }
}

#[async_trait]
impl Runner for SshRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        let work_dir = format!(
            "{}/{}/{}",
            self.config.work_base, config.run_name, config.worker_name
        );

        // Require coordinator_url for remote runners
        let coordinator_url = config.coordinator_url.as_ref().ok_or_else(|| {
            RunnerError::Config("coordinator_url required for SSH runner".to_string())
        })?;

        // Determine API URL for worker to reach coordinator
        let api_url = if let Some(tunnel_port) = self.tunnel_port {
            // Tunnel mode - worker accesses coordinator via localhost tunnel
            format!("http://127.0.0.1:{}", tunnel_port)
        } else {
            // Direct access to coordinator
            coordinator_url.clone()
        };

        info!(
            "Spawning SSH worker {} on {} (work_dir: {})",
            config.worker_name, self.config.address, work_dir
        );

        // Step 1: Setup Tailscale if auth key provided
        if let Some(ref authkey) = config.tailscale_authkey {
            info!(
                "Setting up Tailscale for {} on {}",
                config.worker_name, self.config.address
            );
            let tailscale_script = format!(
                r#"
# Install Tailscale if not present
if ! command -v tailscale &> /dev/null; then
    curl -fsSL https://tailscale.com/install.sh | sh
fi

# Connect to tailnet if not already connected
if ! tailscale status &> /dev/null; then
    sudo tailscale up --authkey={} --accept-routes --hostname=hirsel-{}
fi
"#,
                authkey, config.worker_name
            );

            if !self.run_ssh_command(&tailscale_script, Duration::from_secs(120))? {
                info!("Tailscale setup may have failed, continuing...");
            }
        }

        // Step 2: Download and setup project files from coordinator
        info!(
            "Setting up project files for {} on {}",
            config.worker_name, self.config.address
        );
        let setup_config = WorkerSetupConfig {
            coordinator_url: coordinator_url.clone(),
            run_name: config.run_name.clone(),
            worker_name: config.worker_name.clone(),
            work_dir: work_dir.clone(),
        };
        let files_setup_script = setup::generate_setup_script(&setup_config);

        if !self.run_ssh_command(&files_setup_script, Duration::from_secs(120))? {
            return Err(RunnerError::SetupFailed(format!(
                "Failed to setup workspace for {} on {}",
                config.worker_name, self.config.address
            )));
        }

        // Step 3: Start worker process (bare or in Docker)
        info!(
            "Starting worker {} on {}",
            config.worker_name, self.config.address
        );

        let agent_command_json =
            serde_json::to_string(&config.agent_command).unwrap_or_else(|_| "[]".to_string());
        let env_vars: Vec<(String, String)> = config.collect_env_vars().into_iter().collect();

        // Dispatch based on container config
        if let Some(ref container) = self.container {
            // Spawn in Docker container on remote
            let docker_script = setup::generate_docker_worker_script(
                &work_dir,
                &api_url,
                &config.run_name,
                &config.worker_name,
                &agent_command_json,
                config.is_leader,
                config.leader_name.as_deref(),
                config.teammates.as_deref(),
                &env_vars,
                &container.image,
                config.is_plan_task,
            );

            let container_id = self.spawn_remote_docker(&docker_script)?;

            info!(
                "Worker {} started on {} in Docker (container: {}, image: {})",
                config.worker_name,
                self.config.address,
                &container_id[..12.min(container_id.len())],
                container.image
            );

            Ok(SpawnResult {
                handle: WorkerHandle {
                    worker_name: config.worker_name.clone(),
                    runner_id: container_id,
                    runner_type: "ssh-docker".to_string(),
                },
                pid: None,
            })
        } else {
            // Spawn bare process
            let worker_script = setup::generate_worker_start_script(
                &work_dir,
                &api_url,
                &config.run_name,
                &config.worker_name,
                &agent_command_json,
                config.is_leader,
                config.leader_name.as_deref(),
                config.teammates.as_deref(),
                &env_vars,
                config.is_plan_task,
            );

            let pid = self.spawn_remote_process(&worker_script)?;

            info!(
                "Worker {} started on {} (PID: {})",
                config.worker_name, self.config.address, pid
            );

            Ok(SpawnResult {
                handle: WorkerHandle {
                    worker_name: config.worker_name.clone(),
                    runner_id: pid.to_string(),
                    runner_type: "ssh".to_string(),
                },
                pid: Some(pid),
            })
        }
    }

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        if handle.runner_type == "ssh-docker" {
            // Stop docker container on remote
            let container_id = &handle.runner_id;
            let mut cmd = self.build_ssh_cmd();
            cmd.arg(format!("docker stop -t 10 {} 2>/dev/null", container_id));

            match cmd.output() {
                Ok(output) => {
                    if output.status.success() {
                        info!(
                            "Stopped remote docker worker {} (container: {})",
                            handle.worker_name,
                            &container_id[..12.min(container_id.len())]
                        );
                    }
                    Ok(())
                }
                Err(e) => Err(RunnerError::StopFailed(e.to_string())),
            }
        } else {
            // Stop bare process by PID
            let pid: u32 = handle
                .runner_id
                .parse()
                .map_err(|_| RunnerError::StopFailed("Invalid PID".to_string()))?;

            let mut cmd = self.build_ssh_cmd();
            cmd.arg(format!("kill {} 2>/dev/null", pid));

            match cmd.output() {
                Ok(output) => {
                    if output.status.success() {
                        info!("Stopped remote worker {} (PID {})", handle.worker_name, pid);
                    }
                    Ok(())
                }
                Err(e) => Err(RunnerError::StopFailed(e.to_string())),
            }
        }
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
        if handle.runner_type == "ssh-docker" {
            // Check if docker container is running
            let container_id = &handle.runner_id;
            let mut cmd = self.build_ssh_cmd();
            cmd.arg(format!(
                "docker inspect -f '{{{{.State.Running}}}}' {} 2>/dev/null || echo false",
                container_id
            ));

            match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
                    stdout.trim() == "true"
                }
                Err(_) => false,
            }
        } else {
            // Check if process is alive by PID
            let pid: u32 = match handle.runner_id.parse() {
                Ok(p) => p,
                Err(_) => return false,
            };

            let mut cmd = self.build_ssh_cmd();
            cmd.arg(format!(
                "kill -0 {} 2>/dev/null && echo alive || echo dead",
                pid
            ));

            match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout.contains("alive")
                }
                Err(_) => false,
            }
        }
    }

    fn runner_type(&self) -> &'static str {
        if self.container.is_some() {
            "ssh-docker"
        } else {
            "ssh"
        }
    }

    async fn get_logs(&self, handle: &WorkerHandle, lines: usize) -> RunnerResult<String> {
        // For SSH runners, we need to fetch logs from the remote machine
        // The worker_name and runner_id can help us find the log file
        let work_dir = format!(
            "{}/{}",
            self.config.work_base,
            // We'd need the run_name here, which we don't have in the handle
            // For now, just return empty. In practice, logs would be streamed via coordinator
            handle.worker_name
        );

        let mut cmd = self.build_ssh_cmd();
        cmd.arg(format!(
            "tail -n {} {}/worker.log 2>/dev/null || echo 'No log file'",
            lines, work_dir
        ));

        match cmd.output() {
            Ok(output) => Ok(String::from_utf8_lossy(&output.stdout).to_string()),
            Err(e) => Err(RunnerError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_runner_type() {
        let config = SshHostConfig::default();
        let runner = SshRunner::new_bare(config);
        assert_eq!(runner.runner_type(), "ssh");
    }

    #[test]
    fn test_ssh_docker_runner_type() {
        let config = SshHostConfig::default();
        let runner = SshRunner::new(
            config,
            Some(ContainerConfig {
                image: "test:latest".to_string(),
            }),
        );
        assert_eq!(runner.runner_type(), "ssh-docker");
    }

    #[test]
    fn test_ssh_runner_with_tunnel_port() {
        let config = SshHostConfig {
            address: "user@example.com".to_string(),
            ..Default::default()
        };
        let runner = SshRunner::with_tunnel_port(config, 19800);
        assert_eq!(runner.tunnel_port, Some(19800));
    }
}
