# Hirsel Architecture Overview

> **Important**: Keep this document up to date as the architecture evolves.

## Overview

**Hirsel** orchestrates multiple AI coding agents working together on software projects.

### Tech Stack
- **Backend**: Rust + Tokio + Tauri v2
- **Frontend**: TypeScript + Alpine.js + Tailwind CSS
- **Database**: SQLite (source of truth)
- **Agent Protocol**: ACP (Agent Control Protocol)

### High-Level Architecture

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
│   - Worker spawning, auto-exit when idle                        │
└──────────────────────────────┬──────────────────────────────────┘
                               │
           ┌───────────────────┼───────────────────┐
           ▼                   ▼                   ▼
    LocalOrchestrator   DaemonOrchestrator   RemoteOrchestrator
    (direct access)     (Unix socket)        (HTTP API)
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│   SQLite State              │   Worker Processes                 │
│   (runs, workers, tasks,    │   (Local, SSH, Sprite, Fly)        │
│    messages, events)        │   + optional Docker container      │
└─────────────────────────────┴───────────────────────────────────┘
```

---

## Core Concepts

### Runs
A **run** is an orchestration instance where AI agents work on a project.

- **Status**: Draft → Working → Eval → Done/Failed/Delivered
- **Storage**: `~/.hirsel/runs/{run_name}/`

### Workers
**Workers** are AI agent processes. Each has its own git worktree.

- **Leader**: First worker, coordinates shared state
- **Teammates**: Claim tasks independently

### Tasks
Work items with status: `Todo` → `Doing` → `Done`

### State: Database vs Files
The **SQLite database is the source of truth**. Markdown files (`tasks.md`, `chats/`) are generated views for AI agents to read easily.

---

## Lifecycle Management

The `lifecycle` module centralizes all run lifecycle operations. **The daemon owns all lifecycle management** - workers are "dumb" executors that just do tasks and report status.

```
┌─────────────────────────────────────────────────────────────────┐
│                    Daemon (polling every 5s)                     │
│   lifecycle.process_event(TimeCheck)                             │
│       → Returns actions: SpawnWorker, EvalTriggered, etc.        │
│   handle_lifecycle_actions()                                     │
│       → orchestrator.spawn_single_worker() for spawning          │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                    Workers (dumb executors)                      │
│   - Claim tasks from state                                       │
│   - Execute work                                                 │
│   - Update status (Working → Awaiting)                           │
│   - Send heartbeats                                              │
│   - NO lifecycle management, NO spawning other workers           │
└─────────────────────────────────────────────────────────────────┘
```

### LifecycleManager Trait

```
┌─────────────────────────────┬───────────────────────────────────┐
│   LocalLifecycleManager     │   RemoteLifecycleManager          │
│   (local/daemon mode)       │   (remote workers)                │
│   - Eval triggering         │   - Delegates to coordinator      │
│   - Worker scaling checks   │   - No-op implementations         │
│   - Time limit enforcement  │                                   │
│   - Pause/resume            │                                   │
└─────────────────────────────┴───────────────────────────────────┘
```

**Events**: `TimeCheck`, `WorkerDone`, `TaskCompleted`, `PauseRequested`, `ResumeRequested`

**Actions**:
- `SpawnWorker { worker_name, work_dir }` - daemon spawns via orchestrator
- `ResumeWorker { worker_name, work_dir, resume_session_id }` - daemon spawns via orchestrator
- `EvalTriggered`, `RunFailed`, `RunCompleted`, `WorkersPaused`, etc.

Usage:
```rust
let lifecycle = LocalLifecycleManager::new(run_name, run_dir, agent_command)?;
let actions = lifecycle.process_event(LifecycleEvent::TimeCheck)?;
// Daemon handles SpawnWorker/ResumeWorker actions via orchestrator.spawn_single_worker()
```

### Why Daemon Owns Spawning

Workers don't spawn other workers because:
1. **Correct runner usage**: The orchestrator knows the runner config (local/docker/fly/sprite)
2. **Docker fix**: A worker inside a Docker container calling `spawn_worker()` would spawn inside the same container (wrong). The daemon uses the runner to spawn in a new container.
3. **Centralized control**: All lifecycle decisions come from one place

---

## Daemon

Background process that owns all lifecycle management (polls every 5 seconds):

**Responsibilities:**
- Triggers eval when all workers become idle
- Enforces time limits
- **Spawns workers** for autoscaling (via `orchestrator.spawn_single_worker()`)
- **Resumes workers** after pause (via `orchestrator.spawn_single_worker()`)
- Auto-exits after 5 minutes of no active runs

**Polling Loop:**
```rust
// Every 5 seconds for each active run:
let actions = lifecycle.process_event(LifecycleEvent::TimeCheck)?;
for action in actions {
    match action {
        LifecycleAction::SpawnWorker { worker_name, work_dir } => {
            orchestrator.spawn_single_worker(run_name, &worker_name, &work_dir, None).await?;
        }
        LifecycleAction::EvalTriggered => { /* eval agent spawned by lifecycle */ }
        // ...
    }
}
```

**Listeners:**
- Unix socket (`~/.hirsel/hirsel.sock`) - CLI/GUI communication
- TCP (`localhost:19700`) - SSH reverse tunnels

```bash
hirsel daemon start|stop|status
```

Auto-starts when CLI runs `hirsel go` or GUI opens.

### TCP Listener for SSH Tunneling

The daemon's TCP listener on `localhost:19700` enables SSH runners to work with local orchestrator via reverse tunnel. When using an SSH runner in local mode:

1. Daemon starts TCP HTTP server on `127.0.0.1:19700`
2. SSH connection creates reverse tunnel: `-R 19700:localhost:19700`
3. Remote worker connects to `http://localhost:19700` (tunneled back to local daemon)

This allows workers on remote SSH hosts to access the local state and git repos without deploying a separate server.

---

## Worker Routes

The `worker_routes` module provides shared HTTP route handlers for worker API endpoints. Both the daemon (multi-run) and coordinator API (single-run) use these shared handlers.

```
┌─────────────────────────────────────────────────────────────────┐
│                      worker_routes module                        │
│   - Request/Response types (SuccessResponse, UpdateWorkerRequest)│
│   - Handler functions (pure logic, no HTTP concerns)             │
│   - Takes &SQLiteState + Optional &dyn LifecycleManager          │
└─────────────────────────────────────────────────────────────────┘
               ▲                                    ▲
               │                                    │
┌──────────────┴────────────┐        ┌─────────────┴──────────────┐
│        Daemon             │        │       Coordinator API      │
│   (multi-run mode)        │        │   (single-run mode)        │
│   - Run name in URL path  │        │   - Run name in ApiState   │
│   - Unix socket + TCP     │        │   - Localhost only         │
└───────────────────────────┘        └────────────────────────────┘
```

See [Worker Endpoints](#worker-endpoints-internal) in REST API section for the full endpoint list.

The `update_worker` handler accepts an optional `&dyn LifecycleManager`. When a worker status changes to `Awaiting`, it triggers lifecycle events (e.g., eval triggering when all workers idle).

---

## Orchestrator Pattern

All run creation and worker spawning goes through the `Orchestrator` trait, providing a unified interface for CLI, GUI, and server:

```
CLI ────┐                              ┌─► LocalOrchestrator ─► direct setup/spawn
        ├─► orchestrator.create_run()  │
GUI ────┘   orchestrator.spawn_workers()└─► RemoteOrchestrator ─► HTTP ─► Server
```

| Orchestrator | Transport | Use Case |
|--------------|-----------|----------|
| **Local** | Direct SQLite access | Internal (daemon, server) |
| **Daemon** | Unix socket / TCP | CLI/GUI local mode |
| **Remote** | HTTP API | Remote server mode |

### Orchestrator Mode Compatibility

| Host | Local Mode | Remote Mode | Notes |
|------|------------|-------------|-------|
| **Local** | ✅ | ✅ | Direct SQLite access |
| **SSH** | ✅ | ✅ | Local: reverse tunnel to daemon TCP |
| **Sprite** | ❌ | ✅ | Requires publicly accessible coordinator |
| **Fly** | ❌ | ✅ | Requires publicly accessible coordinator |
| **Client** | ❌ | ✅ | Only available in remote mode |

### Orchestrator Trait Methods

The trait provides these key methods for run lifecycle:

| Method | Description |
|--------|-------------|
| `create_run(request)` | Create run directory, state, spec, initial worker |
| `upload_files(name, tarball)` | Upload project files as gzipped tarball |
| `spawn_workers(name, count)` | Spawn workers for a run (initial creation) |
| `spawn_single_worker(name, worker, work_dir, session_id)` | Spawn one worker (scaling/resume) |
| `list_runs()` | List all runs |
| `get_run(name)` | Get run details |
| `pause_run(name)` | Pause a running run |
| `resume_run(name)` | Resume a paused run |
| `list_workers(name)` | List workers for a run |
| `restart_worker(name, worker)` | Restart a worker |

**spawn_single_worker**: Used by the daemon when lifecycle manager returns `SpawnWorker` or `ResumeWorker` actions. Uses the runner system to spawn correctly based on runner config (local/docker/fly/sprite).

### Run Creation Flow

**Remote Mode** (via RemoteOrchestrator):
1. `create_run(request)` → `POST /api/runs` → Server creates run
2. `upload_files(name, tarball)` → `POST /api/runs/{name}/files` → Upload project
3. `spawn_workers(name, count)` → `POST /api/runs/{name}/spawn` → Spawn workers
4. Workers download files, connect directly to server

**Local Mode** (via LocalOrchestrator):
1. `create_run(request)` → Create run directory, state, spec, chats
2. `upload_files(name, tarball)` → Extract tarball to work/ directory
3. `spawn_workers(name, count)` → Spawn workers via configured runner

**CLI Local Mode** (direct, uses git worktrees):
1. Set up git worktrees for each worker
2. Register workers in state
3. Spawn workers via configured runner (Local/SSH/Sprite, optionally in Docker container)

---

## State Management

**Location**: `~/.hirsel/runs/{run_name}/hirsel.db`

### Key Tables
```sql
state           -- Run metadata (status, time_limit, worker_scale)
workers         -- Active processes (name, pid, status, work_dir)
tasks           -- Work breakdown (id, status, claimed_by)
messages        -- Chat threads
worker_events   -- Real-time output streaming
evals           -- Evaluation runs
```

---

## Worker Execution

### Lifecycle
1. **Spawn**: `hirsel __worker-run --run X --worker Y` (detached, process_group(0))
2. **Init**: Connect to state, spawn ACP agent
3. **Loop**: Claim tasks, process tool calls, stream output, heartbeat
4. **Complete**: Signal done, daemon triggers eval

### ACP Bridge

Workers communicate with AI agents via ACP (Agent Control Protocol). The built-in `hirsel __acp-bridge` command wraps the Claude CLI:

```
Worker Process
    │
    ▼ ACP JSON-RPC (stdin/stdout)
┌───────────────────────────────────────┐
│       hirsel __acp-bridge             │
│   (ACP server → Claude CLI bridge)    │
│                                       │
│   ┌───────────────────────────────┐   │
│   │      ClaudeCliBridge          │   │
│   │   (JSON streaming protocol)   │   │
│   └───────────────┬───────────────┘   │
│                   │                   │
└───────────────────│───────────────────┘
                    ▼
              claude CLI
         (spawned subprocess)
```

The bridge:
- Accepts ACP JSON-RPC on stdin (initialize, new_session, prompt)
- Spawns Claude CLI with `--input-format stream-json --output-format stream-json`
- Translates Claude's JSON streaming events to ACP notifications
- Pre-approves MCP tools with `--allowedTools mcp__<server>__*`

**Important**: Claude CLI's `--permission-mode delegate` does NOT work for MCP tools. Using delegate mode, MCP tool calls return immediate "permission not granted" errors without sending control_request messages. MCP tools must be pre-approved using `--allowedTools mcp__<server>__*` patterns.

### Runner Types (Host + Container Model)

Runners are configured with a **host** (where compute runs) and an optional **container** (Docker isolation).

**Host Types:**

| Host | Description |
|------|-------------|
| **Local** | Subprocess on local machine |
| **Client** | (Remote mode) SSH back to GUI/CLI user's machine via Tailscale |
| **SSH** | Remote via SSH + reverse tunnel |
| **Sprite** | Sprites.dev cloud VMs (Firecracker) |
| **Fly** | Fly.io ephemeral machines |

**Container:**
- Optional Docker container for any host except Sprite
- Sprites use Firecracker VMs, cannot nest Docker
- Fly machines ARE containers, so container.image is required

**Examples:**
- `local` - bare process on local machine
- `local` + container `rust:latest` - local Docker container
- `ssh` to `user@server.com` - bare process on remote host
- `ssh` + container `ghcr.io/org/dev-env` - Docker on remote host

### Git Synchronization

Workers have local git repos. Coordinator runs a git HTTP server as shared remote. Workers push/pull as needed. Conflicts are resolved by the AI agent using standard git commands.

---

## File Locations

```
~/.hirsel/
├── config.toml           # Configuration
├── hirsel.db             # Global DB (credentials)
├── hirsel.sock           # Daemon Unix socket (CLI/GUI)
├── hirsel.pid            # Daemon PID file
└── runs/{run_name}/
    ├── hirsel.db         # Run state (SOURCE OF TRUTH)
    ├── spec.md           # Specification (input)
    ├── eval.md           # Eval criteria (input)
    ├── tasks.md          # Generated from DB
    ├── tasks/            # Generated from DB
    ├── work/             # Git worktrees (leader/, worker-2/)
    └── chats/            # Generated from DB
```

Daemon also listens on `localhost:19700` (TCP) for SSH reverse tunnels.

---

## Storage Abstraction

The storage abstraction layer allows hirsel to run in multiple deployment scenarios with different storage backends.

### Storage Backends

| Backend | Use Case | Requirement |
|---------|----------|-------------|
| **Local** | Default, self-hosted | Filesystem |
| **S3** | Cloud deployments, MinIO | `--features s3-storage` |

### Configuration

```toml
# Local storage (default)
[storage]
files = "local"

# S3-compatible storage (MinIO, Tigris, AWS S3)
[storage]
files = "s3"

[storage.s3]
endpoint = "http://localhost:9000"  # MinIO URL
bucket = "hirsel"
region = "us-east-1"
access_key_id = "minioadmin"
secret_access_key = "minioadmin"
```

### FileStorage Trait

The `FileStorage` trait provides a unified interface for storage operations:

```rust
#[async_trait]
pub trait FileStorage: Send + Sync {
    async fn read(&self, path: &str) -> StorageResult<Vec<u8>>;
    async fn write(&self, path: &str, data: &[u8]) -> StorageResult<()>;
    async fn delete(&self, path: &str) -> StorageResult<()>;
    async fn exists(&self, path: &str) -> StorageResult<bool>;
    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>>;
    async fn create_dir(&self, path: &str) -> StorageResult<()>;
}
```

### Usage

```rust
// Create storage from config
let storage = create_file_storage(&config.storage).await?;

// Use Files with storage abstraction
let files = Files::new(run_dir);
files.write_spec_async(&*storage, "# My Spec").await?;
let content = files.read_spec_async(&*storage).await?;
```

---

## Snapshot Strategy

The snapshot system preserves worker state across pause/resume cycles. Strategy is determined by host type:

| Host | Default Strategy | Behavior |
|------|------------------|----------|
| **Local** | PersistentDisk | No-op (files remain on local disk) |
| **SSH** | PersistentDisk | No-op (files remain on remote disk) |
| **Sprite** | SpriteCheckpoint | Native Sprites API checkpoint |
| **Fly** | S3 | Tar/gzip to S3-compatible storage |

**Note**: Docker containers on Local/SSH use volume mounts, so files persist after container stop. Container presence doesn't change the snapshot strategy.

### Strategy Types

| Strategy | Description | Requirements |
|----------|-------------|--------------|
| **PersistentDisk** | No-op - files remain in place | None |
| **SpriteCheckpoint** | Native Sprites.dev VM checkpoint API | Sprites API token |
| **S3** | Tar/gzip work_dir, upload to S3 | `--features s3-storage`, `[storage.s3]` config |

### S3 Snapshot Layout

```
s3://bucket/
└── snapshots/
    └── {run_name}/
        └── {worker_name}/
            └── {timestamp}.tar.gz
```

### Configuration

```toml
# Implicit PersistentDisk (inferred from local host)
[runners.local]
host = "local"

# Implicit SpriteCheckpoint (inferred from sprite host)
[runners.sprite]
[runners.sprite.host]
type = "sprite"
api_token = "..."

# Implicit S3 (inferred from fly host + storage.s3 configured)
[runners.fly]
[runners.fly.host]
type = "fly"
app = "hirsel-workers"

# Explicit S3 config with custom prefix
[runners.custom]
[runners.custom.host]
type = "fly"
[runners.custom.snapshot]
type = "s3"
prefix = "custom-snapshots"

# Explicit SpriteCheckpoint with custom comment prefix
[runners.sprite-custom]
[runners.sprite-custom.host]
type = "sprite"
[runners.sprite-custom.snapshot]
type = "sprite_checkpoint"
comment_prefix = "my-prefix"
```

### Pause/Resume Flow

**Pause**:
1. For each active worker, create snapshot (before stopping)
2. Stop the worker process/VM
3. Store snapshot handle in worker's database record

**Resume**:
1. Restore from snapshot (if exists)
2. Spawn worker process
3. Clear snapshot handle after successful spawn

---

## CLI Commands

| Command | Purpose |
|---------|---------|
| `go <run> <spec>` | Start run |
| `runs` | List runs |
| `view <run>` | View status |
| `attach <run>` | TUI output viewer |
| `pause/resume <run>` | Control run |
| `deliver <run>` | Create branch |
| `tasks <run>` | List tasks |

---

## REST API

**Auth**: `Authorization: Bearer $HIRSEL_API_KEY`

### Run Endpoints
| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/runs` | GET | List runs |
| `/api/runs` | POST | Create run (sets up state, spec, initial worker) |
| `/api/runs/{name}` | GET/DELETE | Get/delete run |
| `/api/runs/{name}/files` | GET | Download work directory as tarball |
| `/api/runs/{name}/files` | POST | Upload project files (gzipped tarball) |
| `/api/runs/{name}/spawn` | POST | Spawn workers (body: `{"count": N}`) |
| `/api/runs/{name}/pause` | POST | Pause run |
| `/api/runs/{name}/resume` | POST | Resume run |
| `/api/runs/{name}/tasks` | GET/POST | List/add tasks |
| `/api/runs/{name}/threads/{t}/messages` | GET/POST | Chat messages |

### Config Endpoints
| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/config/general` | PATCH | General settings |
| `/api/config/agent` | PATCH | Agent command |
| `/api/config/runners/{name}` | GET/PUT/DELETE | Runner CRUD |
| `/api/config/profiles/{name}` | GET/PUT/DELETE | Profile CRUD |
| `/api/credentials/{key}` | GET/POST/DELETE | Encrypted credentials |

### Worker Endpoints (Internal)

Used by workers to communicate with daemon/coordinator. All endpoints are scoped to a run via the URL path.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/runs/{run}/workers/list` | GET | List all workers |
| `/api/runs/{run}/workers/active` | GET | List active workers |
| `/api/runs/{run}/workers/all_done` | GET | Check if all workers inactive |
| `/api/runs/{run}/workers/{worker}` | GET | Get specific worker |
| `/api/runs/{run}/workers/{worker}/update` | POST | Update worker state |
| `/api/runs/{run}/workers/{worker}/heartbeat` | POST | Worker heartbeat (returns run status) |
| `/api/runs/{run}/workers/{worker}/claimed_task` | GET | Get task claimed by worker |
| `/api/runs/{run}/config/human_in_the_loop` | GET | Get HITL setting |
| `/api/runs/{run}/config/request` | GET | Get run spec/request |
| `/api/runs/{run}/config/project_path` | GET | Get project path |
| `/api/runs/{run}/config/waiting_reason` | GET/POST | Get/set waiting reason |

---

## Configuration

### config.toml
```toml
[agent]
command = ["hirsel", "__acp-bridge"]  # ACP bridge wrapping Claude CLI

[defaults]
workers = 1
time_limit_minutes = 60

# Local runner (bare process)
[runners.local]
host = "local"

# Local runner with Docker container
[runners.local-docker]
host = "local"
[runners.local-docker.container]
image = "rust:latest"

# SSH runner (bare process on remote)
[runners.my-server]
[runners.my-server.host]
type = "ssh"
address = "user@server.com"
port = 22
work_base = "/tmp/hirsel"

# SSH runner with Docker on remote
[runners.my-server-docker]
[runners.my-server-docker.host]
type = "ssh"
address = "user@server.com"
[runners.my-server-docker.container]
image = "ghcr.io/org/dev-env"

# Sprites runner (no container - Firecracker limitation)
[runners.cloud]
[runners.cloud.host]
type = "sprite"
api_token = "..."
checkpoint = "hirsel-v1"
auto_destroy = true

# Fly.io runner (container required)
[runners.fly]
[runners.fly.host]
type = "fly"
app = "hirsel-workers"
region = "ams"
cpus = 2
memory_mb = 2048
auto_destroy = true
[runners.fly.container]
image = "debian:bookworm-slim"

[profiles.local]
mode = "local"

[profiles.remote]
mode = "remote"
url = "http://server:3000"
```

### Environment Variables
| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Claude API |
| `HIRSEL_API_KEY` | Server auth |
| `FLY_API_TOKEN` | Fly.io API token (for Fly runner) |

---

## Cargo Features

| Feature | Description |
|---------|-------------|
| `gui` | Tauri app (default) |
| `full-cli` | All CLI commands |
| `server` | HTTP server (`hirsel serve`) |
| `tui` | Terminal UI (`hirsel attach`) |
| `worker` | Minimal remote worker binary |
| `s3-storage` | S3-compatible storage backend (MinIO, Tigris, AWS S3) |

### Build Variants
```bash
cargo build                                    # Full GUI app
cargo build --no-default-features -F full-cli  # CLI only
cargo build --no-default-features -F worker    # Minimal worker
```

---

## CI/CD

### GitHub Actions
- **build-linux**: CLI binary for Linux amd64
- **docker**: Image to `ghcr.io` (tags: `latest`, `staging`, `v1.2.3`, sha)
- **release**: GitHub releases on version tags

### Docker
```bash
# Orchestrator
docker run -p 3000:3000 -e HIRSEL_API_KEY=... ghcr.io/OWNER/hirsel serve

# Worker
docker run -e ANTHROPIC_API_KEY=... ghcr.io/OWNER/hirsel __remote-worker ...
```

---

## Fly.io Deployment

Deploy the coordinator to Fly.io for fully cloud-based orchestration.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       Fly.io                                 │
│                                                              │
│   ┌───────────────────────┐    ┌────────────────────────┐   │
│   │     Coordinator       │◄───│   Worker Machines      │   │
│   │   (always-on + vol)   │    │   (ephemeral)          │   │
│   │   hirsel serve        │    │   hirsel __remote-worker│   │
│   └───────────────────────┘    └────────────────────────┘   │
│            ▲                                                 │
└────────────│─────────────────────────────────────────────────┘
             │ HTTPS
        Your laptop
        hirsel go --profile fly
```

### Coordinator Deployment

```bash
# 1. Create app and volume
fly apps create hirsel-coordinator
fly volumes create hirsel_data --size 10 --region ams

# 2. Set secrets
fly secrets set HIRSEL_API_KEY=<your-secret-key>
fly secrets set ANTHROPIC_API_KEY=<your-api-key>

# 3. Build and deploy
cargo build --release --no-default-features --features full-cli
fly deploy
```

### Worker App Setup

Workers run as ephemeral Fly Machines under a separate app:

```bash
# Create workers app (no deployment needed - machines are created on demand)
fly apps create hirsel-workers
```

### Configuration

```toml
# config.toml on your laptop

[runners.fly]
[runners.fly.host]
type = "fly"
app = "hirsel-workers"
region = "ams"
cpus = 2
memory_mb = 2048
[runners.fly.container]
image = "debian:bookworm-slim"

[profiles.fly]
mode = "remote"
url = "https://hirsel-coordinator.fly.dev"
api_key = "your-secret-key"
default_runner = "fly"
```

### Usage

```bash
# Run with Fly workers
hirsel go my-feature spec.md --profile fly

# Or set default profile
export HIRSEL_PROFILE=fly
hirsel go my-feature spec.md
```
