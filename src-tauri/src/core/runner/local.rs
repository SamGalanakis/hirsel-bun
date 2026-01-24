//! Local runner implementation - spawns workers as local processes.
//!
//! This runner spawns worker processes on the local machine using the
//! hirsel __worker-run command as a detached subprocess. Optionally supports
//! running workers inside Docker containers.

use async_trait::async_trait;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use tracing::{debug, info, warn};

use super::{
    ContainerConfig, Runner, RunnerError, RunnerResult, SpawnResult, WorkerHandle,
    WorkerSpawnConfig,
};

/// Local runner - spawns workers as local processes (with optional Docker support)
pub struct LocalRunner {
    /// Optional container configuration for running workers in Docker
    container: Option<ContainerConfig>,
}

impl LocalRunner {
    /// Create a new local runner
    pub fn new(container: Option<ContainerConfig>) -> Self {
        Self { container }
    }

    /// Create a new local runner without container support
    pub fn new_bare() -> Self {
        Self { container: None }
    }

    /// Check if a process is still alive
    pub fn is_pid_alive(pid: u32) -> bool {
        // PID 0 is the kernel scheduler, never a valid user process
        // Also, kill(0, sig) sends to the process group, not PID 0
        if pid == 0 {
            return false;
        }

        #[cfg(unix)]
        {
            // Send signal 0 to check if process exists
            unsafe { libc::kill(pid as i32, 0) == 0 }
        }

        #[cfg(not(unix))]
        {
            // On non-Unix platforms, try to read /proc/{pid}
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
    }

    /// Kill a process group
    #[cfg(unix)]
    pub fn kill_process_group(pid: u32, signal: i32) {
        unsafe {
            libc::kill(-(pid as i32), signal);
        }
    }

    /// Spawn a worker directly on the host (no container).
    async fn spawn_bare(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        // Build environment for worker subprocess
        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.insert(
            "ACP_PERMISSION_MODE".to_string(),
            "bypassPermissions".to_string(),
        );
        env.insert("HIRSEL_WORKER_SUBPROCESS".to_string(), "1".to_string());
        env.insert("HIRSEL_RUN".to_string(), config.run_name.clone());
        env.insert("HIRSEL_WORKER".to_string(), config.worker_name.clone());

        // Set agent command for lifecycle manager in worker subprocess
        if let Ok(agent_cmd_json) = serde_json::to_string(&config.agent_command) {
            env.insert("HIRSEL_AGENT_COMMAND".to_string(), agent_cmd_json);
        }

        // Merge any additional env vars from config
        if let Some(ref extra_env) = config.env_vars {
            for (k, v) in extra_env {
                env.insert(k.clone(), v.clone());
            }
        }

        // Get the current executable path
        let hirsel_exe = std::env::current_exe()
            .map_err(|e| RunnerError::SpawnFailed(format!("Failed to get current exe: {}", e)))?;

        // Build args for hirsel __worker-run
        let agent_command_json = serde_json::to_string(&config.agent_command).map_err(|e| {
            RunnerError::SpawnFailed(format!("Failed to serialize agent command: {}", e))
        })?;

        let mut args = vec![
            "__worker-run".to_string(),
            "--run".to_string(),
            config.run_name.clone(),
            "--worker".to_string(),
            config.worker_name.clone(),
            "--work-dir".to_string(),
            config.work_dir.to_string_lossy().to_string(),
            "--run-dir".to_string(),
            config.run_dir.to_string_lossy().to_string(),
            "--spec".to_string(),
            config.spec_path.to_string_lossy().to_string(),
            "--agent-command".to_string(),
            agent_command_json,
        ];

        if config.is_leader {
            args.push("--is-leader".to_string());
        }

        if let Some(ref leader) = config.leader_name {
            args.push("--leader-name".to_string());
            args.push(leader.clone());
        }

        if let Some(ref teammates) = config.teammates {
            if !teammates.is_empty() {
                args.push("--teammates".to_string());
                args.push(teammates.join(","));
            }
        }

        if let Some(ref session_id) = config.resume_session_id {
            args.push("--resume-session-id".to_string());
            args.push(session_id.clone());
        }

        // Spawn the detached subprocess in its own process group
        // This allows us to kill the entire process tree when stopping workers
        let mut cmd = Command::new(&hirsel_exe);
        cmd.args(&args)
            .current_dir(&config.work_dir)
            .envs(&env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit()); // DEBUG: inherit stderr to see errors

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Create a new process group with the child's PID as the group leader
            // This ensures all descendant processes (hirsel __acp-bridge, claude) are in the same group
            cmd.process_group(0);
        }

        let child = cmd
            .spawn()
            .map_err(|e| RunnerError::SpawnFailed(e.to_string()))?;

        let pid = child.id();

        info!(
            "Spawned local worker {} (hirsel __worker-run, PID {})",
            config.worker_name, pid
        );

        Ok(SpawnResult {
            handle: WorkerHandle {
                worker_name: config.worker_name.clone(),
                runner_id: pid.to_string(),
                runner_type: "local".to_string(),
            },
            pid: Some(pid),
        })
    }

    /// Spawn a worker inside a Docker container.
    ///
    /// The container will:
    /// 1. Use mounted hirsel binary (or download from GitHub if not mounted)
    /// 2. Install Claude CLI
    /// 3. Run the worker
    ///
    /// Mounts:
    /// - /work: work directory (project files)
    /// - /hirsel: run directory (spec, db, etc.)
    /// - /usr/local/bin/hirsel: hirsel binary (from host)
    /// - /tmp/home/.claude: agent session directory (for pause/resume)
    async fn spawn_docker(
        &self,
        config: &WorkerSpawnConfig,
        container: &ContainerConfig,
    ) -> RunnerResult<SpawnResult> {
        let work_dir_str = config.work_dir.to_string_lossy().to_string();
        let run_dir_str = config.run_dir.to_string_lossy().to_string();

        // Create session directory on host for persistent agent sessions
        // This allows Claude sessions to persist across container restarts
        let session_path = config
            .run_dir
            .join("agent-sessions")
            .join(&config.worker_name);
        std::fs::create_dir_all(&session_path).map_err(|e| {
            RunnerError::SpawnFailed(format!(
                "Failed to create session directory {}: {}",
                session_path.display(),
                e
            ))
        })?;
        let session_path_str = session_path.to_string_lossy().to_string();

        // Validate work_dir is not empty - empty work_dir causes Docker to create
        // an anonymous volume which gets deleted when container exits (--rm)
        if work_dir_str.is_empty() {
            return Err(RunnerError::SpawnFailed(
                "work_dir is empty - cannot spawn Docker worker without valid work directory"
                    .into(),
            ));
        }

        // Validate work_dir exists on host
        if !config.work_dir.exists() {
            return Err(RunnerError::SpawnFailed(format!(
                "work_dir does not exist: {}",
                work_dir_str
            )));
        }

        // Get coordinator version for worker binary compatibility
        let version = crate::version::VERSION;

        // Build agent command JSON
        let agent_command_json = serde_json::to_string(&config.agent_command).map_err(|e| {
            RunnerError::SpawnFailed(format!("Failed to serialize agent command: {}", e))
        })?;

        // Build worker args
        let mut worker_cmd_parts = vec![
            "hirsel __worker-run".to_string(),
            format!("--run '{}'", config.run_name),
            format!("--worker '{}'", config.worker_name),
            "--work-dir '/work'".to_string(),
            "--run-dir '/hirsel'".to_string(),
            "--spec '/hirsel/spec.md'".to_string(),
            format!(
                "--agent-command '{}'",
                agent_command_json.replace('\'', "'\\''")
            ),
        ];

        if config.is_leader {
            worker_cmd_parts.push("--is-leader".to_string());
        }

        if let Some(ref leader) = config.leader_name {
            worker_cmd_parts.push(format!("--leader-name '{}'", leader));
        }

        if let Some(ref teammates) = config.teammates {
            if !teammates.is_empty() {
                worker_cmd_parts.push(format!("--teammates '{}'", teammates.join(",")));
            }
        }

        if let Some(ref session_id) = config.resume_session_id {
            worker_cmd_parts.push(format!("--resume-session-id '{}'", session_id));
        }

        // Pass coordinator URL so worker can communicate status
        if let Some(ref url) = config.coordinator_url {
            worker_cmd_parts.push(format!("--api-url '{}'", url));
        }

        let worker_cmd = worker_cmd_parts.join(" ");

        // Build init script that sets up the environment and runs the worker
        // Downloads hirsel worker binary from GitHub releases and installs claude CLI
        // Note: Container runs as non-root user, so we install to /tmp and update PATH
        // Note: $HOME/.claude is mounted from host for session persistence
        let init_script = format!(
            r#"#!/bin/sh
# Hirsel Docker Worker Init Script
# All output goes to /hirsel/worker-init.log AND stdout

LOG="/hirsel/worker-init.log"

# Initialize log file
: > "$LOG" 2>/dev/null || LOG="/tmp/worker-init.log"

# Wrap entire script to capture all output
{{

echo "=== Docker Worker Setup ==="
echo "Started at: $(date -Iseconds 2>/dev/null || date)"
echo "Container: $(hostname)"
echo "User: $(id)"
echo "Log file: $LOG"
echo ""

# Exit on error
set -e

# =============================================================================
# Helper functions (inspired by rustup/nvm install scripts)
# =============================================================================

has_cmd() {{
    command -v "$1" >/dev/null 2>&1
}}

need_cmd() {{
    if ! has_cmd "$1"; then
        echo "ERROR: Required command '$1' not found" >&2
        exit 1
    fi
}}

# Download with curl or wget fallback
download() {{
    local url="$1"
    local output="${{2:-}}"

    if has_cmd curl; then
        if [ -n "$output" ]; then
            curl -fsSL "$url" -o "$output"
        else
            curl -fsSL "$url"
        fi
    elif has_cmd wget; then
        if [ -n "$output" ]; then
            wget -qO "$output" "$url"
        else
            wget -qO- "$url"
        fi
    else
        echo "ERROR: Neither curl nor wget available" >&2
        exit 1
    fi
}}

ensure_downloader() {{
    if has_cmd curl || has_cmd wget; then
        return 0
    fi

    echo "No downloader (curl/wget) found, attempting to install..."

    # Check if we can install packages (need root or sudo)
    CAN_INSTALL=false
    if [ "$(id -u)" = "0" ]; then
        CAN_INSTALL=true
    elif has_cmd sudo; then
        CAN_INSTALL=true
        APT_PREFIX="sudo"
    fi

    if [ "$CAN_INSTALL" = "false" ]; then
        echo "" >&2
        echo "ERROR: Container image missing curl/wget and running as non-root" >&2
        echo "" >&2
        echo "Please use a container image with curl or wget. Recommended:" >&2
        echo "  [runners.docker.container]" >&2
        echo "  image = \"alpine:latest\"   # Has wget, lightweight" >&2
        echo "" >&2
        echo "Alternative images: curlimages/curl, bitnami/minideb" >&2
        exit 1
    fi

    # Try to install curl
    if has_cmd apt-get; then
        ${{APT_PREFIX:-}} apt-get update -qq && ${{APT_PREFIX:-}} apt-get install -y -qq curl ca-certificates
    elif has_cmd apk; then
        ${{APT_PREFIX:-}} apk add --no-cache curl ca-certificates
    elif has_cmd yum; then
        ${{APT_PREFIX:-}} yum install -y -q curl ca-certificates
    else
        echo "ERROR: Cannot install curl - no supported package manager" >&2
        exit 1
    fi

    if ! has_cmd curl && ! has_cmd wget; then
        echo "ERROR: Failed to install downloader" >&2
        exit 1
    fi
}}

# =============================================================================
# Setup
# =============================================================================

# Ensure we have a downloader
ensure_downloader

# Check other required commands
need_cmd uname
need_cmd chmod
need_cmd mkdir

# Set up HOME directory for Claude CLI config
export HOME=/tmp/home
mkdir -p "$HOME/.claude"
echo "HOME=$HOME"

# Write Claude credentials from env var if provided
if [ -n "${{CLAUDE_CREDENTIALS_JSON:-}}" ]; then
    echo "$CLAUDE_CREDENTIALS_JSON" > "$HOME/.claude/.credentials.json"
    echo "Claude credentials configured"
fi

# =============================================================================
# Install hirsel worker binary
# =============================================================================

mkdir -p /tmp/bin
export PATH="/tmp/bin:$PATH"

echo "Installing hirsel worker binary v{version}..."
HIRSEL_TAG="v{version}"
HIRSEL_BINARY_TYPE="worker"
ARCH=$(uname -m)
case "$ARCH" in
    x86_64|amd64) HIRSEL_ARCH="amd64" ;;
    aarch64|arm64) HIRSEL_ARCH="arm64" ;;
    *) echo "ERROR: Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

# Download hirsel binary directly (simpler than install script)
HIRSEL_URL="https://github.com/SamGalanakis/hirsel/releases/download/${{HIRSEL_TAG}}/hirsel-${{HIRSEL_BINARY_TYPE}}-${{HIRSEL_TAG#v}}-linux-${{HIRSEL_ARCH}}"
echo "Downloading from: $HIRSEL_URL"
download "$HIRSEL_URL" "/tmp/bin/hirsel"
chmod +x /tmp/bin/hirsel

# Verify hirsel
if ! /tmp/bin/hirsel --version; then
    echo "ERROR: hirsel binary verification failed" >&2
    exit 1
fi

# =============================================================================
# Install Claude CLI
# =============================================================================

# Clean up any stale /tmp/claude file (Claude needs this as a directory)
[ -f /tmp/claude ] && rm -f /tmp/claude

if ! has_cmd claude; then
    echo "Installing Claude CLI..."
    CLAUDE_VERSION=$(download "https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/latest")
    case "$ARCH" in
        x86_64|amd64) PLATFORM="linux-x64" ;;
        aarch64|arm64) PLATFORM="linux-arm64" ;;
    esac
    CLAUDE_URL="https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/$CLAUDE_VERSION/$PLATFORM/claude"
    echo "Downloading Claude from: $CLAUDE_URL"
    download "$CLAUDE_URL" "/tmp/bin/claude"
    chmod +x /tmp/bin/claude
fi

# Verify claude
if ! /tmp/bin/claude --version; then
    echo "ERROR: claude binary verification failed" >&2
    exit 1
fi

# =============================================================================
# Final setup
# =============================================================================

echo ""
echo "Installed tools:"
echo "  hirsel: $(hirsel --version 2>&1)"
echo "  claude: $(claude --version 2>&1)"

# Fix git remote to use container path
cd /work
if [ -d .git ]; then
    git remote set-url origin /hirsel/work/staging 2>/dev/null || true
    git config user.email "worker@hirsel.local"
    git config user.name "Hirsel Worker"
fi

echo ""
echo "=== Starting worker ==="
echo "Command: {worker_cmd}"

}} 2>&1 | tee -a "$LOG"

# Run worker with HOME set correctly
# The exec replaces the shell, so we need to set HOME inline
exec env HOME=/tmp/home {worker_cmd}
"#,
            version = version,
            worker_cmd = worker_cmd
        );

        // Build docker command
        let mut docker_args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            format!("hirsel-{}-{}", config.run_name, config.worker_name),
            // Run as current user to avoid root-owned files in mounted volumes
            "--user".to_string(),
            format!("{}:{}", unsafe { libc::getuid() }, unsafe {
                libc::getgid()
            }),
            // Enable host.docker.internal on Linux
            // Note: requires `iptables -I INPUT -i docker0 -j ACCEPT` on host
            "--add-host=host.docker.internal:host-gateway".to_string(),
            "-v".to_string(),
            format!("{}:/work", work_dir_str),
            "-v".to_string(),
            format!("{}:/hirsel", run_dir_str),
            // Mount session directory as HOME for Claude session persistence across container restarts
            // This allows pause/resume to work by preserving Claude's session state
            // Note: Mount to /tmp/home (not /tmp/home/.claude) so both .claude/ and .claude.json are writable
            "-v".to_string(),
            format!("{}:/tmp/home", session_path_str),
            "-w".to_string(),
            "/work".to_string(),
        ];

        // Pass through environment variables
        // Note: HIRSEL_RUN, HIRSEL_WORKER, HIRSEL_API_URL are passed as CLI args to the worker.
        // The worker sets these as env vars for child processes (MCP server, agent).
        let mut env_to_pass: HashMap<String, String> = HashMap::new();
        env_to_pass.insert(
            "ACP_PERMISSION_MODE".to_string(),
            "bypassPermissions".to_string(),
        );
        env_to_pass.insert("HIRSEL_WORKER_SUBPROCESS".to_string(), "1".to_string());
        // Set HOME to /tmp/home where we mount the session directory and write credentials
        env_to_pass.insert("HOME".to_string(), "/tmp/home".to_string());

        // Add credentials/env vars
        let env_vars = config.collect_env_vars();
        for (k, v) in &env_vars {
            env_to_pass.insert(k.clone(), v.clone());
        }

        // Forward credentials from host if not in config
        // Priority: config > env var > local OAuth file
        if !env_to_pass.contains_key("ANTHROPIC_API_KEY") {
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                env_to_pass.insert("ANTHROPIC_API_KEY".to_string(), key);
            }
        }
        // Pass full credentials JSON for Claude CLI (needs all fields, not just access token)
        if !env_to_pass.contains_key("CLAUDE_CREDENTIALS_JSON") {
            if let Some(creds_json) = super::super::credentials::get_local_oauth_credentials_raw() {
                env_to_pass.insert("CLAUDE_CREDENTIALS_JSON".to_string(), creds_json);
            }
        }

        for (k, v) in &env_to_pass {
            docker_args.push("-e".to_string());
            docker_args.push(format!("{}={}", k, v));
        }

        // Add image and run init script via sh (POSIX compatible)
        docker_args.push(container.image.clone());
        docker_args.push("sh".to_string());
        docker_args.push("-c".to_string());
        docker_args.push(init_script);

        debug!("Running docker with args: {:?}", docker_args);

        // Spawn docker run
        let output = Command::new("docker")
            .args(&docker_args)
            .output()
            .map_err(|e| RunnerError::SpawnFailed(format!("Failed to run docker: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RunnerError::SpawnFailed(format!(
                "Docker run failed: {}",
                stderr
            )));
        }

        // Get container ID
        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        info!(
            "Spawned docker worker {} (container: {}, image: {})",
            config.worker_name,
            &container_id[..12.min(container_id.len())],
            container.image
        );

        Ok(SpawnResult {
            handle: WorkerHandle {
                worker_name: config.worker_name.clone(),
                runner_id: container_id,
                runner_type: "docker".to_string(),
            },
            pid: None, // Docker handles the process
        })
    }

    /// Stop a docker container.
    async fn stop_docker(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        let container_id = &handle.runner_id;

        // Try graceful stop first
        let stop_result = Command::new("docker")
            .args(["stop", "-t", "10", container_id])
            .output();

        if let Err(e) = stop_result {
            warn!("Failed to stop container {}: {}", container_id, e);
        }

        info!(
            "Stopped docker worker {} (container: {})",
            handle.worker_name,
            &container_id[..12.min(container_id.len())]
        );

        Ok(())
    }

    /// Check if a docker container is running.
    fn is_docker_alive(&self, handle: &WorkerHandle) -> bool {
        let container_id = &handle.runner_id;

        let output = Command::new("docker")
            .args(["inspect", "-f", "{{.State.Running}}", container_id])
            .output();

        match output {
            Ok(o) => {
                let running = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
                running == "true"
            }
            Err(_) => false,
        }
    }
}

impl Default for LocalRunner {
    fn default() -> Self {
        Self::new_bare()
    }
}

#[async_trait]
impl Runner for LocalRunner {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult> {
        // Dispatch to appropriate spawn method
        if let Some(ref container) = self.container {
            self.spawn_docker(config, container).await
        } else {
            self.spawn_bare(config).await
        }
    }

    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()> {
        // Dispatch based on runner type
        if handle.runner_type == "docker" {
            return self.stop_docker(handle).await;
        }

        // Local process - stop by PID
        let pid: u32 = handle
            .runner_id
            .parse()
            .map_err(|_| RunnerError::StopFailed("Invalid PID".to_string()))?;

        if !Self::is_pid_alive(pid) {
            return Ok(());
        }

        #[cfg(unix)]
        {
            // Kill the entire process group using negative PID
            // First try SIGTERM for graceful shutdown
            Self::kill_process_group(pid, libc::SIGTERM);
        }

        // Give a brief moment for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Force kill if still alive
        if Self::is_pid_alive(pid) {
            #[cfg(unix)]
            {
                Self::kill_process_group(pid, libc::SIGKILL);
            }
        }

        info!("Stopped local worker {} (PID {})", handle.worker_name, pid);

        Ok(())
    }

    async fn is_alive(&self, handle: &WorkerHandle) -> bool {
        // Dispatch based on runner type
        if handle.runner_type == "docker" {
            return self.is_docker_alive(handle);
        }

        // Local process - check by PID
        if let Ok(pid) = handle.runner_id.parse::<u32>() {
            Self::is_pid_alive(pid)
        } else {
            false
        }
    }

    fn runner_type(&self) -> &'static str {
        if self.container.is_some() {
            "docker"
        } else {
            "local"
        }
    }

    // get_logs not overridden - default implementation returns empty
    // Worker events are now stored in the database; use orchestrator's get_worker_events
}

/// Pause all workers by killing their process groups.
///
/// Workers can be resumed later from their saved session state.
/// Handles both local and docker workers.
pub async fn pause_all_local_workers(handles: &[WorkerHandle]) -> Vec<String> {
    let mut paused = Vec::new();
    let runner = LocalRunner::new_bare();

    for handle in handles {
        // Handle both local and docker workers
        if handle.runner_type != "local" && handle.runner_type != "docker" {
            continue;
        }

        if runner.is_alive(handle).await {
            if let Err(e) = runner.stop(handle).await {
                warn!("Failed to pause worker {}: {}", handle.worker_name, e);
            } else {
                paused.push(handle.worker_name.clone());
            }
        }
    }

    paused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_runner_type() {
        let runner = LocalRunner::new_bare();
        assert_eq!(runner.runner_type(), "local");
    }

    #[test]
    fn test_docker_runner_type() {
        let runner = LocalRunner::new(Some(ContainerConfig {
            image: "test:latest".to_string(),
        }));
        assert_eq!(runner.runner_type(), "docker");
    }

    #[test]
    fn test_is_pid_alive_invalid() {
        // PID 0 should not be considered alive
        assert!(!LocalRunner::is_pid_alive(0));
    }
}
