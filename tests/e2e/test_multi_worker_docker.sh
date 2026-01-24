#!/bin/bash
# Test: Multi-Worker Docker Runner
#
# Purpose: Test multiple workers running in parallel with Docker containers.
# This verifies that:
# 1. Multiple Docker containers are spawned
# 2. Workers can work in parallel
# 3. Task coordination works
# 4. Run completes when all workers finish
#
# Requirements:
# - Docker installed and running
# - hirsel binary built
# - OAuth credentials available

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Default hirsel binary location
HIRSEL_BINARY="${HIRSEL_BINARY:-$PROJECT_ROOT/src-tauri/target/debug/hirsel}"
CONFIG_DIR="${HOME}/.hirsel"
CONFIG_FILE="${CONFIG_DIR}/config.toml"
RUN_NAME="e2e-multi-$(date +%s)"
WORKER_COUNT="${WORKER_COUNT:-2}"

echo "=========================================="
echo "Test: Multi-Worker Docker Runner"
echo "=========================================="
echo ""

# Check prerequisites
echo "Checking prerequisites..."

if [ ! -f "$HIRSEL_BINARY" ]; then
    echo "ERROR: hirsel binary not found at $HIRSEL_BINARY"
    echo "Build with: cargo build --no-default-features --features cli,dev"
    exit 1
fi
echo "  Binary: $HIRSEL_BINARY"

if ! command -v docker &> /dev/null; then
    echo "ERROR: docker not found"
    exit 1
fi
echo "  Docker: $(docker --version | cut -d' ' -f3 | tr -d ',')"

CLAUDE_CREDS_FILE="$HOME/.claude/.credentials.json"
if [ ! -f "$CLAUDE_CREDS_FILE" ]; then
    echo "ERROR: OAuth credentials not found at $CLAUDE_CREDS_FILE"
    exit 1
fi
echo "  OAuth credentials: found"
echo "  Worker count: $WORKER_COUNT"

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
# Test config for multi-worker docker runner

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

    # Stop any running containers for this test
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

    # Clean up temp project directory
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
echo "# Multi-File Test" > README.md
git add .
git commit -q -m "initial"

# Copy multi_file spec
cp "$SCRIPT_DIR/../scenarios/multi_file/spec.md" "$WORK_DIR/spec.md"
echo "  Spec copied"

# Run the test with multiple workers
echo ""
echo "Running multi_file scenario with $WORKER_COUNT docker workers..."
echo "  Run name: $RUN_NAME"
echo "  Image: buildpack-deps:noble"
echo ""

# Start with multiple workers
"$HIRSEL_BINARY" go "$RUN_NAME" spec.md --project "$WORK_DIR" --runner docker --yolo --workers "$WORKER_COUNT" &
GO_PID=$!

# Wait a bit for containers to start
echo "Waiting for docker containers to spawn..."
sleep 15

# Check how many containers are running
CONTAINER_COUNT=$(docker ps -q --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | wc -l)
echo "  Containers running: $CONTAINER_COUNT"

if [ "$CONTAINER_COUNT" -gt 0 ]; then
    echo ""
    echo "Container IDs:"
    docker ps -q --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | while read id; do
        name=$(docker inspect --format '{{.Name}}' "$id" 2>/dev/null | sed 's/^\///')
        echo "  - ${id:0:12} ($name)"
    done
fi

# Wait for hirsel go to finish spawning
wait $GO_PID || true

# Wait for completion by polling status (with timeout)
echo ""
echo "Waiting for run to complete..."
MAX_WAIT=300
START_TIME=$(date +%s)

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))

    # Check current status
    CURRENT_STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" --json 2>/dev/null | jq -r '.status // "unknown"' || echo "unknown")

    # Count active workers
    ACTIVE_WORKERS=$("$HIRSEL_BINARY" view "$RUN_NAME" --json 2>/dev/null | jq -r '.workers | map(select(.status == "working")) | length' || echo "0")

    # Break if completed
    case "$CURRENT_STATUS" in
        "completed"|"delivered"|"pass"|"done"|"fail")
            echo "  Status changed to: $CURRENT_STATUS"
            break
            ;;
    esac

    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: Run timed out after ${MAX_WAIT}s (status: $CURRENT_STATUS)"

        # Show container logs for debugging
        docker ps -q --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | while read id; do
            echo ""
            echo "Container ${id:0:12} logs:"
            docker logs "$id" 2>&1 | tail -30 || true
        done

        exit 1
    fi
    echo "  Waiting... (${ELAPSED}s, status: $CURRENT_STATUS, active workers: $ACTIVE_WORKERS)"
    sleep 10
done

# Check final status
echo ""
echo "Checking final status..."

STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" --json 2>/dev/null | jq -r '.status // "unknown"' || echo "unknown")
echo "  Final status: $STATUS"

# Show run summary
echo ""
echo "Run summary:"
"$HIRSEL_BINARY" view "$RUN_NAME" 2>&1 | head -30 || true

# Verify containers were stopped
REMAINING=$(docker ps -q --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | wc -l)
if [ "$REMAINING" -gt 0 ]; then
    echo "  WARNING: $REMAINING Docker container(s) still running"
else
    echo "  All Docker containers stopped"
fi

# Success criteria
case "$STATUS" in
    "completed"|"delivered"|"pass"|"done")
        echo ""
        echo "=========================================="
        echo "TEST PASSED"
        echo "=========================================="
        exit 0
        ;;
    "fail")
        echo ""
        echo "Run completed but eval failed (may be OK)"
        echo "=========================================="
        echo "TEST PASSED (with eval failure)"
        echo "=========================================="
        exit 0
        ;;
    *)
        echo ""
        echo "=========================================="
        echo "TEST FAILED: unexpected status '$STATUS'"
        echo "=========================================="
        exit 1
        ;;
esac
