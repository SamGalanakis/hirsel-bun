#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
# Logs to dev.log (gitignored)
#
# Usage:
#   ./dev.sh              - Fresh backend-first dev mode (serve + GUI over localhost)
#   ./dev.sh --mcp        - Fresh automation mode (serve + tauri-driver, launch app separately)
#   ./dev.sh --profiling  - Fresh backend-first dev mode with profiling
#   ./dev.sh --remote     - Legacy alias for the default backend-first mode
#   ./dev.sh --mcp --profiling  - Fresh automation mode with profiling

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOG_FILE="$SCRIPT_DIR/dev.log"
TAURI_DRIVER_PORT="${TAURI_DRIVER_PORT:-4444}"
SERVER_BIN="$SCRIPT_DIR/src-tauri/target/debug/hirsel-server"
DESKTOP_BIN="$SCRIPT_DIR/src-tauri/target/debug/hirsel-desktop"
REMOTE_PORT="${HIRSEL_DEV_REMOTE_PORT:-8080}"
DEV_ROOT_DEFAULT="$SCRIPT_DIR/.hirsel-dev"

mcp_mode=false
profiling_mode=false
server_pid=""
tauri_driver_pid=""

cleanup() {
    if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
        kill "$server_pid" 2>/dev/null || true
    fi

    if [[ -n "$tauri_driver_pid" ]] && kill -0 "$tauri_driver_pid" 2>/dev/null; then
        kill "$tauri_driver_pid" 2>/dev/null || true
    fi

    if [[ "$profiling_mode" == true ]]; then
        echo ""
        echo "Profiling data saved to: $SESSION_DIR"
        ls -lh "$SESSION_DIR" 2>/dev/null || true
        echo ""
        echo "Run: scripts/profiling-report.py $SESSION_DIR"
    fi
}

trap cleanup EXIT

# Validate arguments
for arg in "$@"; do
    case "$arg" in
        --mcp) mcp_mode=true ;;
        --profiling) profiling_mode=true ;;
        --remote) ;;
        *) echo "Error: Unknown flag '$arg'. Usage: ./dev.sh [--mcp] [--profiling]"; exit 1 ;;
    esac
done

# Profiling mode
if [[ "$profiling_mode" == true ]]; then
    export HIRSEL_PROFILING=1
    export RUST_LOG="${RUST_LOG:-hirsel=debug}"
    SESSION_DIR="$SCRIPT_DIR/.profiling/$(date +%Y-%m-%dT%H-%M-%S)"
    export HIRSEL_PROFILING_DIR="$SESSION_DIR"
    mkdir -p "$SESSION_DIR"
    echo ""
    echo "Profiling enabled: $SESSION_DIR"
    echo "  Backend trace: trace.json  (open in ui.perfetto.dev)"
    echo "  Frontend IPC:  frontend.json"
    echo ""
else
    export RUST_LOG="${RUST_LOG:-hirsel=info}"
fi

# Add target/debug to PATH for hirsel commands
export PATH="$SCRIPT_DIR/src-tauri/target/debug:$PATH"

echo "=== Dev server started at $(date) ===" > "$LOG_FILE"
echo "Logging to: $LOG_FILE"

build_target() {
    local target="$1"
    local features="$2"
    local manifest="$SCRIPT_DIR/src-tauri/Cargo.toml"
    local cmd=(cargo build --manifest-path "$manifest" --bin "$target")

    if [[ -n "$features" ]]; then
        cmd+=(--features "$features")
    fi

    echo "Building $target..."
    "${cmd[@]}" 2>&1 | tee -a "$LOG_FILE"
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "Build failed for $target!"
        exit 1
    fi
}

build_binaries() {
    local server_features="server"
    local desktop_features="gui"

    if [[ "$profiling_mode" == true ]]; then
        server_features="server,profiling"
        desktop_features="gui,profiling"
    fi

    build_target "hirsel-server" "$server_features"
    build_target "hirsel-desktop" "$desktop_features"
}

worker_image_build_label() {
    local version
    local sha

    version=$(grep '^version = "' "$SCRIPT_DIR/src-tauri/Cargo.toml" | head -n1 | sed -E 's/.*"([^"]+)".*/\1/')
    sha=$(git -C "$SCRIPT_DIR" rev-parse --short HEAD 2>/dev/null || echo "unknown")
    if ! git -C "$SCRIPT_DIR" diff --quiet --ignore-submodules HEAD -- 2>/dev/null; then
        sha="${sha}-dirty"
    fi

    printf "%s-%s" "$version" "$sha"
}

build_worker_image() {
    local image="hirsel-worker:local"
    local expected_label

    expected_label="$(worker_image_build_label)"
    echo "Building fresh worker image $image..."
    docker build \
        -f "$SCRIPT_DIR/deploy/worker.Dockerfile" \
        --build-arg "HIRSEL_WORKER_BUILD_LABEL=$expected_label" \
        -t "$image" \
        "$SCRIPT_DIR" 2>&1 | tee -a "$LOG_FILE"
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "Worker image build failed!"
        exit 1
    fi
}

build_shell_assets() {
    echo "Building local shell assets..."
    bun run vite:build 2>&1 | tee -a "$LOG_FILE"
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "Frontend build failed!"
        exit 1
    fi
}

prepare_local_backend() {
    export HIRSEL_API_KEY="${HIRSEL_API_KEY:-${HIRSEL_DEV_API_KEY:-dev-test-key}}"
    export HIRSEL_ROOT="${HIRSEL_ROOT:-$DEV_ROOT_DEFAULT}"

    mkdir -p "$HIRSEL_ROOT"
    cat > "$HIRSEL_ROOT/config.toml" <<EOF
[backend]
url = "http://127.0.0.1:$REMOTE_PORT"
api_key = "$HIRSEL_API_KEY"
EOF

    echo "Backend-first dev mode"
    echo "  Root: $HIRSEL_ROOT"
    echo "  Backend URL: http://127.0.0.1:$REMOTE_PORT"
    echo "  API key: $HIRSEL_API_KEY"
    echo ""
}

remove_container_if_present() {
    local name="$1"
    if [[ -z "$name" ]]; then
        return
    fi
    docker rm -f "$name" >/dev/null 2>&1 || true
}

stop_existing_local_runtime() {
    echo "Stopping existing local Hirsel runtime..."

    pkill -f "$DESKTOP_BIN" 2>/dev/null || true
    pkill -f "$SERVER_BIN" 2>/dev/null || true

    if command -v sqlite3 >/dev/null 2>&1 && [[ -f "$HIRSEL_ROOT/hirsel.db" ]]; then
        while IFS= read -r container_name; do
            remove_container_if_present "$container_name"
        done < <(
            sqlite3 "$HIRSEL_ROOT/hirsel.db" \
                "select container_name from shepherd_sessions where container_name is not null and trim(container_name) <> '';"
        )

        sqlite3 "$HIRSEL_ROOT/hirsel.db" <<'SQL' >/dev/null 2>&1 || true
DELETE FROM shepherd_sessions;
DELETE FROM project_runtime_preparations;
SQL
    fi

    rm -rf "$HIRSEL_ROOT/agent-sessions"
    rm -f "$HIRSEL_ROOT/server/control.sock"
}

start_local_backend() {
    echo "Starting local hirsel-server..."
    "$SERVER_BIN" --port "$REMOTE_PORT" >> "$LOG_FILE" 2>&1 &
    server_pid=$!

    for _ in $(seq 1 30); do
        if curl -sf "http://127.0.0.1:$REMOTE_PORT/health" > /dev/null 2>&1; then
            break
        fi

        if ! kill -0 "$server_pid" 2>/dev/null; then
            echo "hirsel-server exited early; tailing dev log:" >&2
            tail -n 50 "$LOG_FILE" >&2 || true
            exit 1
        fi

        sleep 0.2
    done

    if ! curl -sf "http://127.0.0.1:$REMOTE_PORT/health" > /dev/null 2>&1; then
        echo "hirsel-server did not become healthy on port $REMOTE_PORT" >&2
        exit 1
    fi
}

ensure_tauri_driver() {
    if ! curl -s "http://localhost:$TAURI_DRIVER_PORT/status" > /dev/null 2>&1; then
        echo "Starting tauri-driver on port $TAURI_DRIVER_PORT..."
        if command -v tauri-driver > /dev/null 2>&1; then
            tauri-driver --port "$TAURI_DRIVER_PORT" &
            tauri_driver_pid=$!
        elif [[ -x "$HOME/.cargo/bin/tauri-driver" ]]; then
            "$HOME/.cargo/bin/tauri-driver" --port "$TAURI_DRIVER_PORT" &
            tauri_driver_pid=$!
        else
            echo "Error: tauri-driver not found. Install with: cargo install tauri-driver"
            exit 1
        fi
        sleep 2
    fi
}

prepare_local_backend
build_binaries
build_worker_image
build_shell_assets
stop_existing_local_runtime
start_local_backend

if [[ "$mcp_mode" == true ]]; then
    echo "Automation mode: local backend plus tauri-driver..."
    ensure_tauri_driver

    echo ""
    echo "Ready for MCP automation!"
    echo "  - backend: http://127.0.0.1:$REMOTE_PORT"
    echo "  - tauri-driver running on port $TAURI_DRIVER_PORT"
    echo "  - Desktop binary: $DESKTOP_BIN"
    echo "  - Server binary: $SERVER_BIN"
    echo ""
    echo "Use mcp__tauri-automation__launch_app with appPath: $DESKTOP_BIN"
    echo "Press Ctrl+C to stop"

    wait
else
    echo "Running hirsel-desktop against local hirsel-server..."
    GDK_BACKEND=x11 "$DESKTOP_BIN" 2>&1 | tee -a "$LOG_FILE"
fi
