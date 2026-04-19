#!/usr/bin/env python3
"""Upload macOS arm64 build artifacts to Aliyun OSS from GitHub Actions runner.

Runs on the macos-14 CI runner after `tauri build` completes. Uploads the
.app.tar.gz (required by the auto-updater), its signature, and the DMG
(direct download) to OSS.

Env vars required:
  OSS_ACCESS_KEY_ID
  OSS_ACCESS_KEY_SECRET

Usage:
  python3 scripts/ci-upload-macos.py <version>
"""

import os
import sys
from pathlib import Path

import oss2

BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
OSS_PREFIX = "aijia"


def upload(bucket, local, key):
    size = os.path.getsize(local)
    print(f"↑ {os.path.basename(local)} ({size / 1024 / 1024:.1f}MB) → {key}")
    oss2.resumable_upload(
        bucket, key, str(local),
        multipart_threshold=10 * 1024 * 1024,
        part_size=5 * 1024 * 1024,
        num_threads=4,
    )


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 ci-upload-macos.py <version>")
        sys.exit(1)
    version = sys.argv[1].lstrip("v")

    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if not key_id or not key_secret:
        print("Error: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set")
        sys.exit(1)

    project_root = Path(__file__).resolve().parent.parent
    bundle = project_root / "src-tauri" / "target" / "release" / "bundle"
    dmg = bundle / "dmg" / f"AIjia_{version}_aarch64.dmg"
    tar = bundle / "macos" / "AIjia.app.tar.gz"
    sig = bundle / "macos" / "AIjia.app.tar.gz.sig"

    if not tar.exists() or not sig.exists():
        print(f"✗ Updater bundle missing: {tar} / {sig}")
        macos_dir = bundle / "macos"
        print(f"  macos dir: {[p.name for p in macos_dir.iterdir()] if macos_dir.exists() else '(missing)'}")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    tar_key = f"{OSS_PREFIX}/v{version}/AIjia.app.tar.gz"
    upload(bucket, tar, tar_key)
    upload(bucket, sig, tar_key + ".sig")

    if dmg.exists():
        dmg_key = f"{OSS_PREFIX}/v{version}/AIjia_{version}_aarch64.dmg"
        upload(bucket, dmg, dmg_key)
        latest_key = f"{OSS_PREFIX}/latest/macos-arm64"
        bucket.copy_object(
            BUCKET_NAME, dmg_key, latest_key,
            headers={
                "x-oss-metadata-directive": "REPLACE",
                "Content-Type": "application/x-apple-diskimage",
                "Content-Disposition": f'attachment; filename="AIjia_{version}_aarch64.dmg"',
            },
        )
        print(f"  → latest: {latest_key}")
    else:
        print(f"⚠ DMG not found: {dmg} — updater still works via .app.tar.gz, but direct DMG download won't")

    print(f"\n✅ macOS arm64 v{version} uploaded to OSS")


if __name__ == "__main__":
    main()
