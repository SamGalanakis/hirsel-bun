#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
# Logs to dev.log (gitignored)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOG_FILE="$SCRIPT_DIR/dev.log"

# Build the Rust binary first (from src-tauri directory)
echo "Building hirsel..."
cargo build --manifest-path "$SCRIPT_DIR/src-tauri/Cargo.toml" || exit 1

# Add target/debug to PATH so hirsel __acp-bridge can be found
export PATH="$SCRIPT_DIR/src-tauri/target/debug:$PATH"
echo "Added hirsel to PATH"

echo "=== Dev server started at $(date) ===" > "$LOG_FILE"
echo "Logging to: $LOG_FILE"
GDK_BACKEND=x11 npm run dev 2>&1 | tee -a "$LOG_FILE"
