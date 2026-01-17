//! Remote worker runner - runs on remote machine via SSH.
//!
//! This module implements the worker that runs on remote machines
//! and communicates with the coordinator via HTTP tunnel.
//!
//! Remote workers run the same ACP worker loop as local workers,
//! but the MCP server they spawn uses HttpState (via HIRSEL_API_URL)
//! instead of SQLiteState.

use std::path::PathBuf;

use crate::worker::acp_client::{run_acp_worker, WorkerRunConfig};
use crate::worker::http_state::HttpState;

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
    tracing::info!(
        "Starting remote worker {} for run {} at {}",
        worker_name,
        run_name,
        api_url
    );

    // Ensure HIRSEL_API_URL is set - this is how the MCP server knows
    // to use HttpState instead of SQLiteState
    std::env::set_var("HIRSEL_API_URL", api_url);

    // Create HTTP state client for initial validation
    let http_state = HttpState::new(api_url, worker_name, 30);

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
    let work_path = PathBuf::from(work_dir);
    let spec_file = PathBuf::from(spec_path);

    // For remote workers, we use work_dir as both work_dir and run_dir
    // The actual run state is managed by the coordinator via HTTP
    let run_dir = work_path.clone();

    // Create worker run config
    let config = WorkerRunConfig {
        run_name: run_name.to_string(),
        worker_name: worker_name.to_string(),
        work_dir: work_path,
        run_dir,
        spec_path: spec_file,
        agent_command: agent_command.to_vec(),
        is_leader,
        leader_name: leader_name.map(String::from),
        teammates,
        resume_session_id: None,
    };

    tracing::info!("Remote worker {} starting ACP worker loop", worker_name);
    tracing::info!("Agent command: {:?}", agent_command);
    tracing::info!("Is leader: {}, leader_name: {:?}", is_leader, leader_name);

    // Run the ACP worker loop - same as local workers
    // The MCP server spawned by the agent will detect HIRSEL_API_URL
    // and use HttpState for all state operations
    run_acp_worker(config).await?;

    tracing::info!("Remote worker {} completed", worker_name);
    Ok(())
}
