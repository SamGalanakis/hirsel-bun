#!/bin/bash
# Test: Local Runner
#
# Purpose: Run hello_world task on the orchestrator server itself using local runner.
#
# Steps:
# 1. Deploy hirsel server to a sprite
# 2. Create a run via the server API with --runner local
# 3. Wait for run completion
# 4. Verify hello.py was created and outputs "Hello, World!"

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

test_header "Local Runner"

# ==============================================================================
# Validation
# ==============================================================================

check_env
check_binary

# ==============================================================================
# Setup
# ==============================================================================

SERVER_NAME=$(generate_sprite_name "hirsel-local")
HIRSEL_API_KEY=$(generate_api_key)
RUN_NAME="e2e-local-$(date +%s)"
WORK_DIR="/home/sprite/projects/hello-world"

register_cleanup "$SERVER_NAME"

echo "Configuration:"
echo "  Sprite name: $SERVER_NAME"
echo "  Run name: $RUN_NAME"
echo "  Test scenario: $TEST_SCENARIO"
echo ""

# ==============================================================================
# Deploy Server
# ==============================================================================

echo "Phase 1: Deploying server..."
echo ""

create_sprite "$SERVER_NAME"
wait_sprite_ready "$SERVER_NAME"
install_deps "$SERVER_NAME"
upload_hirsel "$SERVER_NAME"

# Create project directory and init git repo
echo "Setting up project directory..."
sprite_exec "$SERVER_NAME" "mkdir -p '$WORK_DIR' && cd '$WORK_DIR' && git init && git config user.email 'test@test.com' && git config user.name 'Test'"

# Copy test scenario spec
SPEC_CONTENT=$(cat "$SCRIPT_DIR/../scenarios/$TEST_SCENARIO/spec.md")
sprite_exec "$SERVER_NAME" "cat > '$WORK_DIR/spec.md' << 'SPECEOF'
$SPEC_CONTENT
SPECEOF"

# Check if eval exists
EVAL_FLAG=""
if [ -f "$SCRIPT_DIR/../scenarios/$TEST_SCENARIO/eval.md" ]; then
    EVAL_CONTENT=$(cat "$SCRIPT_DIR/../scenarios/$TEST_SCENARIO/eval.md")
    sprite_exec "$SERVER_NAME" "cat > '$WORK_DIR/eval.md' << 'EVALEOF'
$EVAL_CONTENT
EVALEOF"
    EVAL_FLAG="--eval '$WORK_DIR/eval.md'"
fi

# Start server
start_server "$SERVER_NAME" "$HIRSEL_API_KEY"

# ==============================================================================
# Run Test
# ==============================================================================

echo ""
echo "Phase 2: Running test..."
echo ""

# Forward API keys to the sprite
# Read Claude credentials if available
CLAUDE_CREDS=""
if [ -f "$HOME/.claude/.credentials.json" ]; then
    CLAUDE_ACCESS_TOKEN=$(jq -r '.access_token // empty' "$HOME/.claude/.credentials.json" 2>/dev/null || true)
    if [ -n "$CLAUDE_ACCESS_TOKEN" ]; then
        CLAUDE_CREDS="CLAUDE_ACCESS_TOKEN='$CLAUDE_ACCESS_TOKEN'"
    fi
fi

# Check for Anthropic API key
ANTHROPIC_CREDS=""
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
    ANTHROPIC_CREDS="ANTHROPIC_API_KEY='$ANTHROPIC_API_KEY'"
fi

# Start the run on the sprite
echo "Starting run '$RUN_NAME' on sprite..."

# Build go command
GO_CMD="cd '$WORK_DIR' && $CLAUDE_CREDS $ANTHROPIC_CREDS /usr/local/bin/hirsel go '$RUN_NAME' spec.md --project '$WORK_DIR' --yolo --runner local"

if [ -n "$EVAL_FLAG" ]; then
    GO_CMD="$GO_CMD $EVAL_FLAG"
fi

echo "  Command: hirsel go $RUN_NAME spec.md --project $WORK_DIR --yolo --runner local"

sprite_exec "$SERVER_NAME" "$GO_CMD"

# ==============================================================================
# Wait for Completion
# ==============================================================================

echo ""
echo "Phase 3: Waiting for completion..."
echo ""

# Poll run status
MAX_WAIT=300  # 5 minutes
START_TIME=$(date +%s)

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))

    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: Run timed out after ${MAX_WAIT}s"
        # Get logs for debugging
        echo ""
        echo "Server log:"
        sprite_exec_stdout "$SERVER_NAME" "tail -50 /tmp/hirsel-server.log" || true
        test_fail "Timeout waiting for run completion"
    fi

    # Check run status via CLI
    STATUS=$(sprite_exec_stdout "$SERVER_NAME" "/usr/local/bin/hirsel view '$RUN_NAME' --json 2>/dev/null | jq -r '.status // \"unknown\"'" || echo "unknown")

    case "$STATUS" in
        "completed"|"delivered"|"pass")
            echo "  Run completed with status: $STATUS"
            break
            ;;
        "fail")
            echo "  Run completed with status: $STATUS (eval failed, but that's ok for this test)"
            break
            ;;
        "error")
            echo "ERROR: Run failed with error status"
            sprite_exec_stdout "$SERVER_NAME" "/usr/local/bin/hirsel log '$RUN_NAME' --limit 50" || true
            test_fail "Run failed with error"
            ;;
        *)
            echo "  Status: $STATUS (${ELAPSED}s elapsed)"
            sleep 5
            ;;
    esac
done

# ==============================================================================
# Verify Output
# ==============================================================================

echo ""
echo "Phase 4: Verifying output..."
echo ""

# Verify using common helper
verify_hello_world "$SERVER_NAME" "$WORK_DIR"

# ==============================================================================
# Success
# ==============================================================================

test_pass
