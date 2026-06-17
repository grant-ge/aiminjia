#!/usr/bin/env python3
"""Bump version across all config files (package.json, Cargo.toml, tauri.conf.json).

Accepts plain semver (0.5.22) or pre-release (0.5.22-beta.1). Pre-release
strings flow into the binary's About dialog and into Tauri/Cargo crates,
which both accept full SemVer. Windows MSI ProductVersion is stricter, so the
script also writes bundle.windows.wix.version as a numeric MSI version.

Usage:
    python scripts/bump-version.py 0.5.22
    python scripts/bump-version.py 0.5.22-beta.1
"""

import json
import re
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent


def msi_version_for(version: str) -> str:
    match = re.match(r"^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$", version)
    if not match:
        raise ValueError(f"invalid version format '{version}'")

    major, minor, patch, prerelease = match.groups()
    parts = [int(major), int(minor), int(patch)]
    limits = [255, 255, 65535]
    for value, limit in zip(parts, limits):
        if value > limit:
            raise ValueError(f"MSI version component {value} exceeds {limit}")

    if not prerelease:
        return f"{major}.{minor}.{patch}"

    numeric_identifiers = [
        int(part)
        for part in re.split(r"[.-]", prerelease)
        if re.match(r"^\d+$", part)
    ]
    build = numeric_identifiers[-1] if numeric_identifiers else 0
    if build > 65535:
        raise ValueError(f"MSI build component {build} exceeds 65535")
    return f"{major}.{minor}.{patch}.{build}"


def main():
    if len(sys.argv) < 2:
        print("Usage: python scripts/bump-version.py <version>")
        print("Example: python scripts/bump-version.py 0.5.22")
        print("         python scripts/bump-version.py 0.5.22-beta.1")
        sys.exit(1)

    version = sys.argv[1].lstrip("v")
    # Accept plain X.Y.Z plus optional pre-release suffix (-beta.N, -rc.1, etc.)
    if not re.match(r'^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$', version):
        print(f"Error: invalid version format '{version}'. Use X.Y.Z[-prerelease]")
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
    conf.setdefault("bundle", {}).setdefault("windows", {}).setdefault("wix", {})["version"] = msi_version_for(version)
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
