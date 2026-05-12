#!/bin/bash
# macOS post-build: codesign, notarize, staple, Tauri updater sig, upload to OSS.
#
# Workflow:
#   1. CI builds unsigned artifacts on GitHub-hosted runner
#   2. Download artifacts: gh run download <run-id> -n macos-arm64-unsigned -D build/
#   3. Run this script
#
# Usage:
#   bash scripts/sign-and-upload-macos.sh <version> <release|beta> [arch]
#
# Examples:
#   bash scripts/sign-and-upload-macos.sh 0.5.22 beta            # arm64 (default)
#   bash scripts/sign-and-upload-macos.sh 0.5.22 release x86_64  # Intel
#
# Prerequisites:
#   - Developer ID Application certificate in login keychain
#   - APPLE_ID, APPLE_PASSWORD (app-specific), APPLE_TEAM_ID env vars
#   - TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD env vars
#   - OSS_ACCESS_KEY_ID, OSS_ACCESS_KEY_SECRET env vars
#   - Python with oss2: pip3 install oss2

set -euo pipefail

if [ $# -lt 2 ]; then
    echo "Usage: bash scripts/sign-and-upload-macos.sh <version> <release|beta> [arch]"
    echo ""
    echo "Examples:"
    echo "  bash scripts/sign-and-upload-macos.sh 0.5.22 beta"
    echo "  bash scripts/sign-and-upload-macos.sh 0.5.22 release x86_64"
    exit 1
fi

VERSION="$1"
RELEASE_TYPE="$2"
ARCH="${3:-aarch64}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Determine artifact directory — look in build/ (downloaded) or src-tauri/target/
if [ "$ARCH" = "x86_64" ]; then
    ARTIFACT_SUFFIX="x64"
    # Try downloaded artifact first
    BUILD_DIR="$PROJECT_DIR/build/macos-x64-unsigned"
    if [ ! -d "$BUILD_DIR" ]; then
        BUILD_DIR="$PROJECT_DIR/build"
    fi
else
    ARTIFACT_SUFFIX="aarch64"
    BUILD_DIR="$PROJECT_DIR/build/macos-arm64-unsigned"
    if [ ! -d "$BUILD_DIR" ]; then
        BUILD_DIR="$PROJECT_DIR/build"
    fi
fi

DMG_NAME="AIjia_${VERSION}_${ARTIFACT_SUFFIX}.dmg"

# Find DMG
DMG=$(find "$BUILD_DIR" -name "$DMG_NAME" -type f 2>/dev/null | head -1)
if [ -z "$DMG" ]; then
    echo "ERROR: Cannot find $DMG_NAME in $BUILD_DIR"
    echo ""
    echo "Download artifacts first:"
    echo "  gh run download <run-id> -n macos-${ARCH/aarch64/arm64}-unsigned -D build/"
    exit 1
fi

# Find .app
APP=$(find "$BUILD_DIR" -name "AIjia.app" -type d 2>/dev/null | head -1)

echo "=== AIjia macOS Sign & Upload ==="
echo "Version: $VERSION ($RELEASE_TYPE)"
echo "Arch: $ARCH"
echo "DMG: $DMG"
echo "App: ${APP:-not found}"
echo ""

# ── 1. Code sign ──
echo "=== Step 1: Code sign ==="
# Check for signing identity
IDENTITY=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed 's/.*"\(.*\)"/\1/')
if [ -z "$IDENTITY" ]; then
    echo "ERROR: No Developer ID Application certificate found"
    echo "  Run: security find-identity -v -p codesigning"
    exit 1
fi
echo "  Identity: $IDENTITY"

if [ -n "$APP" ]; then
    echo "  Signing .app..."
    codesign --force --deep --options runtime --sign "$IDENTITY" "$APP"
    echo "  .app signed"
fi

echo "  Signing .dmg..."
codesign --force --sign "$IDENTITY" "$DMG"
echo "  .dmg signed"

# ── 2. Notarize ──
echo ""
echo "=== Step 2: Notarize ==="
if [ -z "${APPLE_ID:-}" ]; then
    echo "ERROR: APPLE_ID env var not set"
    exit 1
fi

echo "  Notarizing DMG..."
xcrun notarytool submit "$DMG" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait --timeout 30m
xcrun stapler staple "$DMG"
echo "  DMG notarized + stapled"

if [ -n "$APP" ]; then
    echo "  Notarizing .app..."
    APP_ZIP=$(mktemp /tmp/AIjia-notarize-XXXX.zip)
    ditto -c -k --keepParent "$APP" "$APP_ZIP"
    xcrun notarytool submit "$APP_ZIP" \
        --apple-id "$APPLE_ID" \
        --password "$APPLE_PASSWORD" \
        --team-id "$APPLE_TEAM_ID" \
        --wait --timeout 30m
    xcrun stapler staple "$APP"
    rm -f "$APP_ZIP"
    echo "  .app notarized + stapled"
fi

# ── 3. Re-package updater artifacts + Tauri signer ──
echo ""
echo "=== Step 3: Re-package updater tar.gz + Tauri signer ==="
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    echo "ERROR: TAURI_SIGNING_PRIVATE_KEY env var not set"
    exit 1
fi

if [ -n "$APP" ]; then
    TAR_DIR="$(dirname "$APP")"
    TAR="$TAR_DIR/AIjia.app.tar.gz"
    SIG="$TAR.sig"
    rm -f "$TAR" "$SIG"
    tar czf "$TAR" -C "$TAR_DIR" "AIjia.app"

    # Use global tauri-cli or npx
    if command -v tauri &>/dev/null; then
        tauri signer sign "$TAR"
    else
        npx --yes @tauri-apps/cli@latest signer sign "$TAR"
    fi
    echo "  Updater artifacts ready: $TAR + $SIG"
else
    echo "  Skipped (no .app found, DMG-only)"
fi

# ── 4. Upload to OSS ──
echo ""
echo "=== Step 4: Upload to OSS ==="
if [ -z "${OSS_ACCESS_KEY_ID:-}" ]; then
    echo "ERROR: OSS_ACCESS_KEY_ID env var not set"
    exit 1
fi

python3 -m pip install --disable-pip-version-check --user --break-system-packages oss2 2>/dev/null || true

# Point upload script at the build/ directory where signed artifacts live
# The upload script looks for bundle/dmg/ and bundle/macos/ subdirectories
# We need to create the expected structure
UPLOAD_DIR=$(mktemp -d /tmp/aijia-upload-XXXX)
mkdir -p "$UPLOAD_DIR/dmg" "$UPLOAD_DIR/macos"
cp "$DMG" "$UPLOAD_DIR/dmg/"
if [ -n "$APP" ]; then
    cp "$TAR" "$UPLOAD_DIR/macos/"
    cp "$SIG" "$UPLOAD_DIR/macos/"
fi

export BUNDLE_DIR="$UPLOAD_DIR"

if [ "$RELEASE_TYPE" = "beta" ]; then
    python3 "$SCRIPT_DIR/ci-upload-macos-beta.py" "$VERSION" "$ARCH"
else
    python3 "$SCRIPT_DIR/ci-upload-macos.py" "$VERSION" "$ARCH"
fi

rm -rf "$UPLOAD_DIR"

echo ""
echo "=== Done ==="
echo "macOS $ARCH v$VERSION ($RELEASE_TYPE) signed, notarized, and uploaded to OSS."
