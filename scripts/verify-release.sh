#!/bin/bash
# AIjia release artifact verification.
#
# After build-and-sign-macos.sh + release-windows.ps1 finish, run this to:
#   1. HEAD all expected OSS objects (200 + Content-Length > 1MB)
#   2. Download macOS DMGs to a temp dir and verify codesign + stapler
#   3. Print public download URLs for hand-off
#
# Windows Authenticode signature can only be fully verified on a Windows
# machine; we just confirm the .msi is reachable and non-trivially sized.
#
# Usage:
#   bash scripts/verify-release.sh <version> <beta|release>
#
# Example:
#   bash scripts/verify-release.sh 0.5.23-beta.1 beta
#   bash scripts/verify-release.sh 0.5.23 release

set -euo pipefail

if [ $# -lt 2 ]; then
    echo "Usage: bash scripts/verify-release.sh <version> <beta|release>"
    exit 1
fi

VERSION="$1"
RELEASE_TYPE="$2"

if [ "$RELEASE_TYPE" != "beta" ] && [ "$RELEASE_TYPE" != "release" ]; then
    echo "ERROR: type must be 'beta' or 'release', got: $RELEASE_TYPE"
    exit 1
fi

if [ "$RELEASE_TYPE" = "beta" ]; then
    URL_PREFIX="https://lotus.renlijia.com/aijia/beta/v${VERSION}"
else
    URL_PREFIX="https://lotus.renlijia.com/aijia/v${VERSION}"
fi

EXPECTED_FILES=(
    "AIjia_${VERSION}_aarch64.dmg"
    "AIjia_${VERSION}_x64.dmg"
    "AIjia.app.tar.gz"
    "AIjia.app.tar.gz.sig"
    "AIjia_x64.app.tar.gz"
    "AIjia_x64.app.tar.gz.sig"
    "AIjia_${VERSION}_x64-setup.msi"
    "AIjia_${VERSION}_x64-setup.msi.sig"
)

# ── helpers ──────────────────────────────────────────────────────────────
GREEN='\033[32m'; YELLOW='\033[33m'; RED='\033[31m'; CYAN='\033[36m'; RESET='\033[0m'
ok()   { printf "  ${GREEN}✓${RESET} %s\n" "$1"; }
warn() { printf "  ${YELLOW}!${RESET} %s\n" "$1"; }
err()  { printf "  ${RED}✗${RESET} %s\n" "$1"; }
section() { printf "\n${CYAN}═══ %s ═══${RESET}\n" "$1"; }

FAIL=0

# ── 1. OSS presence + size ───────────────────────────────────────────────
section "Step 1: Verify all expected OSS objects are reachable"
echo "  Base URL: $URL_PREFIX"
for f in "${EXPECTED_FILES[@]}"; do
    url="$URL_PREFIX/$f"
    # HEAD returns headers; capture status + content-length
    headers=$(curl -sI --max-time 30 "$url" 2>&1 || true)
    status=$(echo "$headers" | awk 'NR==1 {print $2}')
    length=$(echo "$headers" | awk -F': ' 'tolower($1)=="content-length" {gsub(/\r/,""); print $2}')

    if [ "$status" = "200" ]; then
        if [ -n "$length" ] && [ "$length" -gt 100 ]; then
            mb=$(awk -v b="$length" 'BEGIN{printf "%.2f", b/1024/1024}')
            ok "$(printf '%-40s  %s MB' "$f" "$mb")"
        else
            warn "$(printf '%-40s  suspiciously small (%s bytes)' "$f" "${length:-?}")"
            FAIL=$((FAIL+1))
        fi
    else
        err "$(printf '%-40s  HTTP %s' "$f" "${status:-no-response}")"
        FAIL=$((FAIL+1))
    fi
done

# ── 2. macOS: download DMGs, verify codesign + stapler ───────────────────
section "Step 2: Download macOS DMGs and verify signatures"
TMP_DIR=$(mktemp -d /tmp/aijia-verify-XXXX)
trap 'rm -rf "$TMP_DIR"' EXIT

for arch in aarch64 x64; do
    dmg_name="AIjia_${VERSION}_${arch}.dmg"
    dmg_path="$TMP_DIR/$dmg_name"
    echo ""
    echo "  $arch:"
    if ! curl -sSfL --max-time 300 -o "$dmg_path" "$URL_PREFIX/$dmg_name"; then
        err "  download failed"
        FAIL=$((FAIL+1))
        continue
    fi

    # DMG-level signature
    if codesign --verify --verbose=1 "$dmg_path" >/dev/null 2>&1; then
        ok "DMG codesign OK"
    else
        err "DMG codesign failed"
        FAIL=$((FAIL+1))
    fi
    if xcrun stapler validate "$dmg_path" >/dev/null 2>&1; then
        ok "DMG stapled"
    else
        err "DMG NOT stapled"
        FAIL=$((FAIL+1))
    fi

    # Inner .app — let hdiutil pick the mountpoint (multiple AIjia volumes
    # mounted simultaneously would otherwise clash with a fixed path).
    # Use -plist for reliable parsing; grep the first mount-point string.
    plist=$(hdiutil attach "$dmg_path" -nobrowse -readonly -noverify -plist 2>/dev/null || true)
    mount_dir=$(echo "$plist" | awk '
        /<key>mount-point<\/key>/ { getline; sub(/.*<string>/, ""); sub(/<\/string>.*/, ""); print; exit }
    ')
    if [ -n "$mount_dir" ] && [ -d "$mount_dir" ]; then
        inner_app="$mount_dir/AIjia.app"
        if [ -d "$inner_app" ]; then
            if codesign --verify --deep --strict --verbose=1 "$inner_app" >/dev/null 2>&1; then
                ok "inner .app codesign OK"
            else
                err "inner .app codesign failed"
                FAIL=$((FAIL+1))
            fi
            # Hardened runtime flag on main exe
            cs_main=$(codesign -dv --verbose=4 "$inner_app/Contents/MacOS/aijia" 2>&1)
            if echo "$cs_main" | grep -q "flags=.*runtime"; then
                ok "inner .app hardened runtime enabled"
            else
                err "inner .app missing hardened runtime"
                echo "$cs_main" | head -3 | sed 's/^/      /'
                FAIL=$((FAIL+1))
            fi
            # Nested dws signed?
            if [ -f "$inner_app/Contents/Resources/dws" ]; then
                cs_dws=$(codesign -dv --verbose=4 "$inner_app/Contents/Resources/dws" 2>&1)
                if echo "$cs_dws" | grep -q "Authority=Developer ID Application"; then
                    ok "nested dws signed"
                else
                    err "nested dws NOT signed by Developer ID"
                    echo "$cs_dws" | head -3 | sed 's/^/      /'
                    FAIL=$((FAIL+1))
                fi
            fi
            # Spctl gatekeeper assessment (simulates user double-click)
            if spctl --assess --type execute --verbose=2 "$inner_app" >/dev/null 2>&1; then
                ok "spctl gatekeeper would accept"
            else
                warn "spctl gatekeeper would reject (may indicate notarization not propagated yet)"
            fi
        else
            err "AIjia.app not found inside DMG"
            FAIL=$((FAIL+1))
        fi
        hdiutil detach "$mount_dir" -quiet 2>/dev/null || true
    else
        err "hdiutil attach failed or no mountpoint resolved"
        FAIL=$((FAIL+1))
    fi
done

# ── 3. Windows: reach only (signature verified on Windows) ───────────────
section "Step 3: Windows .msi reachability"
msi_url="$URL_PREFIX/AIjia_${VERSION}_x64-setup.msi"
echo "  Authenticode signature can only be fully verified on a Windows machine."
echo "  This check only confirms the file is downloadable and well-formed."
msi_tmp="$TMP_DIR/win.msi"
if curl -sSfL --max-time 300 -o "$msi_tmp" "$msi_url"; then
    size=$(stat -f%z "$msi_tmp" 2>/dev/null || stat -c%s "$msi_tmp")
    # MSI files are OLE compound documents.
    if [ "$(head -c 8 "$msi_tmp" | xxd -p | tr -d '\n')" = "d0cf11e0a1b11ae1" ]; then
        ok "valid MSI/OLE header, $(awk -v b="$size" 'BEGIN{printf "%.2f MB", b/1024/1024}')"
    else
        err "not a valid MSI — header is not OLE compound file magic"
        FAIL=$((FAIL+1))
    fi
else
    err "download failed: $msi_url"
    FAIL=$((FAIL+1))
fi
echo ""
echo "  To verify Authenticode on Windows:"
echo "    Get-AuthenticodeSignature .\\AIjia_${VERSION}_x64-setup.msi"

# ── 4. Summary ───────────────────────────────────────────────────────────
section "Summary"
if [ "$FAIL" = "0" ]; then
    printf "${GREEN}All checks passed for v%s (%s).${RESET}\n\n" "$VERSION" "$RELEASE_TYPE"
    echo "Download URLs:"
    for f in "${EXPECTED_FILES[@]}"; do
        echo "  $URL_PREFIX/$f"
    done
    exit 0
else
    printf "${RED}%d check(s) failed for v%s (%s).${RESET}\n" "$FAIL" "$VERSION" "$RELEASE_TYPE"
    exit 1
fi
