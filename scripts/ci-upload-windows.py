#!/usr/bin/env python3
"""Upload Windows build artifacts to Aliyun OSS from GitHub Actions runner.

Runs on the Windows CI runner after `tauri build` completes. Uploads the
NSIS installer and its updater signature directly to OSS, bypassing the
developer's local network / proxy.

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


def main():
    if len(sys.argv) < 2:
        print("Usage: python ci-upload-windows.py <version>")
        sys.exit(1)
    version = sys.argv[1].lstrip("v")

    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if not key_id or not key_secret:
        print("Error: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set")
        sys.exit(1)

    project_root = Path(__file__).resolve().parent.parent
    nsis_dir = project_root / "src-tauri" / "target" / "release" / "bundle" / "nsis"
    exe = nsis_dir / f"AIjia_{version}_x64-setup.exe"
    sig = nsis_dir / f"AIjia_{version}_x64-setup.exe.sig"

    if not exe.exists():
        print(f"✗ Installer not found: {exe}")
        print(f"  NSIS dir contents: {[p.name for p in nsis_dir.iterdir()] if nsis_dir.exists() else '(missing)'}")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    exe_key = f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64-setup.exe"
    latest_key = f"{OSS_PREFIX}/latest/windows-x64"

    print(f"↑ {exe.name} ({exe.stat().st_size / 1024 / 1024:.1f}MB) → {exe_key}")
    oss2.resumable_upload(
        bucket, exe_key, str(exe),
        multipart_threshold=10 * 1024 * 1024,
        part_size=5 * 1024 * 1024,
        num_threads=4,
    )

    bucket.copy_object(
        BUCKET_NAME, exe_key, latest_key,
        headers={
            "x-oss-metadata-directive": "REPLACE",
            "Content-Type": "application/octet-stream",
            "Content-Disposition": f'attachment; filename="AIjia_{version}_x64-setup.exe"',
        },
    )
    print(f"  → latest: {latest_key}")

    if sig.exists():
        sig_key = exe_key + ".sig"
        print(f"↑ {sig.name} → {sig_key}")
        bucket.put_object_from_file(sig_key, str(sig))
    else:
        print(f"⚠ Signature not found: {sig} — updater won't work without it")

    print(f"\n✅ Windows v{version} uploaded to OSS")


if __name__ == "__main__":
    main()
