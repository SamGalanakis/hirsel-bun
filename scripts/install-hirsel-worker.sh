#!/bin/bash
#
# Install hirsel worker binary from GitHub releases
#
# Environment variables:
#   HIRSEL_TAG          - Git tag (e.g., "v0.1.0"). If not set, uses latest release.
#   HIRSEL_BINARY_TYPE  - Binary type: worker, server, cli (default: worker)
#   HIRSEL_INSTALL_DIR  - Install directory (default: /usr/local/bin)
#
# Usage:
#   ./install-hirsel-worker.sh                    # Install latest release
#   HIRSEL_TAG=v0.1.0 ./install-hirsel-worker.sh  # Install specific tag
#
# Or via curl:
#   curl -fsSL https://raw.githubusercontent.com/SamGalanakis/hirsel/main/scripts/install-hirsel-worker.sh | bash
#   curl -fsSL ... | HIRSEL_TAG=v0.1.0 bash
#

set -e

REPO="SamGalanakis/hirsel"
INSTALL_DIR="${HIRSEL_INSTALL_DIR:-/usr/local/bin}"
BINARY_TYPE="${HIRSEL_BINARY_TYPE:-worker}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Get latest release tag
get_latest_tag() {
    local tag
    tag=$(curl -sS "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    [ -z "$tag" ] && error "Failed to get latest release"
    echo "$tag"
}

# Download binary from release
download_release() {
    local tag="$1"
    local output="$2"

    # Extract version from tag (v0.1.0 -> 0.1.0)
    local version="${tag#v}"

    # Binary naming: hirsel-{type}-{version}-linux-amd64
    local asset="hirsel-${BINARY_TYPE}-${version}-linux-amd64"
    local url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

    info "Downloading ${asset}..."

    if ! curl -sS -L -f -o "$output" "$url"; then
        error "Failed to download from ${url}"
    fi
}

main() {
    local tag="${HIRSEL_TAG:-}"
    local tmp_binary="/tmp/hirsel-download-$$"

    if [ -z "$tag" ]; then
        tag=$(get_latest_tag)
        info "Latest release: ${tag}"
    fi

    info "Installing hirsel ${BINARY_TYPE} (${tag})"
    download_release "$tag" "$tmp_binary"

    chmod +x "$tmp_binary"

    # Verify it runs
    if ! "$tmp_binary" --version >/dev/null 2>&1; then
        warn "Binary verification failed - may not run on this platform"
    fi

    # Install
    info "Installing to ${INSTALL_DIR}/hirsel..."

    if [ -w "$INSTALL_DIR" ]; then
        mv "$tmp_binary" "${INSTALL_DIR}/hirsel"
    else
        sudo mv "$tmp_binary" "${INSTALL_DIR}/hirsel"
        sudo chmod +x "${INSTALL_DIR}/hirsel"
    fi

    info "Done: $("${INSTALL_DIR}/hirsel" --version 2>&1 || echo 'installed')"
}

main "$@"
