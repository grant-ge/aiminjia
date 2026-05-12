#!/usr/bin/env python3
"""Upload unsigned CI build to aijia/dev/ on OSS (overwritten every build).

Usage:
  python3 scripts/ci-upload-dev.py <platform> <version>
    platform: macos-arm64 | macos-x64 | windows-x64

Env vars required:
  OSS_ACCESS_KEY_ID
  OSS_ACCESS_KEY_SECRET
"""

import os
import sys
from pathlib import Path

import oss2

BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
OSS_PREFIX = "aijia/dev"

PLATFORM_CONFIG = {
    "macos-arm64": {
        "find_pattern": "AIjia_*_aarch64.dmg",
        "oss_name": "AIjia_latest_aarch64.dmg",
        "search_dirs": ["src-tauri/target/release/bundle/dmg"],
    },
    "macos-x64": {
        "find_pattern": "AIjia_*_x64.dmg",
        "oss_name": "AIjia_latest_x64.dmg",
        "search_dirs": ["src-tauri/target/x86_64-apple-darwin/release/bundle/dmg"],
    },
    "windows-x64": {
        "find_pattern": "AIjia_*_x64-setup.exe",
        "oss_name": "AIjia_latest_x64-setup.exe",
        "search_dirs": ["src-tauri/target/release/bundle/nsis"],
    },
}


def find_file(root, dirs, pattern):
    import fnmatch
    for d in dirs:
        search = root / d
        if not search.exists():
            continue
        for f in search.iterdir():
            if fnmatch.fnmatch(f.name, pattern):
                return f
    return None


def upload(bucket, local, key):
    size = os.path.getsize(local)
    print(f"[upload] {os.path.basename(local)} ({size / 1024 / 1024:.1f}MB) -> {key}")
    oss2.resumable_upload(
        bucket, key, str(local),
        multipart_threshold=10 * 1024 * 1024,
        part_size=5 * 1024 * 1024,
        num_threads=4,
    )


def main():
    if len(sys.argv) < 3:
        print("Usage: python3 ci-upload-dev.py <platform> <version>")
        sys.exit(1)

    platform = sys.argv[1]
    version = sys.argv[2].lstrip("v")
    cfg = PLATFORM_CONFIG.get(platform)
    if not cfg:
        print(f"[error] unknown platform: {platform}")
        print(f"  valid: {', '.join(PLATFORM_CONFIG.keys())}")
        sys.exit(1)

    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if not key_id or not key_secret:
        print("[error] OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set")
        sys.exit(1)

    root = Path(__file__).resolve().parent.parent
    local = find_file(root, cfg["search_dirs"], cfg["find_pattern"])
    if not local:
        print(f"[error] cannot find {cfg['find_pattern']} in {cfg['search_dirs']}")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    # Upload as latest dev build
    oss_key = f"{OSS_PREFIX}/{cfg['oss_name']}"
    upload(bucket, local, oss_key)

    # Also keep versioned copy: dev/v0.5.23/
    versioned_key = f"{OSS_PREFIX}/v{version}/{local.name}"
    upload(bucket, local, versioned_key)

    print(f"\n[ok] {platform} dev build uploaded")
    print(f"  latest: https://lotus.renlijia.com/{oss_key}")
    print(f"  versioned: https://lotus.renlijia.com/{versioned_key}")


if __name__ == "__main__":
    main()
