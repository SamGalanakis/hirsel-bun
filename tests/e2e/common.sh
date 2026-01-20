#!/bin/bash
# Common utilities for e2e tests
# Uses the sprite CLI for all sprite operations

set -euo pipefail

# ==============================================================================
# Configuration
# ==============================================================================

SERVER_PORT="${SERVER_PORT:-8080}"
TEST_SCENARIO="${TEST_SCENARIO:-hello_world}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Default hirsel binary location
HIRSEL_BINARY="${HIRSEL_BINARY:-$PROJECT_ROOT/src-tauri/target/release/hirsel}"

# ==============================================================================
# Validation
# ==============================================================================

# Check sprite CLI is available and authenticated
check_sprite_cli() {
    if ! command -v sprite &> /dev/null; then
        echo "ERROR: sprite CLI not found"
        echo "Install from: https://sprites.dev"
        exit 1
    fi

    if ! sprite list &> /dev/null; then
        echo "ERROR: sprite CLI not authenticated"
        echo "Run: sprite login"
        exit 1
    fi
}

# Check hirsel binary exists
check_binary() {
    if [ ! -f "$HIRSEL_BINARY" ]; then
        echo "ERROR: hirsel binary not found at $HIRSEL_BINARY"
        echo "Build with: cd src-tauri && cargo build --release --no-default-features"
        exit 1
    fi
    echo "Using binary: $HIRSEL_BINARY"
}

# ==============================================================================
# Sprite Management
# ==============================================================================

# Generate a random name for the sprite
generate_sprite_name() {
    local prefix="${1:-hirsel-e2e}"
    echo "${prefix}-$(openssl rand -hex 4)"
}

# Generate random API key for server auth
generate_api_key() {
    openssl rand -hex 32
}

# Create a sprite
# Args: name
create_sprite() {
    local name="$1"
    echo "Creating sprite: $name"

    # sprite create returns 500 sometimes but still creates the sprite
    # So we ignore the error and verify the sprite exists
    sprite create "$name" 2>&1 || true

    # Verify sprite was created
    if sprite list 2>/dev/null | grep -q "^$name$"; then
        echo "  Sprite $name created"
        return 0
    else
        echo "ERROR: Sprite $name was not created"
        return 1
    fi
}

# Wait for sprite to be ready
# Args: name, [max_attempts=60]
wait_sprite_ready() {
    local name="$1"
    local max_attempts="${2:-60}"

    echo "Waiting for sprite $name to be ready..."

    for i in $(seq 1 "$max_attempts"); do
        if sprite exec -s "$name" true 2>/dev/null; then
            echo "Sprite $name is ready"
            return 0
        fi
        echo "  Attempt $i/$max_attempts..."
        sleep 2
    done

    echo "ERROR: Sprite $name failed to become ready after $max_attempts attempts"
    return 1
}

# Execute command on sprite
# Args: name, command...
sprite_exec() {
    local name="$1"
    shift
    sprite exec -s "$name" -- sh -c "$*"
}

# Execute command and get stdout
# Args: name, command...
sprite_exec_stdout() {
    local name="$1"
    shift
    sprite exec -s "$name" -- sh -c "$*" 2>/dev/null || true
}

# Destroy sprite
# Args: name
destroy_sprite() {
    local name="$1"
    echo "Destroying sprite: $name"
    sprite destroy -s "$name" --force 2>/dev/null || true
}

# ==============================================================================
# Server Setup
# ==============================================================================

# Upload hirsel binary to sprite
# Args: sprite_name
upload_hirsel() {
    local name="$1"

    echo "Uploading hirsel binary to sprite $name..."

    # Method 1: Download from release URL
    if [ -n "${HIRSEL_RELEASE_URL:-}" ]; then
        echo "  Downloading from: $HIRSEL_RELEASE_URL"
        sprite_exec "$name" "curl -fsSL '$HIRSEL_RELEASE_URL' -o /usr/local/bin/hirsel && chmod +x /usr/local/bin/hirsel"

        if verify_hirsel_binary "$name"; then
            return 0
        fi
        echo "  WARNING: Download failed, trying other methods..."
    fi

    # Method 2: sprite CLI with -file flag
    check_binary

    echo "  Uploading via sprite CLI (this may take a moment for 34MB)..."
    if sprite exec -s "$name" -file "$HIRSEL_BINARY:/usr/local/bin/hirsel" -- chmod +x /usr/local/bin/hirsel; then
        if verify_hirsel_binary "$name"; then
            return 0
        fi
    fi

    echo ""
    echo "ERROR: Could not upload hirsel binary to sprite"
    return 1
}

# Verify hirsel binary is working on sprite
verify_hirsel_binary() {
    local name="$1"
    local version
    version=$(sprite_exec_stdout "$name" "/usr/local/bin/hirsel --version 2>&1 || echo 'FAILED'")

    if [[ "$version" == *"FAILED"* ]] || [ -z "$version" ]; then
        return 1
    fi

    echo "  Binary verified: $version"
    return 0
}

# Install dependencies on sprite
# Args: sprite_name
install_deps() {
    local name="$1"

    echo "Installing dependencies on sprite $name..."

    # Update package lists and install dependencies (needs sudo)
    echo "  Installing system packages..."
    sprite exec -s "$name" -- sudo apt-get update -qq
    sprite exec -s "$name" -- sudo apt-get install -y -qq nodejs npm git python3 libgit2-1.9

    # Install ACP packages globally
    echo "  Installing ACP packages..."
    sprite exec -s "$name" -- sudo npm install -g @anthropics/claude-code-acp 2>/dev/null || true

    echo "  Dependencies installed"
}

# Start hirsel server on sprite
# Args: sprite_name, api_key
start_server() {
    local name="$1"
    local api_key="$2"

    echo "Starting hirsel server on sprite $name..."

    # Start server in background
    sprite_exec "$name" "HIRSEL_API_KEY='$api_key' nohup /usr/local/bin/hirsel serve --port $SERVER_PORT > /tmp/hirsel-server.log 2>&1 &"

    # Wait for server to be ready
    local max_attempts=30
    for i in $(seq 1 "$max_attempts"); do
        sleep 1
        local health
        health=$(sprite_exec_stdout "$name" "curl -s http://localhost:$SERVER_PORT/health 2>/dev/null || echo 'not ready'")
        if [[ "$health" != "not ready" ]] && [[ "$health" != "" ]]; then
            echo "  Server is ready"
            return 0
        fi
        echo "  Waiting for server... ($i/$max_attempts)"
    done

    echo "ERROR: Server failed to start. Log:"
    sprite_exec_stdout "$name" "cat /tmp/hirsel-server.log"
    return 1
}

# ==============================================================================
# Test Helpers
# ==============================================================================

# Wait for run to complete
# Args: sprite_name, api_key, run_name, [timeout_seconds=300]
wait_run_complete() {
    local sprite_name="$1"
    local api_key="$2"
    local run_name="$3"
    local timeout="${4:-300}"

    echo "Waiting for run $run_name to complete (timeout: ${timeout}s)..."

    local start_time
    start_time=$(date +%s)

    while true; do
        local elapsed
        elapsed=$(( $(date +%s) - start_time ))

        if [ "$elapsed" -ge "$timeout" ]; then
            echo "ERROR: Run $run_name timed out after ${timeout}s"
            return 1
        fi

        local response
        response=$(sprite_exec_stdout "$sprite_name" "curl -s http://localhost:$SERVER_PORT/api/runs/$run_name -H 'Authorization: Bearer $api_key'" || echo '{}')

        local status
        status=$(echo "$response" | jq -r '.status // "unknown"' 2>/dev/null || echo "unknown")

        case "$status" in
            "completed"|"delivered"|"pass"|"fail")
                echo "  Run completed with status: $status"
                return 0
                ;;
            "error")
                echo "ERROR: Run failed with error"
                return 1
                ;;
            *)
                echo "  Status: $status (${elapsed}s elapsed)"
                sleep 5
                ;;
        esac
    done
}

# Verify hello_world scenario output
# Args: sprite_name, work_dir
verify_hello_world() {
    local sprite_name="$1"
    local work_dir="$2"

    echo "Verifying hello_world output..."

    # Check hello.py exists
    local file_exists
    file_exists=$(sprite_exec_stdout "$sprite_name" "test -f '$work_dir/hello.py' && echo 'yes' || echo 'no'")

    if [ "$file_exists" != "yes" ]; then
        echo "ERROR: hello.py not found in $work_dir"
        sprite_exec_stdout "$sprite_name" "ls -la '$work_dir'"
        return 1
    fi

    # Check output
    local output
    output=$(sprite_exec_stdout "$sprite_name" "cd '$work_dir' && python3 hello.py 2>&1")

    if [ "$output" = "Hello, World!" ]; then
        echo "  Output verified: $output"
        return 0
    else
        echo "ERROR: Unexpected output: '$output'"
        echo "  Expected: 'Hello, World!'"
        return 1
    fi
}

# ==============================================================================
# Cleanup
# ==============================================================================

# List of sprites to clean up on exit
SPRITES_TO_CLEANUP=()

# Register sprite for cleanup
register_cleanup() {
    SPRITES_TO_CLEANUP+=("$1")
}

# Cleanup function called on exit
cleanup() {
    local exit_code=$?

    if [ ${#SPRITES_TO_CLEANUP[@]} -gt 0 ]; then
        echo ""
        echo "Cleaning up sprites..."
        for sprite in "${SPRITES_TO_CLEANUP[@]}"; do
            destroy_sprite "$sprite" || true
        done
    fi

    exit $exit_code
}

# Set up cleanup trap
trap cleanup EXIT

# ==============================================================================
# Test Output
# ==============================================================================

# Print success message
test_pass() {
    echo ""
    echo "=========================================="
    echo "TEST PASSED"
    echo "=========================================="
}

# Print failure message
test_fail() {
    local msg="${1:-Test failed}"
    echo ""
    echo "=========================================="
    echo "TEST FAILED: $msg"
    echo "=========================================="
    exit 1
}

# Print test header
test_header() {
    local name="$1"
    echo ""
    echo "=========================================="
    echo "E2E Test: $name"
    echo "=========================================="
    echo ""
}
