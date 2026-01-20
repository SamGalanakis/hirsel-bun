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
#[cfg(feature = "claude")]
pub mod claude_cli;
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
pub mod lifecycle;
pub mod metrics;
pub mod names;
pub mod ops;
pub mod orchestrator;
pub mod process;
pub mod runner;
#[cfg(feature = "server")]
pub mod server;
pub mod state;
pub mod state_access;
pub mod storage;
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
#[cfg(feature = "claude")]
pub use claude_cli::{
    execute_claude_worker, run_claude_worker, BridgeEvent, ClaudeCliBridge, ClaudeCliConfig,
    ClaudeCliError, ClaudeWorkerConfig, WorkerResult as ClaudeWorkerResult,
};
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
pub use lifecycle::{
    create_lifecycle_manager, LifecycleAction, LifecycleContext, LifecycleError, LifecycleEvent,
    LifecycleManager, LifecycleResult, LocalLifecycleManager, RemoteLifecycleManager,
    RunStateMachine, WorkerStateMachine,
};
pub use names::{
    generate_run_name, generate_unique_names, generate_worker_name, get_available_name,
    get_available_names, slugify,
};
pub use orchestrator::{
    create_local_orchestrator, create_orchestrator, LocalOrchestrator, Orchestrator,
    OrchestratorError, OrchestratorResult, RemoteOrchestrator,
};
pub use runner::{
    create_runner, parse_remote_spec, parse_remote_specs, LocalRunner, Runner, RunnerConfig,
    RunnerError, RunnerResult, SpawnResult as RunnerSpawnResult, SpriteRunner, SpriteRunnerConfig,
    SshRunner, SshRunnerConfig, WorkerHandle, WorkerSpawnConfig as RunnerSpawnConfig,
};
pub use state::*;
pub use state_access::{StateAccess, StateAccessError, StateAccessResult};
#[cfg(feature = "s3-storage")]
pub use storage::S3FileStorage;
pub use storage::{
    create_default_local_storage, create_file_storage, create_local_storage, FileStorage,
    LocalFileStorage, StorageError, StorageResult,
};
pub use workers::{
    check_and_send_time_notifications, check_worker_heartbeats, get_agent_command, is_pid_alive,
    spawn_worker, update_worker_heartbeat, SpawnResult, WorkerError, WorkerResult, WorkerScale,
    WorkerSpawnConfig,
};
