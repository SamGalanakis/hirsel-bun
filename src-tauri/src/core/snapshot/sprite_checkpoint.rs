//! Sprite checkpoint snapshot strategy.
//!
//! This strategy uses the Sprites.dev native checkpoint API to create
//! and restore snapshots of sprite VMs. This is more efficient than
//! tar/gzip + S3 for Sprite hosts because:
//!
//! 1. Checkpoints capture full VM state, not just files
//! 2. Sprites can be restored instantly from checkpoints
//! 3. No data transfer to/from external storage
//!
//! # How it works
//!
//! On pause:
//! 1. Create a checkpoint of the running sprite
//! 2. The sprite is then destroyed (if auto_destroy is enabled)
//!
//! On resume:
//! 1. Create a new sprite from the checkpoint
//! 2. The checkpoint can optionally be deleted after restore

use async_trait::async_trait;
use std::path::Path;
use tracing::{debug, info};

use super::{SnapshotError, SnapshotHandle, SnapshotResult, SnapshotStrategy};
use crate::core::runner::sprite::SpritesClient;
use crate::core::runner::SpriteHostConfig;

/// Sprite checkpoint strategy - uses native Sprites API for checkpoints.
pub struct SpriteCheckpointStrategy {
    client: SpritesClient,
    comment_prefix: String,
}

impl SpriteCheckpointStrategy {
    /// Create a new Sprite checkpoint strategy.
    pub fn new(config: &SpriteHostConfig, comment_prefix: Option<String>) -> SnapshotResult<Self> {
        let token = config
            .api_token
            .clone()
            .unwrap_or_else(|| std::env::var("SPRITES_TOKEN").unwrap_or_default());

        if token.is_empty() {
            return Err(SnapshotError::Config(
                "Sprites API token not configured".into(),
            ));
        }

        let client = SpritesClient::new(token, Some(config.api_url.clone()));

        Ok(Self {
            client,
            comment_prefix: comment_prefix.unwrap_or_else(|| "hirsel-snapshot".to_string()),
        })
    }

    /// Generate sprite name for a worker (must match SpriteRunner::sprite_name)
    fn sprite_name(run_name: &str, worker_name: &str) -> String {
        let name = format!("hirsel-{}-{}", run_name, worker_name);
        name.to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    }
}

#[async_trait]
impl SnapshotStrategy for SpriteCheckpointStrategy {
    async fn snapshot(
        &self,
        run_name: &str,
        worker_name: &str,
        _work_dir: &Path,
    ) -> SnapshotResult<SnapshotHandle> {
        let sprite_name = Self::sprite_name(run_name, worker_name);
        let comment = format!("{}-{}-{}", self.comment_prefix, run_name, worker_name);

        info!(
            "Creating Sprite checkpoint for {} (sprite: {})",
            worker_name, sprite_name
        );

        let checkpoint = self
            .client
            .checkpoint(&sprite_name, &comment)
            .await
            .map_err(|e| SnapshotError::Storage(format!("Failed to create checkpoint: {}", e)))?;

        info!(
            "Created Sprite checkpoint {} for worker {}",
            checkpoint.id, worker_name
        );

        Ok(SnapshotHandle {
            strategy_type: self.strategy_type().to_string(),
            snapshot_id: checkpoint.id,
            created_at: checkpoint.created_at,
            size_bytes: None, // Sprites API doesn't report checkpoint size
        })
    }

    async fn restore(&self, handle: &SnapshotHandle, _work_dir: &Path) -> SnapshotResult<()> {
        // Note: For Sprite checkpoints, we don't restore to an existing sprite.
        // Instead, we use create_from_checkpoint when spawning a new sprite.
        // This restore function is called, but the actual restore happens
        // in the spawn process which uses the checkpoint_id from the handle.
        //
        // The work_dir is not used because the checkpoint contains the full VM state.

        info!(
            "Sprite checkpoint {} is ready for restore (will be used on next spawn)",
            handle.snapshot_id
        );

        // We could verify the checkpoint exists here
        // For now, we just log and return success - the actual restore
        // happens when SpriteRunner::spawn() is called with the checkpoint

        Ok(())
    }

    async fn delete(&self, handle: &SnapshotHandle) -> SnapshotResult<()> {
        info!("Deleting Sprite checkpoint {}", handle.snapshot_id);

        // Note: The Sprites API doesn't have a delete checkpoint endpoint
        // Checkpoints are cleaned up automatically or managed via the web UI
        // For now, we just log and return success
        debug!(
            "Sprite checkpoint {} deletion skipped (API doesn't support delete)",
            handle.snapshot_id
        );

        Ok(())
    }

    fn strategy_type(&self) -> &'static str {
        "sprite_checkpoint"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_name_generation() {
        assert_eq!(
            SpriteCheckpointStrategy::sprite_name("my-run", "achilles"),
            "hirsel-my-run-achilles"
        );

        assert_eq!(
            SpriteCheckpointStrategy::sprite_name("Test Run", "Worker 1"),
            "hirsel-test-run-worker-1"
        );
    }
}
