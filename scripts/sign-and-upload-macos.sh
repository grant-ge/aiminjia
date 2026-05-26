#!/bin/bash
# macOS post-build: codesign, notarize, staple, Tauri updater sig, upload to OSS.
#
# Workflow A (local build):
#   1. bash scripts/build-and-sign-macos.sh <version> <beta|release>
#      → builds + calls this script internally for each arch
#
# Workflow B (CI artifact download — deprecated for mac, kept for emergency):
#   1. Download artifacts: gh run download <run-id> -n macos-arm64-unsigned -D build/
#   2. Run this script
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

# Determine artifact directory — prefer downloaded CI artifact, fall back to
# local `pnpm tauri build` output under src-tauri/target/.
if [ "$ARCH" = "x86_64" ]; then
    ARTIFACT_SUFFIX="x64"
    CANDIDATES=(
        "$PROJECT_DIR/build/macos-x64-unsigned"
        "$PROJECT_DIR/src-tauri/target/x86_64-apple-darwin/release/bundle"
        "$PROJECT_DIR/build"
    )
else
    ARTIFACT_SUFFIX="aarch64"
    CANDIDATES=(
        "$PROJECT_DIR/build/macos-arm64-unsigned"
        "$PROJECT_DIR/src-tauri/target/release/bundle"
        "$PROJECT_DIR/build"
    )
fi

BUILD_DIR=""
for dir in "${CANDIDATES[@]}"; do
    if [ -d "$dir" ]; then
        BUILD_DIR="$dir"
        break
    fi
done
if [ -z "$BUILD_DIR" ]; then
    echo "ERROR: No artifact directory found. Tried:"
    for dir in "${CANDIDATES[@]}"; do echo "  - $dir"; done
    exit 1
fi

DMG_NAME="AIjia_${VERSION}_${ARTIFACT_SUFFIX}.dmg"

# Find .app — required.
APP=$(find "$BUILD_DIR" -name "AIjia.app" -type d 2>/dev/null | head -1)
if [ -z "$APP" ]; then
    echo "ERROR: Cannot find AIjia.app in $BUILD_DIR"
    echo ""
    echo "Either build locally (bash scripts/build-and-sign-macos.sh) or"
    echo "download CI artifacts:"
    echo "  gh run download <run-id> -n macos-${ARCH/aarch64/arm64}-unsigned -D build/"
    exit 1
fi

# DMG is now optional. tauri.conf.json bundles only "app" (+nsis for Windows);
# bundle_dmg.sh has been unreliable for months. Step 1b will build the DMG
# from the signed .app via hdiutil. If a prior DMG exists (placeholder, old
# build, or CI-produced), we still find it and let Step 1b decide whether
# to rebuild via `is_dmg_built_from_signed_app`.
DMG=$(find "$BUILD_DIR" -name "$DMG_NAME" -type f 2>/dev/null | head -1)
if [ -z "$DMG" ]; then
    # Auto-create empty placeholder so downstream `rm -f $DMG; hdiutil create`
    # has a known path. Step 1b will overwrite it.
    DMG_DIR="$BUILD_DIR/dmg"
    mkdir -p "$DMG_DIR"
    DMG="$DMG_DIR/$DMG_NAME"
    PH_STAGE=$(mktemp -d /tmp/aijia-ph-XXXX)
    echo "x" > "$PH_STAGE/x.txt"
    hdiutil create -volname "stub" -srcfolder "$PH_STAGE" -ov -format UDZO "$DMG" >/dev/null 2>&1
    rm -rf "$PH_STAGE"
    echo "  Note: no DMG produced by tauri build — auto-created placeholder at $DMG (Step 1b will rebuild from signed .app)"
fi

# Auto-detach any leftover /Volumes/AIjia* mounts and aijia-related attached
# images. hdiutil create silently fails ("Operation not permitted") if a
# stale image is still attached with the same volname.
ls /Volumes/ 2>/dev/null | grep -i "^aijia" | while IFS= read -r v; do
    hdiutil detach "/Volumes/$v" -force >/dev/null 2>&1 || true
done || true
hdiutil info 2>/dev/null | awk '
    /^image-path/ { is_aijia = (tolower($0) ~ /aijia/) }
    is_aijia && /^\/dev\/disk[0-9]+\t/ { print $1 }
' | while IFS= read -r disk; do
    hdiutil detach "$disk" -force >/dev/null 2>&1 || true
done || true

echo "=== AIjia macOS Sign & Upload ==="
echo "Version: $VERSION ($RELEASE_TYPE)"
echo "Arch: $ARCH"
echo "DMG: $DMG"
echo "App: ${APP:-not found}"
echo ""

# ── Pre-flight: verify .app version matches expected (#7) ────────────────
# Guards against stale src-tauri/target/ from a prior failed build silently
# shipping the previous version. CFBundleShortVersionString is set by
# bump-version → tauri.conf.json → Info.plist.
if [ -n "$APP" ]; then
    PLIST="$APP/Contents/Info.plist"
    if [ ! -f "$PLIST" ]; then
        echo "ERROR: Info.plist missing at $PLIST"
        exit 1
    fi
    APP_VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$PLIST" 2>/dev/null || true)
    if [ -z "$APP_VERSION" ]; then
        echo "ERROR: cannot read CFBundleShortVersionString from $PLIST"
        exit 1
    fi
    # Tauri may strip -beta.N suffix when writing Info.plist, accept both forms.
    EXPECTED_BASE="${VERSION%%-*}"
    if [ "$APP_VERSION" != "$VERSION" ] && [ "$APP_VERSION" != "$EXPECTED_BASE" ]; then
        echo "ERROR: .app version mismatch — refusing to sign stale build"
        echo "  expected: $VERSION (or $EXPECTED_BASE)"
        echo "  actual  : $APP_VERSION"
        echo "  at      : $APP"
        echo ""
        echo "This usually means src-tauri/target/ has a stale build."
        echo "Clean and rebuild:"
        echo "  CLEAN_BUILD=1 bash scripts/build-and-sign-macos.sh $VERSION $RELEASE_TYPE"
        exit 1
    fi
    echo "  .app version verified: $APP_VERSION"
    echo ""
fi

# Also peek at DMG inner .app version (defends against a stale DMG produced
# by pnpm tauri build before bump-version ran). Non-fatal — Step 2 rebuilds
# the DMG from the signed external .app — but warn loudly.
if [ -n "$DMG" ] && command -v hdiutil &>/dev/null; then
    DMG_CHECK_DIR=$(mktemp -d /tmp/aijia-dmg-version-XXXX)
    if hdiutil attach "$DMG" -mountpoint "$DMG_CHECK_DIR" -nobrowse -readonly -quiet 2>/dev/null; then
        INNER_PLIST="$DMG_CHECK_DIR/AIjia.app/Contents/Info.plist"
        if [ -f "$INNER_PLIST" ]; then
            DMG_APP_VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$INNER_PLIST" 2>/dev/null || true)
        fi
        hdiutil detach "$DMG_CHECK_DIR" -quiet 2>/dev/null || true
        rmdir "$DMG_CHECK_DIR" 2>/dev/null || true
        EXPECTED_BASE="${VERSION%%-*}"
        if [ -n "${DMG_APP_VERSION:-}" ] && \
           [ "$DMG_APP_VERSION" != "$VERSION" ] && \
           [ "$DMG_APP_VERSION" != "$EXPECTED_BASE" ]; then
            echo "WARN: DMG inner .app version ($DMG_APP_VERSION) != expected ($VERSION)."
            echo "      Step 2 will rebuild the DMG from the signed external .app,"
            echo "      so this is non-fatal — but ensure pnpm tauri build ran"
            echo "      AFTER bump-version next time."
            echo ""
        fi
    else
        rmdir "$DMG_CHECK_DIR" 2>/dev/null || true
    fi
fi

# ── helpers: idempotency checks ──────────────────────────────────────────
# Returns 0 if `.app` is already fully signed with Developer ID + hardened
# runtime, including nested dws. Saves rework on a re-run.
is_app_fully_signed() {
    local app="$1"
    local main_exe="$app/Contents/MacOS/aijia"
    local dws="$app/Contents/Resources/dws"

    [ -f "$main_exe" ] || return 1
    local info
    info=$(codesign -dv --verbose=4 "$main_exe" 2>&1)
    echo "$info" | grep -q "flags=.*runtime" || return 1
    echo "$info" | grep -q "Authority=Developer ID Application" || return 1

    if [ -f "$dws" ]; then
        info=$(codesign -dv --verbose=4 "$dws" 2>&1)
        echo "$info" | grep -q "flags=.*runtime" || return 1
        echo "$info" | grep -q "Authority=Developer ID Application" || return 1
    fi
    codesign --verify --deep --strict "$app" >/dev/null 2>&1 || return 1
    return 0
}

# Returns 0 if DMG was rebuilt from the signed .app (inner binary signed
# with Developer ID + hardened runtime, not adhoc).
is_dmg_built_from_signed_app() {
    local dmg="$1"
    local mount_dir
    mount_dir=$(mktemp -d /tmp/aijia-check-dmg-XXXX)
    if ! hdiutil attach "$dmg" -mountpoint "$mount_dir" -nobrowse -readonly -quiet 2>/dev/null; then
        rmdir "$mount_dir" 2>/dev/null
        return 1
    fi
    local inner="$mount_dir/AIjia.app/Contents/MacOS/aijia"
    local ok=1
    if [ -f "$inner" ]; then
        local info
        info=$(codesign -dv --verbose=4 "$inner" 2>&1)
        if echo "$info" | grep -q "Authority=Developer ID Application" && \
           echo "$info" | grep -q "flags=.*runtime"; then
            ok=0
        fi
    fi
    hdiutil detach "$mount_dir" -quiet 2>/dev/null || true
    rmdir "$mount_dir" 2>/dev/null || true
    return $ok
}

# Returns 0 if stapler ticket is already embedded (notarization complete).
is_stapled() {
    xcrun stapler validate "$1" >/dev/null 2>&1
}


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

# Idempotency: if the .app is already fully signed, skip the codesign loop.
SKIP_APP_SIGN=0
if [ -n "$APP" ] && is_app_fully_signed "$APP"; then
    echo "  ✓ .app already fully signed (hardened runtime + timestamp + nested dws)"
    echo "    Skipping codesign loop."
    SKIP_APP_SIGN=1
fi

if [ -n "$APP" ] && [ "$SKIP_APP_SIGN" = "0" ]; then
    echo "  Signing nested binaries (inside-out)..."
    # Sign every Mach-O / executable under Contents/ first so the outer
    # .app signature can authenticate them. --deep is unreliable on macOS
    # 11+; sign explicitly with --timestamp + --options runtime.
    while IFS= read -r -d '' bin; do
        # Skip anything inside an already-signed framework; we re-sign frameworks separately below.
        case "$bin" in
            *".framework/"*) continue ;;
        esac
        # Only operate on Mach-O binaries to avoid signing plain text/scripts.
        if file -b "$bin" | grep -qE "Mach-O|universal binary"; then
            echo "    sign $bin"
            codesign --force --timestamp --options runtime --sign "$IDENTITY" "$bin"
        fi
    done < <(find "$APP/Contents" -type f -print0)

    # Frameworks (sign at framework level, not file level).
    if find "$APP/Contents/Frameworks" -mindepth 1 -maxdepth 1 -name "*.framework" -print 2>/dev/null | grep -q .; then
        while IFS= read -r fw; do
            echo "    sign framework $fw"
            codesign --force --timestamp --options runtime --sign "$IDENTITY" "$fw"
        done < <(find "$APP/Contents/Frameworks" -mindepth 1 -maxdepth 1 -name "*.framework")
    fi

    echo "  Signing .app bundle..."
    codesign --force --timestamp --options runtime --sign "$IDENTITY" "$APP"
    echo "  .app signed"

    echo "  Verifying signature..."
    codesign --verify --deep --strict --verbose=2 "$APP"

    # Bundled runtime guard: every Mach-O under Resources/runtime/ MUST have
    # the hardened runtime flag, otherwise Apple notarization rejects the .app.
    # The inside-out loop above already covers them via the generic file walk,
    # but if a future tarball ships a Mach-O the walk misses (e.g. nested in an
    # unusual extension), this loop fails loudly with the exact path.
    runtime_root="$APP/Contents/Resources/runtime"
    if [ -d "$runtime_root" ]; then
        echo "  Auditing bundled runtime signatures under $runtime_root..."
        unsigned_count=0
        while IFS= read -r -d '' bin; do
            if file -b "$bin" | grep -qE "Mach-O|universal binary"; then
                flags=$(codesign -dv --verbose=4 "$bin" 2>&1 | grep -oE "flags=[^[:space:]]*" || echo "")
                if ! echo "$flags" | grep -q "runtime"; then
                    echo "    ✗ NOT hardened: $bin"
                    unsigned_count=$((unsigned_count + 1))
                fi
            fi
        done < <(find "$runtime_root" -type f -print0)
        if [ "$unsigned_count" -gt 0 ]; then
            echo "ERROR: $unsigned_count bundled runtime Mach-O binaries lack hardened runtime — notarization will fail." >&2
            exit 1
        fi
        echo "  ✓ All bundled runtime Mach-O binaries are hardened-runtime signed"
    fi
fi

# ── 1b. Rebuild DMG from signed .app ──
# `tauri build` packages the UNSIGNED .app into the DMG before our codesign
# runs. Re-running codesign on the standalone .app does NOT update the
# .app inside the DMG. Solution: rebuild the DMG using the freshly-signed
# .app, then sign that.
if [ -n "$APP" ]; then
    if is_dmg_built_from_signed_app "$DMG"; then
        echo "  ✓ DMG already contains signed .app — skipping rebuild"
    else
        echo "  Rebuilding DMG from signed .app..."
        DMG_STAGING=$(mktemp -d /tmp/aijia-dmg-staging-XXXX)
        cp -R "$APP" "$DMG_STAGING/"
        # Add /Applications symlink so user sees the standard "drag to install" target.
        # Without this, the dmg only shows the .app icon and users don't know where to drop it.
        ln -s /Applications "$DMG_STAGING/Applications"
        rm -f "$DMG"
        hdiutil create -volname "AIjia $VERSION" \
            -srcfolder "$DMG_STAGING" \
            -ov -format UDZO \
            -fs HFS+ \
            "$DMG"
        rm -rf "$DMG_STAGING"
        echo "  DMG rebuilt: $DMG"

        # If we rebuilt the DMG, the outer signature is gone — re-sign it.
        echo "  Signing .dmg..."
        codesign --force --timestamp --sign "$IDENTITY" "$DMG"
        echo "  .dmg signed"
    fi
fi

# ── 2. Notarize (DMG + .app concurrently) ──
echo ""
echo "=== Step 2: Notarize ==="
if [ -z "${APPLE_ID:-}" ]; then
    echo "ERROR: APPLE_ID env var not set"
    exit 1
fi

# Submit DMG and .app to Apple notary in parallel, wait for both, then staple.
# Cuts wall time by ~50% since each submission spends most of its time
# queued/processing on Apple's side, not in our network upload.
NOTARIZE_TMP=$(mktemp -d /tmp/aijia-notarize-XXXX)
DMG_LOG="$NOTARIZE_TMP/dmg.log"
APP_LOG="$NOTARIZE_TMP/app.log"
DMG_RC_FILE="$NOTARIZE_TMP/dmg.rc"
APP_RC_FILE="$NOTARIZE_TMP/app.rc"
APP_ZIP=""

notarize_dmg() {
    if is_stapled "$DMG"; then
        echo "0" > "$DMG_RC_FILE"
        echo "  ✓ DMG already stapled — skipping" > "$DMG_LOG"
        return
    fi
    {
        set +e  # we need to inspect xcrun's exit code; set -e would abort
        local attempt=1 max=3 rc=0
        while :; do
            echo "  attempt $attempt/$max"
            xcrun notarytool submit "$DMG" \
                --apple-id "$APPLE_ID" \
                --password "$APPLE_PASSWORD" \
                --team-id "$APPLE_TEAM_ID" \
                --wait --timeout 60m
            rc=$?
            if [ $rc -eq 0 ]; then
                xcrun stapler staple "$DMG"
                echo "$?" > "$DMG_RC_FILE"
                return
            fi
            # Retry only on transient network errors. Apple business
            # rejections (Invalid/Rejected) should fail fast.
            if [ $attempt -ge $max ] \
               || ! tail -50 "$DMG_LOG" 2>/dev/null | \
                  grep -qE 'abortedUpload|connectTimeout|HTTPClient|connection.*reset|EOF' ; then
                echo "$rc" > "$DMG_RC_FILE"
                return
            fi
            echo "  transient network error, retrying in 30s..."
            sleep 30
            attempt=$((attempt + 1))
        done
    } > "$DMG_LOG" 2>&1
}

notarize_app() {
    if [ -z "$APP" ]; then
        echo "0" > "$APP_RC_FILE"
        echo "  (no .app to notarize)" > "$APP_LOG"
        return
    fi
    if is_stapled "$APP"; then
        echo "0" > "$APP_RC_FILE"
        echo "  ✓ .app already stapled — skipping" > "$APP_LOG"
        return
    fi
    APP_ZIP=$(mktemp "$NOTARIZE_TMP/AIjia-app-XXXX.zip")
    ditto -c -k --keepParent "$APP" "$APP_ZIP"
    {
        set +e
        local attempt=1 max=3 rc=0
        while :; do
            echo "  attempt $attempt/$max"
            xcrun notarytool submit "$APP_ZIP" \
                --apple-id "$APPLE_ID" \
                --password "$APPLE_PASSWORD" \
                --team-id "$APPLE_TEAM_ID" \
                --wait --timeout 60m
            rc=$?
            if [ $rc -eq 0 ]; then
                xcrun stapler staple "$APP"
                echo "$?" > "$APP_RC_FILE"
                rm -f "$APP_ZIP"
                return
            fi
            if [ $attempt -ge $max ] \
               || ! tail -50 "$APP_LOG" 2>/dev/null | \
                  grep -qE 'abortedUpload|connectTimeout|HTTPClient|connection.*reset|EOF' ; then
                echo "$rc" > "$APP_RC_FILE"
                rm -f "$APP_ZIP"
                return
            fi
            echo "  transient network error, retrying in 30s..."
            sleep 30
            attempt=$((attempt + 1))
        done
    } > "$APP_LOG" 2>&1
}

echo "  Submitting DMG and .app to Apple notary in parallel..."
notarize_dmg & DMG_PID=$!
notarize_app & APP_PID=$!

# Tail both logs while waiting so the user sees progress.
(
    while kill -0 $DMG_PID 2>/dev/null || kill -0 $APP_PID 2>/dev/null; do
        sleep 30
        last_dmg=$(tail -1 "$DMG_LOG" 2>/dev/null | tr -d '\r' | head -c 100)
        last_app=$(tail -1 "$APP_LOG" 2>/dev/null | tr -d '\r' | head -c 100)
        echo "  [$(date +%H:%M:%S)] DMG: $last_dmg | APP: $last_app"
    done
) &
TAIL_PID=$!

wait $DMG_PID || true
wait $APP_PID || true
kill $TAIL_PID 2>/dev/null || true
wait $TAIL_PID 2>/dev/null || true

DMG_RC=$(cat "$DMG_RC_FILE" 2>/dev/null || echo 99)
APP_RC=$(cat "$APP_RC_FILE" 2>/dev/null || echo 99)

echo ""
echo "  --- DMG notarize log ---"
cat "$DMG_LOG"
echo "  --- .app notarize log ---"
cat "$APP_LOG"

if [ "$DMG_RC" != "0" ] || [ "$APP_RC" != "0" ]; then
    echo "ERROR: notarization failed (DMG rc=$DMG_RC, .app rc=$APP_RC)" >&2
    echo "  notarize tmp dir kept for debugging: $NOTARIZE_TMP" >&2
    exit 1
fi

rm -rf "$NOTARIZE_TMP"
echo "  DMG + .app notarized + stapled"

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
    COPYFILE_DISABLE=1 tar czf "$TAR" -C "$TAR_DIR" --exclude='.DS_Store' "AIjia.app"

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
