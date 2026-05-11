#!/usr/bin/env python3
"""Bump version across all config files (package.json, Cargo.toml, tauri.conf.json).

Usage:
    python scripts/bump-version.py 0.5.22
"""

import json
import re
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent


def main():
    if len(sys.argv) < 2:
        print("Usage: python scripts/bump-version.py <version>")
        print("Example: python scripts/bump-version.py 0.5.22")
        sys.exit(1)

    version = sys.argv[1].lstrip("v")
    if not re.match(r'^\d+\.\d+\.\d+$', version):
        print(f"Error: invalid version format '{version}'. Use X.Y.Z")
        sys.exit(1)

    print(f"Bumping version to {version}")

    # package.json
    pkg_path = PROJECT_ROOT / "package.json"
    pkg = json.loads(pkg_path.read_text(encoding="utf-8"))
    pkg["version"] = version
    pkg_path.write_text(json.dumps(pkg, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"  updated package.json")

    # src-tauri/tauri.conf.json
    conf_path = PROJECT_ROOT / "src-tauri" / "tauri.conf.json"
    conf = json.loads(conf_path.read_text(encoding="utf-8"))
    conf["version"] = version
    conf_path.write_text(json.dumps(conf, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"  updated src-tauri/tauri.conf.json")

    # src-tauri/Cargo.toml (only [package] version)
    cargo_path = PROJECT_ROOT / "src-tauri" / "Cargo.toml"
    cargo = cargo_path.read_text(encoding="utf-8")
    # Replace the first version = "..." after [package]
    cargo_new = re.sub(
        r'(\[package\][^\[]*?)version\s*=\s*"[^"]+"',
        lambda m: m.group(1) + f'version = "{version}"',
        cargo,
        count=1,
        flags=re.DOTALL,
    )
    cargo_path.write_text(cargo_new, encoding="utf-8")
    print(f"  updated src-tauri/Cargo.toml")

    print(f"\nVersion bumped to {version}")
    print(f"Note: Run 'cargo build' or 'cargo update -p aijia' to update Cargo.lock")


if __name__ == "__main__":
    main()
