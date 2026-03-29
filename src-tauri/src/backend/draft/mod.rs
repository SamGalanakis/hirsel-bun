//! Draft workspace management
//!
//! This module provides abstractions for managing draft workspaces on the local host.
//!
//! # Architecture
//!
//! - **WorkspaceProvider**: Trait that abstracts storage operations
//! - **LocalWorkspaceProvider**: Filesystem-based storage (default)
//! - **StartingPoint**: Enum defining how workspaces are initialized
//!
//! # Usage
//!
//! ```rust,ignore
//! use hirsel_lib::core::draft::{create_workspace_provider, StartingPoint};
//!
//! // Create provider for the local host
//! let provider = create_workspace_provider();
//!
//! // Initialize a workspace from a git repo
//! let info = provider.init("my-run", &StartingPoint::GitRepo {
//!     url: "https://github.com/user/repo".into(),
//!     branch: Some("main".into()),
//! }).await?;
//! ```

mod local_workspace;
mod types;
mod workspace;

pub use local_workspace::LocalWorkspaceProvider;
pub use types::{FileEntry, StartingPoint, WorkspaceInfo};
pub use workspace::WorkspaceProvider;

use std::sync::Arc;

/// Create a workspace provider based on the current configuration
pub fn create_workspace_provider() -> Arc<dyn WorkspaceProvider> {
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
        let provider = create_workspace_provider();
        // Should get local provider by default
        assert!(provider
            .workspace_path("test")
            .to_string_lossy()
            .contains("test"));
    }
}
