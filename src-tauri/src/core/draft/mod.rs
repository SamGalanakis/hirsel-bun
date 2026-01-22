//! Draft workspace management
//!
//! This module provides abstractions for managing draft workspaces. A workspace
//! is the directory where AI agents work on code. The workspace can be stored
//! locally (filesystem) or remotely (S3).
//!
//! # Architecture
//!
//! - **WorkspaceProvider**: Trait that abstracts storage operations
//! - **LocalWorkspaceProvider**: Filesystem-based storage (default)
//! - **S3WorkspaceProvider**: S3-compatible storage for remote deployments
//! - **StartingPoint**: Enum defining how workspaces are initialized
//!
//! # Usage
//!
//! ```rust,ignore
//! use hirsel_lib::core::draft::{create_workspace_provider, StartingPoint};
//!
//! // Create provider based on profile configuration
//! let provider = create_workspace_provider(Some("fly"));
//!
//! // Initialize a workspace from a git repo
//! let info = provider.init("my-run", &StartingPoint::GitRepo {
//!     url: "https://github.com/user/repo".into(),
//!     branch: Some("main".into()),
//! }).await?;
//! ```

mod local_workspace;
#[cfg(feature = "s3-storage")]
mod s3_workspace;
mod types;
mod workspace;

pub use local_workspace::LocalWorkspaceProvider;
#[cfg(feature = "s3-storage")]
pub use s3_workspace::S3WorkspaceProvider;
pub use types::{FileEntry, StartingPoint, WorkspaceInfo};
pub use workspace::WorkspaceProvider;

use std::sync::Arc;

use crate::core::config;

/// Create a workspace provider based on the current configuration
///
/// If a profile is specified, uses that profile's storage configuration.
/// Otherwise uses the default profile or local storage.
///
/// # Arguments
///
/// * `profile` - Optional profile name to use for configuration
///
/// # Returns
///
/// An Arc-wrapped WorkspaceProvider implementation
pub fn create_workspace_provider(profile: Option<&str>) -> Arc<dyn WorkspaceProvider> {
    let (global_config, _) = config::Config::load().unwrap_or_default();

    // Get the profile configuration
    let profile_name = profile.unwrap_or(&global_config.default_profile);
    let profile_config = global_config.profiles.get(profile_name);

    // Check if we should use remote storage
    let use_remote = profile_config
        .map(|p| matches!(p.mode, config::OrchestratorMode::Remote))
        .unwrap_or(false);

    if use_remote {
        #[cfg(feature = "s3-storage")]
        {
            // Try to create S3 storage from config
            if global_config.storage.files == config::StorageBackend::S3 {
                // Create async runtime to initialize S3 storage
                if let Ok(rt) = tokio::runtime::Handle::try_current() {
                    let storage_config = global_config.storage.clone();
                    let storage = rt.block_on(async {
                        crate::core::storage::create_file_storage(&storage_config).await
                    });

                    if let Ok(storage) = storage {
                        return Arc::new(S3WorkspaceProvider::new(Arc::from(storage)));
                    }
                }
            }
        }

        // Fall back to local if S3 not configured or available
        tracing::warn!(
            "Remote profile '{}' specified but S3 storage not configured, using local storage",
            profile_name
        );
    }

    Arc::new(LocalWorkspaceProvider::new())
}

/// Create a local workspace provider (convenience function)
pub fn create_local_workspace_provider() -> Arc<LocalWorkspaceProvider> {
    Arc::new(LocalWorkspaceProvider::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_local_provider() {
        let provider = create_local_workspace_provider();
        assert!(provider
            .workspace_path("test")
            .to_string_lossy()
            .contains("test"));
    }

    #[test]
    fn test_create_workspace_provider_default() {
        let provider = create_workspace_provider(None);
        // Should get local provider by default
        assert!(provider
            .workspace_path("test")
            .to_string_lossy()
            .contains("test"));
    }
}
