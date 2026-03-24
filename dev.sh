#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
# Logs to dev.log (gitignored)
#
# Usage:
#   ./dev.sh              - Backend-first dev mode (serve + GUI over localhost)
#   ./dev.sh --mcp        - MCP mode for tauri-driver automation
#   ./dev.sh --profiling  - Backend-first dev mode with profiling
#   ./dev.sh --remote     - Legacy alias for the default backend-first mode
#   ./dev.sh --mcp --profiling  - MCP mode with profiling

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOG_FILE="$SCRIPT_DIR/dev.log"
TAURI_DRIVER_PORT="${TAURI_DRIVER_PORT:-4444}"
BINARY="$SCRIPT_DIR/src-tauri/target/debug/hirsel"
REMOTE_PORT="${HIRSEL_DEV_REMOTE_PORT:-8080}"
DEV_ROOT_DEFAULT="$SCRIPT_DIR/.hirsel-dev"

mcp_mode=false
profiling_mode=false
server_pid=""

cleanup() {
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

if [[ "$mcp_mode" == true ]] && [[ "$*" == *"--remote"* ]]; then
    echo "Error: --remote and --mcp are mutually exclusive" >&2
    exit 1
fi

# Profiling mode
CARGO_FEATURES=""
if [[ "$profiling_mode" == true ]]; then
    export HIRSEL_PROFILING=1
    export RUST_LOG="${RUST_LOG:-hirsel=debug}"
    CARGO_FEATURES="--features profiling"
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

if [[ "$*" == *"--mcp"* ]]; then
    echo "MCP mode: Building and launching via tauri-driver..."

    # Build the app
    echo "Building..."
    cargo build --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" $CARGO_FEATURES 2>&1 | tee -a "$LOG_FILE"
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "Build failed!"
        exit 1
    fi

    # Start daemon
    echo "Starting daemon..."
    "$BINARY" __daemon &
    sleep 1

    # Check if tauri-driver is running
    if ! curl -s "http://localhost:$TAURI_DRIVER_PORT/status" > /dev/null 2>&1; then
        echo "Starting tauri-driver on port $TAURI_DRIVER_PORT..."
        if command -v tauri-driver > /dev/null 2>&1; then
            tauri-driver --port "$TAURI_DRIVER_PORT" &
        elif [[ -x "$HOME/.cargo/bin/tauri-driver" ]]; then
            "$HOME/.cargo/bin/tauri-driver" --port "$TAURI_DRIVER_PORT" &
        else
            echo "Error: tauri-driver not found. Install with: cargo install tauri-driver"
            exit 1
        fi
        sleep 2
    fi

    echo ""
    echo "Ready for MCP automation!"
    echo "  - tauri-driver running on port $TAURI_DRIVER_PORT"
    echo "  - Binary: $BINARY"
    echo ""
    echo "Use mcp__tauri-automation__launch_app with appPath: $BINARY"
    echo "Press Ctrl+C to stop"

    # Keep script running
    wait
else
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
    echo "Building latest binary for local server..."
    cargo build --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" $CARGO_FEATURES 2>&1 | tee -a "$LOG_FILE"
    if [[ ${PIPESTATUS[0]} -ne 0 ]]; then
        echo "Build failed!"
        exit 1
    fi

    if curl -sf "http://127.0.0.1:$REMOTE_PORT/health" > /dev/null 2>&1; then
        echo "Reusing existing local hirsel serve on port $REMOTE_PORT"
    else
        echo "Starting local hirsel serve..."
        "$BINARY" serve --port "$REMOTE_PORT" >> "$LOG_FILE" 2>&1 &
        server_pid=$!

        for _ in $(seq 1 30); do
            if curl -sf "http://127.0.0.1:$REMOTE_PORT/health" > /dev/null 2>&1; then
                break
            fi

            if ! kill -0 "$server_pid" 2>/dev/null; then
                echo "hirsel serve exited early; tailing dev log:" >&2
                tail -n 50 "$LOG_FILE" >&2 || true
                exit 1
            fi

            sleep 0.2
        done

        if ! curl -sf "http://127.0.0.1:$REMOTE_PORT/health" > /dev/null 2>&1; then
            echo "hirsel serve did not become healthy on port $REMOTE_PORT" >&2
            exit 1
        fi
    fi

    echo "Running tauri dev against local hirsel serve..."

    if [[ -n "$CARGO_FEATURES" ]]; then
        GDK_BACKEND=x11 bunx tauri dev $CARGO_FEATURES 2>&1 | tee -a "$LOG_FILE"
    else
        GDK_BACKEND=x11 bun run dev 2>&1 | tee -a "$LOG_FILE"
    fi
fi
