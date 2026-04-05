# Debugging Guide

## Logs

Hirsel writes rotated tracing logs under `HIRSEL_ROOT/logs/<role>/`.

Common locations:

| Log | Location | Contents |
|-----|----------|----------|
| Server log | `~/.hirsel/logs/server/server.log.YYYY-MM-DD` | HTTP routes, project loading, queue processing, SSE/UI activity |
| GUI shell log | `~/.hirsel/logs/gui/gui.log.YYYY-MM-DD` | Thin desktop wrapper startup and local shell issues |
| Scope log | `~/.hirsel/logs/scope/scope.log.YYYY-MM-DD` | Hidden shepherd/thread scope runtime launched inside coding containers |
| Profiling traces | `~/.hirsel/profiling/<timestamp>/` | Optional Perfetto-compatible trace output |

## Common Checks

```bash
# Tail the backend server log
tail -f ~/.hirsel/logs/server/server.log.$(date +%F)

# Tail the hidden scope runtime log
tail -f ~/.hirsel/logs/scope/scope.log.$(date +%F)

# Show recent backend errors
grep -i error ~/.hirsel/logs/server/server.log.$(date +%F) | tail -40

# Check backend health
curl http://127.0.0.1:8080/health -H 'x-api-key: replace-me'

# Inspect project workspaces and thread checkouts
find ~/.hirsel/workspaces -maxdepth 3 -type d | sort

# Check whether Docker is reachable from the backend host
docker info >/dev/null && echo ok
```

## Server Startup

The standalone backend requires an API key:

```bash
HIRSEL_API_KEY=replace-me cargo run --manifest-path src-tauri/Cargo.toml --no-default-features --features server --bin hirsel-server -- --port 8080
```

Health check:

```bash
curl http://127.0.0.1:8080/health -H 'x-api-key: replace-me'
```

## Local Dev Auth

`./dev.sh` now disables HTTP API key auth by default in debug builds, even if your shell already has `HIRSEL_API_KEY` set.

Opt in explicitly when you want to test the auth flow:

```bash
HIRSEL_DEV_AUTH=1 HIRSEL_DEV_API_KEY=replace-me ./dev.sh
```

## Desktop Shell

The Tauri app is only a thin wrapper now. It stores:

- backend URL
- backend API key

Then it loads the backend-served UI. If the wrapper opens but the project UI does not, check:

1. backend URL
2. API key
3. backend health
4. server logs

## Docker + Nix Session Failures

Normal coding threads require:

- Docker available to the backend host
- a project `flake.nix` in the central checkout

Useful checks:

```bash
# Inspect the central checkout
find ~/.hirsel/workspaces -path '*/work/central' -type d

# Confirm whether the current project checkout has a flake
find ~/.hirsel/workspaces -path '*/work/central/flake.nix' -type f
```

If a project has no `flake.nix`, shepherd can still answer directly and can bootstrap the flake from the shepherd session, but thread scopes will fail until that file exists.

## Rust Logging

```bash
# Full debug logging
RUST_LOG=hirsel=debug ./dev.sh

# Focus on the backend UI / HTTP path
RUST_LOG=hirsel_lib::backend::server=debug,hirsel_lib::backend::webui=debug ./dev.sh

# Focus on shepherd queueing and container launch
RUST_LOG=hirsel_lib::backend::shepherd_runtime=debug,hirsel_lib::backend::sandbox=debug ./dev.sh
```

## Resetting Local State

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
