# Bundle Node/Python/uv Runtime Into Installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a single installer that contains a fully-working Node/Python/uv runtime so first-launch is 100% offline-capable. Eliminate the post-install OSS download as the critical path; keep it only as an opt-in upgrade channel.

**Architecture:** Add a new `BundledRuntimeResolver` that reads node/python/uv binaries directly from Tauri's `resource_dir()/runtime/<platform>/`. At startup we chain resolvers in priority order: **(1) bundled** → **(2) OSS-installed at `~/.renlijia/runtimes/...` (upgrade path)** → **(3) on-demand OSS download (last resort)**. Build pipelines run a new `scripts/prepare-bundled-runtime.{sh,ps1}` before `tauri build` that materializes `src-tauri/resources/runtime/` from upstream sources (nodejs.org + python-build-standalone + Astral uv releases) so the bundle is reproducible and signed inside-out.

**Tech Stack:** Rust (Tauri 2), TypeScript (React), bash + PowerShell prep scripts, GitHub Actions, Aliyun OSS, codesign + signtool, python-build-standalone, Node.js LTS binaries.

---

## File Map

**Create:**

- `scripts/prepare-bundled-runtime.sh` — macOS/Linux prep: download + layout `node/`, `python/`, `uv/` under `src-tauri/resources/runtime/<platform>/`
- `scripts/prepare-bundled-runtime.ps1` — Windows equivalent
- `scripts/runtime-sources.json` — pinned upstream URLs + sha256 for node, python-build-standalone, uv (per platform)
- `src-tauri/src/runtime/dependencies/bundled_resolver.rs` — new resolver reading from app resource dir
- `src-tauri/src/runtime/dependencies/chain_resolver.rs` — chained resolver wrapper
- `src-tauri/tests/bundled_runtime_resolver_test.rs` — integration test
- `src/components/settings/panels/RuntimePanel.tsx` — diagnostic panel showing active resolver
- `docs/superpowers/specs/2026-05-13-bundled-runtime-spec.md` — short spec doc cross-linked from this plan

**Modify:**

- `src-tauri/tauri.conf.json` — add `bundle.resources` entry for `resources/runtime/<platform>/`
- `src-tauri/src/runtime/dependencies/mod.rs` — export new types
- `src-tauri/src/runtime/dependencies/manager.rs` — accept a primary resolver param, only fall back to OSS install on hard miss
- `src-tauri/src/lib.rs:64-93` — wire bundled resolver first; OSS background-ensure becomes opt-in upgrade
- `scripts/build-and-sign-macos.sh` — call `prepare-bundled-runtime.sh` before `pnpm tauri build`; extend inside-out codesign to walk `Contents/Resources/runtime/`
- `scripts/release-windows.ps1` — staging URL no longer used as runtime fallback; signtool walks `resources\runtime\` exe files
- `.github/workflows/build-desktop.yml` — run `prepare-bundled-runtime.ps1` before `pnpm tauri build`
- `src-tauri/src/transport/tauri_commands/runtime.rs` — add `runtime_diagnostics` command returning active resolver + bundled version
- `src/lib/tauri.ts` — wrap the new command
- `src/components/settings/panels/AboutPanel.tsx` — link to the new RuntimePanel
- `CLAUDE.md` — record the new layout + prep step
- `.gitignore` — exclude `src-tauri/resources/runtime/` (it's built artifact, not source)

**Out of scope (not in this plan):**

- First-launch progress UI / retry dialog (separate PR1 if you want it later)
- Reset-AIjia button
- Runtime auto-update from OSS while app is running (current background ensure stays but becomes informational)

---

## Resolver Layout (locked in)

After this plan ships, the runtime resolution chain at startup is:

```
RuntimeManager::workspace_dependencies()
  ├─ tries: BundledRuntimeResolver(resource_dir/runtime/<platform>/)
  │       └─ Success → done. App ready offline. 95% of users land here.
  ├─ tries: InstalledRuntimeResolver(~/.renlijia/runtimes/renlijia-primary-runtime/current)
  │       └─ Success → done. Users who manually triggered an upgrade.
  └─ falls back: OSS install_from_manifest (only on hard double-miss)
          └─ Network failure here is now non-fatal: bundled was missing AND OSS installed was missing.
            Surfaces as a clear "runtime corrupted, reinstall AIjia" toast.
```

The bundled directory layout under `src-tauri/resources/runtime/<platform>/` mirrors what `RuntimeLayout::workspace_dependencies()` already expects:

```
darwin-arm64/
  node/bin/node             (signed Mach-O)
  node/bin/npm              (#!/usr/bin/env node + node-shim)
  node/bin/npx
  node/lib/node_modules/
  python/bin/python3        (signed Mach-O from python-build-standalone)
  python/lib/python3.12/    (stdlib)
  python/lib/site-packages/ (empty placeholder; uv installs here)
  uv/bin/uv
  uv/bin/uvx
  bundled-version.json      ({"bundleVersion": "2026.05.13-runtime.1", "platform": "darwin-arm64", ...})

win32-x64/
  node/node.exe
  node/npm.cmd
  node/npx.cmd
  node/node_modules/
  python/python.exe         (python-3.12.x-embed-amd64 zip extracted)
  python/python312.zip      (stdlib)
  python/Lib/site-packages/
  uv/uv.exe
  uv/uvx.exe
  bundled-version.json
```

Pinned versions for the first cut:

- Node.js: **20.18.0 LTS** (Iron) — already what CI uses
- Python: **3.12.7** via [python-build-standalone](https://github.com/indygreg/python-build-standalone/releases) on mac, embeddable zip on Windows
- uv: **0.4.27** (Astral) — pinned, can bump per release

---

## Phase 0 — Worktree + Plan Skeleton

### Task 0.1: Create isolated worktree

**Files:** none (git operation)

- [ ] **Step 1: Use superpowers:using-git-worktrees to spawn a worktree on branch `feat/bundled-runtime`**

Expected: agent reports new worktree at `code/.worktrees/feat-bundled-runtime` with branch checked out.

- [ ] **Step 2: Verify clean baseline**

```bash
cd code/.worktrees/feat-bundled-runtime
cargo check -p aijia 2>&1 | tail -5
pnpm test --run 2>&1 | tail -3
```

Expected: `cargo check` finishes without errors; vitest exits 0.

- [ ] **Step 3: Commit empty plan link**

```bash
git add docs/superpowers/plans/2026-05-13-bundle-runtime-into-installer.md
git commit -m "plan: bundled runtime in installer (skeleton)"
```

---

## Phase 1 — Upstream sources manifest

### Task 1.1: Write `scripts/runtime-sources.json`

**Files:**
- Create: `code/scripts/runtime-sources.json`

- [ ] **Step 1: Write the manifest**

```json
{
  "bundleVersion": "2026.05.13-runtime.1",
  "node": {
    "version": "20.18.0",
    "platforms": {
      "darwin-arm64": {
        "url": "https://nodejs.org/dist/v20.18.0/node-v20.18.0-darwin-arm64.tar.gz",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      },
      "darwin-x64": {
        "url": "https://nodejs.org/dist/v20.18.0/node-v20.18.0-darwin-x64.tar.gz",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      },
      "win32-x64": {
        "url": "https://nodejs.org/dist/v20.18.0/node-v20.18.0-win-x64.zip",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      }
    }
  },
  "python": {
    "version": "3.12.7",
    "release": "20241016",
    "platforms": {
      "darwin-arm64": {
        "url": "https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-aarch64-apple-darwin-install_only.tar.gz",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      },
      "darwin-x64": {
        "url": "https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-x86_64-apple-darwin-install_only.tar.gz",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      },
      "win32-x64": {
        "url": "https://www.python.org/ftp/python/3.12.7/python-3.12.7-embed-amd64.zip",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      }
    }
  },
  "uv": {
    "version": "0.4.27",
    "platforms": {
      "darwin-arm64": {
        "url": "https://github.com/astral-sh/uv/releases/download/0.4.27/uv-aarch64-apple-darwin.tar.gz",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      },
      "darwin-x64": {
        "url": "https://github.com/astral-sh/uv/releases/download/0.4.27/uv-x86_64-apple-darwin.tar.gz",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      },
      "win32-x64": {
        "url": "https://github.com/astral-sh/uv/releases/download/0.4.27/uv-x86_64-pc-windows-msvc.zip",
        "sha256": "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT"
      }
    }
  }
}
```

The `AUTO-FILL-AT-FIRST-RUN-AND-COMMIT` placeholders will be replaced by Task 2.3 the first time the prep script runs (it downloads, computes sha256, and asks the human to commit the JSON).

- [ ] **Step 2: Commit**

```bash
git add scripts/runtime-sources.json
git commit -m "feat(runtime): pin upstream sources for bundled runtime"
```

---

## Phase 2 — Prep script (downloads + lays out resources/runtime/)

### Task 2.1: Write `scripts/prepare-bundled-runtime.sh` (macOS/Linux)

**Files:**
- Create: `code/scripts/prepare-bundled-runtime.sh`

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
# Build src-tauri/resources/runtime/<platform>/ from upstream sources.
#
# Inputs: scripts/runtime-sources.json (pinned upstream URLs + sha256).
# Output: src-tauri/resources/runtime/<platform>/ with node/, python/, uv/, bundled-version.json.
#
# Usage:
#   bash scripts/prepare-bundled-runtime.sh                 # auto-detect platform
#   PLATFORM=darwin-x64 bash scripts/prepare-bundled-runtime.sh
#
# Re-runs are idempotent: if the target dir already has the right bundled-version.json
# the script exits 0 without re-downloading.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
SOURCES="$SCRIPT_DIR/runtime-sources.json"
CACHE_DIR="${RUNTIME_PREP_CACHE:-$PROJECT_DIR/.runtime-cache}"

if ! command -v jq >/dev/null; then
  echo "ERROR: jq required (brew install jq)" >&2
  exit 2
fi

# Detect platform
plat="${PLATFORM:-}"
if [ -z "$plat" ]; then
  uname_s="$(uname -s | tr '[:upper:]' '[:lower:]')"
  uname_m="$(uname -m)"
  case "$uname_s-$uname_m" in
    darwin-arm64|darwin-aarch64) plat="darwin-arm64" ;;
    darwin-x86_64) plat="darwin-x64" ;;
    *) echo "ERROR: unsupported platform $uname_s-$uname_m" >&2; exit 2 ;;
  esac
fi

bundle_version="$(jq -r '.bundleVersion' "$SOURCES")"
out_dir="$PROJECT_DIR/src-tauri/resources/runtime/$plat"
existing_version=""
if [ -f "$out_dir/bundled-version.json" ]; then
  existing_version="$(jq -r '.bundleVersion' "$out_dir/bundled-version.json" 2>/dev/null || echo "")"
fi
if [ "$existing_version" = "$bundle_version" ] && [ "${FORCE:-0}" != "1" ]; then
  echo "[prepare-runtime] $plat already at $bundle_version, skipping (FORCE=1 to override)"
  exit 0
fi

mkdir -p "$CACHE_DIR" "$out_dir"

fetch() {
  local url="$1" sha="$2" name="$3"
  local cached="$CACHE_DIR/$name"
  if [ -f "$cached" ]; then
    local got
    got="$(shasum -a 256 "$cached" | awk '{print $1}')"
    if [ "$got" = "$sha" ]; then
      echo "[cache-hit] $name"
      printf '%s' "$cached"
      return 0
    fi
    echo "[cache-stale] $name (got $got, expected $sha) — refetching" >&2
    rm -f "$cached"
  fi
  echo "[download] $url" >&2
  curl -fSL --retry 3 -o "$cached" "$url"
  local got
  got="$(shasum -a 256 "$cached" | awk '{print $1}')"
  if [ "$sha" = "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT" ]; then
    echo "[NOTE] computed sha256 for $name: $got" >&2
    echo "       Edit scripts/runtime-sources.json to pin this value, then re-run." >&2
  elif [ "$got" != "$sha" ]; then
    echo "ERROR: sha256 mismatch for $name (got $got, expected $sha)" >&2
    exit 1
  fi
  printf '%s' "$cached"
}

extract_node() {
  local tarball="$1"
  rm -rf "$out_dir/node"
  mkdir -p "$out_dir/node"
  tar -xzf "$tarball" -C "$out_dir/node" --strip-components=1
  # Sanity
  test -x "$out_dir/node/bin/node" || { echo "ERROR: node binary missing after extract" >&2; exit 1; }
  test -x "$out_dir/node/bin/npm"  || { echo "ERROR: npm shim missing" >&2; exit 1; }
}

extract_python_mac() {
  local tarball="$1"
  rm -rf "$out_dir/python"
  mkdir -p "$out_dir/python"
  # python-build-standalone install_only tarball contains a top-level `python/` dir
  tar -xzf "$tarball" -C "$out_dir/python" --strip-components=1
  test -x "$out_dir/python/bin/python3" || { echo "ERROR: python3 missing" >&2; exit 1; }
  # Ensure site-packages exists for uv to install into
  mkdir -p "$out_dir/python/lib/site-packages"
}

extract_uv() {
  local tarball="$1"
  rm -rf "$out_dir/uv"
  mkdir -p "$out_dir/uv/bin"
  local tmp
  tmp="$(mktemp -d)"
  tar -xzf "$tarball" -C "$tmp"
  # uv tarball top-level dir is uv-aarch64-apple-darwin/
  find "$tmp" -maxdepth 2 -name uv -type f -exec cp {} "$out_dir/uv/bin/uv" \;
  find "$tmp" -maxdepth 2 -name uvx -type f -exec cp {} "$out_dir/uv/bin/uvx" \;
  chmod +x "$out_dir/uv/bin/uv" "$out_dir/uv/bin/uvx"
  rm -rf "$tmp"
}

node_url="$(jq -r ".node.platforms[\"$plat\"].url" "$SOURCES")"
node_sha="$(jq -r ".node.platforms[\"$plat\"].sha256" "$SOURCES")"
python_url="$(jq -r ".python.platforms[\"$plat\"].url" "$SOURCES")"
python_sha="$(jq -r ".python.platforms[\"$plat\"].sha256" "$SOURCES")"
uv_url="$(jq -r ".uv.platforms[\"$plat\"].url" "$SOURCES")"
uv_sha="$(jq -r ".uv.platforms[\"$plat\"].sha256" "$SOURCES")"

node_tar="$(fetch "$node_url" "$node_sha" "node-$plat.tar.gz")"
python_tar="$(fetch "$python_url" "$python_sha" "python-$plat.tar.gz")"
uv_tar="$(fetch "$uv_url" "$uv_sha" "uv-$plat.tar.gz")"

extract_node "$node_tar"
extract_python_mac "$python_tar"
extract_uv "$uv_tar"

# Empty node_modules placeholder so RuntimeLayout::validate sees the dir
mkdir -p "$out_dir/node/lib/node_modules"

# Write bundled-version.json
cat > "$out_dir/bundled-version.json" <<JSON
{
  "bundleVersion": "$bundle_version",
  "platform": "$plat",
  "node": "$(jq -r .node.version "$SOURCES")",
  "python": "$(jq -r .python.version "$SOURCES")",
  "uv": "$(jq -r .uv.version "$SOURCES")"
}
JSON

echo "[prepare-runtime] OK: $out_dir at $bundle_version"
```

- [ ] **Step 2: Make executable and run on local mac**

```bash
chmod +x scripts/prepare-bundled-runtime.sh
bash scripts/prepare-bundled-runtime.sh
```

Expected: First run prints `[NOTE] computed sha256 for ...` three times. Copy those values into `scripts/runtime-sources.json`, then re-run.

- [ ] **Step 3: After sha256 values are pinned, verify second run is idempotent**

```bash
bash scripts/prepare-bundled-runtime.sh
```

Expected: `[prepare-runtime] darwin-arm64 already at 2026.05.13-runtime.1, skipping`.

- [ ] **Step 4: Verify the layout**

```bash
src-tauri/resources/runtime/darwin-arm64/node/bin/node --version
src-tauri/resources/runtime/darwin-arm64/python/bin/python3 --version
src-tauri/resources/runtime/darwin-arm64/uv/bin/uv --version
```

Expected: prints `v20.18.0`, `Python 3.12.7`, `uv 0.4.27`.

- [ ] **Step 5: Add `.gitignore` rule and commit script (not artifact)**

```bash
echo 'src-tauri/resources/runtime/' >> .gitignore
echo '.runtime-cache/' >> .gitignore
git add scripts/prepare-bundled-runtime.sh scripts/runtime-sources.json .gitignore
git commit -m "feat(build): prepare bundled runtime from pinned upstream sources (macOS)"
```

### Task 2.2: Write `scripts/prepare-bundled-runtime.ps1` (Windows)

**Files:**
- Create: `code/scripts/prepare-bundled-runtime.ps1`

- [ ] **Step 1: Write the PowerShell script**

```powershell
# Build src-tauri/resources/runtime/win32-x64/ from upstream sources.
#
# Inputs: scripts/runtime-sources.json
# Output: src-tauri/resources/runtime/win32-x64/{node,python,uv}/ + bundled-version.json
#
# Usage:
#   .\scripts\prepare-bundled-runtime.ps1
#   $env:FORCE=1; .\scripts\prepare-bundled-runtime.ps1

$ErrorActionPreference = "Stop"

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir  = Split-Path -Parent $ScriptDir
$Sources     = Join-Path $ScriptDir "runtime-sources.json"
$CacheDir    = if ($env:RUNTIME_PREP_CACHE) { $env:RUNTIME_PREP_CACHE } else { Join-Path $ProjectDir ".runtime-cache" }
$Plat        = "win32-x64"
$OutDir      = Join-Path $ProjectDir "src-tauri\resources\runtime\$Plat"

$src = Get-Content $Sources -Raw | ConvertFrom-Json
$BundleVersion = $src.bundleVersion

if ((Test-Path "$OutDir\bundled-version.json") -and ($env:FORCE -ne "1")) {
    $existing = (Get-Content "$OutDir\bundled-version.json" -Raw | ConvertFrom-Json).bundleVersion
    if ($existing -eq $BundleVersion) {
        Write-Host "[prepare-runtime] $Plat already at $BundleVersion, skipping (FORCE=1 to override)"
        exit 0
    }
}

New-Item -ItemType Directory -Force -Path $CacheDir, $OutDir | Out-Null

function Fetch-File {
    param([string]$Url, [string]$Sha, [string]$Name)
    $cached = Join-Path $CacheDir $Name
    if (Test-Path $cached) {
        $got = (Get-FileHash $cached -Algorithm SHA256).Hash.ToLower()
        if ($got -eq $Sha.ToLower()) {
            Write-Host "[cache-hit] $Name"
            return $cached
        }
        Write-Host "[cache-stale] $Name (got $got, expected $Sha) — refetching"
        Remove-Item $cached
    }
    Write-Host "[download] $Url"
    Invoke-WebRequest -Uri $Url -OutFile $cached -UseBasicParsing
    $got = (Get-FileHash $cached -Algorithm SHA256).Hash.ToLower()
    if ($Sha -eq "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT") {
        Write-Host "[NOTE] computed sha256 for ${Name}: $got"
        Write-Host "       Edit scripts/runtime-sources.json to pin this value, then re-run."
    } elseif ($got -ne $Sha.ToLower()) {
        throw "sha256 mismatch for $Name (got $got, expected $Sha)"
    }
    return $cached
}

function Extract-Zip {
    param([string]$Zip, [string]$Dest)
    if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
    New-Item -ItemType Directory -Force -Path $Dest | Out-Null
    Expand-Archive -Path $Zip -DestinationPath $Dest -Force
}

# Node
$nodeUrl = $src.node.platforms.$Plat.url
$nodeSha = $src.node.platforms.$Plat.sha256
$nodeZip = Fetch-File $nodeUrl $nodeSha "node-$Plat.zip"
$nodeTmp = Join-Path $env:TEMP "node-extract-$([Guid]::NewGuid().ToString('N'))"
Extract-Zip $nodeZip $nodeTmp
# zip contains node-v20.18.0-win-x64/ as top-level
$nodeRoot = Get-ChildItem $nodeTmp -Directory | Select-Object -First 1
if (Test-Path "$OutDir\node") { Remove-Item -Recurse -Force "$OutDir\node" }
Move-Item $nodeRoot.FullName "$OutDir\node"
Remove-Item -Recurse -Force $nodeTmp
if (-not (Test-Path "$OutDir\node\node.exe")) { throw "node.exe missing after extract" }

# Python (embeddable zip — flat layout, no nested dir)
$pyUrl = $src.python.platforms.$Plat.url
$pySha = $src.python.platforms.$Plat.sha256
$pyZip = Fetch-File $pyUrl $pySha "python-$Plat.zip"
if (Test-Path "$OutDir\python") { Remove-Item -Recurse -Force "$OutDir\python" }
Extract-Zip $pyZip "$OutDir\python"
if (-not (Test-Path "$OutDir\python\python.exe")) { throw "python.exe missing after extract" }
# Enable site-packages: embeddable disables site by default; patch python312._pth
$pthPath = Get-ChildItem "$OutDir\python" -Filter "python*._pth" | Select-Object -First 1
if ($pthPath) {
    $content = Get-Content $pthPath.FullName
    if ($content -notcontains "import site") {
        Add-Content -Path $pthPath.FullName -Value "`r`nimport site`r`nLib\site-packages"
    }
}
New-Item -ItemType Directory -Force -Path "$OutDir\python\Lib\site-packages" | Out-Null

# uv
$uvUrl = $src.uv.platforms.$Plat.url
$uvSha = $src.uv.platforms.$Plat.sha256
$uvZip = Fetch-File $uvUrl $uvSha "uv-$Plat.zip"
$uvTmp = Join-Path $env:TEMP "uv-extract-$([Guid]::NewGuid().ToString('N'))"
Extract-Zip $uvZip $uvTmp
if (Test-Path "$OutDir\uv") { Remove-Item -Recurse -Force "$OutDir\uv" }
New-Item -ItemType Directory -Force -Path "$OutDir\uv" | Out-Null
Get-ChildItem $uvTmp -Recurse -Include "uv.exe","uvx.exe" | ForEach-Object {
    Copy-Item $_.FullName "$OutDir\uv\$($_.Name)"
}
Remove-Item -Recurse -Force $uvTmp

# Empty node_modules placeholder
New-Item -ItemType Directory -Force -Path "$OutDir\node\node_modules" | Out-Null

# bundled-version.json
@{
    bundleVersion = $BundleVersion
    platform      = $Plat
    node          = $src.node.version
    python        = $src.python.version
    uv            = $src.uv.version
} | ConvertTo-Json | Set-Content "$OutDir\bundled-version.json"

Write-Host "[prepare-runtime] OK: $OutDir at $BundleVersion"
```

- [ ] **Step 2: Commit (cannot test locally on mac — Windows CI will exercise it in Phase 5)**

```bash
git add scripts/prepare-bundled-runtime.ps1
git commit -m "feat(build): prepare bundled runtime for Windows"
```

### Task 2.3: Pin all sha256 values

**Files:**
- Modify: `code/scripts/runtime-sources.json`

- [ ] **Step 1: Run mac prep twice (arm64 first run prints SHA, then pin, then second run verifies)**

```bash
bash scripts/prepare-bundled-runtime.sh
# Copy the three [NOTE] sha256 lines into scripts/runtime-sources.json for darwin-arm64.
# Then re-run:
bash scripts/prepare-bundled-runtime.sh
```

Expected: second run completes the full extraction without `[NOTE]` lines.

- [ ] **Step 2: For darwin-x64 and win32-x64, compute SHAs without downloading by running on those platforms — OR pre-compute via curl:**

```bash
for url in \
  "https://nodejs.org/dist/v20.18.0/node-v20.18.0-darwin-x64.tar.gz" \
  "https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-x86_64-apple-darwin-install_only.tar.gz" \
  "https://github.com/astral-sh/uv/releases/download/0.4.27/uv-x86_64-apple-darwin.tar.gz" \
  "https://nodejs.org/dist/v20.18.0/node-v20.18.0-win-x64.zip" \
  "https://www.python.org/ftp/python/3.12.7/python-3.12.7-embed-amd64.zip" \
  "https://github.com/astral-sh/uv/releases/download/0.4.27/uv-x86_64-pc-windows-msvc.zip" ; do
  echo "$url"
  curl -fsSL "$url" | shasum -a 256 | awk '{print "  sha256:", $1}'
done
```

Paste each sha into the matching platform entry of `scripts/runtime-sources.json`.

- [ ] **Step 3: Commit pinned SHAs**

```bash
git add scripts/runtime-sources.json
git commit -m "feat(runtime): pin sha256 for all 9 upstream artifacts"
```

---

## Phase 3 — Bundled resolver in Rust

### Task 3.1: Write failing test for `BundledRuntimeResolver`

**Files:**
- Create: `code/src-tauri/tests/bundled_runtime_resolver_test.rs`

- [ ] **Step 1: Write the failing test**

```rust
//! Verifies BundledRuntimeResolver reads from a resource_dir-style layout
//! and yields valid WorkspaceDependencies for the current platform.

use std::fs;
use tempfile::TempDir;

use app_lib::runtime::dependencies::{
    BundledRuntimeResolver, RuntimePlatform, RuntimeResolver,
};

#[test]
fn bundled_resolver_finds_runtime_for_current_platform() {
    let tmp = TempDir::new().unwrap();
    let resource_dir = tmp.path();
    let platform = RuntimePlatform::current().expect("platform detection");
    let plat_key = platform.manifest_key();
    let runtime_dir = resource_dir.join("runtime").join(plat_key);

    // Lay out per the spec
    let layout = app_lib::runtime::dependencies::RuntimeLayout::for_platform(platform);
    let deps = layout.workspace_dependencies(&runtime_dir);
    for path in [&deps.python, &deps.node, &deps.npm, &deps.npx, &deps.uv, &deps.uvx] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
    }
    for dir in [&deps.node_modules, &deps.python_site_packages] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        runtime_dir.join("bundled-version.json"),
        br#"{"bundleVersion":"test-1","platform":"placeholder"}"#,
    )
    .unwrap();

    let resolver = BundledRuntimeResolver::new(resource_dir.to_path_buf());
    let resolved = resolver.workspace_dependencies().expect("resolves");
    assert_eq!(resolved.node, deps.node);
    assert_eq!(resolved.python, deps.python);
}

#[test]
fn bundled_resolver_errors_when_runtime_dir_missing() {
    let tmp = TempDir::new().unwrap();
    let resolver = BundledRuntimeResolver::new(tmp.path().to_path_buf());
    let err = resolver.workspace_dependencies().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("bundled runtime") || msg.contains("not found"),
        "unexpected error: {msg}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd src-tauri && cargo test --test bundled_runtime_resolver_test 2>&1 | tail -20
```

Expected: FAIL with `cannot find ... BundledRuntimeResolver` (type does not yet exist).

### Task 3.2: Implement `BundledRuntimeResolver`

**Files:**
- Create: `code/src-tauri/src/runtime/dependencies/bundled_resolver.rs`
- Modify: `code/src-tauri/src/runtime/dependencies/mod.rs`

- [ ] **Step 1: Create the module**

```rust
//! `BundledRuntimeResolver` reads node/python/uv from the app's resource dir.
//! Layout: `<resource_dir>/runtime/<platform>/{node,python,uv}/...`
//! Populated at build time by `scripts/prepare-bundled-runtime.{sh,ps1}`.

use std::path::PathBuf;

use super::{
    RuntimeDependencyError, RuntimeDependencyResult, RuntimeLayout, RuntimePlatform,
    RuntimePlatformError, RuntimeResolver, WorkspaceDependencies,
};

#[derive(Debug, Clone)]
pub struct BundledRuntimeResolver {
    resource_dir: PathBuf,
}

impl BundledRuntimeResolver {
    pub fn new(resource_dir: PathBuf) -> Self {
        Self { resource_dir }
    }

    pub fn runtime_dir(&self) -> RuntimeDependencyResult<PathBuf> {
        let platform = RuntimePlatform::current().map_err(platform_err)?;
        Ok(self.resource_dir.join("runtime").join(platform.manifest_key()))
    }

    pub fn bundled_version(&self) -> Option<String> {
        let dir = self.runtime_dir().ok()?;
        let raw = std::fs::read_to_string(dir.join("bundled-version.json")).ok()?;
        let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
        json.get("bundleVersion")?.as_str().map(str::to_string)
    }
}

impl RuntimeResolver for BundledRuntimeResolver {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        let runtime_dir = self.runtime_dir()?;
        if !runtime_dir.is_dir() {
            return Err(RuntimeDependencyError::ResolverUnavailable(format!(
                "bundled runtime dir not found: {}",
                runtime_dir.display()
            )));
        }
        let platform = RuntimePlatform::current().map_err(platform_err)?;
        let deps = WorkspaceDependencies::from_install_dir_for_platform(&runtime_dir, platform)?;
        validate_existing(&deps)?;
        Ok(deps)
    }
}

fn platform_err(e: RuntimePlatformError) -> RuntimeDependencyError {
    RuntimeDependencyError::ResolverUnavailable(e.to_string())
}

fn validate_existing(deps: &WorkspaceDependencies) -> RuntimeDependencyResult<()> {
    for (field, path) in [
        ("node", &deps.node),
        ("npm", &deps.npm),
        ("npx", &deps.npx),
        ("python", &deps.python),
        ("uv", &deps.uv),
        ("uvx", &deps.uvx),
    ] {
        if !path.is_file() {
            return Err(RuntimeDependencyError::MissingExecutable {
                field,
                path: path.clone(),
            });
        }
    }
    for (field, path) in [
        ("node_modules", &deps.node_modules),
        ("python_site_packages", &deps.python_site_packages),
    ] {
        if !path.is_dir() {
            return Err(RuntimeDependencyError::MissingExecutable {
                field,
                path: path.clone(),
            });
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Export it from `mod.rs`**

```rust
// add to src-tauri/src/runtime/dependencies/mod.rs after the existing pub use lines:
pub use bundled_resolver::BundledRuntimeResolver;
```

And declare the module at the top of `mod.rs`:

```rust
mod bundled_resolver;
```

- [ ] **Step 3: Run test to verify it passes**

```bash
cd src-tauri && cargo test --test bundled_runtime_resolver_test 2>&1 | tail -10
```

Expected: `test result: ok. 2 passed`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/dependencies/bundled_resolver.rs \
        src-tauri/src/runtime/dependencies/mod.rs \
        src-tauri/tests/bundled_runtime_resolver_test.rs
git commit -m "feat(runtime): BundledRuntimeResolver reads from app resource_dir"
```

---

## Phase 4 — Chain bundled resolver into RuntimeManager

### Task 4.1: Write failing test for chained resolution

**Files:**
- Create test inside: `code/src-tauri/tests/bundled_runtime_resolver_test.rs` (append)

- [ ] **Step 1: Append test**

```rust
#[test]
fn chain_falls_back_to_installed_when_bundled_missing() {
    use app_lib::runtime::dependencies::{ChainResolver, InstalledRuntimeResolver};

    let tmp = TempDir::new().unwrap();
    // bundled: nothing under tmp/resources, so it'll fail
    let bundled = BundledRuntimeResolver::new(tmp.path().join("resources"));
    // installed: build a fake bundle_root with current pointer + version dir
    let bundle_root = tmp.path().join("renlijia-primary-runtime");
    let version = "2026.05.13-runtime.1";
    let install_dir = bundle_root.join("versions").join(version);
    fs::create_dir_all(&bundle_root).unwrap();
    fs::write(bundle_root.join("current"), format!("versions/{version}")).unwrap();

    let platform = RuntimePlatform::current().unwrap();
    let layout = app_lib::runtime::dependencies::RuntimeLayout::for_platform(platform);
    let deps = layout.workspace_dependencies(&install_dir);
    for p in [&deps.python, &deps.node, &deps.npm, &deps.npx, &deps.uv, &deps.uvx] {
        if let Some(parent) = p.parent() { fs::create_dir_all(parent).unwrap(); }
        fs::write(p, b"").unwrap();
    }
    fs::create_dir_all(&deps.node_modules).unwrap();
    fs::create_dir_all(&deps.python_site_packages).unwrap();
    fs::write(install_dir.join("install.json"), b"{}").unwrap();

    let installed = InstalledRuntimeResolver::new(&bundle_root);
    let chain = ChainResolver::new(vec![Arc::new(bundled), Arc::new(installed)]);

    let resolved = chain.workspace_dependencies().expect("chain resolves via installed");
    assert_eq!(resolved.node, deps.node);
}
```

Also add at top of file:

```rust
use std::sync::Arc;
```

- [ ] **Step 2: Run to verify failure**

```bash
cd src-tauri && cargo test --test bundled_runtime_resolver_test chain_falls_back 2>&1 | tail -10
```

Expected: FAIL — `ChainResolver` not defined.

### Task 4.2: Implement `ChainResolver`

**Files:**
- Create: `code/src-tauri/src/runtime/dependencies/chain_resolver.rs`
- Modify: `code/src-tauri/src/runtime/dependencies/mod.rs`

- [ ] **Step 1: Write the chain resolver**

```rust
//! Try each resolver in order. First success wins. If all fail, return the
//! last error so callers see the deepest-fallback diagnostic.

use std::sync::Arc;

use super::{RuntimeDependencyError, RuntimeDependencyResult, RuntimeResolver, WorkspaceDependencies};

#[derive(Clone)]
pub struct ChainResolver {
    resolvers: Vec<Arc<dyn RuntimeResolver>>,
}

impl ChainResolver {
    pub fn new(resolvers: Vec<Arc<dyn RuntimeResolver>>) -> Self {
        Self { resolvers }
    }
}

impl std::fmt::Debug for ChainResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainResolver")
            .field("len", &self.resolvers.len())
            .finish()
    }
}

impl RuntimeResolver for ChainResolver {
    fn workspace_dependencies(&self) -> RuntimeDependencyResult<WorkspaceDependencies> {
        let mut last_err: Option<RuntimeDependencyError> = None;
        for (idx, resolver) in self.resolvers.iter().enumerate() {
            match resolver.workspace_dependencies() {
                Ok(deps) => {
                    log::info!(
                        "[runtime] chain resolved via resolver[{idx}] {:?}",
                        std::any::type_name_of_val(&**resolver)
                    );
                    return Ok(deps);
                }
                Err(e) => {
                    log::debug!("[runtime] resolver[{idx}] miss: {e}");
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            RuntimeDependencyError::ResolverUnavailable(
                "chain resolver has no entries".to_string(),
            )
        }))
    }
}
```

- [ ] **Step 2: Wire into `mod.rs`**

```rust
mod chain_resolver;
pub use chain_resolver::ChainResolver;
```

- [ ] **Step 3: Run test**

```bash
cd src-tauri && cargo test --test bundled_runtime_resolver_test 2>&1 | tail -10
```

Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/dependencies/chain_resolver.rs \
        src-tauri/src/runtime/dependencies/mod.rs \
        src-tauri/tests/bundled_runtime_resolver_test.rs
git commit -m "feat(runtime): ChainResolver for layered resolution"
```

### Task 4.3: Make `RuntimeManager` use injected primary resolver

**Files:**
- Modify: `code/src-tauri/src/runtime/dependencies/manager.rs`

- [ ] **Step 1: Add a `with_primary_resolver` builder**

In `manager.rs`, change the struct to hold `Arc<dyn RuntimeResolver>` instead of `InstalledRuntimeResolver`:

```rust
// replace:
//   resolver: InstalledRuntimeResolver,
// with:
    resolver: std::sync::Arc<dyn RuntimeResolver>,
    installed_resolver: InstalledRuntimeResolver,  // keep for OSS-install path
```

In `RuntimeManager::new`:

```rust
    pub fn new(paths: RuntimePaths, bundle_version: impl Into<String>) -> Self {
        let bundle_root = paths.bundle_root();
        let installed = InstalledRuntimeResolver::new(&bundle_root);
        Self {
            installer: RuntimeInstaller::new(paths.clone()),
            resolver: std::sync::Arc::new(installed.clone()),
            installed_resolver: installed,
            paths,
            bundle_version: bundle_version.into(),
            health_checker: RuntimeHealthChecker::default(),
            manifest_install: None,
            active_operation: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_primary_resolver(
        mut self,
        primary: std::sync::Arc<dyn RuntimeResolver>,
    ) -> Self {
        let chain = ChainResolver::new(vec![primary, std::sync::Arc::new(self.installed_resolver.clone())]);
        self.resolver = std::sync::Arc::new(chain);
        self
    }
```

(Imports: add `use super::ChainResolver;` if not already imported.)

Update the `workspace_dependencies` impl already in `manager.rs` (it currently calls `self.resolver.workspace_dependencies()` — that signature now goes through the chain transparently). Lines 103/109 just work because `Arc<dyn RuntimeResolver>` derefs correctly.

Update `pub fn resolver(&self) -> InstalledRuntimeResolver` → it currently returns the inner installed resolver; just return `self.installed_resolver.clone()`. Callers using it for the OSS install path keep working.

- [ ] **Step 2: Run `cargo check`**

```bash
cd src-tauri && cargo check 2>&1 | tail -20
```

Expected: clean compile. If type-mismatch errors appear, fix them in this step (single file).

- [ ] **Step 3: Run existing manager tests + the new chain test**

```bash
cd src-tauri && cargo test runtime::dependencies 2>&1 | tail -15
cd src-tauri && cargo test --test bundled_runtime_resolver_test 2>&1 | tail -5
```

Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/dependencies/manager.rs
git commit -m "refactor(runtime): RuntimeManager accepts injected primary resolver"
```

---

## Phase 5 — Wire bundled resolver at startup

### Task 5.1: Wire in `lib.rs`

**Files:**
- Modify: `code/src-tauri/src/lib.rs:64-95`

- [ ] **Step 1: Edit the setup block**

Replace the existing block (around lines 64–93 in the current version):

```rust
            app.manage(aijia_home.clone());
            let runtime_paths = runtime::dependencies::RuntimePaths::new(
                aijia_home.runtimes_dir(),
                "renlijia-primary-runtime",
            )
            .expect("Failed to initialize managed runtime paths");
            let platform = runtime::dependencies::RuntimePlatform::current()
                .expect("Failed to identify managed runtime platform");
            let manifest_url = runtime::dependencies::configured_runtime_manifest_url();

            // Bundled resolver: reads from app resource_dir/runtime/<platform>/
            let resource_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| aijia_home.root().to_path_buf());
            let bundled_resolver = std::sync::Arc::new(
                runtime::dependencies::BundledRuntimeResolver::new(resource_dir.clone()),
            );
            let bundled_version = bundled_resolver.bundled_version();
            log::info!(
                "[runtime] bundled resolver mounted at {} (version={:?})",
                resource_dir.display(),
                bundled_version
            );

            let runtime_manager: runtime::dependencies::ManagedRuntimeManager = Arc::new(
                runtime::dependencies::RuntimeManager::new(
                    runtime_paths.clone(),
                    env!("CARGO_PKG_VERSION"),
                )
                .with_primary_resolver(bundled_resolver.clone())
                .with_manifest_source(
                    runtime::dependencies::RuntimeManifestSource::Url(manifest_url),
                    "primary",
                    platform,
                ),
            );
            let runtime_resolver: runtime::dependencies::ManagedRuntimeResolver =
                runtime_manager.clone();

            // Only run background OSS ensure if bundled resolver could NOT satisfy.
            // Probe synchronously: if bundled resolves we skip the network entirely.
            let bundled_ok = bundled_resolver.workspace_dependencies().is_ok();
            if !bundled_ok {
                log::warn!("[runtime] bundled runtime unavailable; falling back to OSS ensure");
                let runtime_manager_bg = runtime_manager.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = runtime_manager_bg.ensure_managed().await {
                        log::warn!("[runtime] background ensure failed: {}", error);
                    }
                });
            } else {
                log::info!("[runtime] bundled runtime ready; OSS ensure skipped on this launch");
            }

            app.manage(runtime_manager.clone());
            app.manage(runtime_resolver.clone());
```

- [ ] **Step 2: Compile + run desktop in dev**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
# Run app once to confirm startup logs
pnpm tauri:dev
```

Expected log lines on launch:
```
[runtime] bundled resolver mounted at /.../resources (version=Some("2026.05.13-runtime.1"))
[runtime] bundled runtime ready; OSS ensure skipped on this launch
```

Stop the dev app once verified.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(runtime): prefer bundled resolver at startup; skip OSS ensure when available"
```

---

## Phase 6 — Tauri bundle config + .gitignore

### Task 6.1: Declare bundled runtime in `tauri.conf.json`

**Files:**
- Modify: `code/src-tauri/tauri.conf.json`

- [ ] **Step 1: Add to `bundle.resources`**

Replace the existing `resources` block:

```json
    "resources": {
      "prompts": "prompts",
      "resources/dws*": "",
      "resources/runtime": "runtime"
    },
```

The mapping `"resources/runtime": "runtime"` makes Tauri copy `src-tauri/resources/runtime/` into `<App>.app/Contents/Resources/runtime/` on mac and `<App>\resources\runtime\` on Windows.

- [ ] **Step 2: Run a local mac build to verify bundling**

```bash
bash scripts/prepare-bundled-runtime.sh
pnpm tauri build 2>&1 | tail -20
```

Expected: build succeeds; bundle path appears in output.

- [ ] **Step 3: Inspect the bundled app**

```bash
APP="src-tauri/target/release/bundle/macos/AIjia.app"
ls -la "$APP/Contents/Resources/runtime/darwin-arm64/" | head
"$APP/Contents/Resources/runtime/darwin-arm64/node/bin/node" --version
"$APP/Contents/Resources/runtime/darwin-arm64/python/bin/python3" --version
```

Expected: prints `v20.18.0` and `Python 3.12.7`.

- [ ] **Step 4: Verify .gitignore actually excludes runtime dir**

```bash
git status --porcelain src-tauri/resources/runtime | head -3
```

Expected: empty output (gitignore is working).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "feat(bundle): include resources/runtime/ in Tauri bundle"
```

---

## Phase 7 — Diagnostic Tauri command + UI panel

### Task 7.1: Write failing test for `runtime_diagnostics` command

**Files:**
- Modify: `code/src-tauri/src/transport/tauri_commands/runtime.rs` (append test module)

- [ ] **Step 1: Locate the current runtime.rs and add this near end (or open a new tests submodule)**

```rust
#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn diagnostics_payload_includes_active_resolver_and_bundled_version() {
        let payload = RuntimeDiagnosticsPayload {
            active_resolver: "bundled".to_string(),
            bundled_version: Some("2026.05.13-runtime.1".to_string()),
            installed_version: None,
            node: "v20.18.0".to_string(),
            python: "Python 3.12.7".to_string(),
            uv: "uv 0.4.27".to_string(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"activeResolver\":\"bundled\""));
        assert!(json.contains("2026.05.13-runtime.1"));
    }
}
```

- [ ] **Step 2: Run test to verify fail**

```bash
cd src-tauri && cargo test diagnostics_payload 2>&1 | tail -10
```

Expected: FAIL — `RuntimeDiagnosticsPayload` not defined.

### Task 7.2: Implement the command + payload

**Files:**
- Modify: `code/src-tauri/src/transport/tauri_commands/runtime.rs`
- Modify: `code/src-tauri/src/lib.rs` (register command)

- [ ] **Step 1: Add the payload + command**

In `transport/tauri_commands/runtime.rs`:

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiagnosticsPayload {
    pub active_resolver: String,        // "bundled" | "installed" | "none"
    pub bundled_version: Option<String>,
    pub installed_version: Option<String>,
    pub node: String,
    pub python: String,
    pub uv: String,
}

#[tauri::command]
pub async fn runtime_diagnostics(
    app: tauri::AppHandle,
) -> Result<RuntimeDiagnosticsPayload, String> {
    use crate::runtime::dependencies::{
        BundledRuntimeResolver, ManagedRuntimeManager, RuntimeResolver,
    };
    use tauri::Manager;

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("resource_dir: {e}"))?;
    let bundled = BundledRuntimeResolver::new(resource_dir);
    let bundled_version = bundled.bundled_version();

    let mgr = app
        .try_state::<ManagedRuntimeManager>()
        .ok_or("RuntimeManager not registered")?
        .inner()
        .clone();

    let deps = mgr
        .workspace_dependencies()
        .map_err(|e| format!("resolve failed: {e}"))?;

    let active = if bundled.workspace_dependencies().is_ok() {
        "bundled"
    } else if mgr.resolver().workspace_dependencies().is_ok() {
        "installed"
    } else {
        "none"
    };

    let node_v = run_version(&deps.node).await.unwrap_or_else(|| "unknown".into());
    let py_v = run_version(&deps.python).await.unwrap_or_else(|| "unknown".into());
    let uv_v = run_version(&deps.uv).await.unwrap_or_else(|| "unknown".into());

    Ok(RuntimeDiagnosticsPayload {
        active_resolver: active.to_string(),
        bundled_version,
        installed_version: read_installed_version(&app),
        node: node_v,
        python: py_v,
        uv: uv_v,
    })
}

async fn run_version(path: &std::path::Path) -> Option<String> {
    use crate::storage::process_ext::NoWindowExt;
    let out = tokio::process::Command::new(path)
        .arg("--version")
        .no_window()
        .output()
        .await
        .ok()?;
    let merged = if out.stdout.is_empty() { out.stderr } else { out.stdout };
    Some(String::from_utf8_lossy(&merged).trim().to_string())
}

fn read_installed_version(app: &tauri::AppHandle) -> Option<String> {
    use tauri::Manager;
    let home = app.try_state::<crate::storage::aijia_home::AiJiaHome>()?;
    let pointer = home.root().join("runtimes/renlijia-primary-runtime/current");
    let content = std::fs::read_to_string(&pointer).ok()?;
    Some(content.trim().trim_start_matches("versions/").to_string())
}
```

(If `run_version`/`read_installed_version` names collide with existing helpers in the file, prefix them with `diag_`.)

- [ ] **Step 2: Register in `lib.rs` invoke handler**

In `lib.rs`, find the `tauri::generate_handler![...]` list and add:

```rust
                commands::runtime::runtime_diagnostics,
```

(Verify path — it may be `crate::transport::tauri_commands::runtime::runtime_diagnostics` depending on existing module reexports.)

- [ ] **Step 3: Run unit test + cargo check**

```bash
cd src-tauri && cargo test diagnostics_payload 2>&1 | tail -5
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: test passes, clean build.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/runtime.rs src-tauri/src/lib.rs
git commit -m "feat(runtime): runtime_diagnostics tauri command"
```

### Task 7.3: Add TypeScript wrapper

**Files:**
- Modify: `code/src/lib/tauri.ts`

- [ ] **Step 1: Add wrapper**

Append near the other runtime commands:

```ts
export interface RuntimeDiagnostics {
  activeResolver: 'bundled' | 'installed' | 'none'
  bundledVersion: string | null
  installedVersion: string | null
  node: string
  python: string
  uv: string
}

export function runtimeDiagnostics(): Promise<RuntimeDiagnostics> {
  return invoke<RuntimeDiagnostics>('runtime_diagnostics')
}
```

- [ ] **Step 2: tsc + lint**

```bash
pnpm tsc -b 2>&1 | tail -5
pnpm lint 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(ui): runtimeDiagnostics TS wrapper"
```

### Task 7.4: Add the Runtime panel

**Files:**
- Create: `code/src/components/settings/panels/RuntimePanel.tsx`
- Modify: `code/src/components/settings/SettingsMenu.tsx` (add entry)

- [ ] **Step 1: Write the panel**

```tsx
import { useEffect, useState } from 'react'
import { runtimeDiagnostics, type RuntimeDiagnostics } from '@/lib/tauri'
import { Button } from '@/components/ui/button'

export function RuntimePanel() {
  const [data, setData] = useState<RuntimeDiagnostics | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const load = async () => {
    setLoading(true)
    setError(null)
    try {
      setData(await runtimeDiagnostics())
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { void load() }, [])

  return (
    <div className="flex flex-col gap-4 p-4">
      <header>
        <h2 className="text-sm font-semibold text-foreground">运行时</h2>
        <p className="text-xs text-muted-foreground">Node / Python / uv 状态</p>
      </header>
      {error && <div className="text-xs text-destructive">{error}</div>}
      {data && (
        <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
          <dt className="text-muted-foreground">来源</dt>
          <dd className="font-mono">{data.activeResolver}</dd>
          <dt className="text-muted-foreground">内置版本</dt>
          <dd className="font-mono">{data.bundledVersion ?? '—'}</dd>
          <dt className="text-muted-foreground">已安装升级版本</dt>
          <dd className="font-mono">{data.installedVersion ?? '—'}</dd>
          <dt className="text-muted-foreground">Node</dt>
          <dd className="font-mono">{data.node}</dd>
          <dt className="text-muted-foreground">Python</dt>
          <dd className="font-mono">{data.python}</dd>
          <dt className="text-muted-foreground">uv</dt>
          <dd className="font-mono">{data.uv}</dd>
        </dl>
      )}
      <Button variant="outline" onClick={load} disabled={loading} className="self-start">
        {loading ? '检查中…' : '重新检查'}
      </Button>
    </div>
  )
}
```

- [ ] **Step 2: Add to settings menu**

Inspect `src/components/settings/SettingsMenu.tsx`, locate the existing menu items array, and append an entry for the runtime panel (mirror an existing panel's pattern — registration is local convention).

- [ ] **Step 3: Smoke test in dev**

```bash
pnpm tauri:dev
```

Open Settings → 运行时 → verify panel shows `activeResolver: bundled`, `bundledVersion: 2026.05.13-runtime.1`, real `node --version` output.

- [ ] **Step 4: Commit**

```bash
git add src/components/settings/panels/RuntimePanel.tsx src/components/settings/SettingsMenu.tsx
git commit -m "feat(ui): RuntimePanel showing active resolver + versions"
```

---

## Phase 8 — macOS signing of bundled binaries

### Task 8.1: Extend inside-out codesign in `sign-and-upload-macos.sh`

**Files:**
- Modify: `code/scripts/sign-and-upload-macos.sh`

- [ ] **Step 1: Find the inside-out walk loop in the script**

```bash
grep -n "find .*-type f\|codesign --sign" scripts/sign-and-upload-macos.sh | head
```

Expected: existing loop iterates Mach-O files in `Contents/Resources`. Verify it does NOT exclude `runtime/`.

- [ ] **Step 2: If `runtime/` is excluded, remove the exclusion. Otherwise add explicit signing pass**

If the existing loop is `find "$APP/Contents" -type f -perm -u+x`, that already covers our binaries. Otherwise add after main signing:

```bash
echo "--- Signing bundled runtime binaries ---"
for bin in \
    "$APP/Contents/Resources/runtime/darwin-"*"/node/bin/node" \
    "$APP/Contents/Resources/runtime/darwin-"*"/python/bin/python3" \
    "$APP/Contents/Resources/runtime/darwin-"*"/uv/bin/uv" \
    "$APP/Contents/Resources/runtime/darwin-"*"/uv/bin/uvx"; do
    if [ -f "$bin" ]; then
        codesign --force --sign "$IDENTITY" --timestamp --options runtime "$bin"
    fi
done
# Walk dylibs inside python/lib
find "$APP/Contents/Resources/runtime" -name '*.dylib' -o -name '*.so' | while read -r lib; do
    codesign --force --sign "$IDENTITY" --timestamp --options runtime "$lib"
done
```

- [ ] **Step 3: Local test build + sign**

```bash
bash scripts/build-and-sign-macos.sh 0.5.23-dev beta 2>&1 | tail -30
```

Expected: signing pass completes; notarization step uploads and waits.

If notarization rejects any binary, the rejection email lists the path — add that path explicitly to the script's walk and re-run.

- [ ] **Step 4: Verify with `spctl`**

```bash
APP="src-tauri/target/release/bundle/macos/AIjia.app"
spctl --assess --type execute --verbose "$APP" 2>&1 | head
codesign -dv --verbose=4 "$APP/Contents/Resources/runtime/darwin-arm64/node/bin/node" 2>&1 | grep -E "Authority|flags"
```

Expected: `accepted (Notarized)` for app, `flags=0x10000(runtime)` + `Authority=Developer ID Application` for node binary.

- [ ] **Step 5: Commit**

```bash
git add scripts/sign-and-upload-macos.sh
git commit -m "build(mac): sign bundled runtime binaries inside-out"
```

---

## Phase 9 — CI integration (Windows GitHub Actions)

### Task 9.1: Add prep step to Windows workflow

**Files:**
- Modify: `code/.github/workflows/build-desktop.yml`

- [ ] **Step 1: Insert step before `Build Tauri app`**

After the `Setup dws CLI (Windows)` step, add:

```yaml
      - name: Cache bundled runtime sources
        uses: actions/cache@v4
        with:
          path: .runtime-cache
          key: runtime-sources-v1-${{ hashFiles('scripts/runtime-sources.json') }}
          restore-keys: runtime-sources-v1-

      - name: Prepare bundled runtime
        shell: pwsh
        run: .\scripts\prepare-bundled-runtime.ps1

      - name: Verify bundled runtime layout
        shell: pwsh
        run: |
          $base = "src-tauri\resources\runtime\win32-x64"
          if (-not (Test-Path "$base\node\node.exe")) { throw "node.exe missing" }
          if (-not (Test-Path "$base\python\python.exe")) { throw "python.exe missing" }
          if (-not (Test-Path "$base\uv\uv.exe")) { throw "uv.exe missing" }
          & "$base\node\node.exe" --version
          & "$base\python\python.exe" --version
          & "$base\uv\uv.exe" --version
```

- [ ] **Step 2: Commit + push to a test branch + trigger workflow_dispatch**

```bash
git add .github/workflows/build-desktop.yml
git commit -m "ci(windows): prepare bundled runtime before tauri build"
git push origin feat/bundled-runtime
gh workflow run build-desktop.yml --ref feat/bundled-runtime -f release_type=beta
gh run watch
```

Expected: workflow succeeds, the new "Verify bundled runtime layout" step prints all three versions.

- [ ] **Step 3: Download the produced unsigned exe, verify size**

The exe should be **~85–130 MB** vs the previous ~12 MB. Anything under 50 MB means runtime didn't get bundled.

```bash
ls -lh AIjia_*_x64-setup.exe
```

If size is wrong, inspect `tauri.conf.json` resources block and the prep script output.

### Task 9.2: Add prep step to macOS local script

**Files:**
- Modify: `code/scripts/build-and-sign-macos.sh`

- [ ] **Step 1: Add prep call inside `build_one_arch`**

Before `pnpm tauri build`:

```bash
    echo ""
    echo "--- Prepare bundled runtime ($arch) ---"
    if [ "$arch" = "x86_64" ]; then
        PLATFORM=darwin-x64 bash "$SCRIPT_DIR/prepare-bundled-runtime.sh"
    else
        PLATFORM=darwin-arm64 bash "$SCRIPT_DIR/prepare-bundled-runtime.sh"
    fi
```

- [ ] **Step 2: Commit**

```bash
git add scripts/build-and-sign-macos.sh
git commit -m "build(mac): run prepare-bundled-runtime before tauri build"
```

---

## Phase 10 — End-to-end smoke test on fresh VM

### Task 10.1: Windows fresh-install smoke test

**Files:** none (manual verification)

- [ ] **Step 1: Open Windows Sandbox (Win 10/11 Pro) or a clean Win11 VM**

Open Windows Sandbox via Start menu. (Reminder: Sandbox is wiped at close.)

- [ ] **Step 2: Disable internet inside the sandbox**

In Sandbox: Settings → Network → disable the adapter, OR run:
```powershell
Get-NetAdapter | Disable-NetAdapter -Confirm:$false
```

- [ ] **Step 3: Copy the signed exe into Sandbox and install**

Drag-drop `AIjia_*_x64-setup.exe` from host. Double-click to install with NSIS defaults.

- [ ] **Step 4: Launch AIjia**

Expected: app opens, no error toasts. Open Settings → 运行时 panel.

Pass criteria:
- `activeResolver: bundled`
- `node: v20.18.0`
- `python: Python 3.12.7`
- `uv: uv 0.4.27`

- [ ] **Step 5: Trigger an MCP / Python skill in the app**

Try a feature that uses Python (e.g., spawn a uvx tool). Should work entirely offline.

- [ ] **Step 6: Record results in spec file**

Append observations to `docs/superpowers/specs/2026-05-13-bundled-runtime-spec.md` (create if missing).

### Task 10.2: macOS fresh-user smoke test

- [ ] **Step 1: Create a new macOS user `aijia-test` via System Settings → Users & Groups**

- [ ] **Step 2: Log into `aijia-test`, disable Wi-Fi**

- [ ] **Step 3: Install the dmg, drag to Applications**

- [ ] **Step 4: Launch — verify Gatekeeper passes (notarization stapled)**

- [ ] **Step 5: Open Settings → 运行时, verify bundled resolver active**

- [ ] **Step 6: Test a skill that requires python**

Expected: works offline.

- [ ] **Step 7: Commit final notes**

```bash
git add docs/superpowers/specs/2026-05-13-bundled-runtime-spec.md
git commit -m "docs: bundled runtime end-to-end smoke test results"
```

---

## Phase 11 — Documentation + release notes

### Task 11.1: Update CLAUDE.md

**Files:**
- Modify: `code/CLAUDE.md`

- [ ] **Step 1: Add a section under "发布流程"**

Insert this block before "### 三种包":

```markdown
### 内置运行时（自 0.5.24 起）

每次发版前**必须**先跑 `scripts/prepare-bundled-runtime.{sh,ps1}`，把 Node 20.18 / Python 3.12.7 / uv 0.4.27 打入 `src-tauri/resources/runtime/<platform>/`。Tauri build 会把目录复制进安装包（~85MB 增量），用户首启完全离线可用。

- 升级 runtime 版本：编辑 `scripts/runtime-sources.json`，bump 各组件版本号 + `bundleVersion`，先在 mac 上跑一次记录 sha256，再 push 出 Windows CI sha。
- 加新依赖：扩展 `scripts/runtime-sources.json` 的 `runtimes` 块并改 prep 脚本 extract 逻辑。
- 启动顺序：`BundledRuntimeResolver`（resource_dir）→ `InstalledRuntimeResolver`（~/.renlijia/runtimes，OSS 升级路径）→ OSS download（兜底）。前两个任一成功即可，第三个只在两个都 miss 时跑。
- 诊断：Settings → 运行时 显示 `activeResolver`、版本号、可重新检查。
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(claude): bundled runtime workflow + diagnostic UI"
```

---

## Phase 12 — Open PR

### Task 12.1: Open PR with checklist

- [ ] **Step 1: Use superpowers:finishing-a-development-branch to decide merge strategy**

- [ ] **Step 2: PR description checklist**

```markdown
## Summary
Ship Node 20.18 / Python 3.12.7 / uv 0.4.27 inside the installer. First launch is 100% offline-capable.

## Architecture
- `BundledRuntimeResolver` reads from `resource_dir/runtime/<platform>/`
- `ChainResolver` tries bundled → installed → on-demand OSS
- OSS background ensure now only runs when bundled is missing

## Size impact
- macOS dmg: 12 MB → ~100 MB
- Windows nsis: 12 MB → ~120 MB

## Verification
- [x] cargo test runtime::dependencies — passes
- [x] cargo test --test bundled_runtime_resolver_test — passes (3 cases)
- [x] mac local build → spctl --assess accepted
- [x] Windows CI build → exe size ~120 MB
- [x] Windows Sandbox offline install → bundled resolver active
- [x] macOS fresh user offline install → bundled resolver active

## Follow-ups (not in this PR)
- First-launch progress UI when bundled is corrupted (rare)
- Settings → "下载最新运行时" button to trigger OSS upgrade path
- "重置 AIjia" button (separate spec)
```

---

## Self-Review Checklist (run after writing this plan)

1. **Spec coverage** — every requirement from our chat is addressed:
   - ✅ Bundle node/python/uv into installer (Phases 1–6)
   - ✅ Make bundled resolver primary (Phases 3–5)
   - ✅ OSS path remains as upgrade fallback (Phases 4–5)
   - ✅ Cross-platform CI integration (Phase 9)
   - ✅ macOS inside-out signing (Phase 8)
   - ✅ Diagnostic surface for support (Phase 7)
   - ✅ Smoke test on truly fresh machines (Phase 10)
   - ✅ Docs update (Phase 11)

2. **Placeholder scan** — no `TBD` / `add error handling` / `similar to Task N` left. SHA256 values in `runtime-sources.json` are explicitly marked as `AUTO-FILL-AT-FIRST-RUN-AND-COMMIT` with a defined Task 2.3 to pin them.

3. **Type consistency** —
   - `BundledRuntimeResolver` name used consistently across resolver test, module, command (Phases 3, 4, 5, 7).
   - `RuntimeDiagnosticsPayload` matches TS `RuntimeDiagnostics` field-for-field (camelCase via `serde(rename_all)`).
   - `activeResolver` values `"bundled"|"installed"|"none"` typed both in TS and Rust.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-13-bundle-runtime-into-installer.md`.
