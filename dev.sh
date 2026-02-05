#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
# Logs to dev.log (gitignored)
#
# Usage:
#   ./dev.sh         - Normal dev mode with hot reload
#   ./dev.sh --mcp   - MCP mode for tauri-driver automation

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOG_FILE="$SCRIPT_DIR/dev.log"
TAURI_DRIVER_PORT="${TAURI_DRIVER_PORT:-4444}"
BINARY="$SCRIPT_DIR/src-tauri/target/debug/hirsel"

# Enable tracing output (set RUST_LOG to customize, default: info for hirsel)
export RUST_LOG="${RUST_LOG:-hirsel=info}"

# Kill any existing daemon - will be auto-started by GUI after tauri builds
pkill -9 -f "hirsel.*__daemon" 2>/dev/null && echo "Killed old daemon" || true
rm -f ~/.hirsel/hirsel.pid 2>/dev/null

# Add target/debug to PATH for hirsel commands
export PATH="$SCRIPT_DIR/src-tauri/target/debug:$PATH"

echo "=== Dev server started at $(date) ===" > "$LOG_FILE"
echo "Logging to: $LOG_FILE"

if [[ "$1" == "--mcp" ]]; then
    echo "MCP mode: Building and launching via tauri-driver..."

    # Build the app
    echo "Building..."
    cargo build --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" 2>&1 | tee -a "$LOG_FILE"
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
    echo "Running tauri dev (daemon will auto-start on first use)..."
    GDK_BACKEND=x11 bun run dev 2>&1 | tee -a "$LOG_FILE"
fi
