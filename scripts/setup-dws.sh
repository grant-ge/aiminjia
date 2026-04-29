#!/usr/bin/env bash
# Setup DingTalk Workspace CLI (dws) for AI小家
# Downloads the dws binary and places it in src-tauri/resources/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
RESOURCES_DIR="$PROJECT_DIR/src-tauri/resources"

echo "=== Setting up DingTalk Workspace CLI (dws) ==="

# Create resources directory
mkdir -p "$RESOURCES_DIR"

# Check if dws already exists
if [ -f "$RESOURCES_DIR/dws" ]; then
    EXISTING_VERSION=$("$RESOURCES_DIR/dws" --version 2>/dev/null || echo "unknown")
    echo "dws already installed: $EXISTING_VERSION"
    if [ "${CI:-}" = "true" ] || [ "${DWS_NONINTERACTIVE:-}" = "1" ]; then
        echo "Non-interactive mode, skipping reinstall."
        exit 0
    fi
    read -p "Reinstall? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Skipping install."
        exit 0
    fi
fi

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "Platform: $OS $ARCH"

# Method 1: Install via official install script (preferred)
echo "Installing dws via official installer..."
INSTALL_SCRIPT_URL="https://raw.githubusercontent.com/DingTalk-Real-AI/dingtalk-workspace-cli/main/scripts/install.sh"

# Download to temp and install
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

if curl -fsSL "$INSTALL_SCRIPT_URL" -o "$TEMP_DIR/install.sh" 2>/dev/null; then
    # Run installer (installs to ~/.dws/bin/dws by default)
    bash "$TEMP_DIR/install.sh"

    # Find the installed binary
    DWS_BIN=""
    for candidate in "$HOME/.dws/bin/dws" "$HOME/.local/bin/dws" "/usr/local/bin/dws"; do
        if [ -f "$candidate" ]; then
            DWS_BIN="$candidate"
            break
        fi
    done

    if [ -n "$DWS_BIN" ]; then
        cp "$DWS_BIN" "$RESOURCES_DIR/dws"
        chmod +x "$RESOURCES_DIR/dws"
        echo "✅ dws copied to $RESOURCES_DIR/dws"
        "$RESOURCES_DIR/dws" --version
        exit 0
    fi
fi

# Method 2: Try npm global install
echo "Trying npm install..."
if command -v npm &>/dev/null; then
    npm install -g dingtalk-workspace-cli 2>/dev/null || true
    DWS_NPM=$(command -v dws 2>/dev/null || true)
    if [ -n "$DWS_NPM" ]; then
        cp "$DWS_NPM" "$RESOURCES_DIR/dws"
        chmod +x "$RESOURCES_DIR/dws"
        echo "✅ dws (npm) copied to $RESOURCES_DIR/dws"
        "$RESOURCES_DIR/dws" --version
        exit 0
    fi
fi

# Method 3: Manual instructions
echo ""
echo "❌ Could not auto-install dws."
echo ""
echo "Please install manually:"
echo "  curl -fsSL https://raw.githubusercontent.com/DingTalk-Real-AI/dingtalk-workspace-cli/main/scripts/install.sh | sh"
echo ""
echo "Then copy the binary:"
echo "  cp \$(which dws) $RESOURCES_DIR/dws"
echo ""
exit 1
