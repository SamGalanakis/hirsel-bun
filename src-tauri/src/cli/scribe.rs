//! Scribe command - process retained-context submissions for a run.
//!
//! This is an internal command spawned by the daemon to condense batched
//! worker learnings into project-level retained context.

use crate::core::{
    config::{self, Config},
    scribe::{process_scribe_batch, ScribeError},
};
use tracing::{info, warn};

/// Execute the scribe command for a run
pub async fn execute(run_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check run exists
    if !config::run_exists(run_name) {
        return Err(format!("Run '{}' not found", run_name).into());
    }

    // Load config
    let (global_config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

    // Check if scribe is enabled
    if !global_config.scribe_enabled {
        info!("Scribe disabled in config, skipping");
        return Ok(());
    }

    // Run scribe processing
    let result = match process_scribe_batch(run_name).await {
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
