//! Shared worker setup scripts for all remote runners.
//!
//! This module provides standardized shell scripts for setting up worker
//! environments. All remote runners (SSH, Sprite) use these to ensure
//! consistent file structure and git configuration.
//!
//! ## Worker Directory Structure
//!
//! All workers expect this structure in their work directory:
//! ```text
//! {work_dir}/
//! ├── .git/              # Git repo with coordinator as remote
//! ├── spec.md            # Task specification
//! ├── eval.md            # Evaluation criteria (optional)
//! ├── chats/             # Synced chat files
//! └── ... project files from tarball
//! ```
//!
//! ## Setup Flow
//!
//! 1. Create work directory
//! 2. Download tarball from coordinator's `/api/runs/{name}/files` endpoint
//! 3. Extract tarball
//! 4. Initialize git repo with coordinator as remote
//! 5. Create required subdirectories

/// Configuration for worker setup scripts
#[derive(Debug, Clone)]
pub struct WorkerSetupConfig {
    /// Coordinator API URL (e.g., "http://coordinator:19700")
    pub coordinator_url: String,
    /// Run name
    pub run_name: String,
    /// Worker name
    pub worker_name: String,
    /// Work directory path on the remote machine
    pub work_dir: String,
}

impl WorkerSetupConfig {
    /// URL to download project files tarball
    pub fn files_url(&self) -> String {
        format!("{}/api/runs/{}/files", self.coordinator_url, self.run_name)
    }

    /// URL for git remote (coordinator's git HTTP server)
    pub fn git_url(&self) -> String {
        format!("{}/git/{}", self.coordinator_url, self.run_name)
    }
}

/// Generate the complete worker setup script.
///
/// This script:
/// 1. Creates work directory
/// 2. Downloads and extracts project tarball from coordinator
/// 3. Initializes git with coordinator as remote
/// 4. Creates required subdirectories
pub fn generate_setup_script(config: &WorkerSetupConfig) -> String {
    let work_dir = &config.work_dir;
    let files_url = config.files_url();
    let git_url = config.git_url();

    format!(
        r#"
set -e

echo "=== Setting up worker environment ==="

# Step 1: Create work directory
echo "Creating work directory: {work_dir}"
mkdir -p {work_dir}
cd {work_dir}

# Step 2: Download and extract project files from coordinator
echo "Downloading project files from {files_url}..."
curl -sS -f -H "Authorization: Bearer $HIRSEL_API_KEY" \
    "{files_url}" | tar -xzf -

# Step 3: Initialize git repository with coordinator as remote
echo "Initializing git repository..."
if [ ! -d .git ]; then
    git init
    git add .
    git commit -m "Initial import from coordinator" 2>/dev/null || true
fi

# Configure git remote for syncing with coordinator
git remote remove origin 2>/dev/null || true
git remote add origin "{git_url}"

# Configure git user for commits (worker identity)
git config user.email "worker@hirsel.local"
git config user.name "Hirsel Worker"

# Step 4: Create required subdirectories
mkdir -p chats

echo "=== Worker setup complete ==="
echo "Work directory: {work_dir}"
echo "Git remote: {git_url}"
"#,
        work_dir = work_dir,
        files_url = files_url,
        git_url = git_url,
    )
}

/// Generate script to start the worker process.
///
/// This is called after setup_script completes.
pub fn generate_worker_start_script(
    work_dir: &str,
    api_url: &str,
    run_name: &str,
    worker_name: &str,
    spec_path: &str,
    agent_command_json: &str,
    is_leader: bool,
    leader_name: Option<&str>,
    teammates: Option<&[String]>,
    env_vars: &[(String, String)],
) -> String {
    // Build environment exports
    let mut env_exports = vec![
        format!(r#"export HIRSEL_RUN="{}""#, run_name),
        format!(r#"export HIRSEL_WORKER="{}""#, worker_name),
        format!(r#"export HIRSEL_API_URL="{}""#, api_url),
        "export HIRSEL_REMOTE=1".to_string(),
        "export ACP_PERMISSION_MODE=bypassPermissions".to_string(),
    ];

    // Add custom environment variables
    for (key, value) in env_vars {
        let escaped_value = value.replace('\'', "'\\''");
        env_exports.push(format!("export {}='{}'", key, escaped_value));
    }

    let env_block = env_exports.join("\n");

    // Escape agent command for shell
    let agent_command_escaped = agent_command_json.replace('\'', "'\\''");

    // Build optional args
    let leader_arg = if is_leader { "--is-leader" } else { "" };
    let leader_name_arg = leader_name
        .map(|n| format!("--leader-name '{}'", n))
        .unwrap_or_default();
    let teammates_arg = teammates
        .map(|t| format!("--teammates '{}'", t.join(",")))
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    format!(
        r#"
cd {work_dir}

# Set environment
{env_block}

# Run worker in background
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
        run_name = run_name,
        worker_name = worker_name,
        spec_path = spec_path,
        agent_command = agent_command_escaped,
        leader_arg = leader_arg,
        leader_name_arg = leader_name_arg,
        teammates_arg = teammates_arg,
    )
}

/// Generate script to start worker in a Docker container on remote host.
///
/// This runs `docker run` with the work directory mounted and the worker command inside.
#[allow(clippy::too_many_arguments)]
pub fn generate_docker_worker_script(
    work_dir: &str,
    api_url: &str,
    run_name: &str,
    worker_name: &str,
    _spec_path: &str,
    agent_command_json: &str,
    is_leader: bool,
    leader_name: Option<&str>,
    teammates: Option<&[String]>,
    env_vars: &[(String, String)],
    docker_image: &str,
) -> String {
    // Build environment variables for docker -e flags
    let mut env_flags = vec![
        format!("-e HIRSEL_RUN={}", run_name),
        format!("-e HIRSEL_WORKER={}", worker_name),
        format!("-e HIRSEL_API_URL={}", api_url),
        "-e HIRSEL_REMOTE=1".to_string(),
        "-e ACP_PERMISSION_MODE=bypassPermissions".to_string(),
    ];

    // Add custom environment variables
    for (key, value) in env_vars {
        // Escape for shell
        let escaped_value = value.replace('\'', "'\\''").replace('"', "\\\"");
        env_flags.push(format!("-e {}=\"{}\"", key, escaped_value));
    }

    let env_block = env_flags.join(" ");

    // Escape agent command for shell
    let agent_command_escaped = agent_command_json.replace('\'', "'\\''");

    // Build optional args
    let leader_arg = if is_leader { "--is-leader" } else { "" };
    let leader_name_arg = leader_name
        .map(|n| format!("--leader-name '{}'", n))
        .unwrap_or_default();
    let teammates_arg = teammates
        .map(|t| format!("--teammates '{}'", t.join(",")))
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    // Container name for lifecycle management
    let container_name = format!("hirsel-{}-{}", run_name, worker_name);

    format!(
        r#"
# Run worker in Docker container
docker run -d --rm \
    --name '{container_name}' \
    -v '{work_dir}:/work' \
    -w /work \
    {env_block} \
    '{docker_image}' \
    hirsel __remote-worker \
        --api-url '{api_url}' \
        --run-name '{run_name}' \
        --worker-name '{worker_name}' \
        --work-dir '/work' \
        --spec '/work/spec.md' \
        --agent-command '{agent_command}' \
        {leader_arg} {leader_name_arg} {teammates_arg}
"#,
        container_name = container_name,
        work_dir = work_dir,
        env_block = env_block,
        docker_image = docker_image,
        api_url = api_url,
        run_name = run_name,
        worker_name = worker_name,
        agent_command = agent_command_escaped,
        leader_arg = leader_arg,
        leader_name_arg = leader_name_arg,
        teammates_arg = teammates_arg,
    )
}

/// Generate script to sync changes back to coordinator via git push.
pub fn generate_sync_script(work_dir: &str) -> String {
    format!(
        r#"
cd {work_dir}

# Stage all changes
git add -A

# Commit if there are changes
if ! git diff --cached --quiet; then
    git commit -m "Worker changes $(date +%Y-%m-%d_%H:%M:%S)"
fi

# Push to coordinator
git push -u origin HEAD 2>&1 || echo "Push failed (may need pull first)"
"#,
        work_dir = work_dir,
    )
}

/// Generate script to pull latest changes from coordinator.
pub fn generate_pull_script(work_dir: &str) -> String {
    format!(
        r#"
cd {work_dir}

# Fetch and merge from coordinator
git fetch origin
git merge origin/HEAD --no-edit 2>&1 || echo "Merge failed (may have conflicts)"
"#,
        work_dir = work_dir,
    )
}

/// Generate init script for Fly machines.
///
/// This script is run as the machine's init command (entrypoint).
/// It downloads hirsel binary, installs agent tools, fetches project files, and starts the worker.
///
/// Note: The worker environment variables (ANTHROPIC_API_KEY, etc.) are passed via Fly's
/// machine config, not in this script.
pub fn generate_fly_init_script(
    coordinator_url: &str,
    run_name: &str,
    worker_name: &str,
    agent_command_json: &str,
    is_leader: bool,
    leader_name: Option<&str>,
    teammates: Option<&[String]>,
) -> String {
    let work_dir = "/work";
    let setup_config = WorkerSetupConfig {
        coordinator_url: coordinator_url.to_string(),
        run_name: run_name.to_string(),
        worker_name: worker_name.to_string(),
        work_dir: work_dir.to_string(),
    };

    let files_url = setup_config.files_url();
    let git_url = setup_config.git_url();

    // Build optional args for worker command
    let leader_arg = if is_leader { "--is-leader" } else { "" };
    let leader_name_arg = leader_name
        .map(|n| format!("--leader-name '{}'", n))
        .unwrap_or_default();
    let teammates_arg = teammates
        .map(|t| format!("--teammates '{}'", t.join(",")))
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    // Escape agent command for shell
    let agent_command_escaped = agent_command_json.replace('\'', "'\\''");

    format!(
        r#"#!/bin/sh
set -e

echo "=== Fly Worker Setup ==="
echo "Coordinator: {coordinator_url}"
echo "Run: {run_name}, Worker: {worker_name}"

# Step 1: Download hirsel binary from coordinator
echo "Downloading hirsel binary..."
curl -sSL -H "Authorization: Bearer $HIRSEL_API_KEY" \
    "{coordinator_url}/api/binary/hirsel" -o /usr/local/bin/hirsel
chmod +x /usr/local/bin/hirsel

# Step 2: Install Node.js and agent tools if not present
if ! command -v node > /dev/null 2>&1; then
    echo "Installing Node.js..."
    if command -v apt-get > /dev/null 2>&1; then
        apt-get update && apt-get install -y curl ca-certificates
        curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
        apt-get install -y nodejs
    elif command -v apk > /dev/null 2>&1; then
        apk add --no-cache nodejs npm
    fi
fi

# Note: The agent (hirsel __acp-bridge) is provided via the Docker container
# or deployed separately. No npm installation needed.

# Step 3: Create work directory and fetch project files
echo "Setting up work directory..."
mkdir -p {work_dir}
cd {work_dir}

echo "Downloading project files from {files_url}..."
curl -sS -f -H "Authorization: Bearer $HIRSEL_API_KEY" \
    "{files_url}" | tar -xzf -

# Step 4: Initialize git repository
echo "Initializing git repository..."
if [ ! -d .git ]; then
    git init
    git add .
    git commit -m "Initial import from coordinator" 2>/dev/null || true
fi

git remote remove origin 2>/dev/null || true
git remote add origin "{git_url}"
git config user.email "worker@hirsel.local"
git config user.name "Hirsel Worker"
mkdir -p chats

# Step 5: Start worker
echo "=== Starting Worker ==="
exec hirsel __remote-worker \
    --api-url '{coordinator_url}' \
    --run-name '{run_name}' \
    --worker-name '{worker_name}' \
    --work-dir '{work_dir}' \
    --spec '{work_dir}/spec.md' \
    --agent-command '{agent_command}' \
    {leader_arg} {leader_name_arg} {teammates_arg}
"#,
        coordinator_url = coordinator_url,
        run_name = run_name,
        worker_name = worker_name,
        work_dir = work_dir,
        files_url = files_url,
        git_url = git_url,
        agent_command = agent_command_escaped,
        leader_arg = leader_arg,
        leader_name_arg = leader_name_arg,
        teammates_arg = teammates_arg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_config_urls() {
        let config = WorkerSetupConfig {
            coordinator_url: "http://localhost:19700".to_string(),
            run_name: "my-run".to_string(),
            worker_name: "worker-1".to_string(),
            work_dir: "/workspaces/hirsel".to_string(),
        };

        assert_eq!(
            config.files_url(),
            "http://localhost:19700/api/runs/my-run/files"
        );
        assert_eq!(config.git_url(), "http://localhost:19700/git/my-run");
    }

    #[test]
    fn test_generate_setup_script() {
        let config = WorkerSetupConfig {
            coordinator_url: "http://localhost:19700".to_string(),
            run_name: "test-run".to_string(),
            worker_name: "achilles".to_string(),
            work_dir: "/home/worker/project".to_string(),
        };

        let script = generate_setup_script(&config);

        assert!(script.contains("mkdir -p /home/worker/project"));
        assert!(script.contains("/api/runs/test-run/files"));
        assert!(script.contains("git init"));
        assert!(script.contains("git remote add origin"));
    }

    #[test]
    fn test_generate_worker_start_script() {
        let script = generate_worker_start_script(
            "/workspaces/project",
            "http://localhost:19700",
            "my-run",
            "worker-1",
            "/workspaces/project/spec.md",
            "[\"claude\"]",
            true,
            None,
            None,
            &[("ANTHROPIC_API_KEY".to_string(), "sk-test".to_string())],
        );

        assert!(script.contains("--is-leader"));
        assert!(script.contains("ANTHROPIC_API_KEY"));
        assert!(script.contains("nohup hirsel __remote-worker"));
    }
}
