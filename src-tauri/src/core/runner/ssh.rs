//! SSH runner implementation - spawns workers on remote machines via SSH.
//!
//! This runner connects to remote machines via SSH and spawns worker processes.
//! It uses reverse SSH tunnels to allow workers to connect back to the coordinator.

use async_trait::async_trait;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{debug, error, info};

use super::{
    Runner, RunnerError, RunnerResult, SpawnResult, SshRunnerConfig, WorkerHandle,
    WorkerSpawnConfig,
};

/// SSH runner - spawns workers on remote machines via SSH
pub struct SshRunner {
    config: SshRunnerConfig,
    /// Port on remote that tunnels back to coordinator
    tunnel_port: Option<u16>,
}

impl SshRunner {
    /// Create a new SSH runner
    pub fn new(config: SshRunnerConfig) -> Self {
        Self {
            config,
            tunnel_port: None,
        }
    }

    /// Create a new SSH runner with a tunnel port
    pub fn with_tunnel_port(config: SshRunnerConfig, tunnel_port: u16) -> Self {
        Self {
            config,
            tunnel_port: Some(tunnel_port),
        }
    }

    /// Set the tunnel port
    pub fn set_tunnel_port(&mut self, port: u16) {
        self.tunnel_port = Some(port);
    }

    /// Build the base SSH command
    fn build_ssh_cmd(&self) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.args(["-o", "BatchMode=yes"])
            .args(["-o", "StrictHostKeyChecking=accept-new"])
            .args(["-p", &self.config.ssh_port.to_string()]);

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

        cmd.arg(&self.config.host);
        cmd
    }

    /// Run a command on the remote machine
    fn run_ssh_command(&self, script: &str, _timeout: Duration) -> RunnerResult<bool> {
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(script);

        debug!("Running SSH command on {}", self.config.host);

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

    /// Build the script to setup the remote workspace via git clone
    fn build_setup_script(&self, work_dir: &str, git_http_url: &str) -> String {
        format!(
            r#"
set -e
mkdir -p {work_dir}
cd {work_dir}

# Clone or update from coordinator's staging repo via HTTP
if [ -d .git ]; then
    git fetch origin
    git reset --hard origin/HEAD
else
    git clone {git_http_url} .
fi

# Create chats directory for synced files
mkdir -p chats

echo "Workspace ready at {work_dir}"
"#,
            work_dir = work_dir,
            git_http_url = git_http_url
        )
    }

    /// Build the script to setup the remote workspace via tarball download
    fn build_tarball_setup_script(&self, work_dir: &str, files_url: &str) -> String {
        format!(
            r#"
set -e
mkdir -p {work_dir}
cd {work_dir}

# Download and extract project tarball from coordinator
echo "Downloading project files from {files_url}..."
curl -sS -H "Authorization: Bearer $HIRSEL_API_KEY" \
    "{files_url}" | tar -xzf -

# Initialize git repo for worker to use
if [ ! -d .git ]; then
    git init
    git add .
    git commit -m "Initial import from server"
fi

# Create chats directory for synced files
mkdir -p chats

echo "Workspace ready at {work_dir}"
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
}

#[async_trait]
impl Runner for SshRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        let work_dir = format!(
            "{}/{}/{}",
            self.config.work_base, config.run_name, config.worker_name
        );

        // Determine API URL - prefer coordinator_url from config (server mode),
        // fall back to tunnel port (tunnel mode)
        let api_url = if let Some(ref coordinator_url) = config.coordinator_url {
            // Server mode - use direct URL to coordinator
            info!(
                "Using direct coordinator URL for {} on {}",
                config.worker_name, self.config.host
            );
            coordinator_url.clone()
        } else {
            // Tunnel mode - require tunnel_port
            let tunnel_port = self.tunnel_port.ok_or_else(|| {
                RunnerError::Config("Tunnel port not set for SSH runner".to_string())
            })?;
            format!("http://127.0.0.1:{}", tunnel_port)
        };

        // Get git URL from config or construct from API URL
        let git_url = config.project_url.clone().unwrap_or_else(|| {
            if let Some(ref coordinator_url) = config.coordinator_url {
                // Server mode - get files from coordinator API
                format!("{}/api/runs/{}/files", coordinator_url, config.run_name)
            } else if let Some(tunnel_port) = self.tunnel_port {
                // Tunnel mode - local git server
                format!("http://127.0.0.1:{}/git", tunnel_port)
            } else {
                // Fallback
                format!("{}/git", api_url)
            }
        });

        // If using server mode with tarball download, use different setup script
        let uses_tarball = config.coordinator_url.is_some() && config.project_url.is_none();

        // Step 1: Setup remote workspace
        info!(
            "Setting up remote workspace for {} on {}",
            config.worker_name, self.config.host
        );
        let setup_script = if uses_tarball {
            self.build_tarball_setup_script(&work_dir, &git_url)
        } else {
            self.build_setup_script(&work_dir, &git_url)
        };

        if !self.run_ssh_command(&setup_script, Duration::from_secs(120))? {
            return Err(RunnerError::SetupFailed(format!(
                "Failed to setup workspace for {} on {}",
                config.worker_name, self.config.host
            )));
        }

        // Step 2: Start worker process
        info!(
            "Starting worker {} on {}",
            config.worker_name, self.config.host
        );
        let worker_script = self.build_worker_script(config, &work_dir, &api_url);

        let pid = self.spawn_remote_process(&worker_script)?;

        info!(
            "Worker {} started on {} (PID: {})",
            config.worker_name, self.config.host, pid
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

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
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
                    Ok(())
                } else {
                    // Process might already be dead
                    Ok(())
                }
            }
            Err(e) => Err(RunnerError::StopFailed(e.to_string())),
        }
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
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

    fn runner_type(&self) -> &'static str {
        "ssh"
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
        let config = SshRunnerConfig::default();
        let runner = SshRunner::new(config);
        assert_eq!(runner.runner_type(), "ssh");
    }

    #[test]
    fn test_ssh_runner_with_tunnel_port() {
        let config = SshRunnerConfig {
            host: "user@example.com".to_string(),
            ..Default::default()
        };
        let runner = SshRunner::with_tunnel_port(config, 19800);
        assert_eq!(runner.tunnel_port, Some(19800));
    }
}
