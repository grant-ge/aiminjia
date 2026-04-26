#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  cat >&2 <<'USAGE'
Usage: build-runtime-artifact.sh <runtime-dir> <bundle-version> <output-dir>

Packages a prepared renlijia runtime directory into a tar.gz artifact and emits
an adjacent manifest fragment with sha256/size metadata. The runtime directory
must already contain node/, python/, uv/ payloads; this script does not install
Node or Python from upstream package managers.
USAGE
  exit 2
fi

runtime_dir="$1"
bundle_version="$2"
out_dir="$3"
platform="${RENLJ_RUNTIME_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"
case "$platform" in
  darwin-arm64|darwin-aarch64) platform="darwin-arm64" ;;
  darwin-x86_64) platform="darwin-x64" ;;
  linux-x86_64) platform="linux-x64" ;;
esac

required=(
  "node/bin/node"
  "node/bin/npm"
  "node/bin/npx"
  "python/bin/python3"
  "uv/bin/uv"
  "uv/bin/uvx"
)
for rel in "${required[@]}"; do
  if [[ ! -f "$runtime_dir/$rel" ]]; then
    echo "missing runtime payload: $runtime_dir/$rel" >&2
    exit 1
  fi
done

mkdir -p "$out_dir"
artifact="renlijia-primary-runtime-${bundle_version}-${platform}.tar.gz"
artifact_path="$out_dir/$artifact"

tar -C "$runtime_dir" -czf "$artifact_path" node python uv
sha256="$(shasum -a 256 "$artifact_path" | awk '{print $1}')"
size_bytes="$(wc -c < "$artifact_path" | tr -d ' ')"
cat > "$out_dir/${artifact}.manifest-fragment.json" <<JSON
{
  "bundleVersion": "$bundle_version",
  "runtime": "primary",
  "platform": "$platform",
  "artifact": {
    "url": "https://download.renlijia.com/runtimes/$artifact",
    "sha256": "$sha256",
    "sizeBytes": $size_bytes,
    "archiveFormat": "tar.gz"
  }
}
JSON

echo "$artifact_path"
