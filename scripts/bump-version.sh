#!/bin/bash
# Bump version across all config files (package.json, Cargo.toml, tauri.conf.json)
#
# Usage:
#   ./scripts/bump-version.sh 0.5.22

set -e

VERSION=${1:-}
if [[ -z "$VERSION" ]]; then
    echo "Usage: ./scripts/bump-version.sh <version>"
    echo "Example: ./scripts/bump-version.sh 0.5.22"
    exit 1
fi

VERSION="${VERSION#v}"
cd "$(dirname "$0")/.."

echo "Bumping version to $VERSION"

# package.json
sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" package.json
echo "  updated package.json"

# src-tauri/Cargo.toml (only the package version, not dependency versions)
sed -i '' "/^\[package\]/,/^\[/ s/^version = \"[^\"]*\"/version = \"$VERSION\"/" src-tauri/Cargo.toml
echo "  updated src-tauri/Cargo.toml"

# src-tauri/tauri.conf.json
sed -i '' "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" src-tauri/tauri.conf.json
echo "  updated src-tauri/tauri.conf.json"

echo ""
echo "Version bumped to $VERSION"
echo "Verify: grep version package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json"
