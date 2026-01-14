#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
# Logs to dev.log (gitignored)

LOG_FILE="$(dirname "$0")/dev.log"
echo "=== Dev server started at $(date) ===" > "$LOG_FILE"
echo "Logging to: $LOG_FILE"
GDK_BACKEND=x11 npm run dev 2>&1 | tee -a "$LOG_FILE"
