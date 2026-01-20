#!/bin/bash
# Setup script for creating a Hirsel worker sprite checkpoint.
#
# This script prepares a fresh sprite with all dependencies needed
# to run AI workers:
# - Node.js and npm
# - Claude Code CLI and ACP adapter
# - Codex CLI and ACP adapter
# - Hirsel worker binary
#
# Usage:
#   SPRITES_TOKEN=xxx ./scripts/setup-sprite-checkpoint.sh [checkpoint-name]
#
# The script will:
# 1. Create a new sprite named "hirsel-setup"
# 2. Install all dependencies
# 3. Copy the hirsel binary
# 4. Create a checkpoint
# 5. Print the checkpoint ID for use in config

set -euo pipefail

CHECKPOINT_NAME="${1:-hirsel-worker-base}"
SPRITE_NAME="hirsel-setup-$$"
SPRITES_API="${SPRITES_API_URL:-https://api.sprites.dev/v1}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log() { echo -e "${GREEN}[+]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[-]${NC} $1" >&2; }

# Check requirements
if [[ -z "${SPRITES_TOKEN:-}" ]]; then
    error "SPRITES_TOKEN environment variable is required"
    exit 1
fi

# Check for hirsel binary
HIRSEL_BIN=""
if [[ -f "./src-tauri/target/release/hirsel" ]]; then
    HIRSEL_BIN="./src-tauri/target/release/hirsel"
elif [[ -f "./target/release/hirsel" ]]; then
    HIRSEL_BIN="./target/release/hirsel"
elif [[ -f "./docker/hirsel-linux-amd64" ]]; then
    HIRSEL_BIN="./docker/hirsel-linux-amd64"
else
    error "Hirsel binary not found. Build first with:"
    error "  cargo build --release --no-default-features --features worker"
    exit 1
fi

log "Using hirsel binary: $HIRSEL_BIN"

# Sprites API helper
sprites() {
    local method="$1"
    local endpoint="$2"
    shift 2
    curl -s -X "$method" \
        -H "Authorization: Bearer $SPRITES_TOKEN" \
        -H "Content-Type: application/json" \
        "${SPRITES_API}${endpoint}" "$@"
}

# Create sprite
log "Creating sprite: $SPRITE_NAME"
RESULT=$(sprites POST "/sprites" -d "{\"name\": \"$SPRITE_NAME\"}")
if echo "$RESULT" | grep -q "error"; then
    error "Failed to create sprite: $RESULT"
    exit 1
fi

# Cleanup on exit
cleanup() {
    if [[ -n "${SPRITE_NAME:-}" ]]; then
        warn "Cleaning up sprite: $SPRITE_NAME"
        sprites DELETE "/sprites/$SPRITE_NAME" || true
    fi
}
trap cleanup EXIT

# Execute command on sprite
exec_on_sprite() {
    local cmd="$1"
    log "Running: $cmd"
    RESULT=$(sprites POST "/sprites/$SPRITE_NAME/exec" \
        -d "{\"command\": [\"sh\", \"-c\", \"$cmd\"]}")

    EXIT_CODE=$(echo "$RESULT" | jq -r '.exit_code // 0')
    if [[ "$EXIT_CODE" != "0" ]]; then
        STDERR=$(echo "$RESULT" | jq -r '.stderr // ""')
        warn "Command exited with code $EXIT_CODE: $STDERR"
    fi
}

# Wait for sprite to be ready
log "Waiting for sprite to be ready..."
for i in {1..30}; do
    STATUS=$(sprites GET "/sprites/$SPRITE_NAME" | jq -r '.status')
    if [[ "$STATUS" == "running" ]]; then
        break
    fi
    sleep 2
done

# Install system dependencies
log "Installing system dependencies..."
exec_on_sprite "apt-get update && apt-get install -y curl git ca-certificates"

# Install Node.js via nvm
log "Installing Node.js..."
exec_on_sprite "curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash"
exec_on_sprite "export NVM_DIR=\\\"\\\$HOME/.nvm\\\" && . \\\"\\\$NVM_DIR/nvm.sh\\\" && nvm install 22"

# Install Claude Code CLI and ACP adapter
log "Installing Claude Code CLI and ACP adapter..."
exec_on_sprite "export NVM_DIR=\\\"\\\$HOME/.nvm\\\" && . \\\"\\\$NVM_DIR/nvm.sh\\\" && npm install -g @anthropic-ai/claude-code"
exec_on_sprite "export NVM_DIR=\\\"\\\$HOME/.nvm\\\" && . \\\"\\\$NVM_DIR/nvm.sh\\\" && npm install -g claude-code-acp"

# Install Codex CLI and ACP adapter
log "Installing Codex CLI and ACP adapter..."
exec_on_sprite "export NVM_DIR=\\\"\\\$HOME/.nvm\\\" && . \\\"\\\$NVM_DIR/nvm.sh\\\" && npm install -g @openai/codex"
exec_on_sprite "export NVM_DIR=\\\"\\\$HOME/.nvm\\\" && . \\\"\\\$NVM_DIR/nvm.sh\\\" && npm install -g codex-acp"

# Create bin directory
exec_on_sprite "mkdir -p /usr/local/bin"

# Upload hirsel binary
log "Uploading hirsel binary..."
# Base64 encode the binary for transfer via JSON
HIRSEL_B64=$(base64 -w0 "$HIRSEL_BIN")

# Write to a temp file on sprite and decode
exec_on_sprite "echo '$HIRSEL_B64' | base64 -d > /usr/local/bin/hirsel && chmod +x /usr/local/bin/hirsel"

# Verify installation
log "Verifying installations..."
exec_on_sprite "export NVM_DIR=\\\"\\\$HOME/.nvm\\\" && . \\\"\\\$NVM_DIR/nvm.sh\\\" && node --version"
exec_on_sprite "export NVM_DIR=\\\"\\\$HOME/.nvm\\\" && . \\\"\\\$NVM_DIR/nvm.sh\\\" && which claude-code-acp"
exec_on_sprite "/usr/local/bin/hirsel --version"

# Create work directory
exec_on_sprite "mkdir -p /home/sprite/work"

# Create checkpoint
log "Creating checkpoint: $CHECKPOINT_NAME"
CHECKPOINT_RESULT=$(sprites POST "/sprites/$SPRITE_NAME/checkpoints" \
    -d "{\"comment\": \"$CHECKPOINT_NAME - Hirsel worker base image with Node.js, Claude Code, Codex, and ACP adapters\"}")

CHECKPOINT_ID=$(echo "$CHECKPOINT_RESULT" | jq -r '.id')

if [[ -z "$CHECKPOINT_ID" || "$CHECKPOINT_ID" == "null" ]]; then
    error "Failed to create checkpoint: $CHECKPOINT_RESULT"
    exit 1
fi

echo ""
echo "========================================"
echo -e "${GREEN}Checkpoint created successfully!${NC}"
echo "========================================"
echo ""
echo "Checkpoint ID: $CHECKPOINT_ID"
echo "Checkpoint Name: $CHECKPOINT_NAME"
echo ""
echo "Add this to your Hirsel config (config.toml):"
echo ""
echo "[runners.sprite]"
echo "base_checkpoint = \"$CHECKPOINT_ID\""
echo ""

# Disable cleanup since checkpoint was successful
trap - EXIT
log "Destroying setup sprite..."
sprites DELETE "/sprites/$SPRITE_NAME" || true

log "Done!"
