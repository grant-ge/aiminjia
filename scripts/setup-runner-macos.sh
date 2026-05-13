#!/bin/bash
# Setup macOS self-hosted GitHub Actions runner for AIjia
# Run this script once on a new Mac build machine.
#
# Prerequisites:
#   - macOS with Apple Silicon (ARM64)
#   - Developer ID Application certificate in login keychain
#   - Xcode Command Line Tools installed
#
# Usage: bash scripts/setup-runner-macos.sh

set -e

echo "=== AIjia macOS Runner Setup ==="
echo ""

# ── 1. Xcode CLI tools ──
echo "[1/6] Checking Xcode CLI tools..."
if xcode-select -p &>/dev/null; then
    echo "  Xcode CLI tools found: $(xcode-select -p)"
else
    echo "  Installing Xcode CLI tools..."
    xcode-select --install
    echo "  Please complete the installation dialog, then re-run this script."
    exit 1
fi

# ── 2. Node.js ──
echo "[2/6] Checking Node.js..."
if command -v node &>/dev/null; then
    echo "  Node.js $(node --version) found"
else
    echo "  Node.js not found. Install via: brew install node@20"
fi

# ── 3. pnpm ──
echo "[3/6] Checking pnpm..."
if command -v pnpm &>/dev/null; then
    echo "  pnpm $(pnpm --version) found"
else
    echo "  Installing pnpm..."
    npm install -g pnpm@9
fi

# ── 4. Rust ──
echo "[4/6] Checking Rust..."
if command -v cargo &>/dev/null; then
    echo "  $(rustc --version) found"
    echo "  Adding x86_64 target for Intel cross-compile..."
    rustup target add x86_64-apple-darwin 2>/dev/null || true
else
    echo "  Rust not found. Installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    rustup target add x86_64-apple-darwin
fi

# ── 5. Python + oss2 ──
echo "[5/6] Checking Python..."
if command -v python3 &>/dev/null; then
    echo "  $(python3 --version) found"
    echo "  Installing oss2..."
    python3 -m pip install --disable-pip-version-check --user --break-system-packages oss2 2>/dev/null || true
else
    echo "  Python3 not found. Install via: brew install python"
fi

# ── 6. Code signing identity ──
echo "[6/6] Checking code signing identity..."
if security find-identity -v -p codesigning 2>/dev/null | grep -q "Developer ID Application"; then
    echo "  Developer ID Application certificate found:"
    security find-identity -v -p codesigning | grep "Developer ID Application"
else
    echo "  WARNING: No Developer ID Application certificate found in keychain."
    echo "  Import your .p12 certificate into login keychain first."
fi

# ── Summary ──
echo ""
echo "=== GitHub Secrets Required ==="
echo "Set these in: GitHub repo -> Settings -> Secrets and variables -> Actions"
echo ""
echo "  MACOS_KEYCHAIN_PASSWORD  - Mac login password (to unlock keychain)"
echo "  APPLE_ID                 - Apple Developer account email"
echo "  APPLE_PASSWORD           - App-Specific Password (appleid.apple.com)"
echo "  APPLE_TEAM_ID            - Apple Developer Team ID"
echo "  TAURI_SIGNING_PRIVATE_KEY          - Tauri updater Ed25519 key"
echo "  TAURI_SIGNING_PRIVATE_KEY_PASSWORD - Ed25519 key password"
echo "  OSS_ACCESS_KEY_ID        - Aliyun OSS Access Key"
echo "  OSS_ACCESS_KEY_SECRET    - Aliyun OSS Secret Key"
echo ""
echo "=== GitHub Actions Runner ==="
echo "Register at: GitHub repo -> Settings -> Actions -> Runners -> New self-hosted runner"
echo "Labels: macOS, ARM64"
echo "Run: ./run.sh (foreground) or ./svc.sh install && ./svc.sh start (service)"
echo ""
echo "=== Important Notes ==="
echo "- Mac must be logged in for keychain access (LaunchAgent mode)"
echo "- After changing Mac password, update MACOS_KEYCHAIN_PASSWORD secret"
echo "- CI does NOT configure network proxy — ensure machine can reach GitHub, npm, etc."
echo ""
echo "Setup complete!"
