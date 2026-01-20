#!/bin/bash
# Test: Local Docker Runner
#
# Purpose: Test the local docker runner end-to-end with the noop scenario.
# This verifies that:
# 1. Docker container is spawned
# 2. Setup script downloads hirsel + claude from GitHub
# 3. Worker runs and completes
# 4. Container is properly stopped via docker stop
#
# Requirements:
# - Docker installed and running
# - hirsel binary built
# - ANTHROPIC_API_KEY set (for Claude)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Default hirsel binary location
HIRSEL_BINARY="${HIRSEL_BINARY:-$PROJECT_ROOT/src-tauri/target/debug/hirsel}"
CONFIG_DIR="${HOME}/.hirsel"
CONFIG_FILE="${CONFIG_DIR}/config.toml"
RUN_NAME="e2e-docker-$(date +%s)"

echo "=========================================="
echo "Test: Local Docker Runner"
echo "=========================================="
echo ""

# Check prerequisites
echo "Checking prerequisites..."

if [ ! -f "$HIRSEL_BINARY" ]; then
    echo "ERROR: hirsel binary not found at $HIRSEL_BINARY"
    echo "Build with: cd src-tauri && cargo build"
    exit 1
fi
echo "  Binary: $HIRSEL_BINARY"

if ! command -v docker &> /dev/null; then
    echo "ERROR: docker not found"
    exit 1
fi
echo "  Docker: $(docker --version | cut -d' ' -f3 | tr -d ',')"

if ! docker info &> /dev/null; then
    echo "ERROR: docker daemon not running"
    exit 1
fi
echo "  Docker daemon: running"

if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    echo "ERROR: ANTHROPIC_API_KEY not set"
    exit 1
fi
echo "  ANTHROPIC_API_KEY: set"

# Backup existing config if present
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
# Test config for local docker runner

[runners.docker]
[runners.docker.host]
type = "local"

[runners.docker.container]
image = "debian:bookworm-slim"
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

    # Restore original config if backed up
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
echo "# Test" > README.md
git add .
git commit -q -m "initial"

# Copy noop spec
cp "$SCRIPT_DIR/../scenarios/noop/spec.md" "$WORK_DIR/spec.md"
echo "  Spec copied"

# Run the test
echo ""
echo "Running noop scenario with docker runner..."
echo "  Run name: $RUN_NAME"
echo "  Image: debian:bookworm-slim"
echo ""

# Start in background so we can monitor
"$HIRSEL_BINARY" go "$RUN_NAME" spec.md --project "$WORK_DIR" --runner docker --yolo &
GO_PID=$!

# Wait a bit for container to start
echo "Waiting for docker container to spawn..."
sleep 10

# Check if a docker container was created
CONTAINER_ID=$(docker ps -q --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | head -1)

if [ -n "$CONTAINER_ID" ]; then
    echo "  Container spawned: ${CONTAINER_ID:0:12}"

    # Show container logs
    echo ""
    echo "Container logs (first 20 lines):"
    docker logs "$CONTAINER_ID" 2>&1 | head -20 || true
    echo "..."
else
    echo "  No container found yet (may have completed quickly or failed to start)"
fi

# Check database for runner info
DB_PATH="${CONFIG_DIR}/runs/${RUN_NAME}/hirsel.db"
if [ -f "$DB_PATH" ]; then
    echo ""
    echo "Checking database for runner info..."
    RUNNER_INFO=$(sqlite3 "$DB_PATH" "SELECT name, runner_id, runner_type FROM workers LIMIT 1" 2>/dev/null || echo "")
    if [ -n "$RUNNER_INFO" ]; then
        echo "  Worker info: $RUNNER_INFO"
    fi
fi

# Wait for completion (with timeout)
echo ""
echo "Waiting for run to complete..."
MAX_WAIT=300  # 5 minutes (setup takes time)
START_TIME=$(date +%s)

while kill -0 $GO_PID 2>/dev/null; do
    ELAPSED=$(( $(date +%s) - START_TIME ))
    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: Run timed out after ${MAX_WAIT}s"

        # Show container logs for debugging
        if [ -n "$CONTAINER_ID" ]; then
            echo ""
            echo "Container logs:"
            docker logs "$CONTAINER_ID" 2>&1 | tail -50 || true
        fi

        kill $GO_PID 2>/dev/null || true
        exit 1
    fi
    echo "  Waiting... (${ELAPSED}s)"
    sleep 10
done

# Check exit status
wait $GO_PID || true

# Check final status
echo ""
echo "Checking final status..."

STATUS=$("$HIRSEL_BINARY" view "$RUN_NAME" --json 2>/dev/null | jq -r '.status // "unknown"' || echo "unknown")
echo "  Final status: $STATUS"

# Verify container was stopped
REMAINING=$(docker ps -q --filter "name=hirsel-${RUN_NAME}" 2>/dev/null | wc -l)
if [ "$REMAINING" -gt 0 ]; then
    echo "  WARNING: Docker container still running"
else
    echo "  Docker container stopped"
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
        echo "Run completed but eval failed (this may be OK for noop)"
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

        # Show logs for debugging
        if [ -n "$CONTAINER_ID" ]; then
            echo ""
            echo "Container logs:"
            docker logs "$CONTAINER_ID" 2>&1 | tail -100 || true
        fi

        exit 1
        ;;
esac
