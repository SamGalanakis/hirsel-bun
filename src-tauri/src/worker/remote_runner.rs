//! Remote worker runner - runs on remote machine via SSH.
//!
//! This module implements the worker that runs on remote machines
//! and communicates with the coordinator via HTTP tunnel.
//!
//! Remote workers run the same ACP worker loop as local workers,
//! but the MCP server they spawn uses HttpState (via HIRSEL_API_URL)
//! instead of SQLiteState.
//!
//! ## File Receiver Mode
//!
//! When `wait_for_files` is true, the worker starts an HTTP server
//! that waits to receive a tarball of project files before starting
//! the ACP worker loop. This allows the coordinator to push files
//! directly to the worker instead of the worker pulling them.

use std::path::PathBuf;

use crate::worker::acp_client::{run_worker, WorkerRunConfig};
use crate::worker::http_state::HttpState;

/// Configuration for running a remote worker
pub struct RemoteWorkerConfig<'a> {
    pub api_url: &'a str,
    pub run_name: &'a str,
    pub worker_name: &'a str,
    pub work_dir: &'a str,
    pub spec_path: &'a str,
    pub agent_command: &'a [String],
    pub is_leader: bool,
    pub leader_name: Option<&'a str>,
    pub teammates: Option<Vec<String>>,
    /// If true, start HTTP file receiver and wait for files before running
    pub wait_for_files: bool,
    /// Port for file receiver (default: 19800)
    pub file_receiver_port: Option<u16>,
}

/// Run a remote worker that communicates with coordinator via HTTP.
///
/// This is the entry point for `hirsel __remote-worker` on remote machines.
/// It runs the same ACP worker loop as local workers, but the MCP server
/// spawned by the agent will detect HIRSEL_API_URL and use HttpState.
#[allow(clippy::too_many_arguments)]
pub async fn run_remote_worker(
    api_url: &str,
    run_name: &str,
    worker_name: &str,
    work_dir: &str,
    spec_path: &str,
    agent_command: &[String],
    is_leader: bool,
    leader_name: Option<&str>,
    teammates: Option<Vec<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_remote_worker_with_config(RemoteWorkerConfig {
        api_url,
        run_name,
        worker_name,
        work_dir,
        spec_path,
        agent_command,
        is_leader,
        leader_name,
        teammates,
        wait_for_files: false,
        file_receiver_port: None,
    })
    .await
}

/// Run a remote worker with full configuration including file receiver options.
pub async fn run_remote_worker_with_config(
    config: RemoteWorkerConfig<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!(
        "Starting remote worker {} for run {} at {}",
        config.worker_name,
        config.run_name,
        config.api_url
    );

    // If wait_for_files is enabled, start file receiver and wait
    #[cfg(any(feature = "server", feature = "worker"))]
    if config.wait_for_files {
        use crate::worker::file_server;

        let work_path = PathBuf::from(config.work_dir);
        tracing::info!(
            "Starting file receiver on port {:?}, waiting for files...",
            config.file_receiver_port
        );

        let handle = file_server::start_file_server(work_path, config.file_receiver_port)
            .await
            .map_err(|e| -> Box<dyn std::error::Error> { e })?;

        tracing::info!("File receiver listening on port {}", handle.port);

        // Wait for files to be uploaded
        match handle.files_ready.await {
            Ok(Ok(())) => {
                tracing::info!("Files received and extracted successfully");
            }
            Ok(Err(e)) => {
                return Err(format!("Failed to extract files: {}", e).into());
            }
            Err(_) => {
                return Err("File receiver channel closed unexpectedly".into());
            }
        }

        // Server shuts down automatically after receiving files
        let _ = handle.task.await;
    }

    // Ensure HIRSEL_API_URL is set - this is how the MCP server knows
    // to use HttpState instead of SQLiteState
    std::env::set_var("HIRSEL_API_URL", config.api_url);

    // Create HTTP state client for initial validation
    let http_state = HttpState::new(config.api_url, config.worker_name, 30);

    // Verify connection to coordinator
    if !http_state.health_check().await? {
        return Err("Failed to connect to coordinator".into());
    }

    tracing::info!("Connected to coordinator API");

    // Get initial config from coordinator for validation
    let request = http_state.get_request().await?;
    let _human_in_the_loop = http_state.get_human_in_the_loop().await?;

    tracing::info!(
        "Run config - request: {:?}",
        request.as_deref().map(|s| &s[..s.len().min(50)])
    );

    // Create paths for the worker
    let work_path = PathBuf::from(config.work_dir);
    let spec_file = PathBuf::from(config.spec_path);

    // For remote workers, we use work_dir as both work_dir and run_dir
    // The actual run state is managed by the coordinator via HTTP
    let run_dir = work_path.clone();

    // Create worker run config
    let worker_config = WorkerRunConfig {
        run_name: config.run_name.to_string(),
        worker_name: config.worker_name.to_string(),
        work_dir: work_path,
        run_dir,
        spec_path: spec_file,
        agent_command: config.agent_command.to_vec(),
        is_leader: config.is_leader,
        leader_name: config.leader_name.map(String::from),
        teammates: config.teammates,
        resume_session_id: None,
    };

    tracing::info!(
        "Remote worker {} starting ACP worker loop",
        config.worker_name
    );
    tracing::info!("Agent command: {:?}", config.agent_command);
    tracing::info!(
        "Is leader: {}, leader_name: {:?}",
        config.is_leader,
        config.leader_name
    );

    // Run the worker loop - same as local workers
    // The MCP server spawned by the agent will detect HIRSEL_API_URL
    // and use HttpState for all state operations
    run_worker(worker_config).await?;

    tracing::info!("Remote worker {} completed", config.worker_name);
    Ok(())
}
