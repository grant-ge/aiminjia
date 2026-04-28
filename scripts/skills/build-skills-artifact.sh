#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  cat >&2 <<'USAGE'
Usage: build-skills-artifact.sh <skills-root> <bundle-version> <output-dir>

Packages app-bundled global skills into a zip artifact and emits an adjacent
manifest fragment with sha256/size metadata. <skills-root> must contain one
subdirectory per skill; skill ids must match ^[a-z0-9][a-z0-9_-]{0,63}$ and
each included skill directory must contain SKILL.md.
USAGE
  exit 2
fi

skills_root="$1"
bundle_version="$2"
out_dir="$3"

if [[ ! -d "$skills_root" ]]; then
  echo "skills-root is not a directory: $skills_root" >&2
  exit 1
fi

if [[ ! "$bundle_version" =~ ^[A-Za-z0-9._-]+$ || "$bundle_version" == .* || "$bundle_version" == *[\\/]* ]]; then
  echo "invalid bundle-version: must match ^[A-Za-z0-9._-]+$, must not start with '.', and must not contain / or \\" >&2
  exit 1
fi

if ! command -v zip >/dev/null 2>&1; then
  echo "required command not found: zip" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "required command not found: python3" >&2
  exit 1
fi

skill_ids=()
skill_paths=()
while IFS= read -r -d '' skill_path; do
  skill_paths+=("$skill_path")
done < <(find "$skills_root" -mindepth 1 -maxdepth 1 -type d -print0 | sort -z)

for skill_path in "${skill_paths[@]}"; do
  skill_id="$(basename "$skill_path")"

  case "$skill_id" in
    .*|_*)
      echo "skipping hidden/private skill directory: $skill_id" >&2
      continue
      ;;
  esac

  if [[ ! "$skill_id" =~ ^[a-z0-9][a-z0-9_-]{0,63}$ ]]; then
    echo "skipping invalid skill id: $skill_id" >&2
    continue
  fi

  if [[ ! -f "$skill_path/SKILL.md" ]]; then
    echo "missing SKILL.md for skill: $skill_id" >&2
    exit 1
  fi

  if find "$skill_path" -type l -print -quit | grep -q .; then
    echo "symlink not allowed in skill directory: $skill_id" >&2
    exit 1
  fi

  skill_ids+=("$skill_id")
done

if [[ ${#skill_ids[@]} -eq 0 ]]; then
  echo "no valid skills found in: $skills_root" >&2
  exit 1
fi

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd -P)"
artifact="renlijia-global-skills-${bundle_version}.zip"
artifact_path="$out_dir/$artifact"
manifest_path="$out_dir/${artifact}.manifest-fragment.json"
rm -f "$artifact_path" "$manifest_path"

staging_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$staging_dir"
}
trap cleanup EXIT

for skill_id in "${skill_ids[@]}"; do
  cp -R "$skills_root/$skill_id" "$staging_dir/$skill_id"
done

(
  cd "$staging_dir"
  zip -X -qr "$artifact_path" "${skill_ids[@]}" -x '*/.DS_Store' '*/__MACOSX/*' '__MACOSX/*' '.DS_Store'
)

sha256="$(shasum -a 256 "$artifact_path" | awk '{print $1}')"
size_bytes="$(wc -c < "$artifact_path" | tr -d ' ')"
base_url="${RENLJ_GLOBAL_SKILLS_BASE_URL:-https://rlj-cdn.oss-cn-hangzhou.aliyuncs.com/lotus/skills}"
base_url="${RENLIJIA_GLOBAL_SKILLS_BASE_URL:-$base_url}"
base_url="${base_url%/}"

python3 - <<'PY' "$manifest_path" "$bundle_version" "$base_url" "$artifact" "$sha256" "$size_bytes"
import json
import sys

manifest_path, bundle_version, base_url, artifact, sha256, size_bytes = sys.argv[1:]
manifest = {
    "bundleVersion": bundle_version,
    "artifact": {
        "url": f"{base_url}/{artifact}",
        "sha256": sha256,
        "sizeBytes": int(size_bytes),
        "archiveFormat": "zip",
    },
}
with open(manifest_path, "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2, ensure_ascii=False)
    f.write("\n")
PY

echo "$artifact_path"
