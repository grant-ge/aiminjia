#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  cat >&2 <<'USAGE'
Usage: build-runtime-artifact.sh <runtime-dir> <bundle-version> <output-dir>

Packages a prepared renlijia runtime directory into a platform artifact and emits
an adjacent manifest fragment with sha256/size metadata. The runtime directory
must already contain node/, python/, uv/ payloads; this script does not install
Node or Python from upstream package managers.

Set RENLJ_RUNTIME_PLATFORM to one of: darwin-arm64, darwin-x64, win32-x64.
USAGE
  exit 2
fi

runtime_dir="$1"
bundle_version="$2"
out_dir="$3"
platform="${RENLJ_RUNTIME_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"
case "$platform" in
  darwin-arm64|darwin-aarch64) platform="darwin-arm64" ;;
  darwin-x64|darwin-x86_64) platform="darwin-x64" ;;
  win32-x64|windows-x64|windows-x86_64|mingw64-x86_64|msys_nt-*-x86_64) platform="win32-x64" ;;
  linux-x64|linux-x86_64) platform="linux-x64" ;;
  *) echo "unsupported runtime platform: $platform" >&2; exit 2 ;;
esac

archive_format="tar.gz"
required=(
  "node/bin/node"
  "node/bin/npm"
  "node/bin/npx"
  "python/bin/python3"
  "uv/bin/uv"
  "uv/bin/uvx"
  "node/node_modules"
  "python/lib/site-packages"
)
if [[ "$platform" == "win32-x64" ]]; then
  archive_format="zip"
  required=(
    "node/node.exe"
    "node/npm.cmd"
    "node/npx.cmd"
    "python/python.exe"
    "uv/uv.exe"
    "uv/uvx.exe"
    "node/node_modules"
    "python/Lib/site-packages"
  )
fi

for rel in "${required[@]}"; do
  if [[ ! -e "$runtime_dir/$rel" ]]; then
    echo "missing runtime payload: $runtime_dir/$rel" >&2
    exit 1
  fi
done

mkdir -p "$out_dir"
artifact="renlijia-primary-runtime-${bundle_version}-${platform}.${archive_format}"
artifact_path="$out_dir/$artifact"
rm -f "$artifact_path"

if [[ "$archive_format" == "zip" ]]; then
  (cd "$runtime_dir" && zip -qr "$artifact_path" node python uv)
else
  tar -C "$runtime_dir" -czf "$artifact_path" node python uv
fi

sha256="$(shasum -a 256 "$artifact_path" | awk '{print $1}')"
size_bytes="$(wc -c < "$artifact_path" | tr -d ' ')"
cat > "$out_dir/${artifact}.manifest-fragment.json" <<JSON
{
  "bundleVersion": "$bundle_version",
  "runtime": "primary",
  "platform": "$platform",
  "artifact": {
    "url": "https://datamind-pzc.oss-cn-hangzhou.aliyuncs.com/runtimes/$artifact",
    "sha256": "$sha256",
    "sizeBytes": $size_bytes,
    "archiveFormat": "$archive_format"
  }
}
JSON

echo "$artifact_path"
