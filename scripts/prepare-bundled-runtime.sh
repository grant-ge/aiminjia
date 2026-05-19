#!/usr/bin/env bash
# Build src-tauri/resources/runtime/<platform>/ from upstream sources.
#
# Inputs: scripts/runtime-sources.json (pinned upstream URLs + sha256).
# Output: src-tauri/resources/runtime/<platform>/ with node/, python/, uv/, bundled-version.json.
#
# Usage:
#   bash scripts/prepare-bundled-runtime.sh                 # auto-detect platform
#   PLATFORM=darwin-x64 bash scripts/prepare-bundled-runtime.sh
#
# Re-runs are idempotent: if the target dir already has the right bundled-version.json
# the script exits 0 without re-downloading.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
SOURCES="$SCRIPT_DIR/runtime-sources.json"
CACHE_DIR="${RUNTIME_PREP_CACHE:-$PROJECT_DIR/.runtime-cache}"

if ! command -v jq >/dev/null; then
  echo "ERROR: jq required (brew install jq)" >&2
  exit 2
fi

# Detect platform
plat="${PLATFORM:-}"
if [ -z "$plat" ]; then
  uname_s="$(uname -s | tr '[:upper:]' '[:lower:]')"
  uname_m="$(uname -m)"
  case "$uname_s-$uname_m" in
    darwin-arm64|darwin-aarch64) plat="darwin-arm64" ;;
    darwin-x86_64) plat="darwin-x64" ;;
    *) echo "ERROR: unsupported platform $uname_s-$uname_m" >&2; exit 2 ;;
  esac
fi

bundle_version="$(jq -r '.bundleVersion' "$SOURCES")"
out_dir="$PROJECT_DIR/src-tauri/resources/runtime/$plat"
existing_version=""
if [ -f "$out_dir/bundled-version.json" ]; then
  existing_version="$(jq -r '.bundleVersion' "$out_dir/bundled-version.json" 2>/dev/null || echo "")"
fi
if [ "$existing_version" = "$bundle_version" ] && [ "${FORCE:-0}" != "1" ]; then
  echo "[prepare-runtime] $plat already at $bundle_version, skipping (FORCE=1 to override)"
  exit 0
fi

mkdir -p "$CACHE_DIR" "$out_dir"

fetch() {
  local url="$1" sha="$2" name="$3"
  local cached="$CACHE_DIR/$name"
  local got
  if [ -f "$cached" ]; then
    got="$(shasum -a 256 "$cached" | awk '{print $1}')"
    if [ "$got" = "$sha" ]; then
      echo "[cache-hit] $name" >&2
      printf '%s' "$cached"
      return 0
    fi
    echo "[cache-stale] $name (got $got, expected $sha) — refetching" >&2
    rm -f "$cached"
  fi
  echo "[download] $url" >&2
  curl -fSL --retry 3 -o "$cached" "$url"
  got="$(shasum -a 256 "$cached" | awk '{print $1}')"
  if [ "$sha" = "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT" ]; then
    echo "[NOTE] computed sha256 for $name: $got" >&2
    echo "       Edit scripts/runtime-sources.json to pin this value, then re-run." >&2
  elif [ "$got" != "$sha" ]; then
    echo "ERROR: sha256 mismatch for $name (got $got, expected $sha)" >&2
    exit 1
  fi
  printf '%s' "$cached"
}

extract_node() {
  local tarball="$1"
  rm -rf "$out_dir/node"
  mkdir -p "$out_dir/node"
  tar -xzf "$tarball" -C "$out_dir/node" --strip-components=1
  # Sanity
  test -x "$out_dir/node/bin/node" || { echo "ERROR: node binary missing after extract" >&2; exit 1; }
  test -x "$out_dir/node/bin/npm"  || { echo "ERROR: npm shim missing" >&2; exit 1; }
}

extract_python_unix() {
  local tarball="$1"
  rm -rf "$out_dir/python"
  mkdir -p "$out_dir/python"
  # python-build-standalone install_only tarball contains a top-level `python/` dir
  tar -xzf "$tarball" -C "$out_dir/python" --strip-components=1
  test -x "$out_dir/python/bin/python3" || { echo "ERROR: python3 missing" >&2; exit 1; }
  # Ensure site-packages exists for uv to install into
  mkdir -p "$out_dir/python/lib/site-packages"
}

extract_uv() {
  local tarball="$1"
  rm -rf "$out_dir/uv"
  mkdir -p "$out_dir/uv/bin"
  local tmp
  tmp="$(mktemp -d)"
  # Subshell scopes the EXIT trap so it fires when the function returns,
  # not at script exit (where $tmp would be unbound under `set -u`).
  (
    trap 'rm -rf "$tmp"' EXIT
    tar -xzf "$tarball" -C "$tmp"
    # uv tarball top-level dir is uv-aarch64-apple-darwin/
    find "$tmp" -maxdepth 2 -name uv -type f -exec cp {} "$out_dir/uv/bin/uv" \;
    find "$tmp" -maxdepth 2 -name uvx -type f -exec cp {} "$out_dir/uv/bin/uvx" \;
    chmod +x "$out_dir/uv/bin/uv" "$out_dir/uv/bin/uvx"
    test -x "$out_dir/uv/bin/uv"  || { echo "ERROR: uv binary missing after extract" >&2; exit 1; }
    test -x "$out_dir/uv/bin/uvx" || { echo "ERROR: uvx binary missing after extract" >&2; exit 1; }
  )
}

node_url="$(jq -r ".node.platforms[\"$plat\"].url" "$SOURCES")"
node_sha="$(jq -r ".node.platforms[\"$plat\"].sha256" "$SOURCES")"
python_url="$(jq -r ".python.platforms[\"$plat\"].url" "$SOURCES")"
python_sha="$(jq -r ".python.platforms[\"$plat\"].sha256" "$SOURCES")"
uv_url="$(jq -r ".uv.platforms[\"$plat\"].url" "$SOURCES")"
uv_sha="$(jq -r ".uv.platforms[\"$plat\"].sha256" "$SOURCES")"

node_tar="$(fetch "$node_url" "$node_sha" "node-$plat.tar.gz")"
python_tar="$(fetch "$python_url" "$python_sha" "python-$plat.tar.gz")"
uv_tar="$(fetch "$uv_url" "$uv_sha" "uv-$plat.tar.gz")"

extract_node "$node_tar"
extract_python_unix "$python_tar"
extract_uv "$uv_tar"

# --- Trim unused runtime content to shrink installer (~110MB saved per arch) ---
# Default tarballs ship Node C++ headers, Python stdlib test suites, IDLE,
# tkinter etc. that the bundled runtime never executes. Strip aggressively;
# anything genuinely required is asserted right after.
#
# Disable with TRIM_RUNTIME=0 if a future change starts depending on one of
# these paths (we'll learn fast: the next build fails the post-trim asserts
# or the runtime panics at start).
if [ "${TRIM_RUNTIME:-1}" = "1" ]; then
  echo "[prepare-runtime] trimming $plat (-headers -tests -tk -docs)"
  # Node: headers (~53MB) + npm cache/docs + corepack
  rm -rf "$out_dir/node/include"
  rm -rf "$out_dir/node/share"
  rm -rf "$out_dir/node/lib/node_modules/corepack"
  rm -f  "$out_dir/node/bin/corepack"
  rm -f  "$out_dir/node/CHANGELOG.md" "$out_dir/node/README.md" "$out_dir/node/LICENSE"
  # Trim npm payload (tests/docs/man/changelogs inside npm itself)
  find "$out_dir/node/lib/node_modules/npm" -type d \( \
        -name test -o -name tests -o -name __tests__ -o -name docs \
        -o -name man -o -name changelogs -o -name .github \
      \) -prune -exec rm -rf {} + 2>/dev/null || true

  # Python: stdlib bits we never use
  py_lib="$out_dir/python/lib/python3.12"
  rm -rf "$py_lib/idlelib" \
         "$py_lib/tkinter" \
         "$py_lib/turtledemo" \
         "$py_lib/test" \
         "$py_lib/tests" \
         "$py_lib/ensurepip" \
         "$py_lib/pydoc_data" \
         "$py_lib/distutils" \
         "$out_dir/python/include" \
         "$out_dir/python/share"
  # tkinter native module + IDLE entrypoints
  find "$py_lib/lib-dynload" -name "_tkinter*.so" -delete 2>/dev/null || true
  # Bytecode caches regenerate at first import; safe to delete
  find "$out_dir/python" -type d -name __pycache__ -prune -exec rm -rf {} + 2>/dev/null || true
  # Strip Python test/ dirs nested inside packages (NOT the stdlib root one we already deleted)
  find "$py_lib" -type d \( -name test -o -name tests \) -prune -exec rm -rf {} + 2>/dev/null || true

  # Sanity: things we MUST still have after trimming
  test -x "$out_dir/node/bin/node" || { echo "ERROR: node binary lost during trim" >&2; exit 1; }
  test -x "$out_dir/node/bin/npm"  || { echo "ERROR: npm shim lost during trim" >&2; exit 1; }
  test -x "$out_dir/python/bin/python3" || { echo "ERROR: python3 lost during trim" >&2; exit 1; }
  test -x "$out_dir/uv/bin/uv"  || { echo "ERROR: uv binary lost during trim" >&2; exit 1; }
  # python3 -V quickly proves the interpreter still boots after trim
  if ! "$out_dir/python/bin/python3" -c 'import json,ssl,sqlite3,zlib' >/dev/null 2>&1; then
    echo "ERROR: python3 stdlib check failed after trim (json/ssl/sqlite3/zlib)" >&2
    exit 1
  fi
fi

# Empty node_modules placeholder so RuntimeLayout::validate sees the dir
mkdir -p "$out_dir/node/node_modules"

# Write bundled-version.json
cat > "$out_dir/bundled-version.json" <<JSON
{
  "bundleVersion": "$bundle_version",
  "platform": "$plat",
  "node": "$(jq -r .node.version "$SOURCES")",
  "python": "$(jq -r .python.version "$SOURCES")",
  "uv": "$(jq -r .uv.version "$SOURCES")"
}
JSON

echo "[prepare-runtime] OK: $out_dir at $bundle_version"
