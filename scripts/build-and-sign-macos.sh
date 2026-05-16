#!/bin/bash
# macOS local end-to-end release: build → sign → notarize → staple → upload.
#
# Runs arm64 and x86_64 SERIALLY (Playwright/dws runtime symlinks are swapped
# per arch via TARGET_ARCH=x86_64; parallel builds would clobber each other).
#
# Usage:
#   bash scripts/build-and-sign-macos.sh <version> <beta|release>
#
# Example:
#   bash scripts/build-and-sign-macos.sh 0.5.22 beta
#
# To re-run only one arch after a failure, call sign-and-upload-macos.sh
# directly:
#   bash scripts/sign-and-upload-macos.sh 0.5.22 beta            # arm64
#   bash scripts/sign-and-upload-macos.sh 0.5.22 beta x86_64     # Intel
#
# Prerequisites: same as sign-and-upload-macos.sh (Developer ID cert + APPLE_*
# + TAURI_SIGNING_* + OSS_* env vars).

set -euo pipefail

if [ $# -lt 2 ]; then
    echo "Usage: bash scripts/build-and-sign-macos.sh <version> <beta|release>"
    exit 1
fi

VERSION="$1"
RELEASE_TYPE="$2"

if [ "$RELEASE_TYPE" != "beta" ] && [ "$RELEASE_TYPE" != "release" ]; then
    echo "ERROR: release type must be 'beta' or 'release', got: $RELEASE_TYPE"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

echo "=== AIjia macOS local build + sign + upload ==="
echo "Version: $VERSION ($RELEASE_TYPE)"
echo "Project: $PROJECT_DIR"
echo ""

# Auto-source local env file if present. Avoids "TAURI_SIGNING_PRIVATE_KEY
# not set" failures when the caller forgot to `source .env.local.aijia`.
# Variables already in the environment are NOT overridden.
ENV_FILE="$PROJECT_DIR/.env.local.aijia"
if [ -f "$ENV_FILE" ]; then
    echo "--- Sourcing $ENV_FILE (missing vars only) ---"
    set -a
    # shellcheck disable=SC1090
    . "$ENV_FILE"
    set +a
    echo ""
fi

# Fail fast if any required var is still unset, so we don't burn 30 min of
# build time only to fail at the sign step.
for v in TAURI_SIGNING_PRIVATE_KEY APPLE_ID APPLE_TEAM_ID APPLE_PASSWORD \
         OSS_ACCESS_KEY_ID OSS_ACCESS_KEY_SECRET; do
    if [ -z "${!v:-}" ]; then
        echo "ERROR: required env var $v is not set."
        echo "  Add it to $ENV_FILE or export it before running this script."
        exit 1
    fi
done

# Optional cleanup: nuke target/ before build to defeat stale-artifact bugs
# where a previous failed build leaves an old-version .app in
# src-tauri/target/release/bundle/macos/. sign-and-upload-macos.sh has a
# version pre-flight that fails closed if this happens, but it's quicker to
# clean up front when you know the prior run was bad.
#
#   CLEAN_BUILD=1 bash scripts/build-and-sign-macos.sh 0.5.24 release
if [ "${CLEAN_BUILD:-0}" = "1" ]; then
    echo "--- CLEAN_BUILD=1 → cargo clean ---"
    (cd "$PROJECT_DIR/src-tauri" && cargo clean)
    echo ""
fi

build_one_arch() {
    local arch="$1"        # aarch64 | x86_64
    local tauri_target=""  # extra --target flag for tauri build
    local target_arch=""   # TARGET_ARCH env for setup scripts

    if [ "$arch" = "x86_64" ]; then
        tauri_target="--target x86_64-apple-darwin"
        target_arch="x86_64"
    fi

    echo ""
    echo "############################################################"
    echo "# Building $arch"
    echo "############################################################"

    echo ""
    echo "--- Setup dws CLI ($arch) ---"
    DWS_NONINTERACTIVE=1 TARGET_ARCH="$target_arch" bash "$SCRIPT_DIR/setup-dws.sh"

    echo ""
    echo "--- Prepare bundled runtime ($arch) ---"
    # Stash the OTHER platform's runtime out of resources/ so tauri bundle
    # only ships the runtime that matches this build's CPU. Without this
    # the .app for arm64 would carry darwin-x64/ too (and vice versa),
    # nearly doubling installer size. Stash dir is restored by trap.
    local plat_keep plat_stash
    if [ "$arch" = "x86_64" ]; then
        plat_keep="darwin-x64"
        plat_stash="darwin-arm64"
    else
        plat_keep="darwin-arm64"
        plat_stash="darwin-x64"
    fi
    PLATFORM="$plat_keep" bash "$SCRIPT_DIR/prepare-bundled-runtime.sh"

    local stash_src="$PROJECT_DIR/src-tauri/resources/runtime/$plat_stash"
    local stash_tmp=""
    if [ -d "$stash_src" ]; then
        stash_tmp="$(mktemp -d "/tmp/aijia-runtime-stash-XXXX")"
        echo "  stashing $plat_stash -> $stash_tmp"
        mv "$stash_src" "$stash_tmp/"
        # Restore on any exit path so a failed build doesn't lose the stash
        trap "[ -n '$stash_tmp' ] && [ -d '$stash_tmp/$plat_stash' ] && mv '$stash_tmp/$plat_stash' '$stash_src' && rmdir '$stash_tmp' 2>/dev/null; true" RETURN
    fi

    echo ""
    echo "--- pnpm tauri build $tauri_target ---"
    # shellcheck disable=SC2086
    pnpm tauri build $tauri_target

    echo ""
    echo "--- Sign + notarize + upload ($arch) ---"
    bash "$SCRIPT_DIR/sign-and-upload-macos.sh" "$VERSION" "$RELEASE_TYPE" "$arch"
}

# Run arm64 first (default tauri target on Apple Silicon), then x86_64.
# Serial only — DO NOT parallelize (Playwright/dws symlinks).
#
# Set ARCH=aarch64 or ARCH=x86_64 to only build one arch (useful when the
# other arch already succeeded and you need to retry the failed half).
case "${ARCH:-both}" in
    both)
        build_one_arch aarch64
        build_one_arch x86_64
        ;;
    aarch64|arm64)
        build_one_arch aarch64
        ;;
    x86_64|x64|intel)
        build_one_arch x86_64
        ;;
    *)
        echo "ERROR: ARCH must be 'aarch64', 'x86_64', or unset (both). Got: $ARCH"
        exit 1
        ;;
esac

echo ""
echo "=== All done ==="
echo "macOS arm64 + x86_64 v$VERSION ($RELEASE_TYPE) built, signed, notarized, uploaded."
