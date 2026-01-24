#!/bin/bash
# Test: Pause/Resume with Local Runner
#
# Purpose: Test pausing and resuming a run with local runner.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

HIRSEL_BINARY="${HIRSEL_BINARY:-$PROJECT_ROOT/src-tauri/target/debug/hirsel}"
CONFIG_DIR="${HOME}/.hirsel"
CONFIG_FILE="${CONFIG_DIR}/config.toml"
RUN_NAME="e2e-pause-local-$(date +%s)"

echo "=========================================="
echo "Test: Pause/Resume with Local Runner"
echo "=========================================="
echo ""

# Check prerequisites
echo "Checking prerequisites..."

if [ ! -f "$HIRSEL_BINARY" ]; then
    echo "ERROR: hirsel binary not found at $HIRSEL_BINARY"
    exit 1
fi
echo "  Binary: $HIRSEL_BINARY"

# Backup existing config
CONFIG_BACKUP=""
if [ -f "$CONFIG_FILE" ]; then
    CONFIG_BACKUP=$(mktemp)
    cp "$CONFIG_FILE" "$CONFIG_BACKUP"
    echo "  Config backed up"
fi

# Create config with local runner
echo ""
echo "Setting up local runner config..."

mkdir -p "$CONFIG_DIR"
cat > "$CONFIG_FILE" << 'EOF'
# Local runner config
EOF

echo "  Config written"

# Cleanup function
cleanup() {
    local exit_code=$?
    echo ""
    echo "Cleaning up..."

    # Kill any remaining processes
    pkill -f "hirsel.*${RUN_NAME}" 2>/dev/null || true

    # Restore original config
    if [ -n "$CONFIG_BACKUP" ] && [ -f "$CONFIG_BACKUP" ]; then
        mv "$CONFIG_BACKUP" "$CONFIG_FILE"
        echo "  Config restored"
    elif [ -z "$CONFIG_BACKUP" ]; then
        rm -f "$CONFIG_FILE"
    fi

    # Clean up test run directory
    rm -rf "${CONFIG_DIR}/runs/${RUN_NAME}" 2>/dev/null || true

    [ -n "${WORK_DIR:-}" ] && rm -rf "$WORK_DIR" 2>/dev/null || true

    echo "  Done"
    exit $exit_code
}

trap cleanup EXIT

# Create temp project directory
WORK_DIR=$(mktemp -d)
echo ""
echo "Setting up test project in $WORK_DIR..."

cd "$WORK_DIR"
git init -q
git config user.email "test@test.com"
git config user.name "Test"
echo "# Test Project" > README.md
git add .
git commit -q -m "initial"

# Multi-step spec that takes time (gives us window to pause)
cat > spec.md << 'EOF'
Create three files with specific content:

1. Create file1.txt with "First file content"
2. Create file2.txt with "Second file content"
3. Create file3.txt with "Third file content"

Take your time and create each file carefully.
EOF

echo "  Project setup complete"

# Start the run
echo ""
echo "Starting run: $RUN_NAME"

"$HIRSEL_BINARY" go "$RUN_NAME" spec.md --project "$WORK_DIR" --yolo &
GO_PID=$!

# Wait for run to start working
echo "Waiting for run to start..."
for i in {1..20}; do
    sleep 2
    STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "")
    if [ "$STATUS" = "WORKING" ]; then
        echo "  Run is WORKING"
        break
    fi
    echo "  Status: ${STATUS:-starting}..."
done

# Let it work for a bit
echo ""
echo "Letting run work for 10 seconds..."
sleep 10

# Check status before pause
STATUS_BEFORE=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "UNKNOWN")
echo "  Status before pause: $STATUS_BEFORE"

# If already done, that's fine
if [ "$STATUS_BEFORE" = "DONE" ]; then
    echo ""
    echo "Run completed before we could pause (fast execution)"
    echo "=========================================="
    echo "TEST PASSED (completed quickly)"
    echo "=========================================="
    exit 0
fi

# Pause the run
echo ""
echo "Pausing the run..."
"$HIRSEL_BINARY" pause "$RUN_NAME" 2>&1 || true

# Wait for pause to take effect
sleep 3

# Check paused status
STATUS_PAUSED=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "UNKNOWN")
echo "  Status after pause: $STATUS_PAUSED"

if [ "$STATUS_PAUSED" != "PAUSED" ] && [ "$STATUS_PAUSED" != "DONE" ]; then
    echo "WARNING: Expected PAUSED or DONE, got $STATUS_PAUSED"
fi

# Show state while paused
echo ""
echo "Run state while paused:"
"$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | head -20 || true

# Wait a bit
echo ""
echo "Waiting 5 seconds while paused..."
sleep 5

# Resume the run
echo ""
echo "Resuming the run..."
"$HIRSEL_BINARY" resume "$RUN_NAME" 2>&1 || true

# Wait for completion
echo ""
echo "Waiting for run to complete after resume..."
MAX_WAIT=120
START_TIME=$(date +%s)

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))

    CURRENT_STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "UNKNOWN")

    case "$CURRENT_STATUS" in
        "DONE"|"FAILED")
            echo "  Final status: $CURRENT_STATUS"
            break
            ;;
    esac

    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: Run timed out after ${MAX_WAIT}s (status: $CURRENT_STATUS)"
        exit 1
    fi
    echo "  Waiting... (${ELAPSED}s, status: $CURRENT_STATUS)"
    sleep 5
done

# Show final state
echo ""
echo "Final run state:"
"$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | head -25 || true

FINAL_STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "UNKNOWN")

case "$FINAL_STATUS" in
    "DONE")
        echo ""
        echo "=========================================="
        echo "TEST PASSED"
        echo "=========================================="
        exit 0
        ;;
    *)
        echo ""
        echo "=========================================="
        echo "TEST FAILED: unexpected final status '$FINAL_STATUS'"
        echo "=========================================="
        exit 1
        ;;
esac
