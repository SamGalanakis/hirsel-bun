#!/bin/bash
# Run all E2E tests
#
# Usage:
#   ./run_all.sh              # Run all tests
#   ./run_all.sh --parallel   # Run tests in parallel
#   ./run_all.sh server       # Run only test_server_deploy

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ==============================================================================
# Configuration
# ==============================================================================

TESTS=(
    "test_server_deploy.sh"
    "test_local_runner.sh"
    "test_sprite_runner.sh"
    "test_ssh_runner.sh"
)

PARALLEL=false
SPECIFIC_TEST=""

# Parse arguments
for arg in "$@"; do
    case "$arg" in
        --parallel|-p)
            PARALLEL=true
            ;;
        server)
            SPECIFIC_TEST="test_server_deploy.sh"
            ;;
        local)
            SPECIFIC_TEST="test_local_runner.sh"
            ;;
        sprite)
            SPECIFIC_TEST="test_sprite_runner.sh"
            ;;
        ssh)
            SPECIFIC_TEST="test_ssh_runner.sh"
            ;;
        --help|-h)
            echo "E2E Test Runner"
            echo ""
            echo "Usage: $0 [options] [test]"
            echo ""
            echo "Options:"
            echo "  --parallel, -p    Run tests in parallel"
            echo "  --help, -h        Show this help"
            echo ""
            echo "Tests:"
            echo "  server            Run only server deployment test"
            echo "  local             Run only local runner test"
            echo "  sprite            Run only sprite runner test"
            echo "  ssh               Run only SSH runner test"
            echo ""
            echo "Environment:"
            echo "  SPRITES_TOKEN     Required: sprites.dev API token"
            echo "  HIRSEL_BINARY     Optional: path to hirsel binary"
            echo "  TEST_SCENARIO     Optional: test scenario (default: hello_world)"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg"
            echo "Use --help for usage"
            exit 1
            ;;
    esac
done

# ==============================================================================
# Validation
# ==============================================================================

if [ -z "${SPRITES_TOKEN:-}" ]; then
    echo "ERROR: SPRITES_TOKEN environment variable is required"
    echo "Get your token from https://sprites.dev"
    exit 1
fi

# ==============================================================================
# Run Tests
# ==============================================================================

echo "=========================================="
echo "Hirsel E2E Test Suite"
echo "=========================================="
echo ""
echo "Configuration:"
echo "  Scenario: ${TEST_SCENARIO:-hello_world}"
echo "  Parallel: $PARALLEL"
echo ""

PASSED=0
FAILED=0
RESULTS=()

run_test() {
    local test="$1"
    local test_path="$SCRIPT_DIR/$test"

    if [ ! -x "$test_path" ]; then
        echo "SKIP: $test (not executable)"
        return 0
    fi

    echo ""
    echo "Running: $test"
    echo "---"

    if "$test_path"; then
        RESULTS+=("PASS: $test")
        return 0
    else
        RESULTS+=("FAIL: $test")
        return 1
    fi
}

if [ -n "$SPECIFIC_TEST" ]; then
    # Run specific test
    if run_test "$SPECIFIC_TEST"; then
        PASSED=$((PASSED + 1))
    else
        FAILED=$((FAILED + 1))
    fi
elif [ "$PARALLEL" = true ]; then
    # Run all tests in parallel
    echo "Running tests in parallel..."

    PIDS=()
    for test in "${TESTS[@]}"; do
        (
            "$SCRIPT_DIR/$test" > "/tmp/e2e-$test.log" 2>&1
            echo $? > "/tmp/e2e-$test.exit"
        ) &
        PIDS+=($!)
    done

    # Wait for all tests
    for i in "${!PIDS[@]}"; do
        pid="${PIDS[$i]}"
        test="${TESTS[$i]}"

        if wait "$pid"; then
            PASSED=$((PASSED + 1))
            RESULTS+=("PASS: $test")
        else
            FAILED=$((FAILED + 1))
            RESULTS+=("FAIL: $test")
            echo ""
            echo "--- $test failed, log: ---"
            cat "/tmp/e2e-$test.log" | tail -50
        fi
    done
else
    # Run tests sequentially
    for test in "${TESTS[@]}"; do
        if run_test "$test"; then
            PASSED=$((PASSED + 1))
        else
            FAILED=$((FAILED + 1))
            # Continue with remaining tests
        fi
    done
fi

# ==============================================================================
# Summary
# ==============================================================================

echo ""
echo "=========================================="
echo "Test Results"
echo "=========================================="
echo ""

for result in "${RESULTS[@]}"; do
    echo "  $result"
done

echo ""
echo "Summary: $PASSED passed, $FAILED failed"
echo ""

if [ "$FAILED" -gt 0 ]; then
    exit 1
fi
