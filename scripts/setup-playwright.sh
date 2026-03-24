#!/usr/bin/env bash
# Setup Playwright runtime for AI小家
# Downloads Node.js and installs Playwright with Chromium

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RUNTIME_DIR="$PROJECT_DIR/src-tauri/playwright-runtime"
NODE_VERSION="20.18.0"

echo "=== Setting up Playwright runtime ==="

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64) NODE_PLATFORM="darwin-arm64" ;;
      x86_64) NODE_PLATFORM="darwin-x64" ;;
      *) echo "Unsupported arch: $ARCH"; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64) NODE_PLATFORM="linux-x64" ;;
      aarch64) NODE_PLATFORM="linux-arm64" ;;
      *) echo "Unsupported arch: $ARCH"; exit 1 ;;
    esac
    ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

NODE_DIR="$RUNTIME_DIR/node"
NODE_URL="https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-${NODE_PLATFORM}.tar.gz"

# Download Node.js if not present
if [ ! -f "$NODE_DIR/bin/node" ]; then
  echo "Downloading Node.js v${NODE_VERSION} for ${NODE_PLATFORM}..."
  mkdir -p "$NODE_DIR"
  curl -fsSL "$NODE_URL" | tar xz --strip-components=1 -C "$NODE_DIR"
  echo "Node.js installed: $("$NODE_DIR/bin/node" --version)"
else
  echo "Node.js already installed: $("$NODE_DIR/bin/node" --version)"
fi

# Install npm dependencies
echo "Installing npm dependencies..."
cd "$RUNTIME_DIR"
"$NODE_DIR/bin/node" "$NODE_DIR/bin/npm" install --production 2>&1 | tail -5

# Install Playwright Chromium browser
echo "Installing Playwright Chromium..."
PLAYWRIGHT_BROWSERS_PATH="$RUNTIME_DIR/browsers" "$NODE_DIR/bin/npx" playwright install chromium 2>&1 | tail -5

echo ""
echo "=== Playwright runtime ready ==="
echo "Node: $NODE_DIR/bin/node"
echo "Browsers: $RUNTIME_DIR/browsers/"
echo "Entry: $RUNTIME_DIR/browser.js"
