#!/usr/bin/env bash
# Download python-build-standalone + install pip deps into src-tauri/python-runtime/
# Usage: bash scripts/setup-python.sh
set -euo pipefail

PYTHON_VERSION="3.12.8"
STANDALONE_TAG="20250106"
TARGET_DIR="src-tauri/python-runtime"
REQUIREMENTS="src-tauri/requirements.txt"

# ─── Detect platform ───────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
    Darwin-arm64)
        TRIPLE="aarch64-apple-darwin"
        PYTHON_BIN="${TARGET_DIR}/bin/python3"
        ;;
    Darwin-x86_64)
        TRIPLE="x86_64-apple-darwin"
        PYTHON_BIN="${TARGET_DIR}/bin/python3"
        ;;
    Linux-x86_64)
        TRIPLE="x86_64-unknown-linux-gnu"
        PYTHON_BIN="${TARGET_DIR}/bin/python3"
        ;;
    Linux-aarch64)
        TRIPLE="aarch64-unknown-linux-gnu"
        PYTHON_BIN="${TARGET_DIR}/bin/python3"
        ;;
    *)
        echo "ERROR: Unsupported platform: ${OS}-${ARCH}"
        exit 1
        ;;
esac

FILENAME="cpython-${PYTHON_VERSION}+${STANDALONE_TAG}-${TRIPLE}-install_only_stripped.tar.gz"
URL="https://github.com/astral-sh/python-build-standalone/releases/download/${STANDALONE_TAG}/${FILENAME}"

# ─── Skip if already set up ───────────────────────────────────────
if [ -x "${PYTHON_BIN}" ]; then
    EXISTING_VER=$("${PYTHON_BIN}" --version 2>&1 || true)
    if echo "${EXISTING_VER}" | grep -q "${PYTHON_VERSION}"; then
        echo "Python ${PYTHON_VERSION} already exists at ${PYTHON_BIN}, skipping download."
        echo "To force re-download, delete ${TARGET_DIR}/ and re-run."
        # Still install pip deps in case requirements.txt changed
        echo "Installing pip dependencies..."
        "${PYTHON_BIN}" -m pip install -r "${REQUIREMENTS}" --only-binary :all: --no-cache-dir -q
        echo "Done."
        exit 0
    fi
fi

# ─── Download ───────────────────────────────────���──────────────────
echo "Downloading Python ${PYTHON_VERSION} for ${TRIPLE}..."
echo "URL: ${URL}"

TMPDIR_DL="$(mktemp -d)"
trap 'rm -rf "${TMPDIR_DL}"' EXIT

ARCHIVE="${TMPDIR_DL}/${FILENAME}"
curl -fSL --retry 3 --progress-bar -o "${ARCHIVE}" "${URL}"

# ─── Extract ───────────────────────────────────────────────────────
echo "Extracting to ${TARGET_DIR}/..."
rm -rf "${TARGET_DIR}"

# python-build-standalone archives contain a top-level `python/` directory
tar xzf "${ARCHIVE}" -C "$(dirname "${TARGET_DIR}")"
# Rename `python` → `python-runtime`
mv "$(dirname "${TARGET_DIR}")/python" "${TARGET_DIR}"

echo "Python binary: ${PYTHON_BIN}"
"${PYTHON_BIN}" --version

# ─── Install pip dependencies ──────────────────────────────────────
echo "Installing pip dependencies from ${REQUIREMENTS}..."
# --only-binary :all: — force pre-built wheels only, never compile C extensions
# (avoids needing system build tools like pkg-config, cairo, etc.)
"${PYTHON_BIN}" -m pip install -r "${REQUIREMENTS}" --only-binary :all: --no-cache-dir -q

# ─── Slim down ─────────────────────────────────────────────────────
echo "Removing unnecessary files to reduce bundle size..."

# Remove test directories
find "${TARGET_DIR}" -type d -name "test" -o -name "tests" -o -name "test_*" | \
    xargs rm -rf 2>/dev/null || true

# Remove __pycache__ and .pyc files
find "${TARGET_DIR}" -type d -name "__pycache__" | xargs rm -rf 2>/dev/null || true
find "${TARGET_DIR}" -name "*.pyc" -delete 2>/dev/null || true

# Remove unused stdlib modules
for dir in tkinter idlelib turtle turtledemo ensurepip lib2to3 distutils; do
    rm -rf "${TARGET_DIR}/lib/python3.12/${dir}" 2>/dev/null || true
done

# Remove pip cache inside the runtime
rm -rf "${TARGET_DIR}/lib/python3.12/site-packages/pip" 2>/dev/null || true

# Remove .dist-info (saves ~5MB, not needed at runtime)
find "${TARGET_DIR}" -type d -name "*.dist-info" | xargs rm -rf 2>/dev/null || true

# macOS: clear quarantine flag on all files
if [ "${OS}" = "Darwin" ]; then
    echo "Clearing macOS quarantine attributes..."
    xattr -cr "${TARGET_DIR}" 2>/dev/null || true
fi

# ─── Summary ───────────────────────────────────────────────────────
SIZE=$(du -sh "${TARGET_DIR}" | cut -f1)
echo ""
echo "Setup complete!"
echo "  Python: ${PYTHON_BIN}"
echo "  Size:   ${SIZE}"
echo "  Version: $("${PYTHON_BIN}" --version)"
