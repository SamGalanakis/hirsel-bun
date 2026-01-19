//! Core modules for hirsel - shared between CLI and GUI
//!
//! These modules implement the business logic for hirsel, including:
//! - State management (SQLite)
//! - Configuration
//! - Git operations
//! - Chat/messaging system
//! - File utilities
//! - ACP client
//! - Eval system
//! - Orchestrator abstraction for local/remote coordination

pub mod acp;
pub mod api_types;
pub mod chat_orchestrator;
pub mod chat_session;
pub mod chats;
pub mod compaction;
pub mod config;
#[cfg(feature = "server")]
pub mod coordinator_api;
pub mod credentials;
pub mod eval;
pub mod files;
pub mod git;
#[cfg(feature = "server")]
pub mod git_http;
pub mod gyp_chat;
pub mod metrics;
pub mod names;
pub mod ops;
pub mod orchestrator;
pub mod process;
pub mod remote;
pub mod runner;
#[cfg(feature = "server")]
pub mod server;
pub mod state;
pub mod state_access;
pub mod tailscale;
#[cfg(feature = "server")]
pub mod tunnel;
pub mod workers;

// Re-export commonly used types
pub use acp::{
    ACPClientConfig, ACPError, AcpChild, AcpSpawnConfig, MCPServerConfig, SessionUpdate,
};
pub use chat_orchestrator::{
    create_chat_orchestrator, create_local_chat_orchestrator, ChatContext, ChatOrchestrator,
    ChatOrchestratorError, ChatOrchestratorResult, LocalChatOrchestrator, RemoteChatOrchestrator,
    SessionInfo,
};
pub use chat_session::{
    ChatEvent, ChatSessionConfig, ChatSessionError, ChatSessionManager, PendingPermission,
    PermissionOption, PermissionResponse, UIContext,
};
pub use chats::{ChatHeader, ChatMode};
pub use config::*;
pub use credentials::{
    get_local_oauth_credentials, CredentialError, CredentialResult, CredentialStore,
    ForwardedCredentials,
};
pub use eval::{
    run_eval_acp, run_eval_from_args, EvalAcpConfig, EvalAcpResult, EvalConfig, EvalError,
    EvalResult,
};
pub use files::Files;
pub use gyp_chat::{GypChatError, GypChatMessage, GypChatResult, GypChatStore};
pub use names::{
    generate_run_name, generate_unique_names, generate_worker_name, get_available_name,
    get_available_names,
};
pub use orchestrator::{
    create_local_orchestrator, create_orchestrator, LocalOrchestrator, Orchestrator,
    OrchestratorError, OrchestratorResult, RemoteOrchestrator,
};
pub use remote::{
    parse_remote_spec, parse_remote_specs, RemoteConfig, RemoteError, RemoteResult,
    RemoteWorkerSpawner,
};
pub use runner::{
    create_runner, LocalRunner, Runner, RunnerConfig, RunnerError, RunnerResult,
    SpawnResult as RunnerSpawnResult, SpriteRunner, SpriteRunnerConfig, SshRunner, SshRunnerConfig,
    WorkerHandle, WorkerSpawnConfig as RunnerSpawnConfig,
};
pub use state::*;
pub use state_access::{StateAccess, StateAccessError, StateAccessResult};
pub use workers::{
    check_and_send_time_notifications, check_time_expired, check_worker_heartbeats,
    get_agent_command, handle_time_expired, is_pid_alive, maybe_scale_up, maybe_trigger_eval,
    pause_all_workers, resume_awaiting_workers, spawn_worker, update_worker_heartbeat, SpawnResult,
    WorkerError, WorkerResult, WorkerScale, WorkerSpawnConfig,
};
