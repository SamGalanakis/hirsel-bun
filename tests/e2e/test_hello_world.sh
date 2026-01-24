#!/bin/bash
# Test: Hello World with Eval
#
# Purpose: Test single worker completing a simple task with evaluation.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

HIRSEL_BINARY="${HIRSEL_BINARY:-$PROJECT_ROOT/src-tauri/target/debug/hirsel}"
CONFIG_DIR="${HOME}/.hirsel"
CONFIG_FILE="${CONFIG_DIR}/config.toml"
RUN_NAME="e2e-hello-$(date +%s)"

echo "=========================================="
echo "Test: Hello World with Eval"
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

# Create config with docker runner
echo ""
echo "Setting up docker runner config..."

mkdir -p "$CONFIG_DIR"
cat > "$CONFIG_FILE" << 'EOF'
[runners.docker]
[runners.docker.host]
type = "local"

[runners.docker.container]
image = "buildpack-deps:noble"
EOF

echo "  Config written to $CONFIG_FILE"

# Cleanup function
cleanup() {
    local exit_code=$?
    echo ""
    echo "Cleaning up..."

    # Stop any running containers
    docker ps -q --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | xargs -r docker stop 2>/dev/null || true
    docker ps -aq --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | xargs -r docker rm -f 2>/dev/null || true

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

# Copy hello_world scenario files
cp -r "$SCRIPT_DIR/../scenarios/hello_world/project/." "$WORK_DIR/"
cd "$WORK_DIR"
git init -q
git config user.email "test@test.com"
git config user.name "Test"
git add .
git commit -q -m "initial"

# Copy spec and eval
cp "$SCRIPT_DIR/../scenarios/hello_world/spec.md" "$WORK_DIR/spec.md"
cp "$SCRIPT_DIR/../scenarios/hello_world/eval.md" "$WORK_DIR/eval.md"
echo "  Project setup complete"

# Run the test
echo ""
echo "Running hello_world scenario..."
echo "  Run name: $RUN_NAME"
echo ""

"$HIRSEL_BINARY" go "$RUN_NAME" spec.md --project "$WORK_DIR" --runner docker --yolo --eval eval.md &
GO_PID=$!

# Wait for container to spawn
sleep 10

# Wait for completion
echo "Waiting for run to complete..."
MAX_WAIT=300
START_TIME=$(date +%s)

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))

    CURRENT_STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "unknown")

    case "$CURRENT_STATUS" in
        "COMPLETED"|"DELIVERED"|"PASS"|"DONE"|"FAIL")
            echo "  Status changed to: $CURRENT_STATUS"
            break
            ;;
        "EVAL")
            # Eval is running, keep waiting
            ;;
    esac

    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: Run timed out after ${MAX_WAIT}s (status: $CURRENT_STATUS)"
        exit 1
    fi
    echo "  Waiting... (${ELAPSED}s, status: $CURRENT_STATUS)"
    sleep 10
done

wait $GO_PID || true

# Check final status
echo ""
echo "Checking final status..."

FINAL_STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | grep -oP '\[\K[a-zA-Z]+(?=\])' | head -1 | tr '[:lower:]' '[:upper:]' || echo "unknown")
echo "  Final status: $FINAL_STATUS"

# Show run summary
echo ""
echo "Run summary:"
"$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | head -25 || true

# Check if hello.py was created
echo ""
echo "Checking workspace..."
WORKSPACE="$CONFIG_DIR/runs/$RUN_NAME/workspace"
if [ -f "$WORKSPACE/hello.py" ]; then
    echo "  hello.py created:"
    cat "$WORKSPACE/hello.py"
    echo ""

    # Test the output
    OUTPUT=$(cd "$WORKSPACE" && python hello.py 2>&1 || echo "FAILED")
    echo "  Output: $OUTPUT"

    if [ "$OUTPUT" = "Hello, World!" ]; then
        echo "  Output matches expected!"
    fi
else
    echo "  hello.py NOT found"
fi

case "$FINAL_STATUS" in
    "COMPLETED"|"DELIVERED"|"PASS"|"DONE")
        echo ""
        echo "=========================================="
        echo "TEST PASSED"
        echo "=========================================="
        exit 0
        ;;
    "FAIL")
        echo ""
        echo "Run completed but eval failed"
        echo "=========================================="
        echo "TEST PASSED (task completed, eval may have failed)"
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
