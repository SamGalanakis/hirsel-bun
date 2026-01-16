# Hirsel Development Guidelines

## UI Components

Use [Basecoat UI](https://basecoatui.com/components/) components wherever one is available. This includes:

- **Buttons**: Use `.btn`, `.btn-ghost`, `.btn-destructive` classes
- **Inputs**: Use `.input` class for text inputs, number inputs
- **Select**: Use `.select` class for native select dropdowns
- **Slider**: Use `<input type="range" class="input">` with proper JS initialization for `--slider-value`
- **Toast**: Use basecoat toaster via `basecoat:toast` events
- **Tooltips**: Use `data-tooltip` and `data-side` attributes
- **Modals/Dialogs**: Follow basecoat dialog patterns
- **Forms**: Use `.label`, `.form` classes

Check https://basecoatui.com/components/ for the full list and proper markup.

## Icons

Use [Lucide Icons](https://lucide.dev/icons/) for all iconography. Search for icons at `https://lucide.dev/icons/?search=<term>`.

Icons are rendered using the `data-lucide` attribute:
```html
<i data-lucide="file-text" class="w-4 h-4"></i>
<i data-lucide="upload" class="w-5 h-5 text-amber-500"></i>
```

Lucide is initialized globally and icons are auto-rendered. After dynamically adding icons, call `lucide.createIcons()` to render them.

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

When spawning detached subprocesses that run AI agents (workers, eval, compaction, improve), we use `process_group(0)` to create a new process group. This allows killing all descendant processes when the subprocess exits.

**IMPORTANT:** Any subprocess spawned with `process_group(0)` MUST call the cleanup function before exiting:

```rust
// At the end of the subprocess entry function:
crate::core::process::cleanup_process_group("my-subprocess-name");
```

This ensures grandchild processes (like `node claude-code-acp` spawned by `claude`) are properly terminated. Without this, orphaned processes will accumulate.

**Current subprocesses with cleanup:**
- Worker (`acp_client.rs`)
- Eval (`eval.rs`)
- Compaction (`compact.rs`)
- Improve (`improve.rs`)

See `src/core/process.rs` for the implementation.
