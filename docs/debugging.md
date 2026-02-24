# Debugging Guide

## Log Locations

| Log | Location | Contents |
|-----|----------|----------|
| **Main log** | `~/.local/share/app.hirsel/logs/Hirsel.log` | Backend + frontend logs |
| **Daemon log** | (same file) | Daemon operations |
| **Run logs** | `~/.hirsel/runs/<run>/logs/` | Per-run worker/eval logs |

## Viewing Logs

```bash
# Real-time log viewing
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log

# Filter for errors only
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log | grep -i error

# Filter for frontend logs
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log | grep "\[Frontend\]"

# Filter by component
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log | grep "specflow\|board"
```

## Frontend Logging

Frontend `console.log/warn/error` calls are automatically captured and sent to the backend log file in **dev mode** (`./dev.sh`).

**How it works:**
- `src/lib/dev-logger.ts` intercepts console methods
- Sends to backend via `log_frontend` Tauri command
- Logged with `[Frontend]` prefix

**Log levels in backend log:**
```
[Frontend] {message}           # console.log, console.info
[WARN] [Frontend] {message}    # console.warn
[ERROR] [Frontend] {message}   # console.error
```

**Note:** Frontend logging only works when running via `./dev.sh` (dev mode). Production builds don't capture frontend console output.

## Backend Logging

Backend uses `tracing` crate with `tauri_plugin_log`.

**Log from Rust code:**
```rust
tracing::info!("Message");
tracing::warn!("Warning: {}", details);
tracing::error!("Error: {:?}", error);
tracing::debug!("Debug info");  // Only with RUST_LOG set
```

**Environment variables:**
```bash
# Enable verbose logging
RUST_LOG=debug ./dev.sh

# Filter by module
RUST_LOG=hirsel_lib::core=debug ./dev.sh

# Multiple filters
RUST_LOG=hirsel_lib=info,hirsel_lib::daemon=debug ./dev.sh
```

## Browser DevTools

Open browser developer tools in the Tauri webview:

- **Linux/Windows:** `Ctrl+Shift+I` or `F12`
- **macOS:** `Cmd+Option+I`

Useful DevTools tabs:
- **Console:** JavaScript errors, frontend logs
- **Network:** API calls, WebSocket connections
- **Application > Local Storage:** Persisted state

## Tauri Command Errors
Look for `[ERROR]` in backend log:
```bash
grep "\[ERROR\]" ~/.local/share/app.hirsel/logs/Hirsel.log | tail -20
```

### Daemon Connection Issues
```bash
# Check if daemon is running
ps aux | grep "hirsel __daemon"

# Check daemon PID file
cat ~/.hirsel/hirsel.pid

# Check daemon port (default 19700)
lsof -i :19700
```

## Reset & Clean State

```bash
# Reset runs only (keeps projects, config)
rm -rf ~/.hirsel/runs

# Reset everything (full clean slate)
rm -rf ~/.hirsel ~/.local/share/app.hirsel

# Reset just the database
rm ~/.hirsel/hirsel.db

# Kill daemon if stuck
pkill -f "hirsel __daemon"
```

## Debug Panel

The GUI has a built-in debug panel (if enabled):
- Shows process counts (hirsel worker helpers, node)
- Can kill orphaned worker helper processes
- Access via settings or keyboard shortcut

## MCP Debugging (Dev Builds)

In debug builds, an MCP socket server runs at `/tmp/hirsel-mcp.sock` for AI agent debugging.

## Adding Debug Logging

### Frontend (TypeScript)
```typescript
// These go to backend log in dev mode
console.log('[ComponentName] Action:', data);
console.error('[ComponentName] Error:', error);
```

### Backend (Rust)
```rust
tracing::info!("[module] Action: {:?}", data);
tracing::error!("[module] Error: {:?}", error);
```

## Useful Log Patterns

```bash
# All errors in last 100 lines
tail -100 ~/.local/share/app.hirsel/logs/Hirsel.log | grep -i error

# Frontend errors only
grep "\[Frontend\].*ERROR\|ERROR.*\[Frontend\]" ~/.local/share/app.hirsel/logs/Hirsel.log

# Daemon activity
grep "daemon" ~/.local/share/app.hirsel/logs/Hirsel.log | tail -50

# Project/board operations
grep -i "project\|island\|board" ~/.local/share/app.hirsel/logs/Hirsel.log | tail -50

# Recent activity (last 5 minutes)
# Note: Adjust timestamp format as needed
grep "$(date +%Y-%m-%d)" ~/.local/share/app.hirsel/logs/Hirsel.log | tail -100
```

## Performance Profiling

Capture backend execution traces and frontend IPC timing:

```bash
./dev.sh --profiling
```

Use the app, then close it. Profiling data is saved to `~/.hirsel/profiling/<timestamp>/`:
- `trace.json` - Backend execution trace (Chrome Trace Format)
- `frontend.json` - Frontend IPC call timing and Web Vitals

**Viewing backend traces:**
1. Open https://ui.perfetto.dev
2. Load `trace.json`

**Text report (both backend + frontend):**
```bash
scripts/profiling-report.py                              # most recent session
scripts/profiling-report.py ~/.hirsel/profiling/<dir>    # specific session
```

**Environment variables** (set automatically by `dev.sh --profiling`):
- `HIRSEL_PROFILING=1` - Enables profiling
- `HIRSEL_PROFILING_DIR=<path>` - Session output directory
