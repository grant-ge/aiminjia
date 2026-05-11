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

# Detect platform. TARGET_ARCH env overrides for cross-arch bundling
# (e.g. building x86_64 dmg on an arm64 Mac).
OS="$(uname -s)"
ARCH="${TARGET_ARCH:-$(uname -m)}"

echo "Platform: $OS $ARCH"

# Map to dws GitHub release asset naming
case "$OS" in
    Darwin)
        case "$ARCH" in
            arm64)   DWS_ASSET="dws-darwin-arm64.tar.gz" ;;
            x86_64)  DWS_ASSET="dws-darwin-amd64.tar.gz" ;;
            *) echo "[error] unsupported arch: $ARCH"; exit 1 ;;
        esac
        ;;
    Linux)
        case "$ARCH" in
            x86_64)  DWS_ASSET="dws-linux-amd64.tar.gz" ;;
            aarch64) DWS_ASSET="dws-linux-arm64.tar.gz" ;;
            *) echo "[error] unsupported arch: $ARCH"; exit 1 ;;
        esac
        ;;
    *) echo "[error] unsupported OS: $OS"; exit 1 ;;
esac

# Method 1: pull dingtalk-workspace-cli npm package — bundles all arch binaries
# in assets/ (dws-darwin-amd64.tar.gz / dws-darwin-arm64.tar.gz / etc), so we
# can pick the target arch without needing direct GitHub access (which times
# out from CN networks). npmmirror is a cached mirror of the npm registry.
DWS_NPM_REGISTRY="${DWS_NPM_REGISTRY:-https://registry.npmmirror.com}"
echo "Pulling dingtalk-workspace-cli from $DWS_NPM_REGISTRY..."
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT
if (cd "$TEMP_DIR" && npm pack dingtalk-workspace-cli --registry="$DWS_NPM_REGISTRY" >/dev/null 2>&1); then
    NPM_TGZ=$(ls "$TEMP_DIR"/dingtalk-workspace-cli-*.tgz 2>/dev/null | head -1)
    if [ -n "$NPM_TGZ" ]; then
        tar xzf "$NPM_TGZ" -C "$TEMP_DIR"
        ASSET_PATH="$TEMP_DIR/package/assets/$DWS_ASSET"
        if [ -f "$ASSET_PATH" ]; then
            mkdir -p "$TEMP_DIR/extract"
            tar xzf "$ASSET_PATH" -C "$TEMP_DIR/extract"
            DWS_BIN=$(find "$TEMP_DIR/extract" -type f -name 'dws' | head -1)
            if [ -n "$DWS_BIN" ]; then
                cp "$DWS_BIN" "$RESOURCES_DIR/dws"
                chmod +x "$RESOURCES_DIR/dws"
                echo "✅ dws ($ARCH) copied to $RESOURCES_DIR/dws"
                file "$RESOURCES_DIR/dws"
                exit 0
            fi
        else
            echo "[warn] asset $DWS_ASSET not found in npm package; falling back"
        fi
    fi
fi

# Method 2: Direct GitHub release download (fallback; can be slow from CN)
DWS_RELEASE_URL="https://github.com/DingTalk-Real-AI/dingtalk-workspace-cli/releases/latest/download/$DWS_ASSET"
echo "Downloading $DWS_ASSET from GitHub..."
if curl -fL --connect-timeout 30 -o "$TEMP_DIR/dws.tar.gz" "$DWS_RELEASE_URL"; then
    tar xzf "$TEMP_DIR/dws.tar.gz" -C "$TEMP_DIR"
    DWS_BIN=$(find "$TEMP_DIR" -type f -name 'dws' | head -1)
    if [ -n "$DWS_BIN" ]; then
        cp "$DWS_BIN" "$RESOURCES_DIR/dws"
        chmod +x "$RESOURCES_DIR/dws"
        echo "✅ dws copied to $RESOURCES_DIR/dws"
        file "$RESOURCES_DIR/dws"
        exit 0
    fi
fi

# Method 3 (fallback): official install script — ONLY for native-arch builds
# (script uses `uname -m` unconditionally and may get the wrong arch on cross-builds)
if [ "$ARCH" = "$(uname -m)" ]; then
    echo "Installing dws via official installer (fallback)..."
    INSTALL_SCRIPT_URL="https://raw.githubusercontent.com/DingTalk-Real-AI/dingtalk-workspace-cli/main/scripts/install.sh"
    if curl -fsSL "$INSTALL_SCRIPT_URL" -o "$TEMP_DIR/install.sh"; then
        bash "$TEMP_DIR/install.sh"
        for candidate in "$HOME/.dws/bin/dws" "$HOME/.local/bin/dws" "/usr/local/bin/dws"; do
            if [ -f "$candidate" ]; then
                cp "$candidate" "$RESOURCES_DIR/dws"
                chmod +x "$RESOURCES_DIR/dws"
                echo "✅ dws copied to $RESOURCES_DIR/dws"
                "$RESOURCES_DIR/dws" --version
                exit 0
            fi
        done
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
