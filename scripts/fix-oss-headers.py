#!/usr/bin/env python3
"""
One-time fix: update Content-Type and Content-Disposition on existing OSS latest/ keys.

Fixes:
  - latest/macos-arm64: was application/octet-stream without .dmg extension → users can't open
  - latest/windows-x64: add Content-Disposition with .exe filename
  - update.json: fix broken pub_date

Usage:
  python3 fix-oss-headers.py
"""

import json
import os
import subprocess
import sys
from datetime import datetime, timezone

import oss2

BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
CDN_BASE = "https://lotus.renlijia.com"
OSS_PREFIX = "aijia"
KEYCHAIN_SERVICE = "aijia-oss"


def get_oss_credentials():
    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if key_id and key_secret:
        return key_id, key_secret
    try:
        key_id = subprocess.check_output(
            ["security", "find-generic-password", "-s", KEYCHAIN_SERVICE,
             "-a", "access_key_id", "-w"], stderr=subprocess.DEVNULL,
        ).decode().strip()
        key_secret = subprocess.check_output(
            ["security", "find-generic-password", "-s", KEYCHAIN_SERVICE,
             "-a", "access_key_secret", "-w"], stderr=subprocess.DEVNULL,
        ).decode().strip()
        return key_id, key_secret
    except (subprocess.CalledProcessError, FileNotFoundError):
        return "", ""


def main():
    key_id, key_secret = get_oss_credentials()
    if not key_id:
        print("Error: OSS credentials not found")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    # ── Fix latest/ keys headers ────────────────────────────────
    fixes = [
        {
            "key": f"{OSS_PREFIX}/latest/macos-arm64",
            "Content-Type": "application/x-apple-diskimage",
            "Content-Disposition": 'attachment; filename="AIjia_latest_aarch64.dmg"',
        },
        {
            "key": f"{OSS_PREFIX}/latest/windows-x64",
            "Content-Type": "application/octet-stream",
            "Content-Disposition": 'attachment; filename="AIjia_latest_x64-setup.exe"',
        },
    ]

    for fix in fixes:
        key = fix["key"]
        print(f"\n── Fixing {key} ──")
        # Check if object exists
        if not bucket.object_exists(key):
            print(f"  ⚠ Object not found, skipping")
            continue

        # Get current metadata
        meta = bucket.head_object(key)
        print(f"  Current Content-Type: {meta.content_type}")
        print(f"  Current Content-Disposition: {meta.headers.get('Content-Disposition', '(none)')}")

        # Copy-in-place with new headers
        headers = {
            "x-oss-metadata-directive": "REPLACE",
            "Content-Type": fix["Content-Type"],
            "Content-Disposition": fix["Content-Disposition"],
        }
        bucket.copy_object(BUCKET_NAME, key, key, headers=headers)
        print(f"  ✓ Updated Content-Type: {fix['Content-Type']}")
        print(f"  ✓ Updated Content-Disposition: {fix['Content-Disposition']}")

    # ── Fix update.json pub_date ────────────────────────────────
    print(f"\n── Fixing update.json ──")
    result = bucket.get_object(f"{OSS_PREFIX}/update.json")
    update_json = json.loads(result.read())
    print(f"  Current pub_date: {update_json.get('pub_date')}")
    print(f"  Current version: {update_json.get('version')}")

    if "%" in update_json.get("pub_date", ""):
        update_json["pub_date"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        bucket.put_object(f"{OSS_PREFIX}/update.json", json.dumps(update_json, indent=2))
        print(f"  ✓ Fixed pub_date: {update_json['pub_date']}")
    else:
        print(f"  ℹ pub_date looks OK, skipping")

    # ── Verify ──────────────────────────────────────────────────
    print(f"\n── Verification ──")
    for fix in fixes:
        meta = bucket.head_object(fix["key"])
        ct = meta.content_type
        cd = meta.headers.get("Content-Disposition", "(none)")
        status = "✓" if ct == fix["Content-Type"] else "✗"
        print(f"  {status} {fix['key']}: Content-Type={ct}, Content-Disposition={cd}")

    print(f"\n✅ Done! Users should now be able to download and open files correctly.")
    print(f"   Test: curl -sI {CDN_BASE}/{OSS_PREFIX}/latest/macos-arm64 | grep -i content")


if __name__ == "__main__":
    main()
