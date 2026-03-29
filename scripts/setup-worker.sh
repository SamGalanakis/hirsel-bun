#!/bin/bash
#
# Setup a hirsel worker environment with all dependencies
#
# Environment variables:
#   HIRSEL_AGENT        - Agent to install: claude (required)
#   HIRSEL_TAG          - Hirsel version tag (default: latest release)
#   HIRSEL_INSTALL_DIR  - Install directory (default: /usr/local/bin)
#
# Usage:
#   HIRSEL_AGENT=claude ./setup-worker.sh
#   HIRSEL_AGENT=claude HIRSEL_TAG=v0.1.0 ./setup-worker.sh
#
# Or via curl:
#   curl -fsSL https://raw.githubusercontent.com/SamGalanakis/hirsel/main/scripts/setup-worker.sh | HIRSEL_AGENT=claude bash
#

set -e

REPO="SamGalanakis/hirsel"
INSTALL_DIR="${HIRSEL_INSTALL_DIR:-/usr/local/bin}"
AGENT="${HIRSEL_AGENT:-}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }
step() { echo -e "${CYAN}[STEP]${NC} $1"; }

# Validate agent
validate_agent() {
    case "$AGENT" in
        claude|CLAUDE)
            AGENT="claude"
            ;;
        "")
            error "HIRSEL_AGENT is required. Supported agents: claude"
            ;;
        *)
            error "Unsupported agent: ${AGENT}. Supported agents: claude"
            ;;
    esac
}

# Install hirsel worker binary
install_hirsel() {
    step "Installing hirsel worker binary..."

    # Check if already installed (e.g., mounted from host in docker)
    if command -v hirsel-worker &>/dev/null; then
        local current_version
        current_version=$(hirsel-worker --version 2>&1 || echo "unknown")
        info "Found existing hirsel-worker: ${current_version}, skipping download"
        return 0
    fi

    # Download and run install script
    local install_script="https://raw.githubusercontent.com/${REPO}/main/scripts/install-hirsel-worker.sh"

    if command -v curl &>/dev/null; then
        curl -fsSL "$install_script" | HIRSEL_TAG="$HIRSEL_TAG" HIRSEL_INSTALL_DIR="$INSTALL_DIR" bash
    elif command -v wget &>/dev/null; then
        wget -qO- "$install_script" | HIRSEL_TAG="$HIRSEL_TAG" HIRSEL_INSTALL_DIR="$INSTALL_DIR" bash
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

# Install Claude CLI
install_claude() {
    step "Installing Claude CLI..."

    if command -v claude &>/dev/null; then
        local current_version
        current_version=$(claude --version 2>&1 || echo "unknown")
        info "Found existing Claude CLI: ${current_version}"
        return 0
    fi

    info "Downloading Claude CLI installer..."
    curl -fsSL https://claude.ai/install.sh | bash

    # Verify installation - Claude may install to ~/.local/bin or ~/.claude/local/bin
    if ! command -v claude &>/dev/null; then
        if [ -f "$HOME/.local/bin/claude" ]; then
            warn "Claude installed but not in PATH. Add to PATH: export PATH=\"\$HOME/.local/bin:\$PATH\""
        elif [ -f "$HOME/.claude/local/bin/claude" ]; then
            warn "Claude installed but not in PATH. Add to PATH: export PATH=\"\$HOME/.claude/local/bin:\$PATH\""
        else
            error "Claude CLI installation failed"
        fi
    else
        info "Claude CLI installed: $(claude --version 2>&1 || echo 'ok')"
    fi
}

# Install agent CLI based on HIRSEL_AGENT
install_agent() {
    case "$AGENT" in
        claude)
            install_claude
            ;;
    esac
}

# Verify setup
verify_setup() {
    step "Verifying setup..."

    local errors=0

    # Check hirsel-worker
    if command -v hirsel-worker &>/dev/null; then
        info "hirsel-worker: $(hirsel-worker --version 2>&1)"
    else
        warn "hirsel-worker not in PATH"
        errors=$((errors + 1))
    fi

    # Check agent
    case "$AGENT" in
        claude)
            if command -v claude &>/dev/null; then
                info "claude: $(claude --version 2>&1)"
            elif [ -f "$HOME/.local/bin/claude" ]; then
                info "claude: installed at ~/.local/bin/claude (add to PATH)"
            elif [ -f "$HOME/.claude/local/bin/claude" ]; then
                info "claude: installed at ~/.claude/local/bin/claude (add to PATH)"
            else
                warn "claude CLI not found"
                errors=$((errors + 1))
            fi
            ;;
    esac

    if [ $errors -gt 0 ]; then
        warn "Setup completed with warnings. You may need to update your PATH."
    else
        info "Setup complete!"
    fi
}

main() {
    echo -e "${CYAN}========================================${NC}"
    echo -e "${CYAN}  Hirsel Worker Setup${NC}"
    echo -e "${CYAN}========================================${NC}"
    echo

    validate_agent
    info "Agent: ${AGENT}"
    [ -n "$HIRSEL_TAG" ] && info "Hirsel version: ${HIRSEL_TAG}"
    echo

    install_hirsel
    echo

    install_agent
    echo

    verify_setup
}

main "$@"
