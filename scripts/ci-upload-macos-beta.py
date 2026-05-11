#!/usr/bin/env python3
"""Upload macOS build artifacts to OSS beta path.

Same as ci-upload-macos.py but uploads to aijia/beta/v{version}/ instead of
aijia/v{version}/. Does NOT update latest symlink or update.json.

Env vars required:
  OSS_ACCESS_KEY_ID
  OSS_ACCESS_KEY_SECRET

Usage:
  python3 scripts/ci-upload-macos-beta.py <version> <arch>
    arch: aarch64 | x86_64
"""

import os
import sys
from pathlib import Path

import oss2

BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
OSS_PREFIX = "aijia/beta"

ARCH_CONFIG = {
    "aarch64": {
        "dmg_filename_tpl": "AIjia_{version}_aarch64.dmg",
        "tar_oss_name": "AIjia.app.tar.gz",
    },
    "x86_64": {
        "dmg_filename_tpl": "AIjia_{version}_x64.dmg",
        "tar_oss_name": "AIjia_x64.app.tar.gz",
    },
}


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
        print("Usage: python3 ci-upload-macos-beta.py <version> <arch>")
        print("  arch: aarch64 | x86_64")
        sys.exit(1)
    version = sys.argv[1].lstrip("v")
    arch = sys.argv[2]
    cfg = ARCH_CONFIG.get(arch)
    if not cfg:
        print(f"[error] unknown arch: {arch}")
        sys.exit(1)

    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if not key_id or not key_secret:
        print("[error] OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set")
        sys.exit(1)

    project_root = Path(__file__).resolve().parent.parent
    bundle = project_root / "src-tauri" / "target" / "release" / "bundle"
    dmg = bundle / "dmg" / cfg["dmg_filename_tpl"].format(version=version)
    tar = bundle / "macos" / "AIjia.app.tar.gz"
    sig = bundle / "macos" / "AIjia.app.tar.gz.sig"

    if not tar.exists() or not sig.exists():
        print(f"[error] Updater bundle missing: {tar} / {sig}")
        macos_dir = bundle / "macos"
        print(f"  macos dir: {[p.name for p in macos_dir.iterdir()] if macos_dir.exists() else '(missing)'}")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    # Upload to beta path
    tar_key = f"{OSS_PREFIX}/v{version}/{cfg['tar_oss_name']}"
    upload(bucket, tar, tar_key)
    upload(bucket, sig, tar_key + ".sig")

    if dmg.exists():
        dmg_key = f"{OSS_PREFIX}/v{version}/{dmg.name}"
        upload(bucket, dmg, dmg_key)
    else:
        print(f"[warn] DMG not found: {dmg}")

    print(f"\n[ok] macOS {arch} v{version} (beta) uploaded to OSS")
    print(f"     Path: {OSS_PREFIX}/v{version}/")


if __name__ == "__main__":
    main()
