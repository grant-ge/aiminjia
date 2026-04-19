#!/usr/bin/env python3
"""Upload Windows build artifacts from GitHub Actions runner to Aliyun OSS.

Env vars required:
  OSS_ACCESS_KEY_ID
  OSS_ACCESS_KEY_SECRET

Usage:
  python scripts/ci-upload-windows.py <version>
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
    print(f"[upload] {os.path.basename(local)} ({size / 1024 / 1024:.1f}MB) -> {key}")
    oss2.resumable_upload(
        bucket, key, str(local),
        multipart_threshold=10 * 1024 * 1024,
        part_size=5 * 1024 * 1024,
        num_threads=4,
    )


def main():
    if len(sys.argv) < 2:
        print("Usage: python ci-upload-windows.py <version>")
        sys.exit(1)
    version = sys.argv[1].lstrip("v")

    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if not key_id or not key_secret:
        print("[error] OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set")
        sys.exit(1)

    project_root = Path(__file__).resolve().parent.parent
    nsis_dir = project_root / "src-tauri" / "target" / "release" / "bundle" / "nsis"
    exe = nsis_dir / f"AIjia_{version}_x64-setup.exe"
    sig = nsis_dir / f"AIjia_{version}_x64-setup.exe.sig"

    if not exe.exists():
        print(f"[error] Installer not found: {exe}")
        print(f"  nsis dir contents: {[p.name for p in nsis_dir.iterdir()] if nsis_dir.exists() else '(missing)'}")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    exe_key = f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64-setup.exe"
    upload(bucket, exe, exe_key)

    latest_key = f"{OSS_PREFIX}/latest/windows-x64"
    bucket.copy_object(
        BUCKET_NAME, exe_key, latest_key,
        headers={
            "x-oss-metadata-directive": "REPLACE",
            "Content-Type": "application/octet-stream",
            "Content-Disposition": f'attachment; filename="AIjia_{version}_x64-setup.exe"',
        },
    )
    print(f"  -> latest: {latest_key}")

    if sig.exists():
        sig_key = exe_key + ".sig"
        print(f"[upload] {sig.name} -> {sig_key}")
        bucket.put_object_from_file(sig_key, str(sig))
    else:
        print(f"[warn] Signature not found: {sig} -- updater will not work without it")
        sys.exit(1)

    print(f"\n[ok] Windows v{version} uploaded to OSS")


if __name__ == "__main__":
    main()
