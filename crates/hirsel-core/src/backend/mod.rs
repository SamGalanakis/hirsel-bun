//! Backend modules for Hirsel.

pub mod api_types;
pub mod app;
pub mod app_settings;
pub mod companion_actions;
pub mod config;
pub mod credentials;
pub mod db;
pub mod documents;
pub mod error;
pub(crate) mod knowledge_graph;
pub(crate) mod lash_tools;
pub mod librarian;
pub mod librarian_events;
pub mod live_updates;
pub mod llm_provider;
pub(crate) mod plans;
pub mod project;
pub mod project_focus;
pub(crate) mod prompts;
pub mod sandbox;
pub mod server;
pub mod shepherd_chat;
pub mod shepherd_events;
pub mod shepherd_runtime;
pub mod shepherd_threads;
pub mod skills;
pub mod system;
pub mod tasks;
pub(crate) mod text_patch;
pub(crate) mod tool_results;

pub use app_settings::AppSettingsStore;
pub use app_settings::LlmSettings;
pub use config::*;
pub use credentials::{CredentialError, CredentialResult, CredentialStore};
pub use error::{ErrorKind, HirselError, HirselResult};
pub use project::{CreateProjectRequest, Project, ProjectWorkspaceEntry, UpdateProjectRequest};
pub use project::{ProjectError, ProjectResult, ProjectStore};
pub use sandbox::{ensure_docker_available, humanize_docker_error, SandboxConfig, SandboxError};
pub use shepherd_chat::{ShepherdChatError, ShepherdChatResult};
pub use shepherd_chat::{
    ShepherdChatMessage, ShepherdChatMessageOptions, ShepherdChatStore, ShepherdLiveTurn,
    MESSAGE_KIND_CHAT, MESSAGE_KIND_SHEPHERD_SYNC,
};
pub use shepherd_threads::{ShepherdThread, ShepherdThreadStore};
pub use shepherd_threads::{ShepherdThreadError, ShepherdThreadResult};
