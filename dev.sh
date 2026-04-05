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

load_dotenv_file() {
    local file="$1"
    [[ -f "$file" ]] || return 0

    while IFS= read -r line || [[ -n "$line" ]]; do
        [[ "$line" =~ ^[[:space:]]*$ ]] && continue
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        if [[ "$line" =~ ^[[:space:]]*(export[[:space:]]+)?([A-Za-z_][A-Za-z0-9_]*)=(.*)$ ]]; then
            local key="${BASH_REMATCH[2]}"
            local value="${BASH_REMATCH[3]}"
            value="${value%$'\r'}"
            if [[ ${#value} -ge 2 ]]; then
                if [[ "$value" == \"*\" && "$value" == *\" ]]; then
                    value="${value:1:${#value}-2}"
                elif [[ "$value" == \'*\' && "$value" == *\' ]]; then
                    value="${value:1:${#value}-2}"
                fi
            fi
            if [[ -z "${!key+x}" ]]; then
                export "$key=$value"
            fi
        fi
    done < "$file"
}

load_dotenv_file "$SCRIPT_DIR/.env"
load_dotenv_file "$SCRIPT_DIR/.env.local"

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

build_worker_image() {
    local image="hirsel-worker:local"
    local expected_label
    local cargo_profile

    expected_label="$("$SERVER_BIN" --worker-image-build-label)"
    cargo_profile="$("$SERVER_BIN" --worker-image-cargo-profile)"
    echo "Building fresh worker image $image ($cargo_profile)..."
    docker build \
        -f "$SCRIPT_DIR/deploy/worker.Dockerfile" \
        --build-arg "HIRSEL_WORKER_BUILD_LABEL=$expected_label" \
        --build-arg "HIRSEL_WORKER_CARGO_PROFILE=$cargo_profile" \
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
        echo "Shell assets build failed!"
        exit 1
    fi

    echo "Building webui SPA..."
    (cd "$SCRIPT_DIR/webui" && bun run build) 2>&1 | tee -a "$LOG_FILE"
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "WebUI build failed!"
        exit 1
    fi
}

prepare_local_backend() {
    local dev_auth="${HIRSEL_DEV_AUTH:-0}"
    if [[ "$dev_auth" == "1" || "$dev_auth" == "true" || "$dev_auth" == "yes" || "$dev_auth" == "on" ]]; then
        export HIRSEL_API_KEY="${HIRSEL_API_KEY:-${HIRSEL_DEV_API_KEY:-}}"
    else
        unset HIRSEL_API_KEY
    fi
    export HIRSEL_WORKER_CARGO_PROFILE="${HIRSEL_WORKER_CARGO_PROFILE:-dev}"
    export HIRSEL_ROOT="${HIRSEL_ROOT:-$DEV_ROOT_DEFAULT}"

    mkdir -p "$HIRSEL_ROOT"
    cat > "$HIRSEL_ROOT/config.toml" <<EOF
[backend]
url = "http://127.0.0.1:$REMOTE_PORT"
EOF

    if [[ -n "$HIRSEL_API_KEY" ]]; then
        cat >> "$HIRSEL_ROOT/config.toml" <<EOF
api_key = "$HIRSEL_API_KEY"
EOF
    fi

    echo "Backend-first dev mode"
    echo "  Root: $HIRSEL_ROOT"
    echo "  Backend URL: http://127.0.0.1:$REMOTE_PORT"
    if [[ -n "$HIRSEL_API_KEY" ]]; then
        echo "  API key: $HIRSEL_API_KEY"
    else
        echo "  API key: disabled (set HIRSEL_DEV_AUTH=1 to enable)"
    fi
    echo ""
}

remove_container_if_present() {
    local name="$1"
    if [[ -z "$name" ]]; then
        return
    fi
    docker rm -f "$name" >/dev/null 2>&1 || true
}

kill_pid_list() {
    local signal="$1"
    shift
    if [[ $# -eq 0 ]]; then
        return
    fi

    kill "-$signal" "$@" >/dev/null 2>&1 || true
}

wait_for_pids_exit() {
    local timeout_secs="$1"
    shift
    if [[ $# -eq 0 ]]; then
        return 0
    fi

    local deadline=$((SECONDS + timeout_secs))
    while (( SECONDS < deadline )); do
        local alive=0
        local pid
        for pid in "$@"; do
            if kill -0 "$pid" >/dev/null 2>&1; then
                alive=1
                break
            fi
        done
        if (( alive == 0 )); then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

kill_matching_processes() {
    local label="$1"
    local pattern="$2"
    local -a pids=()
    mapfile -t pids < <(
        pgrep -u "$(id -u)" -f -- "$pattern" 2>/dev/null \
            | awk -v self="$$" '$0 != self' \
            || true
    )
    if [[ ${#pids[@]} -eq 0 ]]; then
        return
    fi

    echo "  Killing $label: ${pids[*]}"
    kill_pid_list TERM "${pids[@]}"
    if ! wait_for_pids_exit 3 "${pids[@]}"; then
        kill_pid_list KILL "${pids[@]}"
        wait_for_pids_exit 1 "${pids[@]}" || true
    fi
}

list_tcp_port_pids() {
    local port="$1"

    if command -v lsof >/dev/null 2>&1; then
        lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true
        return
    fi

    if command -v fuser >/dev/null 2>&1; then
        fuser -n tcp "$port" 2>/dev/null | tr ' ' '\n' | sed '/^$/d' || true
        return
    fi

    if command -v ss >/dev/null 2>&1; then
        ss -ltnp "( sport = :$port )" 2>/dev/null \
            | sed -n 's/.*pid=\([0-9][0-9]*\).*/\1/p' \
            | sort -u || true
    fi
}

kill_processes_on_port() {
    local port="$1"
    local label="$2"
    local -a pids=()
    mapfile -t pids < <(list_tcp_port_pids "$port")
    if [[ ${#pids[@]} -eq 0 ]]; then
        return
    fi

    echo "  Killing listeners on tcp/$port for $label: ${pids[*]}"
    kill_pid_list TERM "${pids[@]}"
    if ! wait_for_pids_exit 3 "${pids[@]}"; then
        kill_pid_list KILL "${pids[@]}"
        wait_for_pids_exit 1 "${pids[@]}" || true
    fi
}

remove_hirsel_dev_containers() {
    if ! command -v docker >/dev/null 2>&1; then
        return
    fi

    local -a containers=()
    mapfile -t containers < <(
        docker ps -aq --format '{{.Names}}' 2>/dev/null | awk '/^hirsel-/ { print $0 }'
    )
    if [[ ${#containers[@]} -eq 0 ]]; then
        return
    fi

    echo "  Removing Hirsel containers: ${containers[*]}"
    local name
    for name in "${containers[@]}"; do
        remove_container_if_present "$name"
    done
}

stop_existing_local_runtime() {
    echo "Stopping existing local Hirsel runtime..."

    kill_matching_processes "other dev launchers" '(^|[ /])dev\.sh($| )'
    kill_matching_processes "debug desktop" "$DESKTOP_BIN"
    kill_matching_processes "debug server" "$SERVER_BIN"
    kill_matching_processes "cargo hirsel builds" 'cargo build.*hirsel-(server|desktop|worker)'
    kill_matching_processes "worker image rebuilds" 'docker build.*worker\.Dockerfile'
    kill_matching_processes "desktop processes" '(^|/)(hirsel-desktop)( |$)'
    kill_matching_processes "server processes" '(^|/)(hirsel-server)( |$)'
    kill_matching_processes "cargo-run server wrappers" 'cargo.*hirsel-server'

    kill_processes_on_port "$REMOTE_PORT" "dev backend"
    if [[ "$mcp_mode" == true ]]; then
        kill_processes_on_port "$TAURI_DRIVER_PORT" "tauri-driver"
    fi

    remove_hirsel_dev_containers

    rm -rf "$HIRSEL_ROOT/agent-sessions"
    rm -rf "$HIRSEL_ROOT/logs"
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
stop_existing_local_runtime
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
