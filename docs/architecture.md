# Hirsel Architecture Overview

> **Important**: This document must be kept up to date as the architecture evolves.

## Table of Contents
1. [Overview](#overview)
2. [Project Structure](#project-structure)
3. [Core Concepts](#core-concepts)
4. [Daemon Architecture](#daemon-architecture)
5. [Orchestrator Pattern](#orchestrator-pattern)
6. [State Management](#state-management)
7. [Worker Execution Model](#worker-execution-model)
8. [CLI Commands](#cli-commands)
9. [GUI Integration](#gui-integration)
10. [File Locations](#file-locations)
11. [Communication Protocols](#communication-protocols)
12. [Server REST API](#server-rest-api)
13. [Configuration](#configuration)
14. [Runner Management](#runner-management)
15. [Data Flow Examples](#data-flow-examples)
16. [Cargo Feature Flags](#cargo-feature-flags)
17. [Separate CLI/GUI Packaging](#separate-cligui-packaging)

---

## Overview

**Hirsel** is a desktop application and CLI tool for orchestrating multiple AI coding agents working together on software projects. Named after the Scottish word for a flock of sheep, Hirsel helps you "herd" your AI agents.

### Tech Stack
- **Backend**: Rust 2021 with Tokio async runtime
- **Desktop**: Tauri v2
- **Database**: SQLite (via rusqlite)
- **Frontend**: TypeScript + Alpine.js
- **Styling**: Tailwind CSS v4 + Basecoat UI
- **CLI**: Clap for argument parsing
- **Agent Protocol**: ACP (Agent Control Protocol)

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        User Interface                            │
├──────────────────────┬──────────────────────┬───────────────────┤
│   CLI (hirsel)       │   GUI (Tauri)        │   HTTP Server     │
│   - go, view, pause  │   - Alpine.js        │   - REST API      │
│   - runs, tasks      │   - Real-time UI     │   - Remote mode   │
└──────────┬───────────┴──────────┬───────────┴─────────┬─────────┘
           │                      │                     │
           ▼                      ▼                     ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Daemon (background process)                   │
│   ~/.hirsel/hirsel.sock                                         │
├─────────────────────────────────────────────────────────────────┤
│  - Lifecycle polling (eval triggering, time limits)             │
│  - Worker spawning and management                               │
│  - Learnings compaction                                         │
│  - Auto-exit when idle                                          │
└──────────────────────────────┬──────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Orchestrator Layer                           │
├─────────────────┬───────────────────┬───────────────────────────┤
│ LocalOrchestrator│ DaemonOrchestrator│ RemoteOrchestrator       │
│ (direct access)  │ (Unix socket)     │ (HTTP API)               │
└─────────────────┴───────────────────┴───────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                     State & Workers                              │
├──────────────────────────┬──────────────────────────────────────┤
│   SQLite State           │   Worker Processes                    │
│   - Runs, Workers, Tasks │   - Local (subprocess)                │
│   - Messages, Evals      │   - SSH (remote)                      │
│   - History, Events      │   - Sprite (cloud VM)                 │
└──────────────────────────┴──────────────────────────────────────┘
```

---

## Project Structure

```
hirsel-bun/
├── src/                          # Frontend (TypeScript, Alpine.js)
│   ├── lib/
│   │   ├── components/           # UI components
│   │   ├── api.ts                # Tauri IPC wrappers
│   │   └── types.ts              # Frontend types
│   ├── templates/                # HTML templates
│   ├── styles/                   # Tailwind + Basecoat
│   └── main.ts                   # Entry point
│
├── src-tauri/                    # Backend (Rust)
│   ├── src/
│   │   ├── main.rs               # Entry (CLI vs GUI dispatch)
│   │   ├── lib.rs                # Shared library
│   │   ├── cli/                  # CLI commands
│   │   │   ├── mod.rs            # Command definitions
│   │   │   ├── go.rs             # Start run
│   │   │   ├── view.rs           # View run
│   │   │   └── ...
│   │   ├── core/                 # Core business logic
│   │   │   ├── orchestrator/     # Orchestration patterns
│   │   │   ├── server/           # HTTP server
│   │   │   ├── state/            # SQLite state
│   │   │   ├── workers.rs        # Worker management
│   │   │   ├── acp.rs            # ACP protocol
│   │   │   └── ...
│   │   ├── daemon/               # Background daemon
│   │   │   ├── mod.rs
│   │   │   ├── server.rs         # Unix socket server
│   │   │   ├── client.rs         # Daemon client
│   │   │   └── lifecycle.rs      # Polling loop
│   │   ├── gui/                  # Tauri commands
│   │   │   └── commands/
│   │   └── worker/               # Worker subprocess
│   │       ├── runner.rs         # Worker execution
│   │       └── acp_client.rs     # ACP client
│   └── Cargo.toml
│
├── tests/scenarios/              # E2E test specs
└── docs/                         # Documentation
```

---

## Core Concepts

### Runs
A **run** represents a single orchestration instance where AI agents work on a project.

| Property | Description |
|----------|-------------|
| `name` | Unique identifier (e.g., "quirky-alpaca-42") |
| `status` | Draft, Working, Paused, Eval, Done, Delivered, Failed |
| `worker_scale` | Max workers for autoscaling |
| `time_limit_minutes` | Auto-pause timeout |
| `human_in_the_loop` | Pause for human approval |

**Storage**: `~/.hirsel/runs/{run_name}/`

### Workers
**Workers** are AI agent processes running concurrently on a run.

| Property | Description |
|----------|-------------|
| `name` | Identifier (e.g., "leader", "worker-2") |
| `pid` | Process ID |
| `status` | Working, Awaiting, Paused, Error |
| `session_id` | ACP session identifier |
| `work_dir` | Git worktree path |

**Roles**:
- **Leader**: First worker, coordinates shared state
- **Teammates**: Claim tasks independently, each has own git worktree

### Tasks
**Tasks** are work items in a hierarchical breakdown.

| Status | Description |
|--------|-------------|
| `Todo` | Available for claiming |
| `Doing` | Claimed by a worker |
| `Done` | Completed |

**Storage**: `tasks.md` (table) + `tasks/{id}.md` (details)

### Evals
**Evals** verify work meets spec before marking a run as Done.

Triggered automatically when all workers become inactive (if `eval.md` exists).

### Specs & Messages
- **spec.md**: Project requirements (immutable after start)
- **eval.md**: Evaluation criteria
- **chats/**: Message threads between workers and humans

---

## Daemon Architecture

The daemon is a persistent background process that manages run lifecycle independently of GUI/CLI.

### Purpose
- **Lifecycle Polling**: Check runs every 5 seconds
- **Eval Triggering**: When all workers idle, run evaluation
- **Time Limits**: Auto-pause when time limit exceeded
- **Auto-Exit**: Stops after 5 minutes of no active runs

### Architecture

```
┌─────────────┐     ┌─────────────┐
│   CLI       │     │   GUI       │
│  (hirsel)   │     │  (Tauri)    │
└──────┬──────┘     └──────┬──────┘
       │                   │
       │ Unix Socket       │ Uses DaemonClient
       │                   │
       ▼                   ▼
┌─────────────────────────────────┐
│         hirsel daemon           │
│  ~/.hirsel/hirsel.sock          │
├─────────────────────────────────┤
│  - LocalOrchestrator            │
│  - Lifecycle polling loop       │
│  - Worker spawning              │
│  - Eval triggering              │
│  - Time limit enforcement       │
└─────────────────────────────────┘
```

### Files
- `daemon/mod.rs` - Module entry, socket/pid path helpers
- `daemon/server.rs` - Unix socket server (reuses axum routes)
- `daemon/client.rs` - Client for connecting to daemon
- `daemon/lifecycle.rs` - Polling loop implementation

### Commands
```bash
hirsel daemon start    # Start daemon (auto-starts when needed)
hirsel daemon stop     # Stop daemon
hirsel daemon status   # Check daemon status
```

### Auto-Start
The daemon auto-starts when:
- CLI runs `hirsel go` (local mode)
- GUI opens and lists runs

---

## Orchestrator Pattern

Three implementations of the `Orchestrator` trait for different deployment scenarios:

```
Local Mode:                          Remote Mode:
┌─────────────┐                      ┌─────────────┐
│ GUI/CLI     │                      │ GUI/CLI     │
└──────┬──────┘                      └──────┬──────┘
       │ (in-process)                       │ (HTTP API)
       ▼                                    ▼
┌─────────────┐                      ┌─────────────┐
│ Local       │                      │ Remote      │
│ Orchestrator│──spawns──►workers    │ Orchestrator│
└─────────────┘                      └──────┬──────┘
                                            │ (HTTP)
                                            ▼
                                     ┌─────────────┐
                                     │ Server      │
                                     │ Orchestrator│──spawns──►workers
                                     └─────────────┘
```

### LocalOrchestrator
Direct synchronous access to local state.
- Used internally by daemon and server
- Direct SQLite and filesystem access
- Spawns workers directly via Runner trait
- No network overhead

### DaemonOrchestrator
Communicates with local daemon via Unix socket.
- Auto-starts daemon if not running
- HTTP-over-Unix-socket protocol
- Used by CLI/GUI for local mode

### RemoteOrchestrator
HTTP API calls to remote server.
- Used when profile mode is "remote"
- Uploads project files as tarball to server
- Server spawns workers (no tunnels needed)
- Bearer token authentication

### Server-Side Spawning
When using remote mode, the server is responsible for spawning workers:

1. **CLI sends `POST /api/runs`** - Creates run on server
2. **CLI uploads files** - `POST /api/runs/{name}/files` with tarball
3. **CLI triggers spawn** - `POST /api/runs/{name}/spawn`
4. **Server spawns workers** - Using its local runner configs
5. **Workers fetch files** - Download from server's `/api/runs/{name}/files`
6. **Workers connect directly** - No tunnels needed (server is public)

### Factory Function
```rust
// Returns appropriate orchestrator based on profile
let orch = create_orchestrator(profile)?;

// For explicit daemon connection
let daemon_orch = create_daemon_orchestrator()?;

// For direct local access (internal use)
let local_orch = create_local_orchestrator()?;
```

---

## State Management

### SQLite Database
**Location**: `~/.hirsel/runs/{run_name}/hirsel.db`

### Key Tables

```sql
state              -- Run metadata
  status, created_at, started_at
  worker_scale, time_limit_minutes
  human_in_the_loop, project_path

workers            -- Active worker processes
  name, pid, status, session_id
  work_dir, last_heartbeat

tasks              -- Work breakdown
  id, name, status
  claimed_by, parent_id, blocked_by

evals              -- Evaluation runs
  status, feedback, log_file

messages           -- Chat threads
  thread, sender, content, timestamp

worker_events      -- Real-time output streaming
  worker_name, event_type, content
```

### State Access
```rust
let state = SQLiteState::new(db_path)?;
state.status()?;
state.set_status(Status::Working)?;
state.get_workers()?;
state.claim_task(task_id, worker_name)?;
```

---

## Worker Execution Model

### Worker Lifecycle

```
1. Spawn Phase
   hirsel __worker-run --run X --worker Y
   (detached subprocess with process_group(0))

2. Initialization
   - Connect to SQLite state
   - Spawn ACP agent (e.g., claude-code-acp)
   - Mark self as Working

3. Main Loop
   - Send spec + tasks to agent
   - Process tool calls (task-claim, task-done, etc.)
   - Stream output to worker_events table
   - Heartbeat updates

4. Completion
   - Signal work_done
   - Daemon detects idle, triggers eval
```

### Runner Types

| Runner | Description | Config |
|--------|-------------|--------|
| **Local** | Subprocess on local machine | Default |
| **SSH** | Remote via SSH + reverse tunnel | `[[remotes]]` |
| **Sprite** | Cloud VMs on Sprites.dev | `[runners.sprite]` |

### ACP Protocol
Agent Control Protocol for AI agent communication.

```rust
// Spawn ACP child with automatic cleanup
let config = AcpSpawnConfig::new(agent_command, work_dir, "context");
let mut child = AcpChild::spawn(config)?;

// Cleanup on drop: SIGTERM → wait 100ms → SIGKILL
```

### Process Group Management
- Workers use `process_group(0)` for isolation
- Parent can kill entire group with single signal
- Handles grandchild processes (agent's children)

---

## CLI Commands

### Run Management
| Command | Purpose |
|---------|---------|
| `hirsel go <run> <spec>` | Start new run |
| `hirsel runs` | List all runs |
| `hirsel view <run>` | View run status |
| `hirsel attach <run>` | TUI worker output viewer |
| `hirsel pause <run>` | Pause all workers |
| `hirsel resume <run>` | Resume run |
| `hirsel deliver <run>` | Create branch with changes |
| `hirsel delete <run>` | Remove run |

### Task Management
| Command | Purpose |
|---------|---------|
| `hirsel tasks <run>` | List tasks |
| `hirsel task-add <run> <id> <desc>` | Add task |
| `hirsel task-done <run> <id>` | Mark complete |
| `hirsel task-delete <run> <id>` | Delete task |

### Configuration
| Command | Purpose |
|---------|---------|
| `hirsel config` | Interactive agent selection |
| `hirsel templates` | List spec templates |
| `hirsel improve` | Update memory from learnings |

### Internal Commands
```bash
hirsel __worker-run ...       # Worker subprocess
hirsel __eval-run ...         # Eval subprocess
hirsel __daemon               # Daemon server
hirsel __compact-learnings    # Learnings compaction
```

---

## GUI Integration

### Architecture
```
Alpine.js Frontend ←→ Tauri IPC ←→ Rust Backend
     (src/)           (invoke)    (gui/commands/)
```

### Key Components
| Component | Purpose |
|-----------|---------|
| `app-state.ts` | Global state management |
| `run-list.ts` | Runs panel with filtering |
| `run-detail.ts` | Selected run overview |
| `worker-panel.ts` | Workers status grid |
| `task-panel.ts` | Task breakdown view |
| `chat-panel.ts` | Agent chat interface |
| `worker-output-viewer.ts` | Real-time logs |

### Tauri Commands
```typescript
// Example IPC calls
await invoke('get_runs');
await invoke('get_run_detail', { runName });
await invoke('pause_run', { runName });
await invoke('send_message', { runName, content, thread });
```

---

## File Locations

### Home Directory Structure
```
~/.hirsel/
├── config.toml               # Global configuration
├── hirsel.db                 # Global DB (credentials)
├── hirsel.sock               # Daemon Unix socket
├── hirsel.pid                # Daemon PID file
└── runs/
    └── {run_name}/
        ├── hirsel.db         # Run state
        ├── spec.md           # Specification
        ├── eval.md           # Evaluation script
        ├── tasks.md          # Task table
        ├── tasks/            # Task detail files
        ├── work/             # Git worktrees
        │   ├── leader/
        │   └── worker-2/
        ├── chats/            # Message threads
        ├── assets/           # Images for spec
        └── tmp/              # Worker logs
```

### Tauri App Data
```
~/.local/share/app.hirsel/
└── logs/
    └── Hirsel.log            # Backend logs
```

---

## Communication Protocols

### Daemon Socket (Local)
- **Location**: `~/.hirsel/hirsel.sock`
- **Protocol**: HTTP/1.1 over Unix socket
- **Auth**: Filesystem permissions (no tokens needed)

### HTTP Server (Remote)
- **Command**: `hirsel serve --port 3000`
- **Auth**: `HIRSEL_API_KEY` environment variable
- **Endpoints**: Same as daemon routes

### Coordinator API (Remote Workers)
- **Purpose**: Remote workers access state via HTTP
- **Setup**: Reverse SSH tunnel to localhost
- **Endpoints**: Task claim, status updates, messages

---

## Server REST API

The HTTP server (`hirsel serve`) exposes a REST API for remote orchestration and configuration management.

### Authentication
All API endpoints (except `/health`) require bearer token authentication:
```bash
curl -H "Authorization: Bearer $HIRSEL_API_KEY" https://server/api/runs
```

### Run Lifecycle

**Create Run:**
```bash
POST /api/runs
{
  "name": "my-run",
  "spec": "# Spec content...",
  "runner": "sprites",           # optional
  "worker_scale": 2,             # optional
  "time_limit_minutes": 60,      # optional
  "eval": "# Eval content..."    # optional
}
# Returns: { "name": "my-run", "run_dir": "...", "files_url": "/api/runs/my-run/files" }
```

**Upload Project Files:**
```bash
# Upload tarball of project (excludes node_modules, .git, target, etc.)
tar czf - ./my-project | curl -X POST https://server/api/runs/my-run/files \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/gzip" \
  --data-binary @-
```

**Download Project Files:**
```bash
# Workers fetch files from server
GET /api/runs/{name}/files
# Returns: application/gzip tarball
```

**Spawn Workers:**
```bash
POST /api/runs/{name}/spawn
{ "count": 1 }
# Returns: { "workers": ["bonnie-cheviot"] }
```

### Run Management

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/runs` | GET | List all runs |
| `/api/runs/{name}` | GET | Get run details |
| `/api/runs/{name}` | DELETE | Delete run |
| `/api/runs/{name}/pause` | POST | Pause run |
| `/api/runs/{name}/resume` | POST | Resume run |
| `/api/runs/{name}/deliver` | POST | Create delivery branch |
| `/api/runs/{name}/workers` | GET | List workers |
| `/api/runs/{name}/tasks` | GET/POST | List/add tasks |
| `/api/runs/{name}/threads` | GET | List chat threads |
| `/api/runs/{name}/threads/{thread}/messages` | GET/POST | Get/send messages |

### Configuration API

**General Settings:**
```bash
PATCH /api/config/general
{
  "eval_timeout": 1800,
  "auto_learn": true,
  "max_iterations": 100,
  "human_in_the_loop": true,
  "default_runner": "sprites",
  "coordinator_port": 19700
}
```

**Agent Settings:**
```bash
PATCH /api/config/agent
{ "command": ["claude-code-acp", "--model", "opus"] }
```

**Compaction Settings:**
```bash
PATCH /api/config/compaction
{
  "enabled": true,
  "threshold": 10000,
  "keep_messages": 40
}
```

**Auth Settings:**
```bash
GET /api/config/auth                    # Get all auth configs
PATCH /api/config/auth/{agent}          # Update agent auth (claude, gemini, etc.)
{ "method": "oauth", "api_key": "...", "env_var": "..." }
DELETE /api/config/auth/{agent}         # Delete agent auth
```

### Runner CRUD

```bash
GET /api/config/runners                 # List all runners
GET /api/config/runners/{name}          # Get runner config
PUT /api/config/runners/{name}          # Create/update runner
{
  "type": "sprite",
  "api_token": "...",
  "base_checkpoint": "hirsel-v1",
  "auto_destroy": true
}
DELETE /api/config/runners/{name}       # Delete runner
```

### Profile CRUD

```bash
GET /api/config/profiles                # List all profiles
GET /api/config/profiles/{name}         # Get profile
PUT /api/config/profiles/{name}         # Create/update profile
{
  "mode": "remote",
  "url": "https://server:3000",
  "api_key": "...",
  "access": { "type": "direct" }
}
DELETE /api/config/profiles/{name}      # Delete profile
```

### Credentials

Credentials are stored encrypted. Values are never returned in full.

```bash
POST /api/credentials/{key}             # Store credential
{ "value": "sk-ant-..." }

GET /api/credentials/{key}              # Check if exists
# Returns: { "key": "api_key", "exists": true, "masked_value": "sk-a...XYZ" }

DELETE /api/credentials/{key}           # Delete credential
```

### Remote Mode Flow

```
Client                              Server                          Worker
──────                              ──────                          ──────
1. POST /api/runs ──────────────────► Creates run (Draft status)
2. POST /api/runs/{name}/files ─────► Stores tarball in work/
3. POST /api/runs/{name}/spawn ─────► Spawns workers ──────────────► Started
4.                                  ◄── GET /api/runs/{name}/files ─┘
                                       Workers download & extract
5. GET /api/runs/{name} ────────────► Monitor progress
```

---

## Configuration

### Global Config (`~/.hirsel/config.toml`)
```toml
[agent]
command = ["claude-code-acp"]
type = "claude"

[defaults]
workers = 1
time_limit_minutes = 60

[[runners]]
name = "local"
type = "local"

[[runners]]
name = "sprite"
type = "sprite"
api_token = "..."

[[remotes]]
name = "gpu-server"
host = "user@gpu.example.com"
ssh_port = 22

[profiles.local]
mode = "local"

[profiles.remote]
mode = "remote"
url = "http://server:3000"
```

### Environment Variables
| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Claude API key |
| `GOOGLE_API_KEY` | Gemini API key |
| `HIRSEL_API_KEY` | HTTP server auth |
| `ACP_PERMISSION_MODE` | Set to `bypassPermissions` for workers |

---

## Runner Management

The GUI settings modal provides UX features for managing runners.

### Contextual Runner Naming

The "local" runner's display changes based on the selected profile:

| Profile | Runner Name | Badge | Description |
|---------|-------------|-------|-------------|
| Local   | `local`     | Built-in | Run workers as subprocesses on this machine |
| Remote  | `orchestrator` | Server | Run workers as subprocesses on the orchestrator server |

This helps clarify where workers will actually run - on the local machine (local profile) or on the remote server (remote profile).

### "This Machine" Feature

When adding a new SSH runner, users can quickly add their current machine if Tailscale is connected:

1. **Detection**: On opening the Add Runner dialog, the GUI checks `tailscale status --json`
2. **Quick-add**: If connected, shows the machine's Tailscale DNS name with a "Use" button
3. **Pre-population**: Clicking "Use" fills in:
   - Name: machine hostname
   - Host: Tailscale DNS name (e.g., `machine.tail12345.ts.net`)
   - Port: 22
   - Work directory: `/tmp/hirsel-remote`

This is useful for remote profiles where you want to use your local machine as an SSH runner.

### Runner Health Monitoring

SSH runners are polled for connectivity when the Runners tab is active:

**Status States:**
| State | Visual | Meaning |
|-------|--------|---------|
| Checking | Gray pulsing dot | SSH connection in progress |
| Online | Green dot | SSH connection successful |
| Offline | Red dot | SSH connection failed |

**Implementation:**
- **Polling interval**: Every 10 seconds while Runners tab is open
- **SSH check command**: `ssh -o BatchMode=yes -o ConnectTimeout=5 {host} echo ok`
- **Tooltip**: Hover shows latency (when online) or error message (when offline)
- **Cleanup**: Polling stops when leaving the Runners tab

**Error messages** are simplified from SSH output:
- "Permission denied" - SSH key not authorized
- "Connection refused" - SSH server not running
- "Connection timed out" - Host unreachable
- "Host not found" - DNS resolution failed

---

## Data Flow Examples

### Starting a Run
```
1. CLI: hirsel go my-run spec.md
2. Create run directory, write spec.md
3. Initialize SQLite state (status=Draft)
4. Start daemon (if not running)
5. Spawn workers:
   - Local: hirsel __worker-run --run my-run --worker leader
   - Set status=Working
6. Workers:
   - Spawn ACP agent
   - Send spec + tasks
   - Enter claim/do/done loop
```

### Worker Processing
```
1. Worker claims task (status=Doing)
2. Agent works on task
3. Agent calls task-done
4. Worker marks task complete (status=Done)
5. Worker checks for more tasks
6. If no tasks: set status=Awaiting
7. Daemon detects all workers idle
8. Daemon triggers eval (if eval.md exists)
```

### Eval Completion
```
1. Daemon spawns: hirsel __eval-run
2. Eval runs test script
3. If passed: status=Done
4. If failed: can restart workers or mark Failed
```

---

## Cargo Feature Flags

The Rust crate uses feature flags to support different build configurations, from full desktop app to minimal worker binary.

### Feature Overview

| Feature | Description | Dependencies Added |
|---------|-------------|-------------------|
| `gui` | Tauri desktop application | tauri, tauri-plugin-*, full-cli |
| `full-cli` | Complete CLI (go, test, attach, serve) | server, tui |
| `server` | HTTP server mode (`hirsel serve`) | axum, tower-http, hyper |
| `tui` | Terminal UI (`hirsel attach`) | ratatui, crossterm |
| `worker` | Minimal worker binary | (none - subset of core) |

### Default Build

```bash
cargo build
# Features: gui + full-cli + server + tui
# Binary size: ~25MB
# Use case: Development, desktop app
```

### Build Variants

**Full CLI (no GUI):**
```bash
cargo build --no-default-features --features full-cli
# Features: full-cli + server + tui
# Use case: Server deployment, headless operation
```

**Worker-only (minimal):**
```bash
cargo build --no-default-features --features worker
# Features: worker only
# Binary size: ~8MB (target)
# Use case: Remote worker deployment on sprites/VMs
```

### Feature Graph

```
default
  └── gui
        ├── tauri, tauri-plugin-*
        └── full-cli
              ├── server (axum, tower-http, hyper)
              └── tui (ratatui, crossterm)

worker (standalone, minimal deps)
```

### Module Feature Gates

Key modules are feature-gated:

```rust
// Server-only modules (core/)
#[cfg(feature = "server")]
pub mod coordinator_api;
#[cfg(feature = "server")]
pub mod git_http;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod tunnel;

// Server-only (orchestrator)
#[cfg(feature = "server")]
mod daemon;  // DaemonOrchestrator
#[cfg(feature = "server")]
pub fn create_daemon_orchestrator();

// TUI-only modules (cli/)
#[cfg(feature = "tui")]
pub mod attach;
#[cfg(feature = "tui")]
pub mod tui;

// Full CLI modules (cli/)
#[cfg(feature = "full-cli")]
pub mod go;
#[cfg(feature = "full-cli")]
pub mod test;
```

### Command Feature Gates

CLI commands are feature-gated to support minimal builds:

| Command | Feature | Description |
|---------|---------|-------------|
| `go` | `full-cli` | Start a new run |
| `test` | `full-cli` | Run e2e test scenarios |
| `attach` | `tui` | TUI worker output viewer |
| `serve` | `server` | HTTP server mode |
| `daemon` | `server` | Background daemon |
| `daemon start/stop/status` | `server` | Daemon control |

Commands like `runs`, `view`, `pause`, `resume`, `tasks`, etc. are always available.

### Worker Binary Purpose

The `worker` feature creates a minimal binary for deployment to remote machines (sprites, VMs):

- Runs the `__worker-run` internal command
- Connects to coordinator via HTTP
- Spawns ACP agent subprocess
- No server, TUI, or GUI dependencies

This enables smaller uploads to remote runners and faster startup times.

### Shared Dependencies

Some dependencies are always included (not feature-gated):
- `serde`, `serde_json` - Serialization
- `rusqlite` - SQLite state
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `git2` - Git operations
- `aes-gcm`, `hex` - Credential encryption

---

## Separate CLI/GUI Packaging

The architecture supports separate installables via feature flags:

**CLI Package** (`hirsel`):
```bash
cargo build --no-default-features --features full-cli
```
- CLI commands + daemon + server
- No Tauri dependencies
- Use case: Servers, headless operation

**GUI Package** (`hirsel-gui`):
```bash
cargo build  # default features
```
- Full Tauri app
- Includes CLI commands
- Use case: Desktop development

**Worker Package** (`hirsel-worker`):
```bash
cargo build --no-default-features --features worker
```
- Minimal binary for remote execution
- Use case: Sprite/VM deployment

Both CLI and GUI connect to the same daemon socket, enabling:
- Install CLI only on servers
- Install GUI only on desktops
- Both on development machines
