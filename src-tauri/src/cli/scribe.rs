//! Scribe command - process scribe submissions for a run
//!
//! This is an internal command spawned by the daemon to process
//! batched scribe submissions and update documentation.

use crate::core::{
    config::{self, Config},
    scribe::{process_scribe_batch, ScribeError},
    Files,
};
use tracing::{info, warn};

/// Execute the scribe command for a run
pub async fn execute(run_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check run exists
    if !config::run_exists(run_name) {
        return Err(format!("Run '{}' not found", run_name).into());
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);

    // Load config
    let (global_config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

    // Check if scribe is enabled
    if !global_config.scribe_enabled {
        info!("Scribe disabled in config, skipping");
        return Ok(());
    }

    // Get agent command
    let agent_command = crate::cli::config::get_agent_command();

    // Run scribe processing
    let result = match process_scribe_batch(&files, &global_config, &agent_command).await {
        Ok(result) => {
            info!(
                "Processed scribe batch {} for run '{}': {} submissions",
                result.batch_id, run_name, result.submissions_processed
            );
            Ok(())
        }
        Err(ScribeError::NoPending) => {
            info!("No pending scribe submissions for run '{}'", run_name);
            Ok(())
        }
        Err(e) => {
            warn!("Scribe processing failed for run '{}': {}", run_name, e);
            Err(e.into())
        }
    };

    // Clean up any remaining child processes
    crate::core::process::cleanup_process_group("scribe");

    result
}
