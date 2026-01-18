#!/bin/bash
# Test: Sprite Runner
#
# Purpose: Run hello_world task using a second sprites.dev machine as the worker.
#
# Steps:
# 1. Deploy hirsel server to sprite #1 (orchestrator)
# 2. Create sprite #2 (worker)
# 3. Configure sprite runner on orchestrator
# 4. Create run with --runner sprite
# 5. Wait for completion
# 6. Verify output on worker sprite

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

test_header "Sprite Runner"

# ==============================================================================
# Validation
# ==============================================================================

check_env
check_binary

# ==============================================================================
# Setup
# ==============================================================================

ORCHESTRATOR_NAME=$(generate_sprite_name "hirsel-orch")
HIRSEL_API_KEY=$(generate_api_key)
RUN_NAME="e2e-sprite-$(date +%s)"
WORK_DIR="/home/sprite/work"

register_cleanup "$ORCHESTRATOR_NAME"

echo "Configuration:"
echo "  Orchestrator sprite: $ORCHESTRATOR_NAME"
echo "  Run name: $RUN_NAME"
echo "  Test scenario: $TEST_SCENARIO"
echo ""

# ==============================================================================
# Deploy Orchestrator
# ==============================================================================

echo "Phase 1: Deploying orchestrator..."
echo ""

create_sprite "$ORCHESTRATOR_NAME"
wait_sprite_ready "$ORCHESTRATOR_NAME"
install_deps "$ORCHESTRATOR_NAME"
upload_hirsel "$ORCHESTRATOR_NAME"

# Create project directory and init git repo
echo "Setting up project directory..."
sprite_exec "$ORCHESTRATOR_NAME" "mkdir -p '$WORK_DIR' && cd '$WORK_DIR' && git init && git config user.email 'test@test.com' && git config user.name 'Test'"

# Copy test scenario spec
SPEC_CONTENT=$(cat "$SCRIPT_DIR/../scenarios/$TEST_SCENARIO/spec.md")
sprite_exec "$ORCHESTRATOR_NAME" "cat > '$WORK_DIR/spec.md' << 'SPECEOF'
$SPEC_CONTENT
SPECEOF"

# Copy eval if exists
if [ -f "$SCRIPT_DIR/../scenarios/$TEST_SCENARIO/eval.md" ]; then
    EVAL_CONTENT=$(cat "$SCRIPT_DIR/../scenarios/$TEST_SCENARIO/eval.md")
    sprite_exec "$ORCHESTRATOR_NAME" "cat > '$WORK_DIR/eval.md' << 'EVALEOF'
$EVAL_CONTENT
EVALEOF"
fi

# Configure sprite runner
echo "Configuring sprite runner..."
sprite_exec "$ORCHESTRATOR_NAME" "mkdir -p /root/.hirsel && cat > /root/.hirsel/config.toml << 'CONFIGEOF'
[runners.sprites]
type = \"sprite\"
api_token = \"$SPRITES_TOKEN\"
auto_destroy = true
CONFIGEOF"

# Start server
start_server "$ORCHESTRATOR_NAME" "$HIRSEL_API_KEY"

# ==============================================================================
# Run Test with Sprite Runner
# ==============================================================================

echo ""
echo "Phase 2: Starting run with sprite runner..."
echo ""

# Forward API keys to the sprite
CLAUDE_CREDS=""
if [ -f "$HOME/.claude/.credentials.json" ]; then
    CLAUDE_ACCESS_TOKEN=$(jq -r '.access_token // empty' "$HOME/.claude/.credentials.json" 2>/dev/null || true)
    if [ -n "$CLAUDE_ACCESS_TOKEN" ]; then
        CLAUDE_CREDS="CLAUDE_ACCESS_TOKEN='$CLAUDE_ACCESS_TOKEN'"
    fi
fi

ANTHROPIC_CREDS=""
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
    ANTHROPIC_CREDS="ANTHROPIC_API_KEY='$ANTHROPIC_API_KEY'"
fi

# Start the run with sprite runner
# Note: The sprite runner will create a new sprite for the worker
echo "Starting run '$RUN_NAME' with sprite runner..."

GO_CMD="cd '$WORK_DIR' && $CLAUDE_CREDS $ANTHROPIC_CREDS SPRITES_TOKEN='$SPRITES_TOKEN' /usr/local/bin/hirsel go '$RUN_NAME' spec.md --project '$WORK_DIR' --yolo --runner sprites"

sprite_exec "$ORCHESTRATOR_NAME" "$GO_CMD"

# ==============================================================================
# Wait for Completion
# ==============================================================================

echo ""
echo "Phase 3: Waiting for completion..."
echo ""

MAX_WAIT=600  # 10 minutes (sprite spawning takes time)
START_TIME=$(date +%s)

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))

    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: Run timed out after ${MAX_WAIT}s"
        sprite_exec_stdout "$ORCHESTRATOR_NAME" "tail -50 /tmp/hirsel-server.log" || true
        test_fail "Timeout waiting for run completion"
    fi

    STATUS=$(sprite_exec_stdout "$ORCHESTRATOR_NAME" "/usr/local/bin/hirsel view '$RUN_NAME' --json 2>/dev/null | jq -r '.status // \"unknown\"'" || echo "unknown")

    case "$STATUS" in
        "completed"|"delivered"|"pass")
            echo "  Run completed with status: $STATUS"
            break
            ;;
        "fail")
            echo "  Run completed with status: $STATUS (eval failed)"
            break
            ;;
        "error")
            echo "ERROR: Run failed"
            sprite_exec_stdout "$ORCHESTRATOR_NAME" "/usr/local/bin/hirsel log '$RUN_NAME' --limit 50" || true
            test_fail "Run failed with error"
            ;;
        *)
            echo "  Status: $STATUS (${ELAPSED}s elapsed)"
            sleep 10
            ;;
    esac
done

# ==============================================================================
# Verify Output
# ==============================================================================

echo ""
echo "Phase 4: Verifying output..."
echo ""

# Get the worker sprite name from run info
WORKER_INFO=$(sprite_exec_stdout "$ORCHESTRATOR_NAME" "/usr/local/bin/hirsel view '$RUN_NAME' --json 2>/dev/null" || echo '{}')
echo "  Run info: $WORKER_INFO"

# For sprite runner, output is on the worker sprite
# We need to get the sprite name from worker info
WORKER_SPRITE=$(echo "$WORKER_INFO" | jq -r '.workers[0].runner_id // empty')

if [ -n "$WORKER_SPRITE" ]; then
    echo "  Worker sprite: $WORKER_SPRITE"
    register_cleanup "$WORKER_SPRITE"

    # Verify on worker sprite
    verify_hello_world "$WORKER_SPRITE" "/home/sprite/work"
else
    echo "  Could not determine worker sprite, checking orchestrator work dir..."
    # Fallback: check if output is in orchestrator's work dir (for testing)
    verify_hello_world "$ORCHESTRATOR_NAME" "$WORK_DIR"
fi

# ==============================================================================
# Success
# ==============================================================================

test_pass
