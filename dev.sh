#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
# Logs to dev.log (gitignored)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOG_FILE="$SCRIPT_DIR/dev.log"

# Enable tracing output (set RUST_LOG to customize, default: info for hirsel)
export RUST_LOG="${RUST_LOG:-hirsel=info}"

# Kill any existing daemon - will be auto-started by GUI after tauri builds
pkill -9 -f "hirsel.*__daemon" 2>/dev/null && echo "Killed old daemon" || true
rm -f ~/.hirsel/hirsel.pid 2>/dev/null

# Add target/debug to PATH for hirsel commands
export PATH="$SCRIPT_DIR/src-tauri/target/debug:$PATH"

echo "=== Dev server started at $(date) ===" > "$LOG_FILE"
echo "Logging to: $LOG_FILE"
echo "Running tauri dev (daemon will auto-start on first use)..."

GDK_BACKEND=x11 npm run dev 2>&1 | tee -a "$LOG_FILE"
