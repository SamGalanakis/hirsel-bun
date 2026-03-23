#!/bin/sh
set -eu

REPO="__REPO__"
TAG="__TAG__"
COMPONENT="${1:-server}"
INSTALL_DIR="${HIRSEL_INSTALL_DIR:-/usr/local/bin}"

case "$COMPONENT" in
  server|worker) ;;
  *)
    echo "usage: install_hirsel.sh [server|worker]" >&2
    exit 1
    ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) SUFFIX="linux-amd64" ;;
  *)
    echo "unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

ASSET="hirsel-${COMPONENT}-${SUFFIX}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"

has_cmd() {
  command -v "$1" >/dev/null 2>&1
}

download() {
  if has_cmd curl; then
    curl -fsSL "$1" -o "$2"
  elif has_cmd wget; then
    wget -qO "$2" "$1"
  else
    echo "curl or wget is required" >&2
    exit 1
  fi
}

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

mkdir -p "$INSTALL_DIR"
download "$URL" "$TMP_DIR/hirsel.tar.gz"
tar -xzf "$TMP_DIR/hirsel.tar.gz" -C "$TMP_DIR"
install -m 0755 "$TMP_DIR/hirsel" "$INSTALL_DIR/hirsel"

echo "Installed hirsel ${COMPONENT} to $INSTALL_DIR/hirsel"
