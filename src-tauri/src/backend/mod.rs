//! Backend modules for Hirsel.
//!
//! This namespace contains the server, orchestration runtime, project and
//! route state, worker lifecycle, and other backend-owned services.

pub mod api_types;
pub mod app;
pub mod capabilities;
pub mod config;
#[cfg(feature = "server")]
pub mod daemon;
// Intentionally not `pub` — only used by webui.rs
pub mod conflict_resolver;
pub mod constants;
pub mod credentials;
pub mod db;
pub mod delivery;
pub(crate) mod delta;
pub mod draft;
pub mod error;
pub mod eval;
pub mod files;
pub mod forge;
pub mod git;
pub mod github;
pub mod http_client;
pub(crate) mod icons;
pub(crate) mod lash_tools;
pub(crate) mod lifecycle;
pub mod llm_provider;
pub mod mcp;
pub mod metrics;
pub mod names;
pub mod ops;
pub mod orchestrator;
pub mod process;
pub mod project;
pub mod route;
pub mod route_runtime;
pub mod runner;
pub mod scribe;
#[cfg(feature = "server")]
pub mod server;
pub mod shepherd;
pub mod shepherd_chat;
pub mod shepherd_runtime;
pub mod shepherd_threads;
pub mod snapshot;
pub mod state;
pub mod state_access;
pub mod storage;
pub mod system;
pub mod webui;
pub mod worker_concerns;
pub mod workers;
pub mod worktree;

// Re-export commonly used types
pub use config::*;
pub use credentials::{CredentialError, CredentialResult, CredentialStore, ForwardedCredentials};
pub use error::{ErrorKind, HirselError, HirselResult};
pub use eval::{run_eval, run_eval_from_args, EvalAcpConfig, EvalAcpResult, EvalError};
pub use files::Files;
pub use names::{
    generate_runtime_name, generate_worker_name, get_available_name, get_available_names, slugify,
};
pub use orchestrator::{
    create_local_orchestrator, create_orchestrator, LocalOrchestrator, Orchestrator,
    OrchestratorError, OrchestratorResult, RemoteOrchestrator,
};
pub use project::{
    CreateProjectRequest, Project, ProjectError, ProjectResult, ProjectStore, UpdateProjectRequest,
};
pub use runner::{
    create_runner, LocalRunner, Runner, RunnerConfig, RunnerError, RunnerResult,
    SpawnResult as RunnerSpawnResult, WorkerHandle, WorkerSpawnConfig as RunnerSpawnConfig,
};
pub use scribe::{process_scribe_batch, should_process_batch, ScribeBatchResult, ScribeError};
pub use shepherd_chat::{
    ShepherdChatError, ShepherdChatMessage, ShepherdChatResult, ShepherdChatStore,
    ShepherdQueuedTurn,
};
pub use shepherd_threads::{
    ShepherdThread, ShepherdThreadError, ShepherdThreadResult, ShepherdThreadStore,
};

// Conflict resolver
#[cfg(feature = "s3-storage")]
pub use snapshot::S3ArchiveStrategy;
pub use snapshot::{
    create_archive_strategy, AgentSnapshot, ArchiveHandle, ArchiveResult, ArchiveStrategy,
    NoOpArchiveStrategy, SnapshotError, SnapshotResult, WorkDirSnapshot, WorkerStateHandle,
};
pub use state::*;
pub use state_access::{StateAccess, StateAccessError, StateAccessResult};
#[cfg(feature = "s3-storage")]
pub use storage::S3FileStorage;
pub use storage::{
    create_file_storage, FileStorage, LocalFileStorage, StorageError, StorageResult,
};
pub use worker_concerns::{
    CreateWorkerConcernRequest, WorkerConcern, WorkerConcernError, WorkerConcernResult,
    WorkerConcernStore,
};
pub use workers::{
    check_and_send_time_notifications, check_worker_heartbeats, get_agent_command, is_pid_alive,
    spawn_worker, update_worker_heartbeat, SpawnResult, WorkerError, WorkerResult,
    WorkerSpawnConfig,
};
pub use worktree::{AgentRef, WorkItem, WorkItemTree, WorkTreeSnapshot};

// Draft workspace management
pub use draft::{
    create_workspace_provider, FileEntry, LocalWorkspaceProvider, StartingPoint, WorkspaceInfo,
    WorkspaceProvider,
};

// GitHub client
pub use github::{GitHubClient, GitHubError, GitHubResult, MergeInfo, PrInfo};

// Delivery service
pub use delivery::{
    delivery_branch_name, pr_body, pr_title, DeliveryError, DeliveryOrchestrator, DeliveryResult,
    DeliveryState, DeliveryStatus, GitOperations, PushResult,
};

// Forge providers
pub use capabilities::CapabilityProfile;
pub use forge::{
    create_forge_for_remote, ForgeError, ForgeProvider, ForgeResult, GitHubForge,
    MergeResult as ForgeMergeResult, PrInfo as ForgePrInfo,
};

// Route management
pub use route::{
    CreateRouteRequest, Route, RouteError, RouteFiles, RouteResult, RouteStore, RouteTree,
};
pub use route_runtime::{
    ensure_route_runtime, get_route_runtime_name, runtime_name_for_route, RouteRuntimeHandle,
};
