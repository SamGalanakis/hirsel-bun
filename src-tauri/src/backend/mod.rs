//! Backend modules for Hirsel.
//!
//! The app now centers on projects, a single shepherd conversation, visible
//! threads, and the canvas artifact.

pub mod api_types;
pub mod app;
pub mod config;
pub mod credentials;
pub mod db;
pub mod draft;
pub mod error;
pub mod git;
pub(crate) mod icons;
pub(crate) mod lash_tools;
pub mod llm_provider;
pub mod project;
pub mod sandbox;
#[cfg(feature = "server")]
pub mod server;
pub mod shepherd_chat;
pub mod shepherd_runtime;
pub mod shepherd_threads;
pub mod storage;
pub mod system;
pub mod webui;
pub mod workspace;

pub use config::*;
pub use credentials::{CredentialError, CredentialResult, CredentialStore, ForwardedCredentials};
pub use draft::{
    create_workspace_provider, FileEntry, LocalWorkspaceProvider, StartingPoint, WorkspaceInfo,
    WorkspaceProvider,
};
pub use error::{ErrorKind, HirselError, HirselResult};
pub use project::{
    CreateProjectRequest, Project, ProjectError, ProjectResult, ProjectStore, UpdateProjectRequest,
};
pub use sandbox::{ensure_docker_available, humanize_docker_error, SandboxConfig, SandboxError};
pub use shepherd_chat::{
    ShepherdChatError, ShepherdChatMessage, ShepherdChatResult, ShepherdChatStore, ShepherdLiveTurn,
};
pub use shepherd_threads::{
    ShepherdThread, ShepherdThreadError, ShepherdThreadResult, ShepherdThreadStore,
};
#[cfg(feature = "s3-storage")]
pub use storage::S3FileStorage;
pub use storage::{
    create_file_storage, FileStorage, LocalFileStorage, StorageError, StorageResult,
};
pub use workspace::{
    ensure_project_workspace, prepare_thread_checkout, workspace_name_for_project, ProjectWorkspace,
};
