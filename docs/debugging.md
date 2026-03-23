# Debugging Guide

## Logs

| Log | Location | Contents |
|-----|----------|----------|
| App log | `~/.local/share/app.hirsel/logs/Hirsel.log` | Desktop app logs, frontend forwarding, embedded backend logs |
| Runtime logs | `~/.hirsel/runtimes/<runtime>/tmp/` | Worker logs, eval logs, runtime-local scratch files |
| Profiling traces | `~/.hirsel/profiling/<timestamp>/` | Backend trace output and frontend profiling JSON |

## Common checks

```bash
# Tail the app log
tail -f ~/.local/share/app.hirsel/logs/Hirsel.log

# Show recent errors
grep -i error ~/.local/share/app.hirsel/logs/Hirsel.log | tail -40

# Inspect active worker helper processes
ps aux | grep '__worker-runtime' | grep -v grep
```

## Frontend logging

In dev mode, frontend console output is forwarded through the `log_frontend` Tauri command and lands in the main log with a `[Frontend]` prefix.

## Rust logging

```bash
# Full debug logging
RUST_LOG=debug ./dev.sh

# Narrow to Hirsel core
RUST_LOG=hirsel_lib::core=debug ./dev.sh

# Mixed filters
RUST_LOG=hirsel_lib=info,hirsel_lib::worker=debug ./dev.sh
```

## Resetting local state

```bash
# Remove local runtime workspaces only
rm -rf ~/.hirsel/runtimes

# Remove the global Hirsel database
rm -f ~/.hirsel/hirsel.db

# Full local reset
rm -rf ~/.hirsel ~/.local/share/app.hirsel
```

## Profiling

```bash
./dev.sh --profiling
```

This writes:

- `trace.json` for backend tracing
- `frontend.json` for frontend IPC timing and Web Vitals

Open `trace.json` in https://ui.perfetto.dev.
