//! Backend modules for Hirsel.
//!
//! The app now centers on projects, a single shepherd conversation, visible
//! threads, and document-backed knowledge surfaces such as the canvas.

pub mod api_types;
pub mod app;
pub mod app_settings;
pub mod config;
pub mod credentials;
#[cfg(feature = "host")]
pub mod db;
#[cfg(feature = "host")]
pub mod documents;
pub mod draft;
pub mod error;
#[cfg(feature = "host")]
pub mod git;
#[cfg(feature = "host")]
pub(crate) mod knowledge_graph;
pub(crate) mod lash_tools;
pub mod librarian;
pub mod live_updates;
pub mod llm_provider;
#[cfg(feature = "host")]
pub(crate) mod plans;
pub mod project;
pub mod sandbox;
#[cfg(feature = "server")]
pub mod server;
pub mod shepherd_chat;
pub mod shepherd_runtime;
pub mod shepherd_threads;
pub mod skills;
pub mod system;
pub(crate) mod text_patch;
pub(crate) mod tool_results;
#[cfg(feature = "host")]
pub mod workspace;

#[cfg(feature = "host")]
pub use app_settings::AppSettingsStore;
pub use app_settings::LlmSettings;
pub use config::*;
pub use credentials::ForwardedCredentials;
#[cfg(feature = "host")]
pub use credentials::{CredentialError, CredentialResult, CredentialStore};
pub use draft::StartingPoint;
#[cfg(feature = "host")]
pub use draft::{
    create_workspace_provider, FileEntry, LocalWorkspaceProvider, WorkspaceInfo, WorkspaceProvider,
};
pub use error::{ErrorKind, HirselError, HirselResult};
#[cfg(feature = "host")]
pub use project::{
    get_project_runtime_preparation, project_runtime_is_ready, retry_project_runtime_preparation,
    start_project_runtime_preparation,
};
pub use project::{
    legacy_workspace_name_for_project_id, workspace_name_for_project, CreateProjectRequest,
    Project, ProjectRuntimePreparation, UpdateProjectRequest,
};
#[cfg(feature = "host")]
pub use project::{ProjectError, ProjectResult, ProjectStore};
pub use sandbox::{ensure_docker_available, humanize_docker_error, SandboxConfig, SandboxError};
#[cfg(feature = "host")]
pub use shepherd_chat::{ShepherdChatError, ShepherdChatResult};
pub use shepherd_chat::{ShepherdChatMessage, ShepherdChatStore, ShepherdLiveTurn};
pub use shepherd_threads::{ShepherdThread, ShepherdThreadStore};
#[cfg(feature = "host")]
pub use shepherd_threads::{ShepherdThreadError, ShepherdThreadResult};
#[cfg(feature = "host")]
pub use workspace::{
    ensure_project_workspace, ensure_thread_checkout, prepare_thread_checkout, ProjectWorkspace,
};
