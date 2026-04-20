#!/usr/bin/env python3
"""Upload x64 macOS build artifacts to OSS and update latest/macos-x64."""

import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

import oss2

BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
CDN_BASE = "https://lotus.renlijia.com"
OSS_PREFIX = "aijia"
KEYCHAIN_SERVICE = "aijia-oss"


def get_oss_credentials():
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


def upload_to_oss(bucket, local_file, oss_key):
    file_size = os.path.getsize(local_file)
    print(f"  ↑ {os.path.basename(local_file)} ({file_size / 1024 / 1024:.1f}MB) → {oss_key}")
    oss2.resumable_upload(
        bucket, oss_key, local_file,
        multipart_threshold=10 * 1024 * 1024,
        part_size=5 * 1024 * 1024,
        num_threads=4,
    )


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 upload-x64.py <version>")
        print("Example: python3 upload-x64.py 0.4.10")
        sys.exit(1)
    VERSION = sys.argv[1]

    key_id, key_secret = get_oss_credentials()
    if not key_id:
        print("Error: OSS credentials not found")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    base = Path(__file__).resolve().parent.parent / "src-tauri" / "target" / "x86_64-apple-darwin" / "release" / "bundle"
    dmg = base / "dmg" / f"AIjia_{VERSION}_x64.dmg"
    tar = base / "macos" / "AIjia.app.tar.gz"
    sig = base / "macos" / "AIjia.app.tar.gz.sig"

    print(f"\n{'='*60}")
    print(f"  AIjia v{VERSION} x64 — Upload to OSS")
    print(f"{'='*60}")

    # Upload DMG (versioned + latest)
    if dmg.exists():
        dmg_key = f"{OSS_PREFIX}/v{VERSION}/AIjia_{VERSION}_x64.dmg"
        upload_to_oss(bucket, str(dmg), dmg_key)

        # Copy to latest with proper headers
        latest_key = f"{OSS_PREFIX}/latest/macos-x64"
        bucket.copy_object(
            BUCKET_NAME, dmg_key, latest_key,
            headers={
                "x-oss-metadata-directive": "REPLACE",
                "Content-Type": "application/x-apple-diskimage",
                "Content-Disposition": f'attachment; filename="AIjia_{VERSION}_x64.dmg"',
            },
        )
        print(f"  → latest: {latest_key}")
    else:
        print(f"  ✗ DMG not found: {dmg}")
        sys.exit(1)

    # Upload updater tar.gz + sig (versioned)
    if tar.exists():
        tar_key = f"{OSS_PREFIX}/v{VERSION}/AIjia_x64.app.tar.gz"
        upload_to_oss(bucket, str(tar), tar_key)
        if sig.exists():
            sig_key = tar_key + ".sig"
            upload_to_oss(bucket, str(sig), sig_key)

            # Update update.json to include x64 platform
            print(f"\n── Updating update.json with darwin-x86_64 ──")
            result = bucket.get_object(f"{OSS_PREFIX}/update.json")
            update_json = json.loads(result.read())
            update_json["platforms"]["darwin-x86_64"] = {
                "url": f"{CDN_BASE}/{tar_key}",
                "signature": sig.read_text().strip(),
            }
            bucket.put_object(f"{OSS_PREFIX}/update.json", json.dumps(update_json, indent=2))
            print(f"  ✓ update.json updated — platforms: {list(update_json['platforms'].keys())}")

    # Verify
    print(f"\n── Verification ──")
    meta = bucket.head_object(f"{OSS_PREFIX}/latest/macos-x64")
    print(f"  Content-Type: {meta.content_type}")
    print(f"  Content-Disposition: {meta.headers.get('Content-Disposition', '(none)')}")
    print(f"  Size: {meta.content_length / 1024 / 1024:.1f}MB")

    print(f"\n✅ Done!")
    print(f"  Download: {CDN_BASE}/{OSS_PREFIX}/latest/macos-x64")


if __name__ == "__main__":
    main()
