//! Compact command - run learnings compaction for a run
//!
//! This is an internal command spawned by the GUI to perform
//! automatic compaction of the learnings thread when thresholds
//! are exceeded.

use crate::core::{
    compaction::{compact_learnings_with_agent, CompactionError},
    config::{self, Config},
    state::SQLiteState,
    Files,
};
use tracing::{info, warn};

/// Execute the compact command for a run
pub async fn execute(run_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check run exists
    if !config::run_exists(run_name) {
        return Err(format!("Run '{}' not found", run_name).into());
    }

    let run_dir = config::run_dir(run_name);
    let files = Files::new(&run_dir);
    let state = SQLiteState::new(files.db_path())?;

    // Load config
    let (global_config, _) = Config::load().unwrap_or_else(|_| (Config::default(), vec![]));

    // Check if compaction is enabled
    if !global_config.compaction_enabled {
        info!("Compaction disabled in config, skipping");
        return Ok(());
    }

    // Run compaction
    let result = match compact_learnings_with_agent(&state, &files, &global_config).await {
        Ok(result) => {
            info!(
                "Compacted {} learnings messages for run '{}'",
                result.messages_compacted, run_name
            );

            // Update last compaction timestamp
            let now = chrono::Utc::now().to_rfc3339();
            if let Err(e) = state.set_last_compaction_at(&now) {
                warn!("Failed to update last_compaction_at: {}", e);
            }

            Ok(())
        }
        Err(CompactionError::NotNeeded) => {
            info!("Compaction not needed for run '{}'", run_name);
            Ok(())
        }
        Err(e) => {
            warn!("Compaction failed for run '{}': {}", run_name, e);
            Err(e.into())
        }
    };

    // Clean up any remaining child processes (e.g., grandchildren like hirsel __acp-bridge)
    crate::core::process::cleanup_process_group("compaction");

    result
}
