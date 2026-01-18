#!/bin/bash
# Test: SSH Runner
#
# Purpose: Run hello_world task via SSH connection to the server sprite.
#
# This test verifies the SSH runner works by SSHing from the orchestrator
# back to itself (loopback test).
#
# Steps:
# 1. Deploy hirsel server to a sprite
# 2. Set up SSH keys for loopback connection
# 3. Configure SSH runner pointing to localhost
# 4. Create run with SSH runner (--remote user@localhost)
# 5. Wait for completion
# 6. Verify output

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

test_header "SSH Runner"

# ==============================================================================
# Validation
# ==============================================================================

check_env
check_binary

# ==============================================================================
# Setup
# ==============================================================================

SERVER_NAME=$(generate_sprite_name "hirsel-ssh")
HIRSEL_API_KEY=$(generate_api_key)
RUN_NAME="e2e-ssh-$(date +%s)"
WORK_DIR="/home/sprite/projects/hello-world"
REMOTE_WORK_DIR="/tmp/hirsel-remote/$RUN_NAME"

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

# Install SSH server
echo "Installing and configuring SSH..."
sprite_exec "$SERVER_NAME" "apt-get install -y -qq openssh-server > /dev/null 2>&1"

# Set up SSH keys for passwordless localhost access
echo "Setting up SSH keys..."
sprite_exec "$SERVER_NAME" "mkdir -p /root/.ssh && ssh-keygen -t rsa -N '' -f /root/.ssh/id_rsa <<< y > /dev/null 2>&1 || true"
sprite_exec "$SERVER_NAME" "cat /root/.ssh/id_rsa.pub >> /root/.ssh/authorized_keys"
sprite_exec "$SERVER_NAME" "chmod 600 /root/.ssh/authorized_keys"
sprite_exec "$SERVER_NAME" "echo 'Host localhost\n  StrictHostKeyChecking no\n  UserKnownHostsFile=/dev/null' > /root/.ssh/config"

# Start SSH server
sprite_exec "$SERVER_NAME" "service ssh start || /usr/sbin/sshd"

# Verify SSH works
echo "Verifying SSH connectivity..."
SSH_TEST=$(sprite_exec_stdout "$SERVER_NAME" "ssh -o BatchMode=yes localhost 'echo ok' 2>/dev/null" || echo "failed")
if [ "$SSH_TEST" != "ok" ]; then
    echo "ERROR: SSH loopback connection failed"
    sprite_exec_stdout "$SERVER_NAME" "ssh -v localhost 'echo test' 2>&1 | tail -20" || true
    test_fail "SSH setup failed"
fi
echo "  SSH connection verified"

# Create project directory and init git repo
echo "Setting up project directory..."
sprite_exec "$SERVER_NAME" "mkdir -p '$WORK_DIR' && cd '$WORK_DIR' && git init && git config user.email 'test@test.com' && git config user.name 'Test'"

# Copy test scenario spec
SPEC_CONTENT=$(cat "$SCRIPT_DIR/../scenarios/$TEST_SCENARIO/spec.md")
sprite_exec "$SERVER_NAME" "cat > '$WORK_DIR/spec.md' << 'SPECEOF'
$SPEC_CONTENT
SPECEOF"

# Copy eval if exists
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
# Run Test with SSH Runner
# ==============================================================================

echo ""
echo "Phase 2: Starting run with SSH runner..."
echo ""

# Forward API keys
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

# Start the run with SSH runner (remote to localhost)
echo "Starting run '$RUN_NAME' with SSH runner (localhost loopback)..."

GO_CMD="cd '$WORK_DIR' && $CLAUDE_CREDS $ANTHROPIC_CREDS /usr/local/bin/hirsel go '$RUN_NAME' spec.md --project '$WORK_DIR' --yolo --remote root@localhost:1"

if [ -n "$EVAL_FLAG" ]; then
    GO_CMD="$GO_CMD $EVAL_FLAG"
fi

sprite_exec "$SERVER_NAME" "$GO_CMD"

# ==============================================================================
# Wait for Completion
# ==============================================================================

echo ""
echo "Phase 3: Waiting for completion..."
echo ""

MAX_WAIT=300  # 5 minutes
START_TIME=$(date +%s)

while true; do
    ELAPSED=$(( $(date +%s) - START_TIME ))

    if [ "$ELAPSED" -ge "$MAX_WAIT" ]; then
        echo "ERROR: Run timed out after ${MAX_WAIT}s"
        sprite_exec_stdout "$SERVER_NAME" "tail -50 /tmp/hirsel-server.log" || true
        test_fail "Timeout waiting for run completion"
    fi

    STATUS=$(sprite_exec_stdout "$SERVER_NAME" "/usr/local/bin/hirsel view '$RUN_NAME' --json 2>/dev/null | jq -r '.status // \"unknown\"'" || echo "unknown")

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

# For SSH runner, output should be in the remote work directory
# Since we're using localhost loopback, check the remote work dir
verify_hello_world "$SERVER_NAME" "$REMOTE_WORK_DIR"

# ==============================================================================
# Success
# ==============================================================================

test_pass
