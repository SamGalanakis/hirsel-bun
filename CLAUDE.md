# Hirsel Development Guidelines

## Architecture Documentation

> **Important**: See [`docs/architecture.md`](docs/architecture.md) for the full system architecture overview.
> This document **must be kept up to date** as the architecture evolves.

The architecture doc covers:
- High-level system design and tech stack
- Daemon architecture (CLI/GUI thin clients → daemon → state)
- Orchestrator pattern (Local, Daemon, Remote)
- Worker execution model and ACP protocol
- State management (SQLite)
- File locations and data flow

## Development Mode - No Backwards Compatibility

This is a development project. **Backwards compatibility with old databases is not required.**

When making schema changes:
- Modify the schema directly in `src-tauri/src/core/state/mod.rs` (the SCHEMA constant)
- Do NOT add migrations
- Reset local data when needed (see below)

### Data Locations

```
~/.hirsel/                    # Main hirsel data
├── config.toml               # Configuration (profiles, agent settings)
├── hirsel.db                 # Global DB (credentials, gyp chat history)
└── runs/                     # Per-run databases

~/.local/share/app.hirsel/    # Tauri app data (logs, frontend state)
```

### Reset Commands

```bash
# Reset runs only (keeps config + credentials)
rm -rf ~/.hirsel/runs

# Full reset (like first install)
rm -rf ~/.hirsel ~/.local/share/app.hirsel
```

## Basecoat UI

Use [Basecoat UI](https://basecoatui.com/) components for all UI elements. Basecoat provides accessible, styled components.

**Key URLs:**
- Components: https://basecoatui.com/components/
- Form: https://basecoatui.com/components/form/
- Select: https://basecoatui.com/components/select/
- Button: https://basecoatui.com/components/button/
- Switch: https://basecoatui.com/components/switch/

### Buttons
```html
<button class="btn">Primary</button>
<button class="btn btn-outline">Outline</button>
<button class="btn btn-ghost">Ghost</button>
<button class="btn btn-destructive">Destructive</button>
```

### Forms
Use `form` class with `grid gap-6` for form containers. Each field uses `grid gap-2` with label, input, then description below:
```html
<form class="form grid gap-6">
  <!-- Standard text field -->
  <div class="grid gap-2">
    <label for="username">Username</label>
    <input type="text" id="username" placeholder="Enter username" />
    <p class="text-muted-foreground text-sm">This is your public display name.</p>
  </div>

  <!-- Select field -->
  <div class="grid gap-2">
    <label for="role">Role</label>
    <select id="role">
      <option value="user">User</option>
      <option value="admin">Admin</option>
    </select>
    <p class="text-muted-foreground text-sm">Select your account role.</p>
  </div>
</form>
```

### Switch Toggle (Bordered Card)
For toggle switches, use the bordered card pattern with label and description on left, switch on right:
```html
<div class="gap-2 flex flex-row items-start justify-between rounded-lg border p-4 shadow-xs">
  <div class="flex flex-col gap-0.5">
    <label for="notifications" class="leading-normal">Email Notifications</label>
    <p class="text-muted-foreground text-sm">Receive emails about new features and updates.</p>
  </div>
  <input type="checkbox" id="notifications" role="switch" />
</div>
```

**Key patterns:**
- Form wrapper: `form class="form grid gap-6"`
- Field wrapper: `div class="grid gap-2"`
- Order: label → input → description
- Description: `p class="text-muted-foreground text-sm"`
- Switch cards: `rounded-lg border p-4 shadow-xs` with flex row layout

### Select (Custom Dropdown)
Native `<select>` dropdowns can't be fully themed. For styled selects, use Basecoat's custom select pattern:
```html
<div class="select" x-data="{ open: false }">
  <button type="button" class="btn-outline w-full" @click="open = !open" @click.away="open = false"
    aria-haspopup="listbox" :aria-expanded="open">
    <span class="truncate">Selected Value</span>
    <i data-lucide="chevrons-up-down" class="w-4 h-4 opacity-50 shrink-0"></i>
  </button>
  <div data-popover :aria-hidden="!open" x-show="open" x-transition>
    <div role="listbox" aria-orientation="vertical">
      <div role="option" data-value="opt1" :aria-selected="value === 'opt1'" @click="value = 'opt1'; open = false">
        Option 1
      </div>
      <div role="option" data-value="opt2" :aria-selected="value === 'opt2'" @click="value = 'opt2'; open = false">
        Option 2
      </div>
    </div>
  </div>
</div>
```
**Important:** Basecoat adds checkmarks via CSS based on `aria-selected` - don't add manual check icons.

### Toast
```javascript
window.dispatchEvent(new CustomEvent('basecoat:toast', {
  detail: { description: 'Message here', variant: 'success' }
}));
// Or use the helper: window.toast.success('Message')
```

### Tooltips
```html
<button data-tooltip="Tooltip text" data-side="top">Hover me</button>
```

## Lucide Icons

Use [Lucide Icons](https://lucide.dev/icons/) for all iconography.

**Search URL:** `https://lucide.dev/icons/?search=<term>`

### Usage
```html
<i data-lucide="file-text" class="w-4 h-4"></i>
<i data-lucide="upload" class="w-5 h-5 text-amber-500"></i>
```

### Important Caveats

1. **Lucide replaces `<i>` with `<svg>`** - Don't put Alpine directives like `x-show` directly on `<i data-lucide>` elements. Wrap in a `<span>`:
   ```html
   <!-- BAD - x-show may not work -->
   <i data-lucide="check" x-show="selected" class="w-4 h-4"></i>

   <!-- GOOD - wrap in span -->
   <span x-show="selected"><i data-lucide="check" class="w-4 h-4"></i></span>
   ```

2. **Dynamic icons need re-initialization** - After adding icons dynamically, call:
   ```javascript
   lucide.createIcons({ inTemplates: true });
   // Or scope to specific element:
   lucide.createIcons({ inTemplates: true, nodes: [element] });
   ```
   **Always use `inTemplates: true`** - This option processes icons inside `<template>` tags, which is essential for Alpine.js `x-for` loops. Without it, icons in dynamically rendered templates won't appear.

3. **Common icons:**
   - Navigation: `chevron-down`, `chevron-right`, `chevrons-up-down`, `arrow-left`
   - Actions: `plus`, `trash-2`, `pencil`, `check`, `x`
   - Status: `circle-check`, `circle-x`, `alert-triangle`, `loader`
   - Files: `file`, `file-text`, `folder`, `code`
   - Misc: `settings`, `search`, `server`, `cpu`, `cloud`

## Alpine.js

The app uses Alpine.js for reactivity. Components are defined in `src/lib/components/` and registered globally in `main.ts`.

## Styling

- Tailwind CSS v4 with custom theme colors (pasture, wool, sage, terra, golden, amber)
- Custom styles in `src/styles/main.css`
- Basecoat component styles in `src/styles/output.css`

## Tauri

Backend is Rust with Tauri v2. Commands are invoked via `window.tauriInvoke()`.

## Hirsel CLI

The Rust CLI is built as part of the Tauri app. The binary serves as both GUI (no args) and CLI (with subcommands).

### Location

```bash
# Debug build (use during development)
./src-tauri/target/debug/hirsel

# After cargo build
cargo build  # builds to src-tauri/target/debug/hirsel
```

### Common Commands

```bash
# List all runs
./src-tauri/target/debug/hirsel runs

# View run status
./src-tauri/target/debug/hirsel view <run-name>

# Watch worker output (TUI)
./src-tauri/target/debug/hirsel attach <run-name>

# Start a run
./src-tauri/target/debug/hirsel go <run-name> <spec-file>

# Delete a run
./src-tauri/target/debug/hirsel delete <run-name>
```

### Running E2E Tests

Test scenarios are in `tests/scenarios/`. Each has a spec, optional eval, and project folder.

```bash
# List available test scenarios
./src-tauri/target/debug/hirsel test

# Run calculator test (creates test-calculator run)
./src-tauri/target/debug/hirsel test calculator --yolo

# Run with custom name
./src-tauri/target/debug/hirsel test calculator --run-name my-test --yolo

# Run with multiple workers
./src-tauri/target/debug/hirsel test calculator --workers 2 --yolo
```

The `--yolo` flag skips confirmation prompts.

### Data Location

Runs are stored in `~/.hirsel/runs/`. Both the CLI and Tauri GUI share this directory.

## Development & Debugging

When running or testing the app, use `./dev.sh` which sets up the proper environment for debugging:

```bash
./dev.sh
```

This is especially important when spawning agents or running in development mode.

### Backend Logs

Backend logs are written to:
```
~/.local/share/app.hirsel/logs/Hirsel.log
```

Use `tracing::info!`, `tracing::warn!`, `tracing::error!` for logging in Rust code. View logs with:
```bash
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log
```

Or filter for specific components:
```bash
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log | grep WorkerStream
```

### Frontend Console Logs

In dev mode, frontend `console.log/warn/error` calls are also written to the backend log file with `[Frontend]` prefix:
```bash
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log | grep Frontend
```

This allows debugging frontend issues alongside backend logs in a single file.

## Subprocess Process Groups

When spawning detached subprocesses that run AI agents (workers, eval, compaction, improve), we use `process_group(0)` to create a new process group. This allows killing all descendant processes together.

### AcpChild - Reusable ACP Process Wrapper

**Always use `AcpChild` when spawning ACP processes.** It handles:
- Spawning with correct configuration (stdin/stdout piped, process group)
- Automatic cleanup on drop (SIGTERM → wait → SIGKILL to process group)
- Environment variable forwarding (API keys, etc.)

```rust
use crate::core::acp::{AcpChild, AcpSpawnConfig};

let spawn_config = AcpSpawnConfig::new(
    agent_command.clone(),
    working_dir.clone(),
    "my-context",  // For logging
);
let mut acp_child = AcpChild::spawn(spawn_config)?;
let stdin = acp_child.take_stdin().unwrap();
let stdout = acp_child.take_stdout().unwrap();

// ... use stdin/stdout for ACP communication ...

// Cleanup happens automatically when acp_child is dropped
```

### Process Cleanup Strategy

Node.js processes (like `claude-code-acp`) may ignore SIGTERM, so we use a two-step approach:

1. **SIGTERM** - Send graceful termination signal to process group
2. **Wait 100-200ms** - Allow graceful shutdown
3. **SIGKILL** - Force kill any remaining processes in the group

This pattern is implemented in:
- `AcpChild::cleanup()` / `AcpChild::Drop` - Primary mechanism for ACP processes
- `core::workers::pause_all_workers()` - GUI-initiated pause
- `core::workers::kill_all_workers()` - Run completion/timeout
- `core::process::cleanup_process_group()` - Subprocess self-cleanup

**IMPORTANT:** When killing a process group:
- Always send SIGKILL after SIGTERM, even if the group leader appears dead
- The leader (hirsel subprocess) dies quickly, but children (claude-code-acp) may survive

### Files using AcpChild:
- `worker/acp_client.rs` - Worker ACP execution
- `core/eval.rs` - Eval agent execution
- `core/compaction.rs` - Learnings compaction
- `core/chat_session.rs` - Interactive chat sessions
- `cli/improve.rs` - Memory file updates

See `src/core/process.rs` for the implementation.

## Cargo Feature Flags

The Rust crate supports feature flags to control binary size and functionality:

```toml
[features]
default = ["gui", "full-cli"]  # Full desktop app
gui = [...]                     # Tauri GUI (includes full-cli)
full-cli = ["server", "tui"]    # Full CLI with all commands
server = [...]                  # Server mode (hirsel serve), daemon, coordinator API
tui = [...]                     # TUI mode (attach command)
worker = []                     # Minimal worker binary for remote deployment
```

### Build Variants

```bash
# Full app (default) - GUI + all CLI commands
cargo build --release

# CLI only (no GUI, ~30% smaller)
cargo build --release --no-default-features --features full-cli

# Worker only (for sprite/remote deployment)
cargo build --release --no-default-features --features worker
```

### Heavy Dependencies by Feature

| Feature | Dependencies | Size Impact |
|---------|-------------|-------------|
| `gui` | tauri, webkit | ~20MB |
| `server` | axum, hyper, tower | ~5MB |
| `tui` | ratatui, crossterm | ~2MB |

### Feature-Gated Commands

| Command | Required Feature |
|---------|-----------------|
| `go` | `full-cli` |
| `test` | `full-cli` |
| `attach` | `tui` |
| `serve` | `server` |
| `daemon` | `server` |

### Version Information

The binary includes build metadata:
```bash
hirsel --version    # Shows: hirsel 0.1.0 (abc1234)
hirsel --build-info # Shows version, git SHA, build date, and features
```

Version info is embedded at compile time via `build.rs`:
- `CARGO_PKG_VERSION` - Cargo.toml version
- `HIRSEL_GIT_SHA` - Git commit (with `-dirty` suffix if uncommitted changes)
- `HIRSEL_BUILD_DATE` - Build date (YYYY-MM-DD)

## CI/CD

### GitHub Actions

The `.github/workflows/build.yml` workflow:
1. **build-linux**: Builds CLI binary for Linux amd64
2. **docker**: Builds and pushes Docker image to ghcr.io
3. **release**: Creates GitHub releases with binaries on version tags

Triggers:
- Push to `main` or `staging` branches
- Pull requests to `main`
- Tags starting with `v` (e.g., `v0.1.0`)

### Docker Image

```bash
# Pull from GitHub Container Registry
docker pull ghcr.io/OWNER/hirsel-bun:latest

# Run as server
docker run -p 3000:3000 -e HIRSEL_API_KEY=secret ghcr.io/OWNER/hirsel-bun serve --port 3000

# Run as worker
docker run -e ANTHROPIC_API_KEY=... ghcr.io/OWNER/hirsel-bun __remote-worker ...
```

Image tags:
- `latest` - Latest main branch build
- `staging` - Latest staging branch build
- `v0.1.0` - Specific version releases
- `abc1234` - Specific commit SHA

## Sprite Workers

Hirsel supports running workers on [Sprites.dev](https://sprites.dev) cloud VMs - lightweight Firecracker VMs that hibernate when idle.

### Setting Up a Sprite Checkpoint

For faster worker startup, create a checkpoint with pre-installed dependencies:

```bash
# Build the worker binary
cargo build --release --no-default-features --features worker

# Set your Sprites API token
export SPRITES_TOKEN=your-token

# Run the setup script
./scripts/setup-sprite-checkpoint.sh my-checkpoint

# Output: Checkpoint ID to use in config
```

The checkpoint includes:
- Node.js 22 + npm
- Claude Code CLI (`@anthropic-ai/claude-code`)
- Codex CLI (`@openai/codex`)
- ACP adapters (`claude-code-acp`, `codex-acp`)
- Hirsel worker binary

### Configuration

Add to `~/.hirsel/config.toml`:

```toml
[runners.sprite]
api_token = "your-sprites-token"
base_checkpoint = "checkpoint-id-from-setup"
auto_destroy = true          # Clean up sprites after completion
use_file_push = false        # Push files via HTTP (requires Tailscale)
```

### File Transfer Modes

**Pull Mode** (default): Worker downloads files from coordinator.
```
Coordinator → (HTTP API) → Worker pulls tarball
```

**Push Mode** (`use_file_push = true`): Coordinator pushes files directly to worker.
Requires Tailscale connectivity between coordinator and sprites.
```
Coordinator → (Tailscale) → Worker HTTP receiver (port 19800)
```

Push mode is faster as it doesn't require the worker to authenticate with the coordinator.

### Worker File Receiver

When running in push mode, the worker starts an HTTP server on port 19800:
- `GET /health` - Readiness check
- `POST /upload` - Receive and extract gzipped tarball

The coordinator waits for the health check to pass before pushing files.
