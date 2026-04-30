# Global Skills Managed Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a non-blocking app-start global skill sync flow that downloads app-managed skills from an OSS manifest and safely installs them into `~/.renlijia/skills` with same-name overwrite.

**Architecture:** Match the existing managed runtime pattern at the workflow level: release-side script builds a versioned artifact + manifest fragment, app-side startup spawns a background sync that fetches the manifest, downloads and verifies the zip, stages extraction, validates `SKILL.md` directories, and replaces global skill directories with rollback. Keep this separate from Python/Node `RuntimeManager`, marketplace/custom skill commands, and user-scoped skill storage.

**Tech Stack:** Rust/Tauri 2 backend, `reqwest`, `zip`, existing `runtime::dependencies::verify_sha256`, existing `plugin::skill` SKILL.md parser/loader rules, Bash release script, Rust integration tests under `src-tauri/tests`.

---

## Confirmed Requirements

- Global only: install to `AiJiaHome::skills_dir()` (`~/.renlijia/skills`), never to `~/.renlijia/users/<scope>/skills`.
- OSS source: app-managed skills are hosted at a public OSS URL.
- Manifest flow: use a separate global skills manifest, not `RENLIJIA_RUNTIME_MANIFEST_URL`.
- Non-blocking startup: app startup must never wait for the skill sync to finish; failures are logged and skipped.
- Same-name overwrite: if a downloaded skill id already exists under `~/.renlijia/skills`, replace it.
- Safe overwrite: do not use existing delete-then-copy marketplace/custom install behavior; use staging + backup/rollback.
- Separate release script: add a new script alongside existing project scripts, not mixed into runtime/python/node scripts.
- Update check: startup background task fetches the small `global-skills-manifest.json`; if local `bundleVersion` matches remote `bundleVersion`, skip zip download and skip install.
- Current-run visibility: if the background sync installs a new version after the initial registry load, best-effort reload the disk skill registry; failure is non-fatal and next startup will see the updated skills.

## Current Runtime Pattern To Mirror

Existing Python/Node runtime flow:

- Release script: `scripts/runtime/build-runtime-artifact.sh`
- App startup init: `src-tauri/src/lib.rs:63` creates `RuntimeManager`
- Background ensure: `src-tauri/src/lib.rs:84` spawns `runtime_manager.ensure_managed().await`
- Manifest URL config: `src-tauri/src/runtime/dependencies/config.rs`
- Fetch/verify/install: `src-tauri/src/runtime/dependencies/manager.rs`, `src-tauri/src/runtime/dependencies/artifact_fetcher.rs`, `src-tauri/src/runtime/dependencies/installer.rs`
- Important behavior: download/install failure logs a warning and does not block app startup.

Global skills should mirror this pattern operationally, but remain in a separate skill-sync module.

## File Structure

### Create

- `scripts/skills/build-skills-artifact.sh`
  - Packages a prepared skills root into a zip artifact.
  - Emits a manifest fragment containing artifact URL, sha256, size, format, and version.
  - Does not install skills on user machines.

- `scripts/skills/README.md`
  - Documents release flow for global skill artifacts.

- `src-tauri/src/plugin/skill/global_sync.rs`
  - Owns global managed skill sync behavior.
  - Defines manifest/artifact/local `globalSkills` state structs.
  - Fetches manifest/artifact from OSS.
  - Verifies sha256/size.
  - Extracts zip safely.
  - Validates skill directories using existing SKILL.md rules.
  - Installs to global skills dir with staging + backup/rollback.
  - Provides a non-blocking startup helper.

- `src-tauri/tests/global_skill_sync_test.rs`
  - Tests pure filesystem install behavior, manifest parsing, global `state.json` preservation, and version-based update skipping.
  - Avoids real network.

### Modify

- `src-tauri/src/plugin/skill/mod.rs`
  - Export `global_sync`.

- `src-tauri/src/lib.rs`
  - Spawn background global skill sync after `aijia_home` is initialized and after `disk_skill_registry` is managed.
  - Do not await the sync from setup.

- Potentially `src-tauri/Cargo.toml`
  - Only if existing dependencies do not expose needed crates/features. Current code already uses `reqwest`, `zip`, `serde`, `serde_json`, and checksum helpers, so this should likely stay unchanged.

## Manifest Shape

Use a separate manifest, independent from runtime manifest:

```json
{
  "bundleVersion": "2026.04.28",
  "artifact": {
    "url": "https://rlj-cdn.oss-cn-hangzhou.aliyuncs.com/lotus/skills/renlijia-global-skills-2026.04.28.zip",
    "sha256": "<64 hex chars>",
    "sizeBytes": 123456,
    "archiveFormat": "zip"
  }
}
```

Local state file reuses the existing global state file:

```text
~/.renlijia/global/state.json
```

Local state is stored under the `globalSkills` key and must preserve existing keys such as `migrations`:

```json
{
  "migrations": {},
  "globalSkills": {
    "bundleVersion": "2026.04.28",
    "artifactSha256": "<64 hex chars>",
    "installedAtUnixSeconds": 1777342830
  }
}
```

Update rule:

```text
remote bundleVersion == local bundleVersion -> skip artifact download and install
remote bundleVersion != local bundleVersion -> download, verify, install, write local state
missing local state -> download, verify, install, write local state
```

Environment override:

```text
RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL=https://example.com/global-skills-manifest.json
```

Default URL:

```text
https://rlj-cdn.oss-cn-hangzhou.aliyuncs.com/lotus/skills/global-skills-manifest.json
```

## Artifact Shape

The zip should contain skill directories at the archive root:

```text
article-improver/SKILL.md
article-improver/scripts/...
youtube-fetcher/SKILL.md
```

The app-side extractor should also tolerate one wrapper directory:

```text
renlijia-global-skills-2026.04.28/article-improver/SKILL.md
renlijia-global-skills-2026.04.28/youtube-fetcher/SKILL.md
```

The release script should produce the root-direct layout by default.

---

## Task 1: Release Script For Global Skills Artifact

**Files:**
- Create: `scripts/skills/build-skills-artifact.sh`
- Create: `scripts/skills/README.md`

- [ ] **Step 1: Write the script**

Create `scripts/skills/build-skills-artifact.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  cat >&2 <<'USAGE'
Usage: build-skills-artifact.sh <skills-root> <bundle-version> <output-dir>

Packages prepared Renlijia global skills into a zip artifact and emits an
adjacent manifest fragment with sha256/size metadata. The skills root must
contain one directory per skill, and each installed skill directory must contain
SKILL.md.

Set RENLIJIA_GLOBAL_SKILLS_BASE_URL to override the public artifact base URL.
USAGE
  exit 2
fi

skills_root="$1"
bundle_version="$2"
out_dir="$3"
base_url="${RENLJ_GLOBAL_SKILLS_BASE_URL:-${RENLIJIA_GLOBAL_SKILLS_BASE_URL:-https://rlj-cdn.oss-cn-hangzhou.aliyuncs.com/lotus/skills}}"

if [[ ! -d "$skills_root" ]]; then
  echo "skills root is not a directory: $skills_root" >&2
  exit 1
fi

shopt -s nullglob
skill_dirs=("$skills_root"/*)
valid_count=0
for skill_dir in "${skill_dirs[@]}"; do
  [[ -d "$skill_dir" ]] || continue
  name="$(basename "$skill_dir")"
  if [[ "$name" == .* || "$name" == _* ]]; then
    continue
  fi
  if [[ ! "$name" =~ ^[a-z0-9][a-z0-9_-]{0,63}$ ]]; then
    echo "invalid skill directory name: $name" >&2
    exit 1
  fi
  if [[ ! -f "$skill_dir/SKILL.md" ]]; then
    echo "missing SKILL.md in skill directory: $skill_dir" >&2
    exit 1
  fi
  valid_count=$((valid_count + 1))
done

if [[ "$valid_count" -eq 0 ]]; then
  echo "no valid skill directories found under: $skills_root" >&2
  exit 1
fi

mkdir -p "$out_dir"
artifact="renlijia-global-skills-${bundle_version}.zip"
artifact_path="$out_dir/$artifact"
rm -f "$artifact_path"

(
  cd "$skills_root"
  zip -qr "$artifact_path" . -x "*/.DS_Store" ".DS_Store"
)

sha256="$(shasum -a 256 "$artifact_path" | awk '{print $1}')"
size_bytes="$(wc -c < "$artifact_path" | tr -d ' ')"
cat > "$out_dir/${artifact}.manifest-fragment.json" <<JSON
{
  "bundleVersion": "$bundle_version",
  "artifact": {
    "url": "$base_url/$artifact",
    "sha256": "$sha256",
    "sizeBytes": $size_bytes,
    "archiveFormat": "zip"
  }
}
JSON

echo "$artifact_path"
```

- [ ] **Step 2: Make the script executable**

Run:

```bash
chmod +x scripts/skills/build-skills-artifact.sh
```

Expected: command exits with status 0.

- [ ] **Step 3: Write the script README**

Create `scripts/skills/README.md`:

```markdown
# Global skills artifact 发布脚本

`build-skills-artifact.sh` 只负责把已经准备好的 app 自带全局 skills 目录打包成 zip，并生成 manifest fragment。它不会在用户机器上安装 skill，也不会修改 `~/.renlijia/skills`。

生产流程：

1. CI 或发布机准备完整目录：一个子目录对应一个 skill，每个 skill 必须包含 `SKILL.md`。
2. 运行：`scripts/skills/build-skills-artifact.sh <skills-root> <bundle-version> <output-dir>`。
3. 上传生成的 `renlijia-global-skills-<version>.zip` 到 OSS。
4. 把 `.manifest-fragment.json` 合并进生产 `global-skills-manifest.json`。

应用侧启动后会后台读取 manifest，下载 zip，校验 sha256，解压 staging，校验 `SKILL.md`，再安全覆盖安装到全局 `~/.renlijia/skills`。下载或安装失败只写 warning，不阻塞 app 使用。
```

- [ ] **Step 4: Verify the script with a tiny fixture**

Run:

```bash
tmp="$(mktemp -d)"
mkdir -p "$tmp/src/demo-skill/scripts" "$tmp/out"
cat > "$tmp/src/demo-skill/SKILL.md" <<'SKILL'
---
name: demo-skill
description: Demo skill for packaging.
---

# Demo
SKILL
scripts/skills/build-skills-artifact.sh "$tmp/src" 2026.04.28 "$tmp/out"
ls "$tmp/out"
```

Expected: output includes:

```text
renlijia-global-skills-2026.04.28.zip
renlijia-global-skills-2026.04.28.zip.manifest-fragment.json
```

- [ ] **Step 5: Commit**

```bash
git add scripts/skills/build-skills-artifact.sh scripts/skills/README.md
git commit -m "chore: add global skills artifact script"
```

---

## Task 2: Pure Global Skill Sync Types, Config, And Local State

**Files:**
- Create: `src-tauri/src/plugin/skill/global_sync.rs`
- Modify: `src-tauri/src/plugin/skill/mod.rs`
- Test: `src-tauri/tests/global_skill_sync_test.rs`

- [ ] **Step 1: Write failing tests for manifest parsing and config override**

Create `src-tauri/tests/global_skill_sync_test.rs` with:

```rust
use app_lib::plugin::skill::global_sync::{
    configured_global_skills_manifest_url, GlobalSkillsManifest, GlobalSkillsState,
    DEFAULT_GLOBAL_SKILLS_MANIFEST_URL,
};

#[test]
fn parses_global_skills_manifest() {
    let json = r#"
    {
      "bundleVersion": "2026.04.28",
      "artifact": {
        "url": "https://example.com/renlijia-global-skills-2026.04.28.zip",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sizeBytes": 123,
        "archiveFormat": "zip"
      }
    }
    "#;

    let manifest = GlobalSkillsManifest::from_json(json).expect("manifest should parse");

    assert_eq!(manifest.bundle_version, "2026.04.28");
    assert_eq!(manifest.artifact.archive_format, "zip");
    assert_eq!(manifest.artifact.size_bytes, 123);
}

#[test]
fn parses_global_skills_state_from_existing_global_state_json() {
    let json = r#"
    {
      "migrations": { "legacyConversations": true },
      "globalSkills": {
        "bundleVersion": "2026.04.28",
        "artifactSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "installedAtUnixSeconds": 1777342830
      }
    }
    "#;

    let state = GlobalSkillsState::from_global_state_json(json).expect("state should parse");
    let state = state.expect("globalSkills should exist");

    assert_eq!(state.bundle_version, "2026.04.28");
    assert_eq!(
        state.artifact_sha256,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(state.installed_at_unix_seconds, 1777342830);
}

#[test]
fn manifest_url_defaults_to_global_skills_oss_url() {
    std::env::remove_var("RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL");

    assert_eq!(
        configured_global_skills_manifest_url(),
        DEFAULT_GLOBAL_SKILLS_MANIFEST_URL.to_string()
    );
}

#[test]
fn manifest_url_can_be_overridden_by_env() {
    std::env::set_var(
        "RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL",
        "https://example.com/custom-global-skills-manifest.json",
    );

    assert_eq!(
        configured_global_skills_manifest_url(),
        "https://example.com/custom-global-skills-manifest.json"
    );

    std::env::remove_var("RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test parses_global_skills_manifest -- --nocapture
```

Expected: FAIL because `plugin::skill::global_sync` or `GlobalSkillsState` does not exist.

- [ ] **Step 3: Export the new module**

Modify `src-tauri/src/plugin/skill/mod.rs` to include:

```rust
pub mod catalog_prompt;
pub mod frontmatter;
pub mod global_sync;
pub mod invoked;
pub mod loader;
pub mod registry;
pub mod substitution;
pub mod types;
```

Keep any existing module declarations; only add `pub mod global_sync;` if the file already differs.

- [ ] **Step 4: Implement minimal manifest/config code**

Create `src-tauri/src/plugin/skill/global_sync.rs`:

```rust
use serde::Deserialize;

pub const DEFAULT_GLOBAL_SKILLS_MANIFEST_URL: &str =
    "https://rlj-cdn.oss-cn-hangzhou.aliyuncs.com/lotus/skills/global-skills-manifest.json";

pub fn configured_global_skills_manifest_url() -> String {
    std::env::var("RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_GLOBAL_SKILLS_MANIFEST_URL.to_string())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSkillsManifest {
    pub bundle_version: String,
    pub artifact: GlobalSkillsArtifact,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSkillsArtifact {
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(rename = "archiveFormat")]
    pub archive_format: String,
}


#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSkillsState {
    pub bundle_version: String,
    pub artifact_sha256: String,
    pub installed_at_unix_seconds: u64,
}

impl GlobalSkillsState {
    pub fn from_global_state_json(input: &str) -> Result<Option<Self>, GlobalSkillSyncError> {
        let value: serde_json::Value = serde_json::from_str(input)
            .map_err(|error| GlobalSkillSyncError::InvalidManifest(error.to_string()))?;
        match value.get("globalSkills") {
            Some(global_skills) => serde_json::from_value(global_skills.clone())
                .map(Some)
                .map_err(|error| GlobalSkillSyncError::InvalidManifest(error.to_string())),
            None => Ok(None),
        }
    }

    pub fn matches_manifest(&self, manifest: &GlobalSkillsManifest) -> bool {
        self.bundle_version == manifest.bundle_version
    }
}

impl GlobalSkillsManifest {
    pub fn from_json(input: &str) -> Result<Self, GlobalSkillSyncError> {
        let manifest: Self = serde_json::from_str(input)
            .map_err(|error| GlobalSkillSyncError::InvalidManifest(error.to_string()))?;
        if manifest.artifact.archive_format != "zip" {
            return Err(GlobalSkillSyncError::InvalidManifest(format!(
                "global skills artifact format is unsupported: {}",
                manifest.artifact.archive_format
            )));
        }
        Ok(manifest)
    }
}

#[derive(Debug)]
pub enum GlobalSkillSyncError {
    InvalidManifest(String),
}

impl std::fmt::Display for GlobalSkillSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(message) => write!(f, "invalid global skills manifest: {message}"),
        }
    }
}

impl std::error::Error for GlobalSkillSyncError {}
```

- [ ] **Step 5: Run test to verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test parses_global_skills_manifest parses_global_skills_state_from_existing_global_state_json manifest_url_defaults_to_global_skills_oss_url manifest_url_can_be_overridden_by_env -- --nocapture
```

Expected: PASS for the three tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/plugin/skill/mod.rs src-tauri/src/plugin/skill/global_sync.rs src-tauri/tests/global_skill_sync_test.rs
git commit -m "feat: add global skills sync manifest config"
```

---

## Task 3: Safe Local Install From Prepared Directory

**Files:**
- Modify: `src-tauri/src/plugin/skill/global_sync.rs`
- Test: `src-tauri/tests/global_skill_sync_test.rs`

- [ ] **Step 1: Add failing test for same-name overwrite preserving old skill on staging failure**

Append to `src-tauri/tests/global_skill_sync_test.rs`:

```rust
use std::fs;
use tempfile::TempDir;
use app_lib::plugin::skill::global_sync::{install_prepared_global_skills, GlobalSkillInstallReport};

fn write_skill(root: &std::path::Path, id: &str, description: &str, body: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {id}\ndescription: {description}\n---\n\n{body}\n"
        ),
    )
    .unwrap();
}

#[test]
fn installs_prepared_skills_to_global_dir_and_overwrites_same_name() {
    let temp = TempDir::new().unwrap();
    let prepared = temp.path().join("prepared");
    let global = temp.path().join("global-skills");
    fs::create_dir_all(&prepared).unwrap();
    fs::create_dir_all(&global).unwrap();

    write_skill(&global, "demo-skill", "old description", "# Old");
    write_skill(&prepared, "demo-skill", "new description", "# New");
    write_skill(&prepared, "second-skill", "second description", "# Second");

    let report = install_prepared_global_skills(&prepared, &global).expect("install should succeed");

    assert_eq!(
        report,
        GlobalSkillInstallReport {
            installed: vec!["demo-skill".to_string(), "second-skill".to_string()],
            skipped: Vec::new(),
        }
    );
    let demo = fs::read_to_string(global.join("demo-skill/SKILL.md")).unwrap();
    assert!(demo.contains("new description"));
    assert!(global.join("second-skill/SKILL.md").is_file());
}

#[test]
fn skips_invalid_prepared_skill_without_deleting_existing_global_skill() {
    let temp = TempDir::new().unwrap();
    let prepared = temp.path().join("prepared");
    let global = temp.path().join("global-skills");
    fs::create_dir_all(prepared.join("demo-skill")).unwrap();
    fs::create_dir_all(&global).unwrap();

    write_skill(&global, "demo-skill", "old description", "# Old");
    fs::write(prepared.join("demo-skill/SKILL.md"), "not frontmatter").unwrap();

    let report = install_prepared_global_skills(&prepared, &global).expect("bad skill is skipped");

    assert_eq!(report.installed, Vec::<String>::new());
    assert_eq!(report.skipped, vec!["demo-skill".to_string()]);
    let demo = fs::read_to_string(global.join("demo-skill/SKILL.md")).unwrap();
    assert!(demo.contains("old description"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test installs_prepared_skills_to_global_dir_and_overwrites_same_name -- --nocapture
```

Expected: FAIL because `install_prepared_global_skills` and `GlobalSkillInstallReport` do not exist.

- [ ] **Step 3: Implement safe prepared-directory install**

Add to `src-tauri/src/plugin/skill/global_sync.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use crate::plugin::skill::frontmatter::parse_skill_md;
use crate::plugin::skill::loader::is_valid_skill_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalSkillInstallReport {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
}

pub fn install_prepared_global_skills(
    prepared_root: &Path,
    global_skills_dir: &Path,
) -> Result<GlobalSkillInstallReport, GlobalSkillSyncError> {
    if !prepared_root.is_dir() {
        return Err(GlobalSkillSyncError::Install(format!(
            "prepared global skills root is not a directory: {}",
            prepared_root.display()
        )));
    }
    fs::create_dir_all(global_skills_dir).map_err(io_install_error)?;

    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for entry in fs::read_dir(prepared_root).map_err(io_install_error)? {
        let entry = entry.map_err(io_install_error)?;
        let source_dir = entry.path();
        if !source_dir.is_dir() {
            continue;
        }
        let Some(skill_id) = source_dir.file_name().and_then(|name| name.to_str()).map(str::to_string) else {
            continue;
        };
        if skill_id.starts_with('.') || skill_id.starts_with('_') || !is_valid_skill_id(&skill_id) {
            skipped.push(skill_id);
            continue;
        }
        if validate_skill_dir(&source_dir).is_err() {
            skipped.push(skill_id);
            continue;
        }
        install_one_skill_dir(&source_dir, global_skills_dir, &skill_id)?;
        installed.push(skill_id);
    }

    installed.sort();
    skipped.sort();
    Ok(GlobalSkillInstallReport { installed, skipped })
}

fn validate_skill_dir(source_dir: &Path) -> Result<(), GlobalSkillSyncError> {
    let skill_md = source_dir.join("SKILL.md");
    let content = fs::read_to_string(&skill_md).map_err(|error| {
        GlobalSkillSyncError::Install(format!("failed to read {}: {error}", skill_md.display()))
    })?;
    parse_skill_md(&content).map_err(|error| {
        GlobalSkillSyncError::Install(format!("failed to parse {}: {error}", skill_md.display()))
    })?;
    Ok(())
}

fn install_one_skill_dir(
    source_dir: &Path,
    global_skills_dir: &Path,
    skill_id: &str,
) -> Result<(), GlobalSkillSyncError> {
    let target = global_skills_dir.join(skill_id);
    let staging = global_skills_dir.join(format!(".{skill_id}.global-sync-staging"));
    let backup = global_skills_dir.join(format!(".{skill_id}.global-sync-backup"));

    remove_dir_if_exists(&staging)?;
    remove_dir_if_exists(&backup)?;
    copy_dir_recursive(source_dir, &staging).map_err(io_install_error)?;

    if target.exists() {
        fs::rename(&target, &backup).map_err(io_install_error)?;
    }

    if let Err(error) = fs::rename(&staging, &target).map_err(io_install_error) {
        if backup.exists() && !target.exists() {
            let _ = fs::rename(&backup, &target);
        }
        let _ = remove_dir_if_exists(&staging);
        return Err(error);
    }

    remove_dir_if_exists(&backup)?;
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<(), GlobalSkillSyncError> {
    if path.exists() {
        if path.is_dir() {
            fs::remove_dir_all(path).map_err(io_install_error)?;
        } else {
            return Err(GlobalSkillSyncError::Install(format!(
                "global skill sync path is not a directory: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_symlink() {
            log::warn!("Skipping symlink during global skill sync: {}", src_path.display());
            continue;
        }
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn io_install_error(error: std::io::Error) -> GlobalSkillSyncError {
    GlobalSkillSyncError::Install(error.to_string())
}
```

Update `GlobalSkillSyncError` in the same file to include `Install`:

```rust
#[derive(Debug)]
pub enum GlobalSkillSyncError {
    InvalidManifest(String),
    Install(String),
}

impl std::fmt::Display for GlobalSkillSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(message) => write!(f, "invalid global skills manifest: {message}"),
            Self::Install(message) => write!(f, "global skills install failed: {message}"),
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test installs_prepared_skills_to_global_dir_and_overwrites_same_name skips_invalid_prepared_skill_without_deleting_existing_global_skill -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin/skill/global_sync.rs src-tauri/tests/global_skill_sync_test.rs
git commit -m "feat: install global skills with rollback"
```

---

## Task 4: Zip Extraction And Artifact Verification

**Files:**
- Modify: `src-tauri/src/plugin/skill/global_sync.rs`
- Test: `src-tauri/tests/global_skill_sync_test.rs`

- [ ] **Step 1: Add failing test for zip extraction into prepared root**

Append to `src-tauri/tests/global_skill_sync_test.rs`:

```rust
use std::io::Write;
use zip::write::FileOptions;
use app_lib::plugin::skill::global_sync::extract_global_skills_zip;

#[test]
fn extracts_global_skills_zip_and_rejects_path_traversal() {
    let temp = TempDir::new().unwrap();
    let zip_path = temp.path().join("skills.zip");
    let output = temp.path().join("prepared");

    {
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = FileOptions::default();
        zip.add_directory("demo-skill/", options).unwrap();
        zip.start_file("demo-skill/SKILL.md", options).unwrap();
        zip.write_all(b"---\nname: demo-skill\ndescription: Demo skill.\n---\n\n# Demo\n").unwrap();
        zip.finish().unwrap();
    }

    extract_global_skills_zip(&zip_path, &output).expect("zip should extract");

    assert!(output.join("demo-skill/SKILL.md").is_file());

    let bad_zip_path = temp.path().join("bad.zip");
    {
        let file = fs::File::create(&bad_zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = FileOptions::default();
        zip.start_file("../escape.txt", options).unwrap();
        zip.write_all(b"escape").unwrap();
        zip.finish().unwrap();
    }

    let err = extract_global_skills_zip(&bad_zip_path, &temp.path().join("bad-out")).unwrap_err();
    assert!(err.to_string().contains("unsafe zip entry"), "got: {err}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test extracts_global_skills_zip_and_rejects_path_traversal -- --nocapture
```

Expected: FAIL because `extract_global_skills_zip` does not exist.

- [ ] **Step 3: Implement safe zip extraction**

Add to `src-tauri/src/plugin/skill/global_sync.rs`:

```rust
use std::io;

const MAX_GLOBAL_SKILLS_EXTRACTED_BYTES: u64 = 50 * 1024 * 1024;

pub fn extract_global_skills_zip(
    zip_path: &Path,
    output_dir: &Path,
) -> Result<(), GlobalSkillSyncError> {
    remove_dir_if_exists(output_dir)?;
    fs::create_dir_all(output_dir).map_err(io_install_error)?;

    let file = fs::File::open(zip_path).map_err(io_install_error)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| GlobalSkillSyncError::Install(format!("invalid global skills zip: {error}")))?;

    let mut total_extracted = 0_u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| {
            GlobalSkillSyncError::Install(format!("failed to read zip entry: {error}"))
        })?;
        total_extracted = total_extracted.saturating_add(file.size());
        if total_extracted > MAX_GLOBAL_SKILLS_EXTRACTED_BYTES {
            let _ = remove_dir_if_exists(output_dir);
            return Err(GlobalSkillSyncError::Install(
                "global skills package is too large".to_string(),
            ));
        }

        let enclosed = file.enclosed_name().ok_or_else(|| {
            GlobalSkillSyncError::Install(format!("unsafe zip entry: {}", file.name()))
        })?;
        let out_path = output_dir.join(enclosed);

        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(io_install_error)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(io_install_error)?;
            }
            let mut outfile = fs::File::create(&out_path).map_err(io_install_error)?;
            io::copy(&mut file, &mut outfile).map_err(io_install_error)?;
        }
    }

    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test extracts_global_skills_zip_and_rejects_path_traversal -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin/skill/global_sync.rs src-tauri/tests/global_skill_sync_test.rs
git commit -m "feat: extract global skills artifacts safely"
```

---

## Task 5: Version-Gated Async Fetch And Non-Blocking Startup Helper

**Files:**
- Modify: `src-tauri/src/plugin/skill/global_sync.rs`
- Test: `src-tauri/tests/global_skill_sync_test.rs`

- [ ] **Step 1: Add failing test for startup helper returning immediately**

Append to `src-tauri/tests/global_skill_sync_test.rs`:

```rust
use std::sync::{Arc, Mutex};
use app_lib::plugin::skill::global_sync::{spawn_global_skill_sync, GlobalSkillSyncConfig};
use app_lib::plugin::skill::registry::SkillRegistry;

#[test]
fn spawn_global_skill_sync_returns_without_waiting_for_network() {
    let temp = TempDir::new().unwrap();
    let registry = Arc::new(Mutex::new(SkillRegistry::new()));
    let config = GlobalSkillSyncConfig {
        manifest_url: "https://127.0.0.1:9/never-used.json".to_string(),
        global_skills_dir: temp.path().join("global-skills"),
        downloads_dir: temp.path().join("downloads"),
        state_path: temp.path().join("global").join("state.json"),
        skill_roots_for_reload: vec![temp.path().join("global-skills")],
    };

    let start = std::time::Instant::now();
    spawn_global_skill_sync(config, registry);

    assert!(start.elapsed() < std::time::Duration::from_millis(100));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test spawn_global_skill_sync_returns_without_waiting_for_network -- --nocapture
```

Expected: FAIL because `spawn_global_skill_sync`, `GlobalSkillSyncConfig`, or global `state.json` handling does not exist.

- [ ] **Step 3: Implement fetch/install orchestration and spawn helper**

Add to `src-tauri/src/plugin/skill/global_sync.rs`:

```rust
use std::sync::{Arc, Mutex};

use crate::plugin::skill::loader::load_skill_roots;
use crate::plugin::skill::registry::SkillRegistry;
use crate::runtime::dependencies::verify_sha256;

#[derive(Debug, Clone)]
pub struct GlobalSkillSyncConfig {
    pub manifest_url: String,
    pub global_skills_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub state_path: PathBuf,
    pub skill_roots_for_reload: Vec<PathBuf>,
}

pub fn spawn_global_skill_sync(
    config: GlobalSkillSyncConfig,
    registry: Arc<Mutex<SkillRegistry>>,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = sync_global_skills_from_manifest(config.clone()).await {
            log::warn!("[global-skills] background sync skipped: {}", error);
            return;
        }
        if let Err(error) = reload_disk_skill_registry(&registry, &config.skill_roots_for_reload) {
            log::warn!("[global-skills] registry reload skipped: {}", error);
        }
    });
}

pub async fn sync_global_skills_from_manifest(
    config: GlobalSkillSyncConfig,
) -> Result<GlobalSkillInstallReport, GlobalSkillSyncError> {
    let manifest_text = reqwest::get(&config.manifest_url)
        .await
        .map_err(|error| GlobalSkillSyncError::Network(error.to_string()))?
        .text()
        .await
        .map_err(|error| GlobalSkillSyncError::Network(error.to_string()))?;
    let manifest = GlobalSkillsManifest::from_json(&manifest_text)?;
    if let Some(state) = read_global_skills_state(&config.state_path)? {
        if state.matches_manifest(&manifest) {
            log::info!(
                "[global-skills] bundle {} already installed; skipping download",
                manifest.bundle_version
            );
            return Ok(GlobalSkillInstallReport {
                installed: Vec::new(),
                skipped: Vec::new(),
            });
        }
    }
    let archive_path = download_global_skills_artifact(&manifest, &config.downloads_dir).await?;
    verify_sha256(&archive_path, &manifest.artifact.sha256)
        .map_err(|error| GlobalSkillSyncError::Install(error.to_string()))?;

    let prepared_root = config.downloads_dir.join(format!(
        "renlijia-global-skills-{}-prepared",
        manifest.bundle_version
    ));
    extract_global_skills_zip(&archive_path, &prepared_root)?;
    let report = install_prepared_global_skills(&prepared_root, &config.global_skills_dir)?;
    write_global_skills_state(&config.state_path, &manifest)?;
    Ok(report)
}

fn read_global_skills_state(
    state_path: &Path,
) -> Result<Option<GlobalSkillsState>, GlobalSkillSyncError> {
    if !state_path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(state_path).map_err(io_install_error)?;
    GlobalSkillsState::from_global_state_json(&content)
}

fn write_global_skills_state(
    state_path: &Path,
    manifest: &GlobalSkillsManifest,
) -> Result<(), GlobalSkillSyncError> {
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).map_err(io_install_error)?;
    }
    let mut root: serde_json::Value = if state_path.is_file() {
        let content = fs::read_to_string(state_path).map_err(io_install_error)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if !root.get("migrations").map_or(false, serde_json::Value::is_object) {
        root["migrations"] = serde_json::json!({});
    }
    root["globalSkills"] = serde_json::json!({
        "bundleVersion": manifest.bundle_version,
        "artifactSha256": manifest.artifact.sha256,
        "installedAtUnixSeconds": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| GlobalSkillSyncError::Install(error.to_string()))?
            .as_secs(),
    });
    let content = serde_json::to_vec_pretty(&root)
        .map_err(|error| GlobalSkillSyncError::Install(error.to_string()))?;
    let tmp = state_path.with_extension("json.tmp");
    fs::write(&tmp, content).map_err(io_install_error)?;
    fs::rename(tmp, state_path).map_err(io_install_error)
}

async fn download_global_skills_artifact(
    manifest: &GlobalSkillsManifest,
    downloads_dir: &Path,
) -> Result<PathBuf, GlobalSkillSyncError> {
    fs::create_dir_all(downloads_dir).map_err(io_install_error)?;
    let file_name = manifest
        .artifact
        .url
        .rsplit('/')
        .next()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("renlijia-global-skills.zip");
    let archive_path = downloads_dir.join(file_name);
    let response = reqwest::get(&manifest.artifact.url)
        .await
        .map_err(|error| GlobalSkillSyncError::Network(error.to_string()))?;
    if !response.status().is_success() {
        return Err(GlobalSkillSyncError::Network(format!(
            "failed to download global skills artifact: HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| GlobalSkillSyncError::Network(error.to_string()))?;
    if bytes.len() as u64 != manifest.artifact.size_bytes {
        return Err(GlobalSkillSyncError::Install(format!(
            "global skills artifact size mismatch: expected {}, got {}",
            manifest.artifact.size_bytes,
            bytes.len()
        )));
    }
    fs::write(&archive_path, &bytes).map_err(io_install_error)?;
    Ok(archive_path)
}

fn reload_disk_skill_registry(
    registry: &Arc<Mutex<SkillRegistry>>,
    roots: &[PathBuf],
) -> Result<(), GlobalSkillSyncError> {
    let loaded = load_skill_roots(roots).map_err(|error| {
        GlobalSkillSyncError::Install(format!("failed to reload disk skills: {error}"))
    })?;
    let mut guard = registry
        .lock()
        .map_err(|error| GlobalSkillSyncError::Install(format!("registry lock failed: {error}")))?;
    *guard = SkillRegistry::from_skills(loaded.into_values().collect());
    Ok(())
}
```

Update `GlobalSkillSyncError` to include `Network`:

```rust
#[derive(Debug)]
pub enum GlobalSkillSyncError {
    InvalidManifest(String),
    Install(String),
    Network(String),
}

impl std::fmt::Display for GlobalSkillSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(message) => write!(f, "invalid global skills manifest: {message}"),
            Self::Install(message) => write!(f, "global skills install failed: {message}"),
            Self::Network(message) => write!(f, "global skills network failed: {message}"),
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test spawn_global_skill_sync_returns_without_waiting_for_network -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin/skill/global_sync.rs src-tauri/tests/global_skill_sync_test.rs
git commit -m "feat: sync global skills in background"
```

---

## Task 6: Wire Background Sync Into Tauri Startup

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Test: run targeted tests and `cargo check`

- [ ] **Step 1: Identify exact insertion point**

Use this area in `src-tauri/src/lib.rs`:

```rust
let disk_skill_registry = Arc::new(std::sync::Mutex::new(
    plugin::skill::registry::SkillRegistry::from_skills(
        loaded_skills.into_values().collect(),
    ),
));
app.manage(disk_skill_registry.clone());
```

The sync must start immediately after `app.manage(disk_skill_registry.clone());` so the registry exists for best-effort reload. Do not place it before initial `load_skill_roots`; startup must not wait for network.

- [ ] **Step 2: Modify startup to spawn global skill sync**

Add after `app.manage(disk_skill_registry.clone());`:

```rust
{
    let global_skills_dir = aijia_home.skills_dir();
    let downloads_dir = aijia_home.root().join("downloads").join("global-skills");
    let state_path = aijia_home.global_state_path();
    let skill_roots_for_reload = skill_roots.clone();
    let disk_skill_registry_for_sync = disk_skill_registry.clone();
    plugin::skill::global_sync::spawn_global_skill_sync(
        plugin::skill::global_sync::GlobalSkillSyncConfig {
            manifest_url: plugin::skill::global_sync::configured_global_skills_manifest_url(),
            global_skills_dir,
            downloads_dir,
            state_path,
            skill_roots_for_reload,
        },
        disk_skill_registry_for_sync,
    );
}
```

- [ ] **Step 3: Run cargo check**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 4: Run targeted skill tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test skill_md_loader_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test list_skills_returns_skill_md_only_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: start global skill sync on launch"
```

---

## Task 7: Final Verification And Review

**Files:**
- All changed files

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Expected: command exits 0.

- [ ] **Step 2: Run Rust checks**

Run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: PASS.

- [ ] **Step 3: Run targeted test suite**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test global_skill_sync_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test skill_md_loader_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test list_skills_returns_skill_md_only_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test builtin_runtime_registration_test load_skill_routes_through_request_scoped_runtime_factory -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Run script smoke test**

Run:

```bash
tmp="$(mktemp -d)"
mkdir -p "$tmp/src/demo-skill/scripts" "$tmp/out"
cat > "$tmp/src/demo-skill/SKILL.md" <<'SKILL'
---
name: demo-skill
description: Demo skill for packaging.
---

# Demo
SKILL
scripts/skills/build-skills-artifact.sh "$tmp/src" 2026.04.28 "$tmp/out"
test -f "$tmp/out/renlijia-global-skills-2026.04.28.zip"
test -f "$tmp/out/renlijia-global-skills-2026.04.28.zip.manifest-fragment.json"
```

Expected: all commands exit 0.

- [ ] **Step 5: Review changed startup behavior**

Run:

```bash
git diff -- src-tauri/src/lib.rs src-tauri/src/plugin/skill/global_sync.rs scripts/skills/build-skills-artifact.sh scripts/skills/README.md src-tauri/tests/global_skill_sync_test.rs
```

Check:

- No `await` or `block_on` waits for global skill sync in Tauri setup.
- Target directory is `aijia_home.skills_dir()` only.
- No writes to `current_user_storage.resolve_paths().skills_dir()` for this sync.
- Network failure path logs warning and returns.
- Existing skill directories are protected by backup/rollback.

- [ ] **Step 6: Request code review**

Use `superpowers:requesting-code-review` or dispatch a review subagent with scope:

```text
Review global skills managed sync implementation. Focus on startup non-blocking behavior, global-vs-user path correctness, archive extraction safety, overwrite rollback behavior, and whether any runtime/marketplace/custom install concerns were mixed in incorrectly.
```

- [ ] **Step 7: Commit final fixes if review finds issues**

```bash
git add <fixed-files>
git commit -m "fix: address global skills sync review"
```

Only run this step if review finds actionable issues.

---

## Self-Review

**Spec coverage:**

- Release-side separate script: Task 1.
- OSS manifest flow: Tasks 2 and 5.
- Global-only target `~/.renlijia/skills`: Tasks 3 and 6.
- Non-blocking startup: Tasks 5 and 6.
- Safe same-name overwrite: Task 3.
- Version-based update skipping via existing `~/.renlijia/global/state.json`: Tasks 2 and 5.
- Zip safety and sha256/size verification: Tasks 4 and 5.
- Registry visibility after background update: Task 5 reload helper and Task 6 wiring.
- Avoid old marketplace/custom install and runtime manager mixing: File structure and Task 6 review checklist.

**Placeholder scan:** No TBD/TODO placeholders remain in implementation steps. Task 7 review step is intentionally a verification instruction, not implementation code.

**Type consistency:** `GlobalSkillsManifest`, `GlobalSkillsArtifact`, `GlobalSkillSyncConfig`, `GlobalSkillInstallReport`, `GlobalSkillSyncError`, `install_prepared_global_skills`, `extract_global_skills_zip`, `sync_global_skills_from_manifest`, `GlobalSkillsState`, existing global `state.json` handling, and `spawn_global_skill_sync` are introduced before use.
