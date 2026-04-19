#!/usr/bin/env python3
"""
Assemble update.json from OSS-hosted signatures and bump Homebrew cask.

CI uploads platform bundles directly to OSS (see .github/workflows/build-desktop.yml
→ ci-upload-{macos,windows}.py). This script runs locally after CI finishes to:

  1. Fetch each platform's .sig content from OSS
  2. Write update.json for the Tauri auto-updater
  3. Update the Homebrew cask version

Usage:
  python3 upload-to-oss.py <version>
"""

import json
import os
import re
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

# Platform → (bundle OSS key template, sig OSS key template)
# darwin-x86_64 is managed by upload-x64.py (local cross-compile), not CI.
PLATFORM_BUNDLES = {
    "darwin-aarch64": (
        f"{OSS_PREFIX}/v{{version}}/AIjia.app.tar.gz",
        f"{OSS_PREFIX}/v{{version}}/AIjia.app.tar.gz.sig",
    ),
    "windows-x86_64": (
        f"{OSS_PREFIX}/v{{version}}/AIjia_{{version}}_x64-setup.exe",
        f"{OSS_PREFIX}/v{{version}}/AIjia_{{version}}_x64-setup.exe.sig",
    ),
}


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
        if key_id and key_secret:
            print("✓ OSS credentials from macOS Keychain")
            return key_id, key_secret
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass
    return "", ""


def fetch_sig(bucket, key):
    try:
        return bucket.get_object(key).read().decode().strip()
    except oss2.exceptions.NoSuchKey:
        return None


def update_homebrew_cask(version):
    tap_path = Path("/opt/homebrew/Library/Taps/grant-ge/homebrew-tap")
    if not tap_path.exists():
        result = subprocess.run(
            ["brew", "--repository", "grant-ge/tap"],
            capture_output=True, text=True,
        )
        tap_path = Path(result.stdout.strip()) if result.returncode == 0 else None

    if not (tap_path and (tap_path / "Casks" / "aijia.rb").exists()):
        print(f"  ⚠ Homebrew tap not found, skipping cask update")
        return

    cask_file = tap_path / "Casks" / "aijia.rb"
    content = cask_file.read_text()
    new_content = re.sub(r'version "\d+\.\d+\.\d+"', f'version "{version}"', content)
    if new_content == content:
        print(f"  ℹ Cask already at v{version}")
        return

    cask_file.write_text(new_content)
    subprocess.run(["git", "add", "Casks/aijia.rb"], cwd=tap_path, capture_output=True)
    subprocess.run(["git", "commit", "-m", f"chore: bump aijia to v{version}"],
                   cwd=tap_path, capture_output=True)
    subprocess.run(["gh", "auth", "switch", "--user", "grant-ge"], capture_output=True)
    result = subprocess.run(["git", "push", "origin", "main"],
                            cwd=tap_path, capture_output=True, text=True)
    subprocess.run(["gh", "auth", "switch", "--user", "gezhigang000"], capture_output=True)
    if result.returncode == 0:
        print(f"  ✓ Homebrew cask updated to v{version}")
        print(f"    brew tap grant-ge/tap && brew install --cask aijia")
    else:
        print(f"  ✗ git push failed: {result.stderr.strip()}")


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 upload-to-oss.py <version>")
        sys.exit(1)
    version = sys.argv[1]

    key_id, key_secret = get_oss_credentials()
    if not key_id:
        print("Error: OSS credentials not found.")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    print(f"\n{'='*60}")
    print(f"  AIjia v{version} — Finalize release")
    print(f"{'='*60}")

    # ── Fetch platform signatures ────────────────────────────────
    platforms = {}
    missing = []
    for plat, (bundle_tpl, sig_tpl) in PLATFORM_BUNDLES.items():
        sig_key = sig_tpl.format(version=version)
        sig = fetch_sig(bucket, sig_key)
        if sig is None:
            missing.append(plat)
            print(f"  ⚠ {plat}: {sig_key} not on OSS")
            continue
        platforms[plat] = {
            "url": f"{CDN_BASE}/{bundle_tpl.format(version=version)}",
            "signature": sig,
        }
        print(f"  ✓ {plat}")

    if missing:
        print(f"\n⚠ Missing platforms: {missing}")
        print(f"   Wait for 'Build Desktop Apps' CI to finish, then rerun.")
        print(f"   gh run list -R grant-ge/aiminjia")
        sys.exit(1)

    # ── Preserve platforms managed by other scripts (darwin-x86_64) ──
    try:
        existing = json.loads(bucket.get_object(f"{OSS_PREFIX}/update.json").read())
        if existing.get("version") == version:
            for plat, info in existing.get("platforms", {}).items():
                if plat not in platforms:
                    platforms[plat] = info
                    print(f"  ✓ {plat} (kept from existing update.json)")
    except oss2.exceptions.NoSuchKey:
        pass

    # ── Write update.json ────────────────────────────────────────
    update_json = {
        "version": version,
        "notes": f"AIjia v{version}",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    bucket.put_object(f"{OSS_PREFIX}/update.json", json.dumps(update_json, indent=2))
    print(f"\n✓ update.json uploaded — platforms: {list(platforms.keys())}")

    # ── Summary ──────────────────────────────────────────────────
    print(f"\n{'='*60}")
    print(f"  ✅ AIjia v{version} release complete!")
    print(f"{'='*60}")
    print(f"\nDownload URLs:")
    print(f"  macOS ARM:  {CDN_BASE}/{OSS_PREFIX}/latest/macos-arm64")
    print(f"  Windows:    {CDN_BASE}/{OSS_PREFIX}/latest/windows-x64")
    print(f"\nVersioned:    {CDN_BASE}/{OSS_PREFIX}/v{version}/")
    print(f"Updater:      {CDN_BASE}/{OSS_PREFIX}/update.json")

    # ── Homebrew ─────────────────────────────────────────────────
    print(f"\n── Updating Homebrew Cask ──")
    update_homebrew_cask(version)


if __name__ == "__main__":
    main()
