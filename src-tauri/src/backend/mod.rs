//! Backend modules for Hirsel.
//!
//! The app now centers on projects, a single shepherd conversation, visible
//! threads, and document-backed knowledge surfaces such as the canvas.

pub mod api_types;
pub mod app;
pub mod config;
pub mod credentials;
pub mod db;
pub mod documents;
pub mod draft;
pub mod error;
pub mod git;
pub(crate) mod lash_tools;
pub mod librarian;
pub mod live_updates;
pub mod llm_provider;
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
pub mod workspace;

pub use config::*;
pub use credentials::{CredentialError, CredentialResult, CredentialStore, ForwardedCredentials};
pub use draft::{
    create_workspace_provider, FileEntry, LocalWorkspaceProvider, StartingPoint, WorkspaceInfo,
    WorkspaceProvider,
};
pub use error::{ErrorKind, HirselError, HirselResult};
pub use project::{
    ensure_project_runtime_preparation_started, get_project_runtime_preparation,
    project_runtime_is_ready, retry_project_runtime_preparation, CreateProjectRequest, Project,
    ProjectError, ProjectResult, ProjectRuntimePreparation, ProjectStore, UpdateProjectRequest,
};
pub use sandbox::{ensure_docker_available, humanize_docker_error, SandboxConfig, SandboxError};
pub use shepherd_chat::{
    ShepherdChatError, ShepherdChatMessage, ShepherdChatResult, ShepherdChatStore, ShepherdLiveTurn,
};
pub use shepherd_threads::{
    ShepherdThread, ShepherdThreadError, ShepherdThreadResult, ShepherdThreadStore,
};
pub use workspace::{
    ensure_project_workspace, ensure_thread_checkout, legacy_workspace_name_for_project_id,
    prepare_thread_checkout, workspace_name_for_project, ProjectWorkspace,
};
