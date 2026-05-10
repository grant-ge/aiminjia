#!/usr/bin/env bash
# Setup Playwright runtime for AI小家
# Downloads Node.js and installs Playwright with Chromium

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RUNTIME_DIR="$PROJECT_DIR/src-tauri/playwright-runtime"
NODE_VERSION="20.18.0"

echo "=== Setting up Playwright runtime ==="

# Detect platform — TARGET_ARCH env overrides for cross-arch builds
# (e.g. building x86_64 dmg on an arm64 Mac). When TARGET_ARCH differs from
# the host arch, we can't execute the downloaded node locally to run npm +
# playwright install; instead we:
#   1. Run npm + playwright install using the host's node (already present)
#   2. Then swap node/bin/node for the target-arch binary at the end
# This works because node_modules is pure JS and Chromium is downloaded per
# PLAYWRIGHT_DOWNLOAD_HOST_PLATFORM (see below).
OS="$(uname -s)"
HOST_ARCH="$(uname -m)"
ARCH="${TARGET_ARCH:-$HOST_ARCH}"
CROSS_ARCH=false
if [ "$ARCH" != "$HOST_ARCH" ]; then
  CROSS_ARCH=true
  echo "Cross-arch build: HOST=$HOST_ARCH TARGET=$ARCH"
fi

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

# Download Node.js for the TARGET arch (gets bundled into the app)
if [ ! -f "$NODE_DIR/bin/node" ] || $CROSS_ARCH; then
  echo "Downloading Node.js v${NODE_VERSION} for ${NODE_PLATFORM}..."
  rm -rf "$NODE_DIR"
  mkdir -p "$NODE_DIR"
  curl -fsSL "$NODE_URL" | tar xz --strip-components=1 -C "$NODE_DIR"
fi

# Choose how to invoke node for npm/npx during setup. node_modules is pure JS
# and Chromium is downloaded by playwright based on process.arch — so to get
# the correct Intel Chromium when cross-building x86_64 on an arm64 Mac, we
# need to run node under Rosetta. arch -x86_64 transparently re-runs an
# x86_64 binary on Apple Silicon (Rosetta 2 is shipped by default since 11.0).
if $CROSS_ARCH && [ "$OS" = "Darwin" ] && [ "$HOST_ARCH" = "arm64" ] && [ "$ARCH" = "x86_64" ]; then
  if ! /usr/bin/arch -x86_64 /usr/bin/true 2>/dev/null; then
    echo "[error] cross-build x86_64 requires Rosetta 2 (run: softwareupdate --install-rosetta)"
    exit 1
  fi
  RUN="/usr/bin/arch -x86_64"
  echo "Using Rosetta 2 to run x86_64 node for npm/playwright install"
elif ! $CROSS_ARCH; then
  RUN=""
else
  echo "[error] unsupported cross-arch target on this host: HOST=$HOST_ARCH TARGET=$ARCH OS=$OS"
  exit 1
fi
NPM_NODE="$NODE_DIR/bin/node"
NPM_NPM="$NODE_DIR/bin/npm"
NPM_NPX="$NODE_DIR/bin/npx"

# Install npm dependencies
echo "Installing npm dependencies..."
cd "$RUNTIME_DIR"
$RUN "$NPM_NODE" "$NPM_NPM" install --production 2>&1 | tail -5

# Install Playwright Chromium for the target arch.
# On macOS, playwright detects Apple Silicon via os.cpus()[].model.includes("Apple"),
# which returns true even under Rosetta 2 — so we must override via the public
# PLAYWRIGHT_HOST_PLATFORM_OVERRIDE hook to force the Intel chromium download.
echo "Installing Playwright Chromium for $OS/$ARCH..."
PLAYWRIGHT_OVERRIDE=""
if $CROSS_ARCH && [ "$OS" = "Darwin" ] && [ "$ARCH" = "x86_64" ]; then
  PLAYWRIGHT_OVERRIDE="mac15"  # mac15 = macOS 14+, Intel (no -arm64 suffix)
fi
env PLAYWRIGHT_HOST_PLATFORM_OVERRIDE="$PLAYWRIGHT_OVERRIDE" \
    PLAYWRIGHT_BROWSERS_PATH="$RUNTIME_DIR/browsers" \
    $RUN "$NPM_NPX" playwright install chromium 2>&1 | tail -5

echo ""
echo "=== Playwright runtime ready ==="
echo "Node: $NODE_DIR/bin/node"
echo "Browsers: $RUNTIME_DIR/browsers/"
echo "Entry: $RUNTIME_DIR/browser.js"
