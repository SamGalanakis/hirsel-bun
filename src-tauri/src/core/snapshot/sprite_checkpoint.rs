//! Sprite checkpoint archive strategy.
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

use super::archive::{ArchiveHandle, ArchiveResult, ArchiveStrategy};
use super::SnapshotError;
use crate::core::runner::sprite::SpritesClient;
use crate::core::runner::SpriteHostConfig;

/// Sprite checkpoint strategy - uses native Sprites API for checkpoints.
pub struct SpriteCheckpointStrategy {
    client: SpritesClient,
    comment_prefix: String,
}

impl SpriteCheckpointStrategy {
    /// Create a new Sprite checkpoint strategy.
    pub fn new(config: &SpriteHostConfig, comment_prefix: Option<String>) -> ArchiveResult<Self> {
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
            comment_prefix: comment_prefix.unwrap_or_else(|| "hirsel-archive".to_string()),
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
impl ArchiveStrategy for SpriteCheckpointStrategy {
    async fn archive(&self, key: &str, _source_dir: &Path) -> ArchiveResult<ArchiveHandle> {
        // Parse run_name/worker_name from key (format: "run_name/worker_name/...")
        let parts: Vec<&str> = key.split('/').collect();
        if parts.len() < 2 {
            return Err(SnapshotError::Config(format!(
                "Invalid key format for Sprite checkpoint: '{}' (expected 'run_name/worker_name/...')",
                key
            )));
        }

        let run_name = parts[0];
        let worker_name = parts[1];
        let sprite_name = Self::sprite_name(run_name, worker_name);
        let comment = format!("{}-{}", self.comment_prefix, key.replace('/', "-"));

        info!(
            "Creating Sprite checkpoint for key '{}' (sprite: {})",
            key, sprite_name
        );

        let checkpoint = self
            .client
            .checkpoint(&sprite_name, &comment)
            .await
            .map_err(|e| SnapshotError::Storage(format!("Failed to create checkpoint: {}", e)))?;

        info!(
            "Created Sprite checkpoint {} for key '{}'",
            checkpoint.id, key
        );

        Ok(ArchiveHandle::new(self.strategy_type(), checkpoint.id))
    }

    async fn restore(&self, handle: &ArchiveHandle, _target_dir: &Path) -> ArchiveResult<()> {
        // Note: For Sprite checkpoints, we don't restore to an existing sprite.
        // Instead, we use create_from_checkpoint when spawning a new sprite.
        // This restore function is called, but the actual restore happens
        // in the spawn process which uses the checkpoint_id from the handle.

        info!(
            "Sprite checkpoint '{}' is ready for restore (will be used on next spawn)",
            handle.storage_id
        );

        Ok(())
    }

    async fn delete(&self, handle: &ArchiveHandle) -> ArchiveResult<()> {
        // Note: The Sprites API doesn't have a delete checkpoint endpoint
        // Checkpoints are cleaned up automatically or managed via the web UI
        debug!(
            "Sprite checkpoint '{}' deletion skipped (API doesn't support delete)",
            handle.storage_id
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
