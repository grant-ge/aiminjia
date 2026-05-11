#!/usr/bin/env python3
"""Bump AIjia Homebrew cask version in grant-ge/homebrew-tap.

Runs locally after CI finishes a release. CI has already uploaded all
platform bundles + update.json to OSS. This script only patches the
cask version field and pushes to the tap.

Usage:
  python3 scripts/bump-homebrew.py <version>          # release → aijia.rb
  python3 scripts/bump-homebrew.py <version> --beta   # beta → aijia-beta.rb
"""

import re
import subprocess
import sys
from pathlib import Path


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 bump-homebrew.py <version> [--beta]")
        sys.exit(1)
    version = sys.argv[1].lstrip("v")
    is_beta = "--beta" in sys.argv[2:]
    cask_name = "aijia-beta" if is_beta else "aijia"
    cask_filename = f"{cask_name}.rb"

    tap_path = Path("/opt/homebrew/Library/Taps/grant-ge/homebrew-tap")
    if not tap_path.exists():
        result = subprocess.run(
            ["brew", "--repository", "grant-ge/tap"],
            capture_output=True, text=True,
        )
        tap_path = Path(result.stdout.strip()) if result.returncode == 0 else None

    cask_file = tap_path / "Casks" / cask_filename if tap_path else None
    if not cask_file or not cask_file.exists():
        print(f"[error] Cask file not found: {cask_filename}. Tap installed?")
        sys.exit(1)

    content = cask_file.read_text()
    new_content = re.sub(
        r'version "\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?"',
        f'version "{version}"',
        content,
    )
    if new_content == content:
        print(f"[ok] Cask {cask_name} already at v{version}")
        return

    cask_file.write_text(new_content)
    subprocess.run(["git", "add", f"Casks/{cask_filename}"], cwd=tap_path, check=True)
    subprocess.run(["git", "commit", "-m", f"chore: bump {cask_name} to v{version}"],
                   cwd=tap_path, check=True)
    subprocess.run(["gh", "auth", "switch", "--user", "grant-ge"], capture_output=True)
    try:
        r = subprocess.run(["git", "push", "origin", "main"],
                           cwd=tap_path, capture_output=True, text=True)
    finally:
        subprocess.run(["gh", "auth", "switch", "--user", "gezhigang000"], capture_output=True)

    if r.returncode == 0:
        # Double-check: push reported success, but verify remote actually has
        # this commit (v0.5.18 / v0.5.20 silently stayed local once — unclear
        # cause, probably gh auth switch race). Fetch and compare.
        subprocess.run(["git", "fetch", "origin", "main"], cwd=tap_path,
                       capture_output=True)
        ahead = subprocess.run(
            ["git", "rev-list", "--count", "origin/main..HEAD"],
            cwd=tap_path, capture_output=True, text=True,
        ).stdout.strip()
        if ahead != "0":
            print(f"[error] git push reported success but local is {ahead} commit(s) "
                  f"ahead of origin/main — re-push manually: "
                  f"(cd {tap_path} && git push origin main)")
            sys.exit(1)
        print(f"[ok] Homebrew cask {cask_name} updated to v{version}")
        print(f"    brew tap grant-ge/tap && brew install --cask {cask_name}")
    else:
        print(f"[error] git push failed: {r.stderr.strip()}")
        sys.exit(1)


if __name__ == "__main__":
    main()
