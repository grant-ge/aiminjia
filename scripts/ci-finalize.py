#!/usr/bin/env python3
"""Generate update.json for the Tauri auto-updater and upload to Aliyun OSS.

Runs in the CI finalize job after all platform uploads complete. Reads each
platform's signature from OSS, writes update.json, uploads to OSS. Preserves
any platforms already present with the same version (e.g. darwin-x86_64 from
the local upload-x64.py path).

Env vars required:
  OSS_ACCESS_KEY_ID
  OSS_ACCESS_KEY_SECRET

Usage:
  python3 scripts/ci-finalize.py <version>
"""

import json
import os
import sys
from datetime import datetime, timezone

import oss2

BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
CDN_BASE = "https://lotus.renlijia.com"
OSS_PREFIX = "aijia"

# Platforms uploaded by CI: (bundle oss key template, sig oss key template)
PLATFORMS = {
    "darwin-aarch64": (
        f"{OSS_PREFIX}/v{{version}}/AIjia.app.tar.gz",
        f"{OSS_PREFIX}/v{{version}}/AIjia.app.tar.gz.sig",
    ),
    "windows-x86_64": (
        f"{OSS_PREFIX}/v{{version}}/AIjia_{{version}}_x64-setup.exe",
        f"{OSS_PREFIX}/v{{version}}/AIjia_{{version}}_x64-setup.exe.sig",
    ),
}


def fetch_sig(bucket, key):
    try:
        return bucket.get_object(key).read().decode().strip()
    except oss2.exceptions.NoSuchKey:
        return None


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 ci-finalize.py <version>")
        sys.exit(1)
    version = sys.argv[1].lstrip("v")

    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if not key_id or not key_secret:
        print("[error] OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    platforms = {}
    missing = []
    for plat, (bundle_tpl, sig_tpl) in PLATFORMS.items():
        sig_key = sig_tpl.format(version=version)
        sig = fetch_sig(bucket, sig_key)
        if sig is None:
            missing.append((plat, sig_key))
            continue
        platforms[plat] = {
            "url": f"{CDN_BASE}/{bundle_tpl.format(version=version)}",
            "signature": sig,
        }
        print(f"[ok] {plat}")

    if missing:
        for plat, key in missing:
            print(f"[error] {plat}: sig not on OSS ({key})")
        sys.exit(1)

    # Preserve darwin-x86_64 (or any other platform) if already present at this version
    try:
        existing = json.loads(bucket.get_object(f"{OSS_PREFIX}/update.json").read())
        if existing.get("version") == version:
            for plat, info in existing.get("platforms", {}).items():
                if plat not in platforms:
                    platforms[plat] = info
                    print(f"[ok] {plat} (preserved from existing update.json)")
    except oss2.exceptions.NoSuchKey:
        pass

    update_json = {
        "version": version,
        "notes": f"AIjia v{version}",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    bucket.put_object(f"{OSS_PREFIX}/update.json", json.dumps(update_json, indent=2))
    print(f"\n[ok] update.json uploaded -- platforms: {list(platforms.keys())}")
    print(f"     {CDN_BASE}/{OSS_PREFIX}/update.json")


if __name__ == "__main__":
    main()
