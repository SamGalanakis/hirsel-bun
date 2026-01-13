//! Remote worker spawning via SSH
//!
//! Handles setting up and spawning worker processes on remote machines.
//! Remote workers connect back to the coordinator via a reverse SSH tunnel.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, error, info};

/// Errors that can occur during remote operations
#[derive(Debug, Error)]
pub enum RemoteError {
    #[error("SSH command failed: {0}")]
    SshFailed(String),

    #[error("SSH command timed out")]
    Timeout,

    #[error("Failed to spawn remote process: {0}")]
    SpawnFailed(String),

    #[error("Invalid remote PID response")]
    InvalidPid,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type RemoteResult<T> = Result<T, RemoteError>;

/// Configuration for a remote machine
#[derive(Debug, Clone)]
pub struct RemoteConfig {
    /// SSH host (e.g., "user@server.example.com")
    pub host: String,
    /// Path to Python on remote
    pub python_path: String,
    /// Base directory for work on remote
    pub work_base: String,
    /// Path to SSH private key (optional)
    pub ssh_key: Option<PathBuf>,
    /// SSH port
    pub ssh_port: u16,
    /// Display name for workers (defaults to host)
    pub location: Option<String>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            python_path: "python3".to_string(),
            work_base: "/tmp/hirsel-remote".to_string(),
            ssh_key: None,
            ssh_port: 22,
            location: None,
        }
    }
}

impl RemoteConfig {
    /// Create a new RemoteConfig for a host
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            ..Default::default()
        }
    }

    /// Get the location name for display (falls back to host)
    pub fn display_location(&self) -> &str {
        self.location.as_deref().unwrap_or(&self.host)
    }

    /// Set the SSH key path
    pub fn with_ssh_key(mut self, key: impl Into<PathBuf>) -> Self {
        self.ssh_key = Some(key.into());
        self
    }

    /// Set the SSH port
    pub fn with_port(mut self, port: u16) -> Self {
        self.ssh_port = port;
        self
    }

    /// Set the Python path
    pub fn with_python(mut self, path: impl Into<String>) -> Self {
        self.python_path = path.into();
        self
    }

    /// Set the work base directory
    pub fn with_work_base(mut self, path: impl Into<String>) -> Self {
        self.work_base = path.into();
        self
    }

    /// Set the display location
    pub fn with_location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }
}

/// Spawns worker processes on remote machines via SSH
#[derive(Debug, Clone)]
pub struct RemoteWorkerSpawner {
    /// Remote machine configuration
    pub config: RemoteConfig,
    /// Port on remote that tunnels back to coordinator
    pub tunnel_port: u16,
}

impl RemoteWorkerSpawner {
    /// Create a new spawner for a remote machine
    pub fn new(config: RemoteConfig, tunnel_port: u16) -> Self {
        Self { config, tunnel_port }
    }

    /// Spawn a worker process on the remote machine
    ///
    /// # Arguments
    /// * `run_name` - The hirsel run identifier
    /// * `worker_name` - Unique worker name
    /// * `project_url` - Git HTTP URL to clone from coordinator's staging repo
    /// * `agent_command` - Command to run the agent (e.g., ["claude-code-acp"])
    /// * `env_vars` - Environment variables to forward (e.g., API keys)
    ///
    /// # Returns
    /// Remote PID on success
    pub fn spawn_worker(
        &self,
        run_name: &str,
        worker_name: &str,
        project_url: &str,
        agent_command: &[String],
        env_vars: Option<&HashMap<String, String>>,
    ) -> RemoteResult<u32> {
        let work_dir = format!("{}/{}/{}", self.config.work_base, run_name, worker_name);

        // Step 1: Setup remote workspace
        info!(
            "Setting up remote workspace for {} on {}",
            worker_name, self.config.host
        );
        let setup_script = self.build_setup_script(&work_dir, project_url);

        if !self.run_ssh_command(&setup_script, Duration::from_secs(120))? {
            return Err(RemoteError::SshFailed(format!(
                "Failed to setup workspace for {}",
                worker_name
            )));
        }

        // Step 2: Start worker process
        info!("Starting worker {} on {}", worker_name, self.config.host);
        let worker_script =
            self.build_worker_script(run_name, worker_name, &work_dir, agent_command, env_vars);

        let pid = self.spawn_remote_process(&worker_script)?;
        info!(
            "Worker {} started on {} (PID: {})",
            worker_name, self.config.host, pid
        );

        Ok(pid)
    }

    /// Build the script to setup the remote workspace
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

    /// Build the script to start the worker process
    fn build_worker_script(
        &self,
        run_name: &str,
        worker_name: &str,
        work_dir: &str,
        agent_command: &[String],
        env_vars: Option<&HashMap<String, String>>,
    ) -> String {
        let api_url = format!("http://127.0.0.1:{}", self.tunnel_port);

        // Build environment exports
        let mut env_exports = vec![
            format!(r#"export HIRSEL_RUN="{}""#, run_name),
            format!(r#"export HIRSEL_WORKER="{}""#, worker_name),
            format!(r#"export HIRSEL_API_URL="{}""#, api_url),
            "export HIRSEL_REMOTE=1".to_string(),
            "export ACP_PERMISSION_MODE=bypassPermissions".to_string(),
        ];

        // Add forwarded environment variables (API keys, etc.)
        if let Some(vars) = env_vars {
            for (key, value) in vars {
                // Escape single quotes in values
                let escaped_value = value.replace('\'', "'\\''");
                env_exports.push(format!("export {}='{}'", key, escaped_value));
            }
        }

        let env_block = env_exports.join("\n");
        let _cmd_str = agent_command
            .iter()
            .map(|s| shell_escape(s))
            .collect::<Vec<_>>()
            .join(" ");

        format!(
            r#"
cd {work_dir}

# Set environment
{env_block}

# Run worker in background
nohup {python} -m hirsel.remote_worker > worker.log 2>&1 &
echo $!
"#,
            work_dir = work_dir,
            env_block = env_block,
            python = self.config.python_path
        )
    }

    /// Run a command on the remote machine
    fn run_ssh_command(&self, script: &str, _timeout: Duration) -> RemoteResult<bool> {
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(script);

        debug!("Running SSH command on {}", self.config.host);

        let output = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("SSH command failed: {}", stderr);
            return Ok(false);
        }

        Ok(true)
    }

    /// Spawn a background process on remote and return its PID
    fn spawn_remote_process(&self, script: &str) -> RemoteResult<u32> {
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(script);

        let output = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RemoteError::SpawnFailed(stderr.to_string()));
        }

        // Script echoes PID
        let stdout = String::from_utf8_lossy(&output.stdout);
        let pid_str = stdout.trim().lines().last().unwrap_or("");

        pid_str
            .parse::<u32>()
            .map_err(|_| RemoteError::InvalidPid)
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
                    .unwrap_or_else(|| key.clone())
            } else {
                key.clone()
            };
            cmd.args(["-i", &key_path.to_string_lossy()]);
        }

        cmd.arg(&self.config.host);
        cmd
    }

    /// Check if a remote worker process is still running
    pub fn check_worker_alive(&self, pid: u32) -> bool {
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

    /// Kill a remote worker process
    pub fn kill_worker(&self, pid: u32) -> bool {
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(format!("kill {} 2>/dev/null", pid));

        match cmd.output() {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// Get the last N lines of a worker's log file
    pub fn get_worker_log(&self, run_name: &str, worker_name: &str, lines: usize) -> String {
        let work_dir = format!("{}/{}/{}", self.config.work_base, run_name, worker_name);
        let mut cmd = self.build_ssh_cmd();
        cmd.arg(format!(
            "tail -n {} {}/worker.log 2>/dev/null || echo 'No log file'",
            lines, work_dir
        ));

        match cmd.output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
            Err(e) => format!("Error getting log: {}", e),
        }
    }
}

/// Escape a string for shell use
fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

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
    fn test_remote_config_new() {
        let config = RemoteConfig::new("user@example.com");
        assert_eq!(config.host, "user@example.com");
        assert_eq!(config.python_path, "python3");
        assert_eq!(config.ssh_port, 22);
        assert!(config.ssh_key.is_none());
    }

    #[test]
    fn test_remote_config_builder() {
        let config = RemoteConfig::new("user@example.com")
            .with_port(2222)
            .with_python("/usr/local/bin/python3.11")
            .with_ssh_key("~/.ssh/remote_key")
            .with_work_base("/home/user/hirsel")
            .with_location("build-server");

        assert_eq!(config.ssh_port, 2222);
        assert_eq!(config.python_path, "/usr/local/bin/python3.11");
        assert_eq!(
            config.ssh_key,
            Some(PathBuf::from("~/.ssh/remote_key"))
        );
        assert_eq!(config.work_base, "/home/user/hirsel");
        assert_eq!(config.display_location(), "build-server");
    }

    #[test]
    fn test_display_location_fallback() {
        let config = RemoteConfig::new("user@example.com");
        assert_eq!(config.display_location(), "user@example.com");

        let config_with_location = config.with_location("my-server");
        assert_eq!(config_with_location.display_location(), "my-server");
    }

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
    fn test_parse_remote_spec_with_port_in_host() {
        // SSH URL format with port but no worker count
        let (host, count) = parse_remote_spec("user@server.com:22");
        // This will be parsed as count=22, which is probably not intended
        // but matches Python behavior
        assert_eq!(host, "user@server.com");
        assert_eq!(count, 22);
    }

    #[test]
    fn test_parse_remote_specs() {
        let specs = vec![
            "user@server1.com:2".to_string(),
            "user@server2.com".to_string(),
            "user@server3.com:5".to_string(),
        ];

        let parsed = parse_remote_specs(&specs);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], ("user@server1.com".to_string(), 2));
        assert_eq!(parsed[1], ("user@server2.com".to_string(), 1));
        assert_eq!(parsed[2], ("user@server3.com".to_string(), 5));
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
        assert_eq!(shell_escape("path/to/file"), "path/to/file");
        assert_eq!(shell_escape("special$char"), "'special$char'");
    }

    #[test]
    fn test_spawner_creation() {
        let config = RemoteConfig::new("user@example.com");
        let spawner = RemoteWorkerSpawner::new(config, 8080);

        assert_eq!(spawner.tunnel_port, 8080);
        assert_eq!(spawner.config.host, "user@example.com");
    }
}
