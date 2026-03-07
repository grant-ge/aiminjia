#!/usr/bin/env python3
"""
Upload AIjia release files to Aliyun OSS
Usage: python3 upload-to-oss.py <version>
Example: python3 upload-to-oss.py 0.3.6
"""

import os
import sys
import subprocess
import oss2
import requests
from pathlib import Path

# OSS Configuration
BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
OSS_PREFIX = "aijia"
KEYCHAIN_SERVICE = "aijia-oss"


def get_oss_credentials():
    """Read OSS credentials from macOS Keychain, falling back to env vars.

    First-time setup (run once):
        security add-generic-password -s aijia-oss -a access_key_id     -w 'YOUR_KEY_ID'
        security add-generic-password -s aijia-oss -a access_key_secret  -w 'YOUR_SECRET'

    After that, `python3 upload-to-oss.py 0.3.9` works with no env vars.
    """
    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")

    if key_id and key_secret:
        return key_id, key_secret

    # Try macOS Keychain
    try:
        key_id = subprocess.check_output(
            ["security", "find-generic-password", "-s", KEYCHAIN_SERVICE,
             "-a", "access_key_id", "-w"],
            stderr=subprocess.DEVNULL,
        ).decode().strip()
        key_secret = subprocess.check_output(
            ["security", "find-generic-password", "-s", KEYCHAIN_SERVICE,
             "-a", "access_key_secret", "-w"],
            stderr=subprocess.DEVNULL,
        ).decode().strip()
        if key_id and key_secret:
            print("Using OSS credentials from macOS Keychain")
            return key_id, key_secret
    except (subprocess.CalledProcessError, FileNotFoundError):
        pass

    return "", ""

# GitHub Release Configuration
GITHUB_REPO = "grant-ge/aiminjia"

def download_from_github(version, filename, output_path):
    """Download file from GitHub Release"""
    url = f"https://github.com/{GITHUB_REPO}/releases/download/v{version}/{filename}"
    print(f"Downloading {filename} from GitHub...")

    response = requests.get(url, stream=True)
    response.raise_for_status()

    total_size = int(response.headers.get('content-length', 0))
    downloaded = 0

    with open(output_path, 'wb') as f:
        for chunk in response.iter_content(chunk_size=8192):
            if chunk:
                f.write(chunk)
                downloaded += len(chunk)
                if total_size > 0:
                    percent = (downloaded / total_size) * 100
                    print(f"  Progress: {percent:.1f}%", end='\r')

    print(f"\n  Downloaded: {output_path}")
    return output_path

def upload_to_oss(auth, bucket, local_file, oss_key):
    """Upload file to OSS using resumable multipart upload"""
    file_size = os.path.getsize(local_file)
    print(f"Uploading {os.path.basename(local_file)} ({file_size / 1024 / 1024:.1f}MB) to OSS: {oss_key}")

    oss2.resumable_upload(
        bucket, oss_key, local_file,
        multipart_threshold=10 * 1024 * 1024,  # 10MB threshold
        part_size=5 * 1024 * 1024,              # 5MB per part
        num_threads=4,
    )
    print(f"  Uploaded: {oss_key}")

def create_latest_copy(bucket, versioned_key, latest_key):
    """Copy versioned file to latest/ directory"""
    print(f"Copying {versioned_key} -> {latest_key}")

    # copy_object(source_bucket, source_key, target_key)
    # copies source to target (the target is the new object in this bucket)
    bucket.copy_object(bucket.bucket_name, versioned_key, latest_key)
    print(f"  Done")

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 upload-to-oss.py <version>")
        print("  Credentials: macOS Keychain (aijia-oss) or OSS_ACCESS_KEY_ID/OSS_ACCESS_KEY_SECRET env vars")
        print("\n  First-time Keychain setup:")
        print("    security add-generic-password -s aijia-oss -a access_key_id     -w 'YOUR_KEY_ID'")
        print("    security add-generic-password -s aijia-oss -a access_key_secret  -w 'YOUR_SECRET'")
        sys.exit(1)

    ACCESS_KEY_ID, ACCESS_KEY_SECRET = get_oss_credentials()
    if not ACCESS_KEY_ID or not ACCESS_KEY_SECRET:
        print("Error: OSS credentials not found.")
        print("  Option 1: Set env vars OSS_ACCESS_KEY_ID and OSS_ACCESS_KEY_SECRET")
        print("  Option 2: Store in macOS Keychain:")
        print("    security add-generic-password -s aijia-oss -a access_key_id     -w 'YOUR_KEY_ID'")
        print("    security add-generic-password -s aijia-oss -a access_key_secret  -w 'YOUR_SECRET'")
        sys.exit(1)

    version = sys.argv[1]
    print(f"==> Uploading AIjia v{version} to OSS")

    # Initialize OSS
    auth = oss2.Auth(ACCESS_KEY_ID, ACCESS_KEY_SECRET)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    # Temporary download directory
    temp_dir = Path("/tmp/aijia-release")
    temp_dir.mkdir(exist_ok=True)

    # --- Resolve local build paths ---
    # Standard Tauri build output locations (macOS)
    tauri_target = Path(__file__).resolve().parent.parent / "src-tauri" / "target"
    arm_dmg = tauri_target / "release" / "bundle" / "dmg" / f"AIjia_{version}_aarch64.dmg"
    x64_dmg = tauri_target / "x86_64-apple-darwin" / "release" / "bundle" / "dmg" / f"AIjia_{version}_x64.dmg"

    # Files to upload: local-first, fallback to GitHub for CI-built artifacts
    files = [
        {
            "local_path": str(arm_dmg),
            "github_name": f"AIjia_{version}_aarch64.dmg",
            "local_name": f"AIjia_{version}_aarch64.dmg",
            "oss_key": f"{OSS_PREFIX}/v{version}/AIjia_{version}_aarch64.dmg",
            "latest_key": f"{OSS_PREFIX}/latest/macos-arm64",
        },
        {
            "local_path": str(x64_dmg),
            "github_name": f"AIjia_{version}_x64.dmg",
            "local_name": f"AIjia_{version}_x64.dmg",
            "oss_key": f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64.dmg",
            "latest_key": f"{OSS_PREFIX}/latest/macos-x64",
        },
        {
            "local_path": None,  # Windows: GitHub-only
            "github_name": f"AIjia_{version}_x64-setup.exe",
            "local_name": f"AIjia_{version}_x64-setup.exe",
            "oss_key": f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64-setup.exe",
            "latest_key": f"{OSS_PREFIX}/latest/windows-x64",
        },
    ]

    # Process each file: prefer local, fallback to GitHub download
    for file_info in files:
        print(f"\n--- Processing {file_info['local_name']} ---")

        local_path = file_info.get("local_path")
        if local_path and Path(local_path).exists():
            local_file = Path(local_path)
            print(f"  Using local file: {local_file}")
        else:
            # Try downloading from GitHub Release
            local_file = temp_dir / file_info["local_name"]
            try:
                download_from_github(version, file_info["github_name"], local_file)
            except Exception as e:
                print(f"  SKIP: not found locally and GitHub download failed: {e}")
                continue

        # Upload to OSS versioned directory
        upload_to_oss(auth, bucket, str(local_file), file_info["oss_key"])

        # Create/update latest redirect
        create_latest_copy(bucket, file_info["oss_key"], file_info["latest_key"])

    print(f"\n==> Upload complete!")
    print(f"\nDownload URLs:")
    print(f"  macOS ARM:   https://lotus.renlijia.com/{OSS_PREFIX}/latest/macos-arm64")
    print(f"  macOS Intel: https://lotus.renlijia.com/{OSS_PREFIX}/latest/macos-x64")
    print(f"  Windows:     https://lotus.renlijia.com/{OSS_PREFIX}/latest/windows-x64")
    print(f"\nVersioned URLs:")
    print(f"  https://lotus.renlijia.com/{OSS_PREFIX}/v{version}/")

if __name__ == "__main__":
    main()
