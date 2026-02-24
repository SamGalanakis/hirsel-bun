# Hirsel Architecture Reference

> **Keep this document updated** when modifying core modules.

## Quick Reference

### Common Modification Points

| Task | Files to Modify |
|------|-----------------|
| Add CLI command | `src-tauri/src/cli/mod.rs` → `Commands` enum, new `src-tauri/src/cli/<cmd>.rs` |
| Add GUI command | `src-tauri/src/gui/commands/mod.rs` → `get_handlers()`, new handler in relevant submodule |
| Add REST endpoint | `src-tauri/src/core/server/mod.rs` → router, `src-tauri/src/core/server/routes.rs` |
| Modify run state | `src-tauri/src/core/state/mod.rs` → `SCHEMA` const, `src-tauri/src/core/state/types.rs` |
| Add runner type | `src-tauri/src/core/runner/mod.rs` → `create_runner()`, new `src-tauri/src/core/runner/<type>.rs` |
| Modify lifecycle | `src-tauri/src/core/lifecycle/mod.rs`, `src-tauri/src/core/lifecycle/local.rs` |
| Add archive strategy | `src-tauri/src/core/snapshot/mod.rs`, new strategy impl of `ArchiveStrategy` |
| Add service worker | `src-tauri/src/core/service_worker/scribe.rs`, `src-tauri/src/cli/service_worker.rs` |
| Add board node | `src-tauri/src/core/delta/state/nodes.rs`, `src-tauri/src/gui/commands/delta.rs` |
| Modify board UI | `src/components/specflow/SpecBoard.tsx`, `src/components/layout/CanvasToolbar.tsx` |
| Add orchestrator method | `src-tauri/src/core/orchestrator/mod.rs` → trait, `local.rs`, `daemon.rs`, `remote.rs` impls |
| Modify dispatch | `src-tauri/src/core/delta/dispatch.rs`, `src-tauri/src/daemon/lifecycle.rs` |

### Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `gui` | Tauri desktop app (includes `cli`) | Yes |
| `cli` | Full CLI (includes `server` + TUI attach) | No (implied by `gui`) |
| `server` | HTTP server, daemon | No (implied by `cli`) |
| `worker` | Minimal remote worker binary | No |
| `s3-storage` | S3-compatible storage backend | No |
| `profiling` | Backend tracing-chrome + frontend IPC instrumentation | No |

```bash
cargo build                                     # Full GUI
cargo build --no-default-features -F cli        # CLI only
cargo build --no-default-features -F worker     # Remote worker
cargo build --features s3-storage               # With S3 support
```

---

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Interface                            │
├──────────────────────┬──────────────────────┬───────────────────┤
│   CLI (hirsel)       │   GUI (Tauri)        │   HTTP Server     │
└──────────┬───────────┴──────────┬───────────┴─────────┬─────────┘
           │                      │                     │
           ▼                      ▼                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Daemon (background process)                   │
│   - Lifecycle polling, eval triggering, time limits             │
│   - Worker spawning via Orchestrator                            │
│   - Auto-exit when idle                                         │
└──────────────────────────────┬──────────────────────────────────┘
                               │
           ┌───────────────────┼───────────────────┐
           ▼                   ▼                   ▼
    LocalOrchestrator   DaemonOrchestrator   RemoteOrchestrator
    (direct access)     (TCP)                (HTTP API)
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│   SQLite State              │   Worker Processes                 │
│   (runs, workers, tasks,    │   (Local, SSH, Fly)                │
│    messages, events)        │   + optional Docker container      │
└─────────────────────────────┴───────────────────────────────────┘
```

---

## Module Map

### `src-tauri/src/core/` - Core Business Logic

| Submodule | Key Files | Purpose |
|-----------|-----------|---------|
| `state/` | `mod.rs`, `types.rs`, `run.rs`, `workers.rs`, `messages.rs`, `events.rs`, `evals.rs`, `history.rs`, `scribe.rs` | SQLite state management (per-run) |
| `project/` | `mod.rs`, `types.rs`, `store.rs` | Project database (global), SpecFlow per-project configuration |
| `board/` | `mod.rs`, `types.rs`, `storage.rs`, `mcp.rs` | SpecFlow board data (tasks, evals, task tree, file sync, MCP server for Shepherd) |
| `github/` | `mod.rs` | GitHub API client (octocrab) with auth fallback (env → gh config → hirsel config) |
| `forge/` | `mod.rs` → `ForgeProvider` trait, `github.rs` | Extensible forge abstraction for PR/merge operations (currently GitHub only) |
| `dispatch/` | `mod.rs` | Dispatch service: creates runs from board tasks, generates spec/eval, creates work+eval tasks with validated_by relationship |
| `delta/` | `mod.rs`, `dispatch.rs`, `state/`, `types.rs`, `export.rs` | Board tree: unified node tree (spec/task/eval), dispatch, persistent project runs |
| `route/` | `mod.rs`, `types.rs`, `store.rs`, `files.rs` | Route management for parallel project exploration (forking, route-scoped trees/docs/messages) |
| `delivery/` | `mod.rs`, `orchestrator.rs` → `DeliveryOrchestrator`, `git_ops.rs` → `GitOperations`, `workspace.rs` | Delivery orchestration: three-tier delivery (push/PR/merge), git operations, workspace resolution |
| `orchestrator/` | `mod.rs` → `Orchestrator` trait, `local.rs`, `remote.rs`, `daemon.rs` | Run orchestration pattern |
| `lifecycle/` | `mod.rs` → `LifecycleManager` trait, `local.rs`, `remote.rs`, `transitions.rs` | Event-driven state machine |
| `runner/` | `types.rs` → `Runner` trait, `local.rs`, `fly.rs`, `ssh.rs`, `composed.rs`, `config.rs`, `setup.rs` | Worker host implementations |
| `run_manager/` | `mod.rs`, `local.rs`, `remote.rs` | Unified run management wrapping Orchestrator + Lifecycle |
| `snapshot/` | `mod.rs`, `archive.rs`, `noop.rs`, `s3.rs`, `agent_session.rs` | Work/session persistence |
| `shepherd/` | `mod.rs` | Shepherd orchestration domain primitives (decisions/commands) |
| `draft/` | `mod.rs`, `types.rs` → `StartingPoint`, `workspace.rs`, `local_workspace.rs`, `s3_workspace.rs` | StartingPoint, workspace init |
| `config/` | `mod.rs`, `store.rs`, `loader.rs`, `saver.rs`, `types.rs`, `agent.rs`, `storage.rs`, `orchestrator.rs`, `paths.rs` | Config struct, DB storage, profiles, runners |
| `ops/` | `mod.rs`, `run.rs`, `setup.rs`, `spawn.rs`, `project.rs`, `docs.rs`, `types.rs` | Shared CLI/GUI operations |
| `server/` | `mod.rs` → `start_server()`, `routes.rs`, `shared_routes.rs`, `auth.rs`, `board.rs`, `worker_routes.rs`, `eval_routes.rs` | HTTP server for remote mode |
| `eval/` | `mod.rs`, `runner.rs`, `parser.rs`, `types.rs` | Eval runner and parsing |
| `storage/` | `mod.rs` | File storage abstraction (local/S3) |
| `service_worker/` | `mod.rs`, `scribe.rs`, `conflict_resolver.rs`, `types.rs` | Service workers: ScribeService for documentation, ConflictResolverServiceWrapper for merge conflicts |
| `conflict_resolver/` | `mod.rs`, `state.rs` | Git conflict resolution with AI agent |
| `db.rs` | - | Shared SQLite connection utilities (open_db, open_global_db, utc_now, etc.) |
| `error.rs` | - | `HirselError` enum with `ErrorKind` categorization |
| `state_access.rs` | - | Worker state abstraction (SQLite vs HTTP) |
| `metrics.rs` | - | Session metrics extraction with TTL cache |
| `files.rs` | - | Run directory file operations |
| `chats.rs` | - | Chat message storage |
| `shepherd_chat.rs` | - | Project-level Shepherd chat history |
| `project_messages.rs` | - | Sheepfold: route-scoped messaging (Meadow group chat + worker DMs), requires route_id |
| `api_types.rs` | - | Shared API response types |
| `git_http.rs` | - | Git Smart HTTP backend for remote worker git access (`#[cfg(feature = "server")]`) |
| `http_client.rs` | - | Shared HTTP client utilities |
| `constants.rs` | - | Application constants |
| `process.rs` | - | Process management utilities |
| `scribe.rs` | - | Scribe agent prompt and batch processing |
| `credentials.rs` | - | Encrypted credential store |
| `git.rs` | - | Git operations |
| `tailscale.rs` | - | Tailscale integration |

### `src-tauri/src/worker/` - Worker Subprocess

| File | Purpose |
|------|---------|
| `common.rs` | Worker prompt and run configuration builders |
| `lash_runner.rs` | Embedded lash-core runtime bootstrap and event persistence |
| `runner.rs` | Worker MCP-backed operations (`WorkerRunner`) used by lash tools |
| `mcp.rs` | MCP server for worker tools (including eval_pass, eval_fail) |
| `eval_mcp.rs` | Eval MCP server for evaluation tasks |
| `remote_runner.rs` | Remote worker entry point |
| `http_state.rs` | HTTP-based state for remote workers |
| `file_server.rs` | File upload server for remote workers (`server`/`worker` feature) |

**MCP Worker Tools** (available to all workers):
- Live Node Management: `get_task_tree`, `get_available_tasks`, `get_my_tasks`, `get_task_details`, `complete_task`, `add_task`, `add_check`, `delete_task`
- Communication: `list_contacts`, `chat_history`, `chat_send`, `chat_unread`
- Documentation: `scribe`, `read_docs`
- Work Management: `work_done` (signal ready for next task), `time_status`

**Note:** These tools operate on **board_nodes** in the global database (`~/.hirsel/hirsel.db`), not the old per-run SQLiteState tasks.

**MCP Eval Tools** (available to eval tasks):
- `eval_pass` - Mark eval as passed, validate tasks where all evals in `validated_by` pass
- `eval_fail(feedback)` - Mark eval as failed, create repair task as child of eval

**Note:** Workers receive pre-assigned tasks at spawn time. There is no `claim_task` tool - task assignment is handled by the coordinator via direct assignment.

### `src-tauri/src/gui/commands/` - Tauri IPC Commands

| File | Commands |
|------|----------|
| `runs.rs` | `get_runs`, `get_run_detail`, `pause_run`, `resume_run`, `delete_run`, `delete_all_runs`, `deliver_run` |
| `drafts.rs` | `validate_repo`, `create_draft`, `clone_run`, `update_draft`, `start_draft`, `change_starting_point` |
| `workers.rs` | `get_workers`, `attach_worker`, `open_worker_terminal`, `detach_worker`, `restart_worker` |
| `messages.rs` | `get_messages`, `get_threads`, `get_all_unread_notifications`, `send_message`, `mark_messages_read` |
| `events.rs` | `get_worker_events`, `clear_worker_events`, `start_worker_event_stream`, `stop_worker_event_stream` |
| `shepherd.rs` | `start_shepherd_session`, `send_shepherd_message`, `stop_shepherd_session`, `list_shepherd_sessions`, `get_shepherd_history`, `clear_shepherd_history`, `save_shepherd_message` |
| `config_cmd.rs` | `get_config`, `save_config`, `get_tailscale_info`, `check_ssh_runner` |
| `credentials.rs` | `store_credential`, `delete_credential`, `has_credential`, `get_credential`, `get_credential_masked` |
| `files.rs` | `read_spec_file`, `write_spec_file`, `read_eval_file`, `write_eval_file`, `save_asset`, `import_asset_from_path`, `open_assets_folder`, `get_assets_path` |
| `logs.rs` | `get_eval_log`, `get_eval_log_by_path`, `get_history`, `get_eval_spec`, `get_evals` |
| `filesystem.rs` | `pick_folder`, `suggest_paths` |
| `debug.rs` | `log_frontend`, `log_frontend_batch`, `get_version`, `get_process_counts`, `kill_orphaned_worker_processes`, `get_daemon_health`, `get_profiling_enabled` |
| `projects.rs` | `list_projects`, `get_project`, `create_project_from_path`, `delete_project` |
| `delta.rs` | `get_board_tree`, `create_board_node`, `update_board_node`, `delete_board_node`, `move_board_node`, `reset_project_tree`, `start_shepherd_run`, `get_project_run`, `complete_board_node`, `sync_shepherd_changes`, `sync_and_get_shepherd_view`, `sync_and_get_shepherd_view_if_changed` |
| `delivery.rs` | `get_delivery_state`, `check_merge_state`, `get_conflicting_files`, `check_staleness`, `push_run_branch`, `create_run_pr`, `auto_merge_run`, `generate_pr_title`, `generate_pr_body`, `delivery_branch_name`, `get_board_versions`, `get_latest_board_version`, `get_current_board_delivery`, `start_board_delivery`, `get_board_delivery_status`, `retry_board_delivery`, `get_delivery_attempts`, `complete_board_delivery`, `abandon_board_delivery` |
| `routes.rs` | `list_routes`, `get_route`, `get_route_by_name`, `get_route_tree`, `create_route`, `delete_route`, `set_active_route`, `get_active_route` |
| `project_messages.rs` | `get_project_messages`, `get_project_threads`, `send_project_message`, `mark_project_messages_read`, `get_project_unread_count` |
| `docs.rs` | `get_project_docs` |
| `ide.rs` | `open_in_ide` |

### `src-tauri/src/cli/` - CLI Commands

| File | Command | Feature |
|------|---------|---------|
| `runs.rs` | `hirsel runs` | - |
| `view.rs` | `hirsel view <run>` | - |
| `log.rs` | `hirsel log <run>` | - |
| `attach.rs` | `hirsel attach <run>` | `cli` |
| `pause.rs` | `hirsel pause <run>` | - |
| `resume.rs` | `hirsel resume <run>` | - |
| `delete.rs` | `hirsel delete <run>` | - |
| `deliver.rs` | `hirsel deliver <run>` | - |
| `msg.rs` | `hirsel msg <run>` | - |
| `tasks.rs` | `hirsel tasks <run>` - View board nodes for a run | - |
| `diff.rs` | `hirsel diff <run>` | - |
| `summary.rs` | `hirsel summary <run>` | - |
| `spec.rs` | `hirsel spec <run>` | - |
| `asset.rs` | `hirsel asset <run>` | - |
| `config.rs` | `hirsel config` | - |
| `prune.rs` | `hirsel prune` | - |
| `reset.rs` | `hirsel reset` | - |
| `templates.rs` | `hirsel templates` | - |
| `man.rs` | `hirsel man` | - |
| `completions.rs` | `hirsel completions` | - |
| `clone.rs` | `hirsel clone <run> <new_name>` | - |
| `scribe.rs` | `hirsel scribe <run>` | - |
| `helpers.rs` | Shared helper functions | - |
| `tui.rs` | Terminal UI for `attach` command | `cli` |
| `mod.rs` | `hirsel mode <run>`, `hirsel amend <run>` (inline) | - |
| `service_worker.rs` | `hirsel __service-worker --type scribe` | `cli` |
| `mod.rs` | `hirsel serve` (inline) | `server` |

### `src-tauri/src/daemon/` - Background Process

| File | Purpose |
|------|---------|
| `mod.rs` | Socket/PID paths, `is_daemon_running()` |
| `server.rs` | Daemon server, TCP listener |
| `lifecycle.rs` | Polling loop, lifecycle action handling |
| `client.rs` | Client for daemon communication |

### `src/` - Frontend (SolidJS)

| Directory | Purpose |
|-----------|---------|
| `components/layout/` | Layout, TitleBar, StatusBar, LeftDrawer, CanvasToolbar, RadialMenu, ProjectSelector, Notifications, SvgDefinitions, WelcomeScreen |
| `components/runs/` | RunListPanel, RunListItem, WorkerCard, WorkerDetailModal, ActivityLog, tabs/ |
| `components/specflow/` | SpecBoard, DeliveryDialog, RouteSelector, ForkRouteDialog, RunStatusPill, TaskEditorModal |
| `components/modals/` | SettingsModal, ConfirmDialog, AttachPicker |
| `components/chat/` | ShepherdConsole |
| `components/messaging/` | MessagingPanel (right drawer for route-scoped messaging: Meadow group chat + worker DMs) |
| `stores/` | AppProvider, ProjectProvider, RunsProvider, SelectionProvider, DeltaProvider, RouteProvider |
| `hooks/` | useClickOutside, useElapsedTime, useEscapeKey, useShepherdChat, useModalClosing, usePolling, useWindowEvent |
| `lib/` | Icons, theme, toast, dev-logger, utils, API helpers |
| `lib/api.ts` | Tauri invoke wrappers: `safeInvoke`, `safeInvokeWithToast`, polling utilities |
| `lib/elk-layout.ts` | ELK.js wrapper for hierarchical graph layout with orthogonal edge routing |

**UI Hierarchy:**
- **TitleBar** - App-level: app name, notifications, app settings
- **LeftDrawer** - Navigation: project selector, route list (collapsible)
- **CanvasToolbar** - Route-level: run status, worker avatars with hover shortcuts
- **SpecBoard** - Canvas with unified board tree (specs, tasks, evals)
- **Right panels** - DocsPanel and MessagingPanel slide in from right

**SpecBoard Architecture:**
- `SpecBoard.tsx` - Unified canvas component handling:
  - Single board tree visualization (specs, tasks, evals in one tree)
  - Node rendering with kind-based styling (spec/task/eval) and status-based glows
  - Context menus for draft node editing (edit/delete only for status=draft)
  - Drag-and-drop node reordering
  - Keyboard navigation and shortcuts
  - Filter: spec nodes (source=user) vs worker tasks (source=plan/worker/system)
  - Granularity filter (all levels, level 2, top only) - collapses tree depth
- **Graph Layout** - ELK.js (Eclipse Layout Kernel) for hierarchical graph layout:
  - `src/lib/elk-layout.ts` - ELK wrapper with orthogonal edge routing
  - Layered algorithm with proper crossing minimization
  - Uses tree structure (parent-child) + validates/blockedBy edges for positioning
- **DependencyConnectors** - SVG polylines for dependency edges:
  - `validates` (eval→task, computed from validated_by): sage green lines
  - `blockedBy` (task→task): terra red lines
  - `resolves` (repair→eval): blue lines
- **DeliveryDialog** - Dialog for delivering changes:
  - Auto-generates summary from completed root spec nodes
  - User can edit summary before creating PR
  - Summary becomes PR body

---

## Key Traits

### `Orchestrator` (`src-tauri/src/core/orchestrator/mod.rs`)

High-level run management interface. CLI, GUI, and server use this trait.

```rust
pub trait Orchestrator: Send + Sync {
    async fn list_runs(&self) -> OrchestratorResult<Vec<RunSummary>>;
    async fn get_run(&self, name: &str) -> OrchestratorResult<RunDetail>;
    async fn delete_run(&self, name: &str) -> OrchestratorResult<()>;
    async fn pause_run(&self, name: &str) -> OrchestratorResult<()>;
    async fn resume_run(&self, name: &str, time_limit: Option<u32>) -> OrchestratorResult<()>;
    async fn list_workers(&self, run: &str) -> OrchestratorResult<Vec<Worker>>;
    async fn start_run(&self, request: StartRunRequest) -> OrchestratorResult<RunDetail>;
    async fn init_workspace(&self, run: &str, request: InitWorkspaceRequest) -> OrchestratorResult<InitWorkspaceResponse>;
    async fn spawn_single_worker(&self, run: &str, worker: &str, work_dir: &Path, session_id: Option<&str>) -> OrchestratorResult<()>;
    async fn resume_worker(&self, run: &str, worker: &str, work_dir: &Path, session_id: Option<&str>, state: Option<&WorkerStateHandle>) -> OrchestratorResult<()>;
    // ... more methods
}
```

**Note:** Task management has been removed from the Orchestrator trait. Workers now interact with board_nodes directly via StateAccess methods.

| Implementation | Location | Use Case |
|----------------|----------|----------|
| `LocalOrchestrator` | `orchestrator/local.rs` | Direct SQLite access (daemon, server) |
| `DaemonOrchestrator` | `orchestrator/daemon.rs` | TCP to daemon (CLI/GUI local mode) |
| `RemoteOrchestrator` | `orchestrator/remote.rs` | HTTP API to remote server |

### `LifecycleManager` (`src-tauri/src/core/lifecycle/mod.rs`)

Centralized lifecycle state machine. Returns actions for daemon to execute.

```rust
pub trait LifecycleManager {
    fn process_event(&self, event: LifecycleEvent) -> LifecycleResult<Vec<LifecycleAction>>;
    fn pause_run(&self, reason: &str) -> LifecycleResult<Vec<String>>;
    fn resume_run(&self) -> LifecycleResult<Vec<LifecycleAction>>;
    fn worker_done(&self, worker_name: &str) -> LifecycleResult<Vec<LifecycleAction>>;
    fn should_trigger_eval(&self) -> LifecycleResult<bool>;
    fn run_status(&self) -> LifecycleResult<Status>;
}
```

| Event | Action(s) |
|-------|-----------|
| `TimeCheck` | `EvalTriggered`, `RunFailed`, `TimeWarning` |
| `WorkerDone` | `EvalTriggered`, `RunCompleted` |
| `PauseRequested` | `WorkersPaused`, `RunStatusChanged` |
| `ResumeRequested` | (handled via scaling check) |
| `ScalingCheck` | `SpawnWorker(assigned_task_id)`, `WakeWorker(assigned_task_id)` |

| Implementation | Location | Use Case |
|----------------|----------|----------|
| `LocalLifecycleManager` | `lifecycle/local.rs` | Local/daemon mode |
| `RemoteLifecycleManager` | `lifecycle/remote.rs` | Remote workers (delegates to coordinator) |

### `Runner` (`src-tauri/src/core/runner/types.rs`)

Worker spawning interface. Host + optional Container model.

```rust
#[async_trait]
pub trait Runner: Send + Sync {
    async fn spawn(&self, config: &WorkerSpawnConfig) -> RunnerResult<SpawnResult>;
    async fn stop(&self, handle: &WorkerHandle) -> RunnerResult<()>;
    async fn is_alive(&self, handle: &WorkerHandle) -> bool;
    fn runner_type(&self) -> &'static str;
    async fn setup(&self) -> RunnerResult<()>;
    async fn cleanup(&self) -> RunnerResult<()>;
}

// WorkerSpawnConfig includes assigned_task_id for direct task assignment
pub struct WorkerSpawnConfig {
    pub worker_name: String,
    pub work_dir: PathBuf,
    pub assigned_task_id: Option<String>,  // Pre-assigned task
    // ... other fields
}
```

| Implementation | Location | Host Type | Ephemeral |
|----------------|----------|-----------|-----------|
| `LocalRunner` | `runner/local.rs` | Local machine | No |
| `SshRunner` | `runner/ssh.rs` | Remote via SSH | No |
| `FlyRunner` | `runner/fly.rs` | Fly.io machine | Yes |
| `ComposedRunner` | `runner/composed.rs` | Executor + Resource | Varies |

### `ArchiveStrategy` (`src-tauri/src/core/snapshot/archive.rs`)

Unified directory archiving for pause/resume. Replaces the separate `SnapshotStrategy` and `AgentSessionStorage` traits.

```rust
#[async_trait]
pub trait ArchiveStrategy: Send + Sync {
    async fn archive(&self, key: &str, source_dir: &Path) -> ArchiveResult<ArchiveHandle>;
    async fn restore(&self, handle: &ArchiveHandle, target_dir: &Path) -> ArchiveResult<()>;
    async fn delete(&self, handle: &ArchiveHandle) -> ArchiveResult<()>;
    fn strategy_type(&self) -> &'static str;
}
```

| Implementation | Location | Host Types | Requirement |
|----------------|----------|------------|-------------|
| `NoOpArchiveStrategy` | `snapshot/noop.rs` | Local, SSH, Client | None (files persist on disk) |
| `S3ArchiveStrategy` | `snapshot/s3.rs` | Fly | `s3-storage` feature |

### `WorkspaceProvider` (`src-tauri/src/core/draft/workspace.rs`)

Workspace initialization from StartingPoint.

```rust
#[async_trait]
pub trait WorkspaceProvider: Send + Sync {
    async fn init(&self, run_name: &str, starting_point: &StartingPoint) -> Result<WorkspaceInfo>;
    fn workspace_path(&self, run_name: &str) -> PathBuf;
}
```

| Implementation | Location | Storage |
|----------------|----------|---------|
| `LocalWorkspaceProvider` | `draft/local_workspace.rs` | Filesystem |
| `S3WorkspaceProvider` | `draft/s3_workspace.rs` | S3 (feature-gated) |

---

## Cross-Cutting Patterns

### Error Handling (`src-tauri/src/core/error.rs`)

Unified error hierarchy for the codebase.

- `HirselError` enum with variants for all error types
- `ErrorKind` for categorization: `NotFound`, `AlreadyExists`, `InvalidState`, `InvalidInput`, `State`, `Io`, `Git`, `Network`, `Auth`, `Process`, `Serialization`, `Timeout`, `Internal`
- Automatic HTTP status mapping via `http_status()` method
- `is_user_error()` distinguishes user errors from system errors

**Error Conversion (From traits):** Common error types implement `From` for automatic conversion:
- `OrchestratorError`: `From<StateError>`, `From<DeltaStateError>`, `From<ProjectError>`, `From<ShepherdChatError>`, `From<reqwest::Error>`
- `LifecycleError`: `From<StateError>`, `From<DeltaStateError>`, `From<std::io::Error>`
- `RunManagerError`: `From<OrchestratorError>`, `From<std::io::Error>`, `From<serde_json::Error>`

This allows using `?` operator directly instead of `.map_err(|e| Error::State(e.to_string()))`.

### Process Management (`src-tauri/src/core/process.rs`)

Process-group cleanup utilities keep spawned subprocess trees from leaking.

```rust
pub fn cleanup_process_group(context: &str)
```

- Used by subprocess entrypoints spawned with `process_group(0)`
- Sends `SIGTERM`, waits briefly, then sends `SIGKILL` (Unix)
- Ensures helper grandchildren are terminated before process exit

### State Access Abstraction (`src-tauri/src/core/state_access.rs`)

Workers transparently use local (SQLite) or remote (HTTP) state.

```rust
#[async_trait(?Send)]
pub trait StateAccess: Send {
    // Run status
    async fn status(&self) -> StateAccessResult<Status>;
    async fn set_status(&self, status: Status) -> StateAccessResult<()>;

    // Board nodes (work items from dispatch)
    async fn add_node(&self, ...) -> StateAccessResult<()>;
    async fn claim_node(&self, id: &str, worker_name: &str) -> StateAccessResult<()>;
    async fn complete_node(&self, id: &str, worker_name: &str) -> StateAccessResult<()>;
    async fn get_claimed_node(&self, worker_name: &str) -> StateAccessResult<Option<BoardNode>>;
    async fn get_claimable_nodes(&self) -> StateAccessResult<Vec<BoardNode>>;
    async fn node_eval_pass(&self, eval_id: &str, worker_name: &str) -> StateAccessResult<()>;
    async fn node_eval_fail(&self, eval_id: &str, worker_name: &str, feedback: &str) -> StateAccessResult<String>;

    // Scaling
    async fn request_scaling_check(&self) -> StateAccessResult<()>;
    // ... message, worker operations
}
```

- Workers use `HIRSEL_API_URL` environment variable to determine mode
- Enables same worker binary for local and remote deployment
- `SQLiteState` for local, `HttpState` for remote
- Board nodes are stored in global database (`~/.hirsel/hirsel.db`), not per-run
- `request_scaling_check` triggers event-driven worker scaling (via DB flag or HTTP)

### Board Service (`src-tauri/src/core/board/mod.rs`)

Transparent local/remote routing for board file sync with AI agents.

```rust
pub struct BoardService {
    project_id: i64,
}

impl BoardService {
    pub fn export_local_sync(&self) -> Result<PathBuf>; // Export board to JSON files
    pub fn import_local_sync(&self) -> Result<SyncResult>; // Import changes from JSON
    pub fn board_dir(&self) -> PathBuf; // Get board directory path
}
```

- Exports islands as individual JSON files in `~/.hirsel/projects/{id}/board/islands/`
- Enables AI agents to read/modify board state via file system
- Import syncs JSON changes back to SQLite database
- Used by Shepherd AI context integration

---

## Real-Time Events

### Shepherd Event Streaming (`src-tauri/src/gui/commands/shepherd.rs`)

Tauri event-stream updates from the embedded lash runtime.

- Runtime: `lash-core` (`RuntimeEngine`) executes one Shepherd turn per message
- Event bridge maps lash `AgentEvent` values to frontend chat events
- Event types: `TextDelta`, `ThinkingDelta`, `ToolCallStart`, `ToolCallUpdate`, `MessageComplete`, `SessionEnded`, `Error`
- Event name: `shepherd-event` (Tauri event bus)
- Sessions are tracked in-memory and cleaned up on stop
- Message chunks are validated and persisted in `shepherd_chat_messages`
- Pasted images are accepted as structured chunks and decoded to `TurnInput.images_png` (PNG currently)

---

## State Machine

### Run Status (`src-tauri/src/core/state/types.rs`)

```
Draft ──start──► Working ───────────────────────► Done ──deliver──► Delivered
                    │                                │
                    │ (all work+eval tasks done)     │
                  pause                              │
                    │                                │
                    ▼                                ▼
                 Paused              Failed (time limit, manual, etc.)
                    │
                  resume
                    │
                    ▼
                 Working
```

**Note:** Run transitions to `Eval` when all work tasks complete and evaluation begins. After eval passes, run moves to `Done`.

| Status | Description | Terminal |
|--------|-------------|----------|
| `Draft` | Configured, workers not spawned | No |
| `Working` | Workers actively running | No |
| `Paused` | Manually paused by user | No |
| `Eval` | Evaluation in progress | No |
| `Done` | All work complete, eval passed | Yes |
| `Delivered` | Changes pushed to branch | Yes |
| `Failed` | Run failed (see `failure_reason`) | Yes |

### Worker Status (`src-tauri/src/core/state/types.rs`)

| Status | Description |
|--------|-------------|
| `Working` | Actively processing |
| `Awaiting` | Idle (no work or waiting for user) |
| `Paused` | Stopped (run is paused) |
| `Error` | Process died unexpectedly |

### Board Node Status (`src-tauri/src/core/delta/types.rs`)

Board nodes are the unified work items stored in the global database.

| Status | Description |
|--------|-------------|
| `Draft` | User-editable, not yet dispatched |
| `Pending` | Dispatched, waiting to be claimed |
| `Working` | Claimed by worker, in progress |
| `Done` | Completed successfully |
| `AwaitingEval` | Work done, waiting for eval to run |
| `Validated` | All evals passed |
| `NeedsRepair` | Eval failed, repair task created |
| `Failed` | Failed (error or terminal failure) |

### Board Node Kind (`src-tauri/src/core/delta/types.rs`)

| Kind | Description |
|------|-------------|
| `Spec` | User-defined specification (root-level intent) |
| `Task` | Implementation task that produces code changes |
| `Eval` | Task that validates other tasks |

### Board Node Source (`src-tauri/src/core/delta/types.rs`)

| Source | Description |
|--------|-------------|
| `User` | Created by user (specs, manual tasks) |
| `Plan` | Created by plan worker (task decomposition) |
| `Worker` | Added by worker via MCP |
| `System` | System nodes (plan tasks) |

### Board Node Lifecycle

```
SPEC:  Draft → Pending (dispatch creates __plan task as child)
TASK:  Pending → Working → Done → AwaitingEval → Validated
                    ↓ (eval failure)
                  NeedsRepair → (repair task created) → Pending
EVAL:  Pending → Working → Done (pass) or Failed (fail)
                              ↓ (if failed)
                           creates repair task, parent → NeedsRepair
```

### Shepherd Start Flow

1. Find spec nodes with `status=Draft`
2. Set each to `status=Pending`
3. Shepherd decomposes work dynamically and creates actionable task/eval nodes as needed
4. Create/update project_run, board_version
5. Shepherd assigns workers as needed to execute implementation tasks + evals

---

## Data Flow

### SpecFlow Board Start

```
SpecFlow Board (GUI)
   │
   ▼ User writes specs (kind=spec, status=draft)
Board tree (single unified tree)
   │
   ▼ start_shepherd_run(projectId, routeId)
delta.rs command
   │
   ├─► Find all spec nodes with status=draft
   ├─► Set each spec to status=pending
   ├─► Do not create synthetic `__plan_*` nodes
   ├─► Create board_version snapshot
   ├─► Create/update project_run, set status=working
   │
   ▼ Shepherd-driven worker orchestration
Shepherd reads pending specs, explores codebase
   │
   ├─► Creates implementation tasks/evals as needed
   ├─► Updates validates relationships
   ├─► Updates blocked_by relationships
   │
   ▼ Implementation workers spawned
Workers claim pending tasks, execute, complete
   │
   ▼ Eval workers validate completed tasks
eval_pass → tasks validated, eval_fail → repair task created
```

### Run Creation (Local Mode)

```
CLI/GUI
   │
   ▼ StartRunRequest
DaemonOrchestrator ──TCP──► Daemon
                                      │
                                      ▼ start_run_internal
                                LocalOrchestrator
                                      │
   ┌──────────────────────────────────┴──────────────────────────────────┐
   │ 1. Create run directory (~/.hirsel/runs/<name>/)                    │
   │ 2. Initialize SQLite database (hirsel.db)                           │
   │ 3. Write spec.md from request                                       │
   │ 4. Set up workspace via WorkspaceProvider (git worktrees)           │
   │ 5. Register workers in state                                        │
   │ 6. Spawn workers via Runner (unless draft mode)                     │
   └─────────────────────────────────────────────────────────────────────┘
```

### Worker Lifecycle (Event-Driven)

Workers receive pre-assigned tasks at spawn time. No worker-initiated task claiming.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Event-Driven Scaling                              │
│                                                                      │
│  Task state change ──► request_scaling_check() ──► DB flag set      │
│                                                                      │
│  Daemon 5s poll ──► consume_scaling_check() ──► evaluate_scaling()  │
│       │                                                              │
│       └──► SpawnWorker(assigned_task_id) or WakeWorker(...)         │
└─────────────────────────────────────────────────────────────────────┘

Daemon ──spawn(assigned_task_id)──► Worker Process
                                          │
                                          ▼
                                   AcpClient.connect()
                                          │
                                          ▼
                                   Send prompt with assigned task
                                          │
                                          ▼
                                  ┌───────────────┐
                                  │  Main Loop    │◄─────────────────┐
                                  └───────┬───────┘                  │
                                          │                          │
                                          ▼                          │
                                  Process agent response             │
                                  (text, tool calls)                 │
                                          │                          │
                                          ▼                          │
                                  Update state (events, status)      │
                                          │                          │
                                          ▼                          │
                                  Check for messages ────────────────┘
                                          │
                                          ▼ (task complete)
                                  complete_task() + work_done()
                                          │
                                          ▼
                                  request_scaling_check()
                                          │
                                          ▼
                                  Worker exits (fresh context per task)
                                          │
                                          ▼
                                  Daemon evaluates scaling
                                          │
                                          ▼
                                  Respawn with new task (if available)
```

**Key behaviors:**
- **Direct task assignment**: Workers spawn with `assigned_task_id`, no `claim_task` tool
- **Fresh context**: Each task gets a new worker session (no session resume)
- **Scope blocking**: Root tasks are blocked by "scope" task until leader explores spec
- **Tree-walk distance**: Task assignment prefers nearby tasks (same subtree) for work, distant for eval

### Archive/Restore (Ephemeral Runners)

**Pause:**
```
Daemon.pause_run()
   │
   ▼
LocalLifecycleManager.pause_run()
   │
   ├─► For each worker:
   │      1. ArchiveStrategy.archive() ──► ArchiveHandle (work dir)
   │      2. ArchiveStrategy.archive() ──► ArchiveHandle (session)
   │      3. Store handles in WorkerStateHandle, save to worker DB
   │      4. Runner.stop()
   │
   └─► Set run status = Paused
```

**Resume:**
```
Daemon.resume_run()
   │
   ▼
LocalLifecycleManager.resume_run()
   │
   └─► Triggers scaling check
         │
         ▼
Daemon evaluates scaling:
   1. Check if runner is_ephemeral()
   2. If yes: ArchiveStrategy.restore(work_dir_handle)
   3. Runner.spawn() with assigned_task_id (fresh context)
   4. Update worker DB (clear state handle, set status)
```

**Note:** Workers always spawn with fresh context. Session resume is not used - each task gets a new agent session for cleaner context management.

---

## Routes System (`src-tauri/src/core/route/`)

Routes enable parallel exploration of different approaches within a project. Each route has independent:
- Board tree (specs, tasks, evals — unified node tree)
- Documentation
- Workspace code
- Messages (Sheepfold)

### Route Concepts

| Concept | Description |
|---------|-------------|
| **Main route** | Created automatically with each project (id=1) |
| **Fork** | Create new route from parent's current state |
| **Active route** | Currently selected route (`projects.active_route_id`) |
| **Route tree** | Hierarchical display of routes and their ancestry |

### Route-Scoped Data

Data that is scoped to a specific route:
- `board_nodes` - Primary key includes `route_id`
- `board_node_validated_by` - Has `route_id` column
- `board_node_blocked_by` - Has `route_id` column
- `project_messages` - Has `route_id` column

Route files stored at `~/.hirsel/projects/{project_id}/routes/{route_name}/`:
- `docs/` - Route documentation
- `board/tasks/{id}.md` - Task/eval content files for Shepherd editing
- `code/` - Code snapshot directory

### Route Forking

When creating a route with a `parent_route_id`:
1. Database records are copied: `board_nodes`, `board_node_validated_by`, `board_node_blocked_by`
2. Files are copied: `docs/`, `board/tasks/`
3. The new route gets its own folder structure at `routes/{new_route_name}/`

This allows independent exploration of different approaches while preserving the parent state.

### Frontend Integration

- `LeftDrawer` shows routes list with active indicator, fork button, and bottom actions (Meadow, Docs, Settings)
- `ForkRouteDialog` for creating new routes
- Route changes trigger `route-changed` event
- DeltaContext reloads trees when route changes
- All tree/message operations use `active_route_id` from project

---

## Database Schema

### Global Database (`~/.hirsel/hirsel.db`)

| Table | Primary Key | Purpose |
|-------|-------------|---------|
| `config` | `key` | Configuration key-value store |
| `credentials` | `key_type` | Encrypted credential storage |
| `shepherd_chat_messages` | `id` | Shepherd chat history |
| `projects` | `id` | Project registry |
| `board_nodes` | `(id, project_id, route_id)` | Unified board tree (specs, tasks, evals) |
| `board_node_validated_by` | `(eval_id, task_id, project_id, route_id)` | Eval→task validation relationships |
| `board_node_blocked_by` | `(node_id, blocker_id, project_id, route_id)` | Blocking relationships |
| `project_runs` | `id` | Persistent project runs |
| `board_versions` | `id` | Board version history |
| `deliveries` | `id` | Board delivery tracking |
| `delivery_attempts` | `id` | Delivery attempt history |
| `project_messages` | `id` | Sheepfold messages (route-scoped) |
| `routes` | `id` | Project routes for parallel exploration |

**config:**
- `key` - Configuration key (e.g., "runners", "auth", "eval_timeout")
- `value` - JSON or string value
- `updated_at` - Last modification timestamp

**board_nodes** (unified board tree):
- `id` - Unique node ID (slug)
- `project_id` - Parent project
- `route_id` - Route scope
- `parent_id` - Parent node
- `position` - Ordering within siblings
- `name`, `content` - Node details
- `kind` - 'spec', 'task', or 'eval'
- `status` - 'draft', 'pending', 'working', 'done', 'awaiting_eval', 'validated', 'needs_repair', 'failed'
- `source` - 'user', 'plan', 'worker', 'system'
- `validated_by` - Junction table (task→eval): which evals validate this task
- `validates` - Computed from validated_by junction table (eval→tasks)
- `blocked_by` - Junction table of blocking node IDs
- `resolves` - ID of eval this repair task resolves
- `claimed_by`, `claimed_at`, `completed_by`, `completed_at` - Worker assignment
- `eval_result`, `eval_feedback` - Eval outcomes

### Run Database (`~/.hirsel/runs/{name}/hirsel.db`)

| Table | Primary Key | Purpose |
|-------|-------------|---------|
| `state` | `id=1` | Run metadata (singleton) |
| `workers` | `id` | Worker processes |
| `messages` | `id` | Chat threads |
| `message_reads` | `(worker_name, thread)` | Read tracking |
| `worker_events` | `id` | Real-time output streaming |
| `evals` | `id` | Evaluation runs |
| `history` | `id` | Activity log |
| `amendments` | `id` | Spec amendments |

**Note:** Work items (tasks) are stored as **board_nodes** in the global database, not per-run. See Global Database section.

### Key Columns

**state:**
- `status`, `failure_reason`, `started_at`, `time_limit_minutes`
- `worker_scale`, `human_in_the_loop`
- `default_runner`, `worker_runners` (JSON), `runner_configs` (JSON), `starting_point` (JSON)
- `scaling_check_requested` - Boolean flag for event-driven scaling

**workers:**
- `name`, `pid`, `runner_id`, `runner_type`, `status`
- `session_id`, `work_dir`, `hitl_waiting`
- `state_handle` (JSON: WorkerStateHandle with work_dir and agent_session snapshots)
- `assigned_task_id` - Currently assigned board_node (direct assignment model)
- `last_task_id` - Last completed board_node (for tree-walk distance calculation)

### Project Board Tables (in global DB)

The board module stores project-level data in the global database (`~/.hirsel/hirsel.db`):

| Table | Primary Key | Purpose |
|-------|-------------|---------|
| `board_nodes` | `id` | Unified board nodes (spec/task/eval) |
| `board_node_validated_by` | `(node_id, eval_id)` | Task-to-eval validation junction |
| `board_node_blocked_by` | `(node_id, blocker_id)` | Node dependency junction |
| `board_bookmarks` | `id` | Saved viewport positions |
| `board_file_baselines` | `(project_id, file_path)` | File sync change detection |

**board_nodes:**
- `id` - Slug ID (e.g., "build-api")
- `project_id`, `route_id`, `parent_id`, `position`
- `name`, `kind` (spec/task/eval), `source` (user/plan/worker/system)
- `status` (draft/pending/working/done/awaiting_eval/validated/needs_repair/failed)
- `content`, `x`, `y`
- See `delta/state/schema.rs` for full column list

---

## File Layout

```
~/.hirsel/
├── config.toml           # Initial config / one-time override (optional)
├── hirsel.db             # Global DB (config, credentials, projects, board_nodes)
├── key                   # Encryption key for credentials
├── hirsel.pid            # Daemon PID file
├── projects/{id}/
│   └── routes/{route_name}/    # Route-scoped data
│       ├── docs/               # Route documentation
│       ├── board/
│       │   └── tasks/          # Task content files ({id}.md)
│       └── code/               # Code snapshot (for forking)
└── runs/{run_name}/
    ├── hirsel.db         # Run state (workers, messages, events, evals, history)
    ├── spec.md           # Specification (input)
    ├── eval.md           # Eval criteria (input)
    ├── assets/           # Images, files for spec/eval
    ├── work/             # Git worktrees
    │   ├── leader/
    │   └── worker-2/
    ├── chats/            # Generated from DB
    └── tmp/
        └── eval_log.md
```

**Note:** Work items (board_nodes) are stored in the global database, not per-run. This enables cross-run coordination and persistent project runs.

**Route-scoped files:** Each route has independent board content files at `routes/{route_name}/board/tasks/{id}.md`. When forking a route, both database records (board_nodes, relationships) and files (docs/, board/tasks/) are copied from the parent.

### Git Workspace Setup (`src-tauri/src/core/git.rs`, `ops/setup.rs`)

The git workspace structure differs based on whether the run uses single or multiple workers.

**Single-Worker Mode:**
```
work/
└── staging/              # Worker works directly here
    └── .git/             # Original project's .git (no remotes configured)
```
- The single worker uses the staging workspace directly
- No `origin` remote is configured (no push/pull needed)
- Changes stay local until run completion
- Worker instructions tell the agent: "No git push is needed"

**Multi-Worker Mode:**
```
work/
├── staging/              # Shared staging repository
│   └── .git/
│       └── config        # receive.denyCurrentBranch = updateInstead
├── leader/               # Worker clone (origin → staging/)
│   └── .git/
│       └── config        # origin = /path/to/staging
└── worker-2/             # Worker clone (origin → staging/)
    └── .git/
        └── config        # origin = /path/to/staging
```
- Each worker has an isolated clone with `origin` pointing to `staging/`
- Workers must `git push origin staging` before completing tasks
- `staging/` accepts pushes via `receive.denyCurrentBranch = updateInstead`
- Workers handle merge conflicts when pulling others' changes

This distinction is automatically detected in the worker prompt generation based on whether teammates are present.

---

## Configuration (`src-tauri/src/core/config/mod.rs`)

### DB-First Loading

Configuration is loaded with the following priority (later sources override earlier):

1. **Default values** - Built-in defaults
2. **Database** (`~/.hirsel/hirsel.db`) - Source of truth
3. **Config file** (`~/.hirsel/config.toml`) - Seeds DB on first run, or overrides DB
4. **Environment variables** - Always win

When a config file exists, its values are loaded into the database. This enables:
- Remote coordinators (Fly.io) to work without persistent volumes
- API-based config management via PUT/PATCH `/api/config`
- Config changes to persist across restarts

### Config Struct Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `root` | `PathBuf` | `~/.hirsel` | Hirsel root directory |
| `agent` | `AgentConfig` | - | Agent command configuration |
| `eval_timeout` | `u32` | `1800` | Eval timeout in seconds |
| `human_in_the_loop` | `bool` | `true` | HITL mode default |
| `runners` | `HashMap<String, RunnerConfig>` | `{}` | Named runner configs |
| `default_runner` | `Option<String>` | `None` | Default runner name |
| `profiles` | `HashMap<String, OrchestratorProfile>` | local | Orchestrator profiles |
| `storage` | `StorageConfig` | local | Storage backend config |
| `service_workers` | `ServiceWorkersConfig` | local | Service worker runner config |

### Runner Configuration (`src-tauri/src/core/runner/config.rs`)

```toml
# Local runner (default)
[runners.local]
host = "local"

# Local with Docker
[runners.local-docker]
host = "local"
[runners.local-docker.container]
image = "rust:latest"

# SSH runner
[runners.my-server]
[runners.my-server.host]
type = "ssh"
address = "user@server.com"
port = 22
work_base = "/tmp/hirsel"

# Fly runner
[runners.fly]
[runners.fly.host]
type = "fly"
app = "hirsel-workers"
region = "ams"
cpus = 2
memory_mb = 2048
[runners.fly.container]
image = "debian:bookworm-slim"
```

### Orchestrator Profiles

```toml
[profiles.local]
mode = "local"

[profiles.remote]
mode = "remote"
url = "https://hirsel-coordinator.fly.dev"
api_key = "secret"
default_runner = "fly"
```

### Service Workers Configuration

Service workers provide warm worker support for background services like Scribe.

```toml
[service_workers]
runner = "fly"                    # Default runner for service workers

[service_workers.scribe]
runner = "local"                  # Override for scribe (docs agent)
idle_timeout_seconds = 300        # 5 min default
```

**Resolution order:**
1. Service-specific runner (`service_workers.scribe.runner`)
2. Default service runner (`service_workers.runner`)
3. Local (fallback)

**Behavior:**
- `ScribeService.process_batch(run_name)` handles local vs remote internally
- If runner is "local" or unset: runs scribe directly in-process (no HTTP)
- If runner is a configured remote runner: spawns HTTP service worker, routes requests to it
- Callers don't need to know about local vs remote - just call `process_batch()`

---

## REST API (`src-tauri/src/core/server/mod.rs`)

**Auth:** `Authorization: Bearer $HIRSEL_API_KEY`

### Run Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/runs` | `list_runs` |
| POST | `/api/runs` | `create_run` |
| GET | `/api/runs/{name}` | `get_run` |
| DELETE | `/api/runs/{name}` | `delete_run` |
| GET | `/api/runs/{name}/files` | `download_files` |
| POST | `/api/runs/{name}/files` | `upload_files` |
| POST | `/api/runs/{name}/workspace` | `init_workspace` |
| POST | `/api/runs/{name}/spawn` | `spawn_workers` |
| POST | `/api/runs/{name}/pause` | `pause_run` |
| POST | `/api/runs/{name}/resume` | `resume_run` |
| POST | `/api/runs/{name}/deliver` | `deliver_run` |

### Worker/Message Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/runs/{name}/workers` | `list_workers` |
| POST | `/api/runs/{name}/workers/{w}/restart` | `restart_worker` |
| POST | `/api/runs/{name}/workers/{w}/spawn` | `spawn_single_worker` |
| POST | `/api/runs/{name}/workers/{w}/resume` | `resume_worker` |
| GET | `/api/runs/{name}/workers/{w}/events` | `get_worker_events` |
| POST | `/api/runs/{name}/scribe` | `add_scribe` |
| GET | `/api/runs/{name}/docs` | `get_docs` |
| POST | `/api/runs/{name}/docs/sync` | `sync_docs` |
| GET | `/api/runs/{name}/evals` | `list_evals` |
| GET | `/api/runs/{name}/history` | `get_history` |
| POST | `/api/runs/{name}/assets` | `upload_asset` |
| GET | `/api/runs/{name}/assets-path` | `get_assets_path` |
| GET | `/api/runs/{name}/threads` | `list_threads` |
| GET/POST | `/api/runs/{name}/threads/{t}/messages` | `get_messages`, `send_message` |

### Board Node Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/runs/{name}/nodes` | `get_nodes` |
| POST | `/api/runs/{name}/nodes` | `add_node` |
| GET | `/api/runs/{name}/nodes/claimable` | `get_claimable_nodes` |
| POST | `/api/runs/{name}/nodes/{id}/claim` | `claim_node` |
| POST | `/api/runs/{name}/nodes/{id}/complete` | `complete_node` |
| POST | `/api/runs/{name}/nodes/{id}/unclaim` | `unclaim_node` |
| GET | `/api/runs/{name}/nodes/{id}/blocked` | `get_blocked` |
| POST | `/api/runs/{name}/nodes/{id}/eval-pass` | `eval_pass` |
| POST | `/api/runs/{name}/nodes/{id}/eval-fail` | `eval_fail` |
| POST | `/api/runs/{name}/nodes/{id}/tokens` | `add_tokens` |
| GET | `/api/runs/{name}/nodes/{id}/validated` | `get_validated` |

### Config Endpoints

| Method | Path | Handler |
|--------|------|---------|
| GET | `/api/config` | `get_config` |
| PUT | `/api/config` | `put_config` - Replace config fields |
| PATCH | `/api/config` | `patch_config` - Merge partial config |
| PATCH | `/api/config/general` | `patch_general_config` |
| PATCH | `/api/config/agent` | `patch_agent_config` |
| GET/PUT/DELETE | `/api/config/runners/{name}` | runner CRUD |
| GET/PUT/DELETE | `/api/config/profiles/{name}` | profile CRUD |
| POST/GET/DELETE | `/api/credentials/{key}` | credential CRUD |

### Board Endpoints

| Method | Path | Handler |
|--------|------|---------|
| POST | `/api/board/{project_id}/export` | `export_board` - Export board to agent JSON files |
| POST | `/api/board/{project_id}/import` | `import_board` - Import board changes from agent JSON files |
| GET | `/api/board/{project_id}/directory` | `get_board_directory` - Get board directory path |
| GET | `/api/board/{project_id}/tasks` | `list_task_files` - List task files in board |
| GET | `/api/board/{project_id}/tasks/{slug}` | `get_task_file` - Get task file content |
| POST | `/api/board/{project_id}/tasks/{slug}` | `save_task_file` - Save task file |
| DELETE | `/api/board/{project_id}/tasks/{slug}` | `delete_task_file` - Delete task file |

### Git HTTP Endpoints

| Method | Path | Handler |
|--------|------|---------|
| ANY | `/git/{run_name}` | `git_run_root_handler` - Git smart HTTP root (info/refs via query params) |
| ANY | `/git/{run_name}/*path` | `git_run_handler` - Git smart HTTP sub-paths (git-upload-pack, git-receive-pack) |

These endpoints proxy requests to `git-http-backend` CGI, resolving `run_name` to `~/.hirsel/runs/{run_name}/work/staging/`. Used by remote workers (SSH, Fly) to clone/fetch/push against the coordinator's staging repo. Mounted in `build_shared_routes()` so both daemon and remote server expose them.

### Shepherd Chat Commands (Tauri IPC)

Shepherd chat is exposed through Tauri commands and Tauri events (not REST routes):

- `start_shepherd_session`
- `send_shepherd_message`
- `stop_shepherd_session`
- `list_shepherd_sessions`
- `get_shepherd_history`
- `save_shepherd_message`
- `clear_shepherd_history`
- Event stream: `shepherd-event`

### Adding a REST Endpoint

Routes are shared between daemon and remote server via `shared_routes.rs` to avoid duplication:

| Builder | Used By | Description |
|---------|---------|-------------|
| `build_shared_routes()` | Both | Run ops, workers, tasks, messages, evals, history, assets, git HTTP |
| `build_config_routes()` | Remote only | Config CRUD, credentials |
| `build_board_routes()` | Remote only | Board sync for SpecFlow |
| Inline routes in `daemon/server.rs` | Daemon only | `/daemon/*`, worker internal API |

**To add a shared endpoint:**
1. Add handler in `routes.rs` (or an appropriate module like `board.rs`)
2. Add route to the appropriate builder in `shared_routes.rs`
3. Both servers automatically get the new route

**To add a daemon-only endpoint:**
Add the route inline in `daemon/server.rs` after the `build_shared_routes()` call.

**To add a remote-server-only endpoint:**
Add a new builder function or extend `build_config_routes()`/`build_board_routes()`.

---

## Daemon (`src-tauri/src/daemon/`)

Background process that owns lifecycle management.

**Listeners:**
- TCP: `0.0.0.0:{port}` where port is `HIRSEL_DAEMON_PORT` env var (default: 19700)
- CLI/GUI connects via localhost, Docker via host.docker.internal

**Port Configuration:**
- Set `HIRSEL_DAEMON_PORT` to use a different port (useful for testing or multiple instances)
- On startup, daemon checks if port is already in use and errors with helpful message
- Each `HIRSEL_ROOT` should use a unique port to avoid conflicts

**Polling Loop (every 5s):**
```rust
for run in active_runs {
    // Event-driven scaling: check if any task state changes requested scaling
    if state.consume_scaling_check()? {
        let actions = evaluate_scaling(&state)?;
        // Handle: SpawnWorker(assigned_task_id), WakeWorker(assigned_task_id)
        handle_lifecycle_actions(actions).await;
    }

    match status {
        Status::Working => {
            let actions = lifecycle.process_event(LifecycleEvent::TimeCheck)?;
            // Handle: EvalTriggered, RunFailed, TimeWarning
            handle_lifecycle_actions(actions).await;
            maybe_process_scribe(run);
        }
        Status::Eval => {
            // Check time limit
            // Detect eval process crash (PID no longer alive) → re-trigger eval
        }
    }
}
```

**Event-Driven Scaling:** Worker scaling is triggered by task state changes (add_task, complete_task, etc.) setting the `scaling_check_requested` DB flag. The daemon's 5-second poll provides natural debouncing - multiple rapid task changes get batched into one scaling evaluation. The `evaluate_scaling()` function uses tree-walk distance to assign nearby work tasks and distant eval tasks to workers.

**Eval Crash Detection:** When a run is in `Eval` state, the daemon checks if the eval process PID is still alive. If the process crashed, the daemon marks the eval as failed and re-triggers it by resetting the run to `Working` and processing a `TimeCheck` event.

**Auto-start:** CLI/GUI start daemon automatically via `DaemonOrchestrator::connect_or_start()`.

**Auto-exit:** Daemon exits after 5 minutes of no active runs.

---

## Design Notes

### Orchestrator Implementations

All orchestrator methods are fully implemented across Local, Daemon, and Remote:

| Implementation | Transport | Use Case |
|----------------|-----------|----------|
| `LocalOrchestrator` | Direct SQLite | Server, daemon internals |
| `DaemonOrchestrator` | TCP (HTTP) | CLI/GUI in local mode |
| `RemoteOrchestrator` | TCP (HTTP) | CLI/GUI in remote mode |

The daemon exposes the same HTTP API over TCP that the remote server exposes. This allows all orchestrator implementations to share the same route handlers.

### RemoteLifecycleManager (Intentional No-op)

`lifecycle/remote.rs` returns no-ops for all methods because remote workers delegate lifecycle management to the coordinator. The coordinator (running `LocalLifecycleManager`) handles:
- Eval triggering when workers go idle
- Worker scaling decisions
- Time limit enforcement

This is by design, not a stub that needs implementation.

### Partial/Feature-Gated Implementations

| Feature | Location | Status |
|---------|----------|--------|
| SSH runner | `runner/ssh.rs` | Works but less tested than Local/Fly |
| S3WorkspaceProvider | `draft/s3_workspace.rs` | Feature-gated (`s3-storage`) |
| S3SnapshotStrategy | `snapshot/s3.rs` | Feature-gated (`s3-storage`) |

### Runner Config Storage

Runner configurations are stored per-run at creation time in the `runner_configs` column (JSON). This ensures that changes to `config.toml` don't affect in-progress runs. The storage flow:

1. At run creation, resolve runner names to full `RunnerConfig` objects
2. Store the configs in the run's SQLite database
3. When spawning workers, use stored configs

Methods in `state/run.rs`:
- `get_runner_configs()` - Get stored configs
- `set_runner_configs()` - Store configs at run creation
- `get_runner_config_for_worker()` - Get config for a worker from stored configs

### Remote Worker Bootstrap

Ephemeral runners (Fly) bootstrap workers via init scripts in `runner/setup.rs`:

1. **Download hirsel binary** - From GitHub releases with version pinning (`HIRSEL_TAG=v{VERSION}`)
2. **Install dependencies** - Node.js for agent tools if not in image
3. **Fetch project files** - Tarball from coordinator's `/api/runs/{name}/files`
4. **Initialize git** - With coordinator as remote (`{coordinator_url}/git/{run_name}`) for push/pull via git smart HTTP
5. **Start worker** - `hirsel __remote-worker` connects back to coordinator

The coordinator embeds its version at compile time and passes it to workers, ensuring binary compatibility.

### Service Workers

Service workers (`service_worker/`) manage background services that benefit from "warm" instances:

| Service | Purpose | Default Timeout |
|---------|---------|-----------------|
| Scribe | Documentation agent processing learnings | 5 min |
| ConflictResolver | Conflict marker validation/resolution checks | 10 min |

**Architecture:**
```
ScribeService.process_batch(run_name)
    │
    ├─ should_use_remote()?
    │   │
    │   ├─ No → process_locally()
    │   │       └─ Run scribe directly (spawn_blocking + LocalSet)
    │   │
    │   └─ Yes → process_via_remote()
    │           │
    │           ├─ get_or_spawn_worker()
    │           │   ├─ Health check existing worker
    │           │   │   └─ Yes → reuse endpoint
    │           │   │   └─ No → spawn new worker
    │           │
    │           └─ POST to /scribe/batch endpoint
    │
    └─ Worker self-terminates after idle timeout
```

**Key files:**
- `core/service_worker/scribe.rs` - `ScribeService` handles local vs remote internally
- `core/service_worker/conflict_resolver.rs` - `ConflictResolverServiceWrapper` for merge conflicts
- `core/conflict_resolver/` - Core conflict resolution state + validation logic
- `core/service_worker/types.rs` - `ServiceWorkerType`, `ServiceWorkerHandle`
- `cli/service_worker.rs` - HTTP server for `__service-worker` command (remote only)
- `daemon/lifecycle.rs` - Integration in `maybe_process_scribe()`

**HTTP API (exposed by service worker binary):**
- `GET /health` - Health check with idle time
- `POST /scribe/batch` - Process scribe batch
- `POST /shutdown` - Graceful shutdown

### Docs Persistence

Project documentation is copied to the run directory at start, allowing Scribe to maintain it during the run without polluting the workspace git state.

**Flow:**
```
Run Start:
  workspace/docs/ ──copy──► run_dir/docs/
  git update-index --skip-worktree docs/*
  rm -rf workspace/docs/

During Run:
  Scribe writes to run_dir/docs/
  Workers read via read_docs MCP tool

Delivery (persist=true):
  run_dir/docs/ ──copy──► workspace/docs/
  git add docs/ && git commit

Delivery (persist=false):
  git checkout docs/  (restore original)
```

**Config:**
```toml
scribe_docs_path = "docs"           # Relative to workspace
scribe_persist_docs_changes = true  # Commit changes on delivery
```

**Key files:**
- `core/ops/docs.rs` - `setup_docs()`, `deliver_docs()`
- `core/scribe.rs` - Scribe agent prompt and batch processing

### Known Constraints

- **Fly requires container.image** - Machines ARE containers
- **Client host only in remote mode** - Requires Tailscale for SSH-back
- **SSH reverse tunnel required for local mode** - Workers connect to `localhost:19700`
- **Worker binary version** - Must match coordinator version (auto-pinned via `HIRSEL_TAG`)

### Gotchas

#### Tauri Commands Use camelCase Parameters

Tauri automatically converts Rust snake_case parameters to JavaScript camelCase. Use camelCase in the frontend:

```rust
// Backend (Rust) - uses snake_case
#[tauri::command]
pub async fn delete_board_task(project_id: i64, task_id: String) -> Result<(), String> {
    // ...
}
```

```typescript
// Frontend (TypeScript) - CORRECT: use camelCase
await invoke('delete_board_task', { projectId, taskId });

// Frontend (TypeScript) - WRONG: snake_case won't match
await invoke('delete_board_task', { project_id: projectId, task_id: taskId });
```

If a command errors with "missing required key", check that you're using camelCase parameter names.

---

## Testing

### E2E Tests (`tests/e2e/`)

Pytest-based tests with runner × scenario matrix.

```
tests/e2e/
├── pyproject.toml          # uv project config
├── conftest.py             # pytest fixtures, parametrization
├── test_runs.py            # Main tests (3 classes)
├── orchestrator.py         # Orchestrator setup (LOCAL, REMOTE)
└── runners/                # Runner implementations
    ├── base.py             # BaseRunner, RunnerConfig
    ├── local.py            # LocalRunner
    ├── docker.py           # DockerRunner
    ├── ssh.py              # SshRunner
    └── fly.py              # FlyRunner
```

**Test classes:**
- `TestRunScenario` - Main parametrized runner × scenario tests
- `TestPauseResume` - Pause/resume functionality (`@pytest.mark.slow`)
- `TestDockerLifecycle` - Docker container cleanup (`@pytest.mark.docker`)

**Running tests:**
```bash
cd tests/e2e
uv sync
uv run pytest                                    # Default (local, noop)
uv run pytest --runner=fly --scenario=calculator # Specific
uv run pytest -m "not slow"                      # Skip slow
uv run pytest --profile=fly                      # Remote orchestrator
```

**Adding a runner:**
1. Create `runners/myrunner.py` extending `BaseRunner`
2. Implement `configure()`, `verify_output()`, `skip_if_unavailable()`
3. Register in `conftest.py` → `RUNNERS` dict

### Unit Tests

Inline Rust tests in source files (`#[test]`, `#[tokio::test]`).
- CLI modules have unit tests for argument parsing
- Board module has tests for task/eval operations

Run with:
```bash
cargo nextest run
```

### MCP UI Tests (`tests/e2e/create-run.test.ts`)

TypeScript test for UI automation via MCP client.

### Scenarios (`tests/scenarios/`)

| Scenario | Purpose |
|----------|---------|
| `hello_world` | Basic file creation |
| `calculator` | Python with pytest |
| `noop` | Minimal validation |
| `todo_api` | FastAPI with eval |
| `multi_file` | Multi-file changes |
| `tic_tac_toe` | Game implementation |
