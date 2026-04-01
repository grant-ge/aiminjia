#!/usr/bin/env python3
"""
Upload AIjia release files to Aliyun OSS — one-command release.

Usage:
  python3 upload-to-oss.py <version>

Example:
  python3 upload-to-oss.py 0.3.31

What it does:
  1. Finds local macOS build artifacts (DMG, .app.tar.gz, .sig)
  2. Downloads Windows artifacts from GitHub Actions CI via `gh`
  3. Uploads everything to OSS (versioned + latest)
  4. Generates and uploads update.json for auto-updater (macOS + Windows)
"""

import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

import oss2

# ── Configuration ────────────────────────────────────────────────
BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
CDN_BASE = "https://lotus.renlijia.com"
OSS_PREFIX = "aijia"
KEYCHAIN_SERVICE = "aijia-oss"
GITHUB_REPO = "grant-ge/aiminjia"

# ── Credentials ──────────────────────────────────────────────────

def get_oss_credentials():
    """Read OSS credentials from macOS Keychain, falling back to env vars."""
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


# ── Helpers ──────────────────────────────────────────────────────

def upload_to_oss(bucket, local_file, oss_key):
    """Upload file to OSS using resumable multipart upload."""
    file_size = os.path.getsize(local_file)
    print(f"  ↑ {os.path.basename(local_file)} ({file_size / 1024 / 1024:.1f}MB) → {oss_key}")
    oss2.resumable_upload(
        bucket, oss_key, local_file,
        multipart_threshold=10 * 1024 * 1024,
        part_size=5 * 1024 * 1024,
        num_threads=4,
    )


def download_gh_artifacts(version, dest_dir):
    """Download Windows CI artifacts from the latest GitHub Actions run for this tag.

    Returns the directory containing downloaded files, or None on failure.
    """
    print(f"\n── Downloading Windows CI artifacts from GitHub Actions ──")
    dest = Path(dest_dir)
    dest.mkdir(parents=True, exist_ok=True)

    # Find the run for this tag
    try:
        result = subprocess.run(
            ["gh", "run", "list", "-R", GITHUB_REPO,
             "-w", "Build Desktop Apps", "--json", "databaseId,headBranch,status",
             "-L", "5"],
            capture_output=True, text=True, timeout=30,
        )
        runs = json.loads(result.stdout)
        # Find completed run for this tag
        run_id = None
        for run in runs:
            if run.get("headBranch") == f"v{version}" and run.get("status") == "completed":
                run_id = run["databaseId"]
                break
        if not run_id:
            print("  ⚠ No completed CI run found for tag v{version}")
            return None
        print(f"  Found CI run: {run_id}")
    except Exception as e:
        print(f"  ⚠ Failed to list CI runs: {e}")
        return None

    # Download windows-installers artifact
    win_dir = dest / "windows"
    try:
        subprocess.run(
            ["gh", "run", "download", str(run_id), "-R", GITHUB_REPO,
             "-n", "windows-installers", "-D", str(win_dir)],
            check=True, timeout=600,
        )
        print(f"  ✓ Downloaded to {win_dir}")
        return win_dir
    except Exception as e:
        print(f"  ⚠ Download failed: {e}")
        return None


# ── Main ─────────────────────────────────────────────────────────

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 upload-to-oss.py <version>")
        print("\nFirst-time Keychain setup:")
        print("  security add-generic-password -s aijia-oss -a access_key_id     -w 'YOUR_KEY_ID'")
        print("  security add-generic-password -s aijia-oss -a access_key_secret  -w 'YOUR_SECRET'")
        sys.exit(1)

    ACCESS_KEY_ID, ACCESS_KEY_SECRET = get_oss_credentials()
    if not ACCESS_KEY_ID or not ACCESS_KEY_SECRET:
        print("Error: OSS credentials not found.")
        sys.exit(1)

    version = sys.argv[1]
    print(f"\n{'='*60}")
    print(f"  AIjia v{version} — Release to OSS")
    print(f"{'='*60}")

    auth = oss2.Auth(ACCESS_KEY_ID, ACCESS_KEY_SECRET)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    # ── Resolve local build paths ────────────────────────────────
    project_root = Path(__file__).resolve().parent.parent
    tauri_target = project_root / "src-tauri" / "target"
    arm_bundle = tauri_target / "release" / "bundle"

    # ── Download Windows artifacts from CI ───────────────────────
    temp_dir = Path("/tmp/aijia-release")
    win_dir = download_gh_artifacts(version, temp_dir)

    # Collect all resolved files
    # (label, local_path, oss_key, latest_key)
    uploads = []
    # (oss_key_of_bundle, sig_content) — for update.json
    updater_sigs = []

    # ── macOS ARM ────────────────────────────────────────────────
    mac_dmg = arm_bundle / "dmg" / f"AIjia_{version}_aarch64.dmg"
    mac_tar = arm_bundle / "macos" / "AIjia.app.tar.gz"
    mac_sig = arm_bundle / "macos" / "AIjia.app.tar.gz.sig"

    if mac_dmg.exists():
        uploads.append(("macOS ARM DMG", mac_dmg,
                         f"{OSS_PREFIX}/v{version}/AIjia_{version}_aarch64.dmg",
                         f"{OSS_PREFIX}/latest/macos-arm64"))
    else:
        # Tauri's bundle_dmg.sh sometimes fails — build DMG manually with create-dmg
        app_path = arm_bundle / "macos" / "AIjia.app"
        if app_path.exists():
            print(f"\n⚠ macOS ARM DMG not found, building with create-dmg...")
            result = subprocess.run([
                "create-dmg",
                "--volname", "AIjia",
                "--window-pos", "200", "120",
                "--window-size", "600", "400",
                "--icon-size", "100",
                "--icon", "AIjia.app", "175", "190",
                "--hide-extension", "AIjia.app",
                "--app-drop-link", "425", "190",
                "--skip-jenkins",
                str(mac_dmg),
                str(app_path),
            ], capture_output=True, text=True)
            if result.returncode == 0 and mac_dmg.exists():
                print(f"  ✓ DMG created: {mac_dmg}")
                uploads.append(("macOS ARM DMG", mac_dmg,
                                 f"{OSS_PREFIX}/v{version}/AIjia_{version}_aarch64.dmg",
                                 f"{OSS_PREFIX}/latest/macos-arm64"))
            else:
                print(f"  ✗ create-dmg failed: {result.stderr.strip()[-200:]}")
        else:
            print(f"\n⚠ macOS ARM DMG not found: {mac_dmg}")

    if mac_tar.exists():
        tar_key = f"{OSS_PREFIX}/v{version}/AIjia.app.tar.gz"
        uploads.append(("macOS ARM updater", mac_tar, tar_key, None))
        if mac_sig.exists():
            sig_key = tar_key + ".sig"
            uploads.append(("macOS ARM sig", mac_sig, sig_key, None))
            updater_sigs.append(("darwin-aarch64", tar_key, mac_sig.read_text().strip()))

    # ── Windows ──────────────────────────────────────────────────
    # Search order: CI download dir → /tmp/win-artifacts/ fallback
    win_exe = None
    win_exe_sig = None

    search_dirs = []
    if win_dir:
        search_dirs.append(win_dir)
    search_dirs.append(Path("/tmp/win-artifacts"))

    for d in search_dirs:
        candidate = d / f"AIjia_{version}_x64-setup.exe"
        if candidate.exists():
            win_exe = candidate
            break

    for d in search_dirs:
        candidate = d / f"AIjia_{version}_x64-setup.exe.sig"
        if candidate.exists():
            win_exe_sig = candidate
            break

    if win_exe:
        exe_key = f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64-setup.exe"
        uploads.append(("Windows exe", win_exe, exe_key, f"{OSS_PREFIX}/latest/windows-x64"))
        if win_exe_sig:
            sig_key = exe_key + ".sig"
            uploads.append(("Windows sig", win_exe_sig, sig_key, None))
            updater_sigs.append(("windows-x86_64", exe_key, win_exe_sig.read_text().strip()))
    else:
        print(f"\n⚠ Windows exe not found")

    # ── Upload ───────────────────────────────────────────────────
    if not uploads:
        print("\n✗ No files to upload!")
        sys.exit(1)

    print(f"\n── Uploading {len(uploads)} files ──")
    for label, local_path, oss_key, latest_key in uploads:
        print(f"\n[{label}]")
        upload_to_oss(bucket, str(local_path), oss_key)
        if latest_key:
            bucket.copy_object(bucket.bucket_name, oss_key, latest_key)
            print(f"  → latest: {latest_key}")

    # ── Generate update.json ─────────────────────────────────────
    print(f"\n── Generating update.json ──")

    platforms = {}
    for platform_key, bundle_oss_key, sig_content in updater_sigs:
        platforms[platform_key] = {
            "url": f"{CDN_BASE}/{bundle_oss_key}",
            "signature": sig_content,
        }

    if not platforms:
        print("  ⚠ No signed bundles found — update.json not generated")
    else:
        update_json = {
            "version": version,
            "notes": f"AIjia v{version}",
            "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "platforms": platforms,
        }
        bucket.put_object(f"{OSS_PREFIX}/update.json", json.dumps(update_json, indent=2))
        print(f"  ✓ update.json uploaded — platforms: {list(platforms.keys())}")

    # ── Summary ──────────────────────────────────────────────────
    print(f"\n{'='*60}")
    print(f"  ✅ AIjia v{version} release complete!")
    print(f"{'='*60}")
    print(f"\nDownload URLs:")
    print(f"  macOS ARM:  {CDN_BASE}/{OSS_PREFIX}/latest/macos-arm64")
    print(f"  Windows:    {CDN_BASE}/{OSS_PREFIX}/latest/windows-x64")
    print(f"\nVersioned:    {CDN_BASE}/{OSS_PREFIX}/v{version}/")
    print(f"Updater:      {CDN_BASE}/{OSS_PREFIX}/update.json")

    # ── Update Homebrew Cask ─────────────────────────────────────
    print(f"\n── Updating Homebrew Cask ──")
    tap_path = Path("/opt/homebrew/Library/Taps/grant-ge/homebrew-tap")
    if not tap_path.exists():
        # Try to find via brew --repository
        result = subprocess.run(
            ["brew", "--repository", "grant-ge/tap"],
            capture_output=True, text=True
        )
        tap_path = Path(result.stdout.strip()) if result.returncode == 0 else None

    if tap_path and (tap_path / "Casks" / "aijia.rb").exists():
        cask_file = tap_path / "Casks" / "aijia.rb"
        content = cask_file.read_text()
        # Update version line
        import re
        new_content = re.sub(r'version "\d+\.\d+\.\d+"', f'version "{version}"', content)
        if new_content != content:
            cask_file.write_text(new_content)
            subprocess.run(["git", "add", "Casks/aijia.rb"], cwd=tap_path, capture_output=True)
            subprocess.run(["git", "commit", "-m", f"chore: bump aijia to v{version}"], cwd=tap_path, capture_output=True)
            # Switch to grant-ge account for push
            subprocess.run(["gh", "auth", "switch", "--user", "grant-ge"], capture_output=True)
            result = subprocess.run(["git", "push", "origin", "main"], cwd=tap_path, capture_output=True, text=True)
            subprocess.run(["gh", "auth", "switch", "--user", "gezhigang000"], capture_output=True)
            if result.returncode == 0:
                print(f"  ✓ Homebrew cask updated to v{version}")
                print(f"    brew tap grant-ge/tap && brew install --cask aijia")
            else:
                print(f"  ✗ git push failed: {result.stderr.strip()}")
        else:
            print(f"  ℹ Cask already at v{version}")
    else:
        print(f"  ⚠ Homebrew tap not found, skipping cask update")


if __name__ == "__main__":
    main()
