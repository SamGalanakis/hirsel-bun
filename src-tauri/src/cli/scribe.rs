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
pub async fn execute(runtime_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check run exists
    if !config::runtime_exists(runtime_name) {
        return Err(format!("Run '{}' not found", runtime_name).into());
    }

    // Load config
    let (global_config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

    // Check if scribe is enabled
    if !global_config.scribe_enabled {
        info!("Scribe disabled in config, skipping");
        return Ok(());
    }

    // Run scribe processing
    let result = match process_scribe_batch(runtime_name).await {
        Ok(result) => {
            info!(
                "Processed scribe batch {} for run '{}': {} submissions",
                result.batch_id, runtime_name, result.submissions_processed
            );
            Ok(())
        }
        Err(ScribeError::NoPending) => {
            info!("No pending scribe submissions for run '{}'", runtime_name);
            Ok(())
        }
        Err(e) => {
            warn!("Scribe processing failed for run '{}': {}", runtime_name, e);
            Err(e.into())
        }
    };

    // Clean up any remaining child processes
    crate::core::process::cleanup_process_group("scribe");

    result
}
