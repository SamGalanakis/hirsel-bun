//! DevPod runner implementation - spawns workers in DevPod workspaces.
//!
//! DevPod abstracts container/VM providers (docker, ssh, kubernetes, aws, etc.)
//! behind a single CLI, providing a universal runner backend for Hirsel workers.

use async_trait::async_trait;
use std::process::{Command, Stdio};
use tracing::{debug, error, info, warn};

use super::{
    DevpodRunnerConfig, Runner, RunnerError, RunnerResult, SpawnResult, WorkerHandle,
    WorkerSpawnConfig,
};

/// DevPod runner - spawns workers in DevPod workspaces
pub struct DevpodRunner {
    config: DevpodRunnerConfig,
    /// Port on remote that tunnels back to coordinator
    tunnel_port: Option<u16>,
}

impl DevpodRunner {
    /// Create a new DevPod runner
    pub fn new(config: DevpodRunnerConfig) -> Self {
        Self {
            config,
            tunnel_port: None,
        }
    }

    /// Create a new DevPod runner with a tunnel port
    pub fn with_tunnel_port(config: DevpodRunnerConfig, tunnel_port: u16) -> Self {
        Self {
            config,
            tunnel_port: Some(tunnel_port),
        }
    }

    /// Set the tunnel port
    pub fn set_tunnel_port(&mut self, port: u16) {
        self.tunnel_port = Some(port);
    }

    /// Generate workspace name from run and worker names
    fn workspace_name(run_name: &str, worker_name: &str) -> String {
        // DevPod workspace names must be DNS-compatible (lowercase, alphanumeric, dashes)
        let name = format!("hirsel-{}-{}", run_name, worker_name);
        name.to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    }

    /// Check if tunnel is needed based on config and provider
    fn needs_tunnel(&self) -> bool {
        // Explicit config takes precedence
        if let Some(use_tunnel) = self.config.use_tunnel {
            return use_tunnel;
        }
        // Auto-detect: local Docker/Podman doesn't need tunnel
        // Everything else (SSH, K8s, cloud) assumes tunnel needed
        !matches!(self.config.provider.as_str(), "docker" | "podman")
    }

    /// Run a devpod command and return success status
    fn run_devpod_command(&self, args: &[&str]) -> RunnerResult<bool> {
        debug!("Running: devpod {}", args.join(" "));

        let output = Command::new("devpod")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    RunnerError::Config(
                        "DevPod CLI not found. Install from https://devpod.sh".to_string(),
                    )
                } else {
                    RunnerError::Io(e)
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("DevPod command failed: {}", stderr);
            return Ok(false);
        }

        Ok(true)
    }

    /// Run a command in the DevPod workspace via SSH
    fn run_ssh_command(&self, workspace: &str, script: &str) -> RunnerResult<bool> {
        debug!(
            "Running SSH command in workspace {}: {}",
            workspace,
            script.lines().next().unwrap_or("")
        );

        let output = Command::new("devpod")
            .args(["ssh", workspace, "--command", script])
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

    /// Run a command in the DevPod workspace and capture stdout
    fn run_ssh_command_output(&self, workspace: &str, script: &str) -> RunnerResult<String> {
        let output = Command::new("devpod")
            .args(["ssh", workspace, "--command", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(RunnerError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RunnerError::SpawnFailed(stderr.to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get the current hirsel version for matching worker versions
    fn get_hirsel_version() -> Option<String> {
        // Version is embedded at compile time
        let version = env!("CARGO_PKG_VERSION");
        if version.is_empty() {
            None
        } else {
            Some(format!("v{}", version))
        }
    }

    /// Determine agent type from agent command
    fn get_agent_type(agent_command: &[String]) -> &'static str {
        if agent_command.is_empty() {
            return "claude"; // Default
        }
        let cmd = agent_command[0].to_lowercase();
        if cmd.contains("claude") {
            "claude"
        } else if cmd.contains("codex") {
            "codex"
        } else {
            "claude" // Default to claude for now
        }
    }

    /// Build the setup script that downloads and runs setup-worker.sh
    fn build_setup_script(&self, agent_command: &[String]) -> String {
        let version = Self::get_hirsel_version().unwrap_or_default();
        let agent = Self::get_agent_type(agent_command);

        format!(
            r#"
set -e

echo "Setting up hirsel worker environment..."

# Download and run setup script from GitHub
export HIRSEL_AGENT="{agent}"
{version_export}

curl -fsSL https://raw.githubusercontent.com/SamGalanakis/hirsel/main/scripts/setup-worker.sh | bash

echo "Setup complete"
"#,
            agent = agent,
            version_export = if version.is_empty() {
                String::new()
            } else {
                format!("export HIRSEL_TAG=\"{}\"", version)
            },
        )
    }

    /// Build the script to setup hirsel files in the workspace
    /// (Reserved for future use when implementing file push from coordinator)
    #[allow(dead_code)]
    fn build_files_setup_script(&self, work_dir: &str, files_url: &str) -> String {
        format!(
            r#"
set -e
mkdir -p {work_dir}
cd {work_dir}

# Download and extract spec/eval files from coordinator
echo "Downloading files from {files_url}..."
curl -sS -H "Authorization: Bearer $HIRSEL_API_KEY" \
    "{files_url}" | tar -xzf -

# Create chats directory for synced files
mkdir -p chats

echo "Files ready at {work_dir}"
"#,
            work_dir = work_dir,
            files_url = files_url,
        )
    }

    /// Build the script to start the worker process
    fn build_worker_script(
        &self,
        config: &WorkerSpawnConfig,
        work_dir: &str,
        api_url: &str,
    ) -> String {
        // Build environment exports
        let mut env_exports = vec![
            format!(r#"export HIRSEL_RUN="{}""#, config.run_name),
            format!(r#"export HIRSEL_WORKER="{}""#, config.worker_name),
            format!(r#"export HIRSEL_API_URL="{}""#, api_url),
            "export HIRSEL_REMOTE=1".to_string(),
            "export ACP_PERMISSION_MODE=bypassPermissions".to_string(),
        ];

        // Add forwarded environment variables (API keys, etc.)
        if let Some(ref vars) = config.env_vars {
            for (key, value) in vars {
                // Escape single quotes in values
                let escaped_value = value.replace('\'', "'\\''");
                env_exports.push(format!("export {}='{}'", key, escaped_value));
            }
        }

        let env_block = env_exports.join("\n");

        // Build agent command as JSON for passing to hirsel __remote-worker
        let agent_command_json =
            serde_json::to_string(&config.agent_command).unwrap_or_else(|_| "[]".to_string());
        let agent_command_escaped = agent_command_json.replace('\'', "'\\''");

        // Build optional args
        let leader_arg = if config.is_leader { "--is-leader" } else { "" };
        let leader_name_arg = config
            .leader_name
            .as_ref()
            .map(|n| format!("--leader-name '{}'", n))
            .unwrap_or_default();
        let teammates_arg = config
            .teammates
            .as_ref()
            .map(|t| format!("--teammates '{}'", t.join(",")))
            .unwrap_or_default();

        // Spec path is in the work directory
        let spec_path = format!("{}/spec.md", work_dir);

        format!(
            r#"
cd {work_dir}

# Set environment
{env_block}

# Run worker in background using hirsel Rust binary
nohup hirsel __remote-worker \
    --api-url '{api_url}' \
    --run-name '{run_name}' \
    --worker-name '{worker_name}' \
    --work-dir '{work_dir}' \
    --spec '{spec_path}' \
    --agent-command '{agent_command}' \
    {leader_arg} {leader_name_arg} {teammates_arg} \
    > worker.log 2>&1 &
echo $!
"#,
            work_dir = work_dir,
            env_block = env_block,
            api_url = api_url,
            run_name = config.run_name,
            worker_name = config.worker_name,
            spec_path = spec_path,
            agent_command = agent_command_escaped,
            leader_arg = leader_arg,
            leader_name_arg = leader_name_arg,
            teammates_arg = teammates_arg,
        )
    }

    /// Copy spec and related files to the workspace
    fn copy_spec_files(
        &self,
        workspace: &str,
        config: &WorkerSpawnConfig,
        work_dir: &str,
    ) -> RunnerResult<()> {
        info!("Copying spec files to workspace {}", workspace);

        // Create work directory
        self.run_ssh_command(workspace, &format!("mkdir -p {}", work_dir))?;

        // Copy spec file
        let spec_content =
            std::fs::read_to_string(&config.spec_path).map_err(|e| RunnerError::Io(e))?;

        let escaped_spec = spec_content.replace('\'', "'\\''");
        self.run_ssh_command(
            workspace,
            &format!(
                "cat > {}/spec.md << 'HIRSEL_EOF'\n{}\nHIRSEL_EOF",
                work_dir, escaped_spec
            ),
        )?;

        // Copy eval file if it exists
        let eval_path = config
            .spec_path
            .with_extension("")
            .with_extension("eval.md");
        if eval_path.exists() {
            let eval_content =
                std::fs::read_to_string(&eval_path).map_err(|e| RunnerError::Io(e))?;
            let escaped_eval = eval_content.replace('\'', "'\\''");
            self.run_ssh_command(
                workspace,
                &format!(
                    "cat > {}/eval.md << 'HIRSEL_EOF'\n{}\nHIRSEL_EOF",
                    work_dir, escaped_eval
                ),
            )?;
        }

        Ok(())
    }
}

#[async_trait]
impl Runner for DevpodRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        let workspace = Self::workspace_name(&config.run_name, &config.worker_name);
        let work_dir = "/workspaces/hirsel";

        // Determine API URL
        let api_url = if let Some(ref coordinator_url) = config.coordinator_url {
            // Server mode - use direct URL to coordinator
            info!(
                "Using direct coordinator URL for {} in DevPod workspace {}",
                config.worker_name, workspace
            );
            coordinator_url.clone()
        } else if self.needs_tunnel() {
            // Tunnel mode - require tunnel_port
            let tunnel_port = self.tunnel_port.ok_or_else(|| {
                RunnerError::Config("Tunnel port not set for DevPod runner".to_string())
            })?;
            format!("http://127.0.0.1:{}", tunnel_port)
        } else {
            // Local Docker - can reach host
            let tunnel_port = self.tunnel_port.unwrap_or(19700);
            format!("http://host.docker.internal:{}", tunnel_port)
        };

        // Step 1: Create DevPod workspace
        info!(
            "Creating DevPod workspace {} with provider {}",
            workspace, self.config.provider
        );

        let mut up_args = vec![
            "up".to_string(),
            config.work_dir.to_string_lossy().to_string(),
            "--id".to_string(),
            workspace.clone(),
            "--provider".to_string(),
            self.config.provider.clone(),
            "--ide".to_string(),
            "none".to_string(), // Don't open an IDE, we just want the container
        ];

        // Add image if specified
        if let Some(image) = self
            .config
            .prebuild_image
            .as_ref()
            .or(self.config.image.as_ref())
        {
            up_args.push("--devcontainer-image".to_string());
            up_args.push(image.to_string());
        }

        // Add provider options
        for (key, value) in &self.config.provider_options {
            up_args.push("-o".to_string());
            up_args.push(format!("{}={}", key, value));
        }

        let up_args_refs: Vec<&str> = up_args.iter().map(|s| s.as_str()).collect();
        if !self.run_devpod_command(&up_args_refs)? {
            return Err(RunnerError::SetupFailed(format!(
                "Failed to create DevPod workspace {}",
                workspace
            )));
        }

        // Step 2: Setup tools (if no prebuild image)
        if self.config.prebuild_image.is_none() {
            info!("Installing tools in workspace {}", workspace);

            // Run setup script (downloads hirsel + agent CLI from GitHub)
            let setup_script = self.build_setup_script(&config.agent_command);
            if !self.run_ssh_command(&workspace, &setup_script)? {
                warn!("Setup script had issues, continuing anyway...");
            }
        }

        // Step 3: Copy spec files
        self.copy_spec_files(&workspace, config, work_dir)?;

        // Step 4: Start worker process
        info!(
            "Starting worker {} in workspace {}",
            config.worker_name, workspace
        );

        let worker_script = self.build_worker_script(config, work_dir, &api_url);

        // If tunnel is needed, start with SSH reverse tunnel
        let pid = if self.needs_tunnel() && config.coordinator_url.is_none() {
            let tunnel_port = self.tunnel_port.unwrap_or(19700);

            // Start SSH with reverse tunnel in background
            info!(
                "Starting worker with SSH reverse tunnel on port {}",
                tunnel_port
            );

            // We need to use raw ssh through devpod ssh
            // The reverse tunnel maps remote:tunnel_port to local:tunnel_port
            let _tunnel_script = format!(
                r#"
# Worker script with environment
{}
"#,
                worker_script
            );

            // For tunnel mode, we spawn devpod ssh with -R flag
            // This is a bit tricky as devpod ssh doesn't directly support -R
            // We'll run the worker without tunnel for now and document the limitation
            warn!("SSH tunnel mode requires manual tunnel setup. Running without tunnel.");

            let output = self.run_ssh_command_output(&workspace, &worker_script)?;
            let pid_str = output.trim().lines().last().unwrap_or("");
            pid_str.parse::<u32>().map_err(|_| {
                RunnerError::SpawnFailed(format!("Invalid PID response: {}", pid_str))
            })?
        } else {
            let output = self.run_ssh_command_output(&workspace, &worker_script)?;
            let pid_str = output.trim().lines().last().unwrap_or("");
            pid_str.parse::<u32>().map_err(|_| {
                RunnerError::SpawnFailed(format!("Invalid PID response: {}", pid_str))
            })?
        };

        info!(
            "Worker {} started in DevPod workspace {} (PID: {})",
            config.worker_name, workspace, pid
        );

        Ok(SpawnResult {
            handle: WorkerHandle {
                worker_name: config.worker_name.clone(),
                runner_id: format!("{}:{}", workspace, pid),
                runner_type: "devpod".to_string(),
            },
            pid: Some(pid),
        })
    }

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        // Parse workspace:pid from runner_id
        let parts: Vec<&str> = handle.runner_id.split(':').collect();
        if parts.len() != 2 {
            return Err(RunnerError::StopFailed(
                "Invalid runner_id format".to_string(),
            ));
        }

        let workspace = parts[0];
        let pid: u32 = parts[1]
            .parse()
            .map_err(|_| RunnerError::StopFailed("Invalid PID".to_string()))?;

        // Kill the process
        let kill_result =
            self.run_ssh_command(workspace, &format!("kill {} 2>/dev/null || true", pid));

        match kill_result {
            Ok(_) => {
                info!(
                    "Stopped worker {} in workspace {} (PID {})",
                    handle.worker_name, workspace, pid
                );
            }
            Err(e) => {
                // Process might already be dead
                debug!("Kill command failed (process may be dead): {}", e);
            }
        }

        // Delete the workspace
        info!("Deleting DevPod workspace {}", workspace);
        self.run_devpod_command(&["delete", workspace, "--force"])?;

        Ok(())
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
        // Parse workspace:pid from runner_id
        let parts: Vec<&str> = handle.runner_id.split(':').collect();
        if parts.len() != 2 {
            return false;
        }

        let workspace = parts[0];
        let pid: u32 = match parts[1].parse() {
            Ok(p) => p,
            Err(_) => return false,
        };

        // Check if process is alive
        let output = self.run_ssh_command_output(
            workspace,
            &format!("kill -0 {} 2>/dev/null && echo alive || echo dead", pid),
        );

        match output {
            Ok(out) => out.contains("alive"),
            Err(_) => false,
        }
    }

    fn runner_type(&self) -> &'static str {
        "devpod"
    }

    async fn cleanup(&self) -> RunnerResult<()> {
        // Note: Individual workspace cleanup happens in stop()
        // This is called when the entire run completes
        Ok(())
    }

    async fn get_logs(&self, handle: &WorkerHandle, lines: usize) -> RunnerResult<String> {
        // Parse workspace:pid from runner_id
        let parts: Vec<&str> = handle.runner_id.split(':').collect();
        if parts.len() != 2 {
            return Err(RunnerError::WorkerNotFound(
                "Invalid runner_id format".to_string(),
            ));
        }

        let workspace = parts[0];

        self.run_ssh_command_output(
            workspace,
            &format!(
                "tail -n {} /workspaces/hirsel/worker.log 2>/dev/null || echo 'No log file'",
                lines
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_devpod_runner_type() {
        let config = DevpodRunnerConfig::default();
        let runner = DevpodRunner::new(config);
        assert_eq!(runner.runner_type(), "devpod");
    }

    #[test]
    fn test_workspace_name_generation() {
        assert_eq!(
            DevpodRunner::workspace_name("my-run", "worker-1"),
            "hirsel-my-run-worker-1"
        );
        assert_eq!(
            DevpodRunner::workspace_name("Test Run", "Worker 1"),
            "hirsel-test-run-worker-1"
        );
    }

    #[test]
    fn test_needs_tunnel_auto_detect() {
        let mut config = DevpodRunnerConfig::default();
        config.provider = "docker".to_string();
        let runner = DevpodRunner::new(config.clone());
        assert!(!runner.needs_tunnel());

        config.provider = "ssh".to_string();
        let runner = DevpodRunner::new(config.clone());
        assert!(runner.needs_tunnel());

        config.provider = "kubernetes".to_string();
        let runner = DevpodRunner::new(config.clone());
        assert!(runner.needs_tunnel());
    }

    #[test]
    fn test_needs_tunnel_explicit() {
        let mut config = DevpodRunnerConfig::default();
        config.provider = "docker".to_string();
        config.use_tunnel = Some(true);
        let runner = DevpodRunner::new(config);
        assert!(runner.needs_tunnel());
    }
}
