# Debugging Guide

## Logs

Hirsel now writes rotated tracing logs under `HIRSEL_ROOT/logs/<role>/`.

Common locations:

| Log | Location | Contents |
|-----|----------|----------|
| Server log | `~/.hirsel/logs/server/server.log.YYYY-MM-DD` | Backend HTTP routes, orchestration, worker lifecycle, SSE/UI activity |
| GUI shell log | `~/.hirsel/logs/gui/gui.log.YYYY-MM-DD` | Thin desktop wrapper startup and local shell issues |
| Worker log | `~/.hirsel/logs/worker/worker.log.YYYY-MM-DD` | Worker runtime boot, MCP/tool execution, sandbox activity |
| Daemon log | `~/.hirsel/logs/daemon/daemon.log.YYYY-MM-DD` | Background lifecycle helpers if used |
| Scribe log | `~/.hirsel/logs/scribe/scribe.log.YYYY-MM-DD` | Retained-context / artifact condensation |
| Profiling traces | `~/.hirsel/profiling/<timestamp>/` | Optional Perfetto-compatible trace output |

## Common checks

```bash
# Tail the backend server log
tail -f ~/.hirsel/logs/server/server.log.$(date +%F)

# Tail the desktop shell log
tail -f ~/.hirsel/logs/gui/gui.log.$(date +%F)

# Show recent backend errors
grep -i error ~/.hirsel/logs/server/server.log.$(date +%F) | tail -40

# Inspect active worker subprocesses
ps aux | grep '__worker-runtime' | grep -v grep
```

## Server startup

The standalone backend requires an API key:

```bash
HIRSEL_API_KEY=replace-me cargo run --manifest-path src-tauri/Cargo.toml -- serve --port 8080
```

Health check:

```bash
curl http://127.0.0.1:8080/health -H 'x-api-key: replace-me'
```

## Desktop shell

The Tauri app is only a thin wrapper now. It stores:

- backend URL
- backend API key

Then it loads the backend-served web UI. If the wrapper opens but the product UI does not, check:

1. backend URL
2. API key
3. backend health
4. server logs

## Rust logging

```bash
# Full debug logging
RUST_LOG=debug ./dev.sh

# Focus on the backend UI / HTTP path
RUST_LOG=hirsel_lib::backend::server=debug,hirsel_lib::backend::webui=debug ./dev.sh

# Focus on worker execution
RUST_LOG=hirsel_lib::worker=debug,hirsel_lib::backend::workers=debug ./dev.sh
```

## Resetting local state

```bash
# Remove local Hirsel state
rm -rf ~/.hirsel

# Remove dev-local state used by ./dev.sh
rm -rf ./.hirsel-dev
```

## Profiling

```bash
./dev.sh --profiling
```

This writes Perfetto-compatible traces under `~/.hirsel/profiling/`.
