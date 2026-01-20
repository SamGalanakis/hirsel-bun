#!/bin/bash
# Test: Server Deployment
#
# Purpose: Verify hirsel server can be deployed to a sprite and responds to health checks.
#
# Steps:
# 1. Create sprite via sprites.dev API
# 2. Install Node.js + ACP packages
# 3. Upload hirsel binary
# 4. Start hirsel serve --port 8080
# 5. Verify /health endpoint responds

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

test_header "Server Deployment"

# ==============================================================================
# Validation
# ==============================================================================

check_sprite_cli
check_binary

# ==============================================================================
# Setup
# ==============================================================================

SERVER_NAME=$(generate_sprite_name "hirsel-orch")
HIRSEL_API_KEY=$(generate_api_key)

register_cleanup "$SERVER_NAME"

echo "Configuration:"
echo "  Sprite name: $SERVER_NAME"
echo "  API key: ${HIRSEL_API_KEY:0:8}..."
echo ""

# ==============================================================================
# Test Steps
# ==============================================================================

# Step 1: Create sprite
echo "Step 1: Creating sprite..."
create_sprite "$SERVER_NAME"
wait_sprite_ready "$SERVER_NAME"

# Step 2: Install dependencies
echo ""
echo "Step 2: Installing dependencies..."
install_deps "$SERVER_NAME"

# Step 3: Upload hirsel binary
echo ""
echo "Step 3: Uploading hirsel binary..."
upload_hirsel "$SERVER_NAME"

# Step 4: Start server
echo ""
echo "Step 4: Starting hirsel server..."
start_server "$SERVER_NAME" "$HIRSEL_API_KEY"

# Step 5: Verify health check
echo ""
echo "Step 5: Verifying health endpoint..."

HEALTH_RESPONSE=$(sprite_exec_stdout "$SERVER_NAME" "curl -s http://localhost:$SERVER_PORT/health")

if [ -z "$HEALTH_RESPONSE" ]; then
    test_fail "Health endpoint returned empty response"
fi

echo "  Health response: $HEALTH_RESPONSE"

# Parse response to check it's valid JSON
if ! echo "$HEALTH_RESPONSE" | jq -e . > /dev/null 2>&1; then
    test_fail "Health endpoint returned invalid JSON"
fi

# ==============================================================================
# Success
# ==============================================================================

echo ""
echo "Server deployed successfully!"
echo ""
echo "Connection details:"
echo "  SERVER_URL=https://${SERVER_NAME}.sprites.app"
echo "  HIRSEL_API_KEY=$HIRSEL_API_KEY"
echo "  SERVER_SPRITE=$SERVER_NAME"

test_pass
