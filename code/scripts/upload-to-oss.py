#!/usr/bin/env python3
"""
Upload AIjia release files to Aliyun OSS
Usage: python3 upload-to-oss.py <version>
Example: python3 upload-to-oss.py 0.3.6
"""

import os
import sys
import oss2
import requests
from pathlib import Path

# OSS Configuration
BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
ACCESS_KEY_ID = "LTAI5tMYxV7TteizJPQnX3CK"
ACCESS_KEY_SECRET = "xHV5KK6UofIcMud0ngDavuoSjAWPQj"
OSS_PREFIX = "aijia"

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
    """Upload file to OSS"""
    print(f"Uploading {local_file} to OSS: {oss_key}")

    file_size = os.path.getsize(local_file)
    uploaded = 0

    def progress_callback(consumed_bytes, total_bytes):
        nonlocal uploaded
        uploaded = consumed_bytes
        if total_bytes:
            percent = (consumed_bytes / total_bytes) * 100
            print(f"  Progress: {percent:.1f}%", end='\r')

    bucket.put_object_from_file(oss_key, local_file, progress_callback=progress_callback)
    print(f"\n  Uploaded: {oss_key}")

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
        print("Example: python3 upload-to-oss.py 0.3.6")
        sys.exit(1)

    version = sys.argv[1]
    print(f"==> Uploading AIjia v{version} to OSS")

    # Initialize OSS
    auth = oss2.Auth(ACCESS_KEY_ID, ACCESS_KEY_SECRET)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    # Temporary download directory
    temp_dir = Path("/tmp/aijia-release")
    temp_dir.mkdir(exist_ok=True)

    # Files to upload
    files = [
        {
            "github_name": f"AIjia_{version}_aarch64.dmg",
            "local_name": f"AIjia_{version}_aarch64.dmg",
            "oss_key": f"{OSS_PREFIX}/v{version}/AIjia_{version}_aarch64.dmg",
            "latest_key": f"{OSS_PREFIX}/latest/macos-arm64",
            "source": "github"
        },
        {
            "github_name": f"AIjia_{version}_x64-setup.exe",
            "local_name": f"AIjia_{version}_x64-setup.exe",
            "oss_key": f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64-setup.exe",
            "latest_key": f"{OSS_PREFIX}/latest/windows-x64",
            "source": "github"
        },
        {
            "local_path": f"/Users/gezhigang/minjia/code/src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/AIjia_{version}_x64.dmg",
            "local_name": f"AIjia_{version}_x64.dmg",
            "oss_key": f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64.dmg",
            "latest_key": f"{OSS_PREFIX}/latest/macos-x64",
            "source": "local"
        }
    ]

    # Process each file
    for file_info in files:
        print(f"\n--- Processing {file_info['local_name']} ---")

        if file_info["source"] == "github":
            # Download from GitHub
            local_file = temp_dir / file_info["local_name"]
            download_from_github(version, file_info["github_name"], local_file)
        else:
            # Use local file
            local_file = Path(file_info["local_path"])
            if not local_file.exists():
                print(f"  ERROR: Local file not found: {local_file}")
                continue

        # Upload to OSS versioned directory
        upload_to_oss(auth, bucket, str(local_file), file_info["oss_key"])

        # Create/update latest redirect
        create_latest_copy(bucket, file_info["oss_key"], file_info["latest_key"])

    print(f"\n==> Upload complete!")
    print(f"\nDownload URLs:")
    print(f"  macOS ARM:   https://lotus-releases.cn-beijing.taihangtop.cn/{OSS_PREFIX}/latest/macos-arm64")
    print(f"  macOS Intel: https://lotus-releases.cn-beijing.taihangtop.cn/{OSS_PREFIX}/latest/macos-x64")
    print(f"  Windows:     https://lotus-releases.cn-beijing.taihangtop.cn/{OSS_PREFIX}/latest/windows-x64")
    print(f"\nVersioned URLs:")
    print(f"  https://lotus-releases.cn-beijing.taihangtop.cn/{OSS_PREFIX}/v{version}/")

if __name__ == "__main__":
    main()
