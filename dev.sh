#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
# Logs to dev.log (gitignored)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOG_FILE="$SCRIPT_DIR/dev.log"

# Enable tracing output (set RUST_LOG to customize, default: info for hirsel)
export RUST_LOG="${RUST_LOG:-hirsel=info}"

# Build the Rust binary with dev features (tracing)
echo "Building hirsel (with dev features)..."
cargo build --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" --features dev || exit 1

# Add target/debug to PATH so hirsel __acp-bridge can be found
export PATH="$SCRIPT_DIR/src-tauri/target/debug:$PATH"
echo "Added hirsel to PATH"

# Always restart daemon with fresh code (kill old, start new)
hirsel daemon stop >/dev/null 2>&1
hirsel daemon start

echo "=== Dev server started at $(date) ===" > "$LOG_FILE"
echo "Logging to: $LOG_FILE (RUST_LOG=$RUST_LOG)"
echo "Running tauri dev (vite + app with hot reload)..."

GDK_BACKEND=x11 npm run dev 2>&1 | tee -a "$LOG_FILE"
