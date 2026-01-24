#!/bin/bash
# Test: Local Runner with Eval
#
# Purpose: Test that eval works correctly with local runner (no Docker).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

HIRSEL_BINARY="${HIRSEL_BINARY:-$PROJECT_ROOT/src-tauri/target/debug/hirsel}"
CONFIG_DIR="${HOME}/.hirsel"
CONFIG_FILE="${CONFIG_DIR}/config.toml"
RUN_NAME="e2e-local-$(date +%s)"

echo "=========================================="
echo "Test: Local Runner with Eval"
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

# Create config with local runner (default, no docker)
echo ""
echo "Setting up local runner config..."

mkdir -p "$CONFIG_DIR"
cat > "$CONFIG_FILE" << 'EOF'
# Local runner config - no Docker
EOF

echo "  Config written to $CONFIG_FILE"

# Cleanup function
cleanup() {
    local exit_code=$?
    echo ""
    echo "Cleaning up..."

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

# Simple spec
cat > spec.md << 'EOF'
Create a file called greeting.txt with the text "Hello from local runner!"
EOF

# Simple eval
cat > eval.md << 'EOF'
Verify that greeting.txt exists and contains the expected greeting text.
EOF

echo "  Project setup complete"

# Run the test
echo ""
echo "Running local runner scenario..."
echo "  Run name: $RUN_NAME"
echo ""

"$HIRSEL_BINARY" go "$RUN_NAME" spec.md --project "$WORK_DIR" --yolo --eval eval.md &
GO_PID=$!

# Wait for completion
echo "Waiting for run to complete..."
MAX_WAIT=180
START_TIME=$(date +%s)

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))

    CURRENT_STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "UNKNOWN")

    case "$CURRENT_STATUS" in
        "DONE")
            echo "  Status: DONE - Run completed successfully!"
            break
            ;;
        "FAILED")
            echo "  Status: FAILED"
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

wait $GO_PID || true

# Check final status
echo ""
echo "Checking final status..."

FINAL_STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "UNKNOWN")
echo "  Final status: $FINAL_STATUS"

# Show run summary
echo ""
echo "Run summary:"
"$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | head -25 || true

# Check eval logs
echo ""
echo "Eval logs:"
RUN_DIR="${CONFIG_DIR}/runs/${RUN_NAME}"
if [ -d "$RUN_DIR/logs" ]; then
    ls -la "$RUN_DIR/logs/" || true
else
    echo "  No logs directory"
fi

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
        echo "TEST FAILED: unexpected status '$FINAL_STATUS'"
        echo "=========================================="
        exit 1
        ;;
esac
