#!/usr/bin/env python3
"""
AIjia release: upload local macOS arm bundles + fetch Windows from GitHub Release,
push everything to Aliyun OSS, then write update.json and bump Homebrew cask.

Two-stage flow (CI handles the Windows build, this script handles the rest):

  Stage 1 (automatic, on tag push):
    GitHub Actions builds Windows + macOS arm and attaches the artifacts to a
    GitHub Release for the tag (see .github/workflows/build-desktop.yml →
    softprops/action-gh-release).

  Stage 2 (this script, run locally on macOS):
    1. Resolve local macOS arm DMG / .app.tar.gz / .sig from src-tauri/target
    2. Upload macOS arm bundles to OSS
    3. Download Windows .exe + .exe.sig from GitHub Release for this tag
       (tries direct URL → gh-proxy.com mirror — both work for public repos)
    4. Upload Windows bundles to OSS
    5. Write update.json combining both platforms (preserves darwin-x86_64 from
       upload-x64.py if present)
    6. Bump Homebrew cask version

Usage:
  python3 upload-to-oss.py <version>
  python3 upload-to-oss.py <version> --win-dir /path/to/already-downloaded
"""

import json
import os
import re
import shutil
import subprocess
import sys
import urllib.request
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
RELEASE_BASE = f"https://github.com/{GITHUB_REPO}/releases/download"
MIRROR_PREFIX = "https://gh-proxy.com"  # gh-proxy mirror for China access


# ── Credentials ──────────────────────────────────────────────────

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


# ── HTTP download with mirror fallback ───────────────────────────

def http_download(url, dest, timeout=60):
    """Stream a URL to dest, returning True on success."""
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "aijia-release/1.0"})
        with urllib.request.urlopen(req, timeout=timeout) as r, open(dest, "wb") as f:
            total = int(r.headers.get("Content-Length", 0))
            mb = total / 1024 / 1024 if total else 0
            print(f"    streaming {mb:.1f}MB ...")
            shutil.copyfileobj(r, f, length=1024 * 1024)
        actual = dest.stat().st_size
        if total and actual != total:
            print(f"    size mismatch: got {actual} expected {total}")
            dest.unlink()
            return False
        return True
    except Exception as e:
        print(f"    failed: {e}")
        if dest.exists():
            dest.unlink()
        return False


def download_windows_assets(version, dest_dir):
    """Download Windows .exe + .exe.sig from the GitHub Release for this tag.

    Tries the direct GitHub Release URL first, then the gh-proxy mirror.
    Returns dest_dir on success, None on failure.
    """
    print(f"\n── Fetching Windows installer from GitHub Release v{version} ──")
    dest_dir.mkdir(parents=True, exist_ok=True)
    base = f"{RELEASE_BASE}/v{version}"

    for name in [f"AIjia_{version}_x64-setup.exe", f"AIjia_{version}_x64-setup.exe.sig"]:
        dest = dest_dir / name
        if dest.exists() and dest.stat().st_size > 0:
            print(f"  [cached] {name} ({dest.stat().st_size / 1024 / 1024:.1f}MB)")
            continue
        ok = False
        for url in (f"{base}/{name}", f"{MIRROR_PREFIX}/{base}/{name}"):
            print(f"  GET {url}")
            if http_download(url, dest):
                print(f"  ✓ {name}")
                ok = True
                break
        if not ok:
            print(f"  ✗ {name}: all sources failed")
            return None
    return dest_dir


# ── OSS upload ───────────────────────────────────────────────────

def upload_to_oss(bucket, local_file, oss_key):
    file_size = os.path.getsize(local_file)
    print(f"  ↑ {os.path.basename(local_file)} ({file_size / 1024 / 1024:.1f}MB) → {oss_key}")
    oss2.resumable_upload(
        bucket, oss_key, local_file,
        multipart_threshold=10 * 1024 * 1024,
        part_size=5 * 1024 * 1024,
        num_threads=4,
    )


# ── Homebrew ─────────────────────────────────────────────────────

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


# ── Main ─────────────────────────────────────────────────────────

def parse_args():
    args = sys.argv[1:]
    win_dir_override = None
    positional = []
    i = 0
    while i < len(args):
        if args[i] == "--win-dir" and i + 1 < len(args):
            win_dir_override = Path(args[i + 1])
            i += 2
        else:
            positional.append(args[i])
            i += 1
    if not positional:
        return None, None
    return positional[0], win_dir_override


def main():
    version, win_dir_override = parse_args()
    if not version:
        print("Usage: python3 upload-to-oss.py <version> [--win-dir /path/to/exe-dir]")
        sys.exit(1)

    key_id, key_secret = get_oss_credentials()
    if not key_id:
        print("Error: OSS credentials not found.")
        sys.exit(1)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    print(f"\n{'='*60}")
    print(f"  AIjia v{version} — Release to OSS")
    print(f"{'='*60}")

    project_root = Path(__file__).resolve().parent.parent
    arm_bundle = project_root / "src-tauri" / "target" / "release" / "bundle"
    uploads = []  # (label, local_path, oss_key, latest_key)
    updater_sigs = []  # (platform_key, bundle_oss_key, sig_text)

    # ── macOS arm (local) ────────────────────────────────────────
    mac_dmg = arm_bundle / "dmg" / f"AIjia_{version}_aarch64.dmg"
    mac_tar = arm_bundle / "macos" / "AIjia.app.tar.gz"
    mac_sig = arm_bundle / "macos" / "AIjia.app.tar.gz.sig"

    if mac_dmg.exists():
        uploads.append(("macOS arm DMG", mac_dmg,
                        f"{OSS_PREFIX}/v{version}/AIjia_{version}_aarch64.dmg",
                        f"{OSS_PREFIX}/latest/macos-arm64"))
    else:
        # Tauri's bundle_dmg.sh sometimes fails — fall back to create-dmg
        app_path = arm_bundle / "macos" / "AIjia.app"
        if app_path.exists():
            print(f"\n⚠ macOS arm DMG not found, building with create-dmg...")
            mac_dmg.parent.mkdir(parents=True, exist_ok=True)
            r = subprocess.run([
                "create-dmg",
                "--volname", "AIjia",
                "--window-pos", "200", "120",
                "--window-size", "600", "400",
                "--icon-size", "100",
                "--icon", "AIjia.app", "175", "190",
                "--hide-extension", "AIjia.app",
                "--app-drop-link", "425", "190",
                "--skip-jenkins",
                str(mac_dmg), str(app_path),
            ], capture_output=True, text=True)
            if r.returncode == 0 and mac_dmg.exists():
                print(f"  ✓ DMG created: {mac_dmg}")
                uploads.append(("macOS arm DMG", mac_dmg,
                                f"{OSS_PREFIX}/v{version}/AIjia_{version}_aarch64.dmg",
                                f"{OSS_PREFIX}/latest/macos-arm64"))
            else:
                print(f"  ✗ create-dmg failed: {r.stderr.strip()[-200:]}")
        else:
            print(f"\n⚠ macOS arm DMG not found: {mac_dmg}")

    if mac_tar.exists() and mac_sig.exists():
        tar_key = f"{OSS_PREFIX}/v{version}/AIjia.app.tar.gz"
        uploads.append(("macOS arm updater", mac_tar, tar_key, None))
        uploads.append(("macOS arm sig", mac_sig, tar_key + ".sig", None))
        updater_sigs.append(("darwin-aarch64", tar_key, mac_sig.read_text().strip()))
    else:
        print(f"\n⚠ macOS arm updater bundle missing: build locally first (pnpm tauri build)")

    # ── Windows (download from GitHub Release) ───────────────────
    win_dir = win_dir_override
    if win_dir is None:
        win_dir = download_windows_assets(version, Path(f"/tmp/aijia-release/v{version}/windows"))
    if win_dir is None:
        print("\n✗ Could not get Windows assets. Wait for CI to finish, or pass --win-dir.")
        sys.exit(1)

    win_exe = win_dir / f"AIjia_{version}_x64-setup.exe"
    win_sig = win_dir / f"AIjia_{version}_x64-setup.exe.sig"
    if not win_exe.exists():
        print(f"\n✗ Windows exe missing in {win_dir}")
        sys.exit(1)

    exe_key = f"{OSS_PREFIX}/v{version}/AIjia_{version}_x64-setup.exe"
    uploads.append(("Windows exe", win_exe, exe_key, f"{OSS_PREFIX}/latest/windows-x64"))
    if win_sig.exists():
        uploads.append(("Windows sig", win_sig, exe_key + ".sig", None))
        updater_sigs.append(("windows-x86_64", exe_key, win_sig.read_text().strip()))
    else:
        print(f"⚠ Windows .sig not found — auto-updater won't work for Windows users")

    # ── Upload to OSS ────────────────────────────────────────────
    LATEST_HEADERS = {
        f"{OSS_PREFIX}/latest/macos-arm64": {
            "Content-Type": "application/x-apple-diskimage",
            "Content-Disposition": f'attachment; filename="AIjia_{version}_aarch64.dmg"',
        },
        f"{OSS_PREFIX}/latest/windows-x64": {
            "Content-Type": "application/octet-stream",
            "Content-Disposition": f'attachment; filename="AIjia_{version}_x64-setup.exe"',
        },
    }

    print(f"\n── Uploading {len(uploads)} files to OSS ──")
    for label, local_path, oss_key, latest_key in uploads:
        print(f"\n[{label}]")
        upload_to_oss(bucket, str(local_path), oss_key)
        if latest_key:
            copy_headers = {"x-oss-metadata-directive": "REPLACE"}
            copy_headers.update(LATEST_HEADERS.get(latest_key, {}))
            bucket.copy_object(BUCKET_NAME, oss_key, latest_key, headers=copy_headers)
            print(f"  → latest: {latest_key}")

    # ── update.json (preserve darwin-x86_64 from upload-x64.py) ──
    print(f"\n── Generating update.json ──")
    platforms = {}
    for plat, bundle_oss_key, sig in updater_sigs:
        platforms[plat] = {
            "url": f"{CDN_BASE}/{bundle_oss_key}",
            "signature": sig,
        }
    try:
        existing = json.loads(bucket.get_object(f"{OSS_PREFIX}/update.json").read())
        if existing.get("version") == version:
            for plat, info in existing.get("platforms", {}).items():
                if plat not in platforms:
                    platforms[plat] = info
                    print(f"  ✓ {plat} (kept from existing update.json)")
    except oss2.exceptions.NoSuchKey:
        pass

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
    print(f"  macOS arm:  {CDN_BASE}/{OSS_PREFIX}/latest/macos-arm64")
    print(f"  Windows:    {CDN_BASE}/{OSS_PREFIX}/latest/windows-x64")
    print(f"\nVersioned:    {CDN_BASE}/{OSS_PREFIX}/v{version}/")
    print(f"Updater:      {CDN_BASE}/{OSS_PREFIX}/update.json")

    print(f"\n── Updating Homebrew Cask ──")
    update_homebrew_cask(version)


if __name__ == "__main__":
    main()
