# Skill Center Enable State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把“技能已安装”和“技能后续对话可用”拆开，让用户关闭技能后，该技能不再出现在聊天入口、模型 skill catalog 或 `Skill` runtime tool 中，同时市场/企业目录不再登录后自动全量安装；登录初始化只自动安装 allowlist 内的必需内置技能。

**Architecture:** 后端新增 user-scoped enablement store 和 required builtin allowlist，`SkillRegistry` 继续保留全量已安装技能，聊天 catalog 和 `Skill` tool 显式消费 enabled 视图。前端 `skillStore` 保留全量 `skills`，同时提供 `enabledSkills` 给聊天入口，技能中心按 `市场 / 内置 / 已安装` 管理展示。

**Tech Stack:** Tauri IPC, Rust, serde JSON, Zustand, React, Vitest, Cargo tests, existing SkillRegistry/SKILL.md runtime.

---

## Source Spec

Implementation must follow:

- `docs/superpowers/specs/2026-06-10-skill-center-enable-state-design.md`

Do not implement unrelated skill permission policy, rating, ranking, or marketplace recommendation features.

---

## File Structure

Backend core:

- Create `src-tauri/src/plugin/skill/enablement.rs`
  - Owns `SkillEnablementStore` and `SkillEnablementState`.
  - Resolves the current user's `skillsConfig.json` path.
  - Reads/writes `disabledSkillIds` using atomic writes.
- Modify `src-tauri/src/plugin/skill/mod.rs`
  - Exports the new module.
- Modify `src-tauri/src/storage/user_scoped_paths.rs`
  - Adds `skills_config_path()`.
- Modify `src-tauri/src/plugin/skill/registry.rs`
  - Keeps full registry methods.
  - Adds enabled-only ids/catalog/body access helpers.

Backend IPC and runtime:

- Modify `src-tauri/src/commands/skill_management.rs`
  - Adds `enabled` to `SkillInfo`.
  - Adds `set_skill_enabled`.
  - Makes install/import/uninstall update enablement state consistently.
- Modify `src-tauri/src/commands/plugin.rs`
  - Injects `SkillEnablementStore` into `list_skills` and `get_plugin_info`.
- Modify `src-tauri/src/plugin/skill/sync_command.rs`
  - Changes login sync semantics so only required builtins auto-install and other new remote packages are not auto-installed.
- Modify `src-tauri/src/plugin/skill/global_sync.rs`
  - Splits remote list fetch from required-builtin install and installed-package update.
- Create `src-tauri/src/plugin/skill/required_builtin.rs`
  - Defines required builtin skill ids, display aliases, and helper predicates.
- Modify `src-tauri/src/runtime/tools/builtin/load_skill.rs`
  - Filters tool definition and execute by enabled state.
- Modify `src-tauri/src/plugin/context.rs` and `src-tauri/src/plugin/registry.rs`
  - Threads enablement into request-scoped `Skill` tool construction.
- Modify `src-tauri/src/transport/tauri_commands/chat.rs`
  - Uses enabled catalog for dynamic model context.
- Modify `src-tauri/src/lib.rs`
  - Manages `Arc<SkillEnablementStore>` and registers new IPC.

Frontend data and UI:

- Modify `src/lib/tauri.ts`
  - Adds `enabled`, `installed`, enablement event, `setSkillEnabled`.
- Modify `src/stores/skillStore.ts`
  - Adds `enabledSkills`, `setSkillEnabled`, marketplace install wrapper.
- Modify `src/components/auth/AuthGate.tsx`
  - Listens to `skill:enablement-changed`.
- Modify `src/components/chat/SkillPopover.tsx`
  - Uses enabled skills only.
- Modify `src/components/chat-scene/ChatBottomArea.tsx`
  - Uses enabled skills for slash tokens and picker.
- Modify `src/features/skill-detail/SkillDetailPage.tsx`
  - Implements `使用 / 关闭 / 开启并使用 / 保持关闭`.
- Modify `src/features/skill-center/SkillCenterPage.tsx`
  - Adds `市场 / 内置 / 已安装` tabs while preserving card grid style.
- Modify existing skill-card components only if needed for clean actions slot.

Tests:

- Rust unit tests in `src-tauri/src/plugin/skill/enablement.rs`.
- Rust registry tests in `src-tauri/src/plugin/skill/registry.rs`.
- Rust integration tests in `src-tauri/tests/skill_md_catalog_test.rs`, `src-tauri/tests/load_skill_skill_md_test.rs`, and a new focused sync test if needed.
- Frontend tests in `src/stores/skillStore.test.ts`, `src/features/skill-detail/SkillDetailPage.test.tsx`, `src/features/auth/AuthGate.integration.test.tsx`, and focused tests for chat picker/composer.

---

### Task 1: Backend Enablement Store And Registry Enabled View

**Files:**

- Create: `src-tauri/src/plugin/skill/enablement.rs`
- Modify: `src-tauri/src/plugin/skill/mod.rs`
- Modify: `src-tauri/src/storage/user_scoped_paths.rs`
- Modify: `src-tauri/src/plugin/skill/registry.rs`
- Test: `src-tauri/src/plugin/skill/enablement.rs`
- Test: `src-tauri/src/plugin/skill/registry.rs`

- [ ] **Step 1: Add failing tests for enablement state read/write**

Add this test module to the new file `src-tauri/src/plugin/skill/enablement.rs` before implementation:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{AiJiaHome, CurrentUserStorage, UserScope, UserScopedPathResolver};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn enablement_defaults_to_enabled_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(1, 2)).unwrap();

        let store = SkillEnablementStore::new(current_user);
        let state = store.load().unwrap();

        assert!(state.is_enabled("biz-plan"));
        assert!(state.disabled_skill_ids.is_empty());
    }

    #[test]
    fn enablement_persists_disabled_ids_under_user_scope() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(7, 9)).unwrap();

        let store = SkillEnablementStore::new(current_user.clone());
        store.set_enabled("biz-plan", false).unwrap();

        let path = current_user
            .resolve_paths()
            .unwrap()
            .skills_config_path();
        let raw = std::fs::read_to_string(path).unwrap();
        assert!(raw.contains("disabledSkillIds"));
        assert!(raw.contains("biz-plan"));

        let reloaded = store.load().unwrap();
        assert!(!reloaded.is_enabled("biz-plan"));
        assert!(reloaded.is_enabled("deep-research"));
    }

    #[test]
    fn enablement_requires_user_scope_for_writes() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        let store = SkillEnablementStore::new(current_user);

        let err = store.set_enabled("local-only", false).unwrap_err();

        assert!(err.to_string().contains("未登录") || err.to_string().contains("not logged in"));
    }

    #[test]
    fn remove_override_re_enables_skill() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(AiJiaHome::from_path(tmp.path().to_path_buf()));
        let current_user = Arc::new(CurrentUserStorage::new(home));
        current_user.activate_scope(UserScope::new(3, 4)).unwrap();
        let store = SkillEnablementStore::new(current_user);

        store.set_enabled("docx", false).unwrap();
        store.set_enabled("docx", true).unwrap();

        assert!(store.load().unwrap().is_enabled("docx"));
    }
}
```

- [ ] **Step 2: Run the failing enablement tests**

Run:

```powershell
cd src-tauri
cargo test skill_enablement --lib
```

Expected: FAIL because `SkillEnablementStore`, `SkillEnablementState`, and `skills_config_path()` do not exist.

- [ ] **Step 3: Implement `UserScopedPaths::skills_config_path`**

Add this method in `src-tauri/src/storage/user_scoped_paths.rs` near the other per-user JSON paths:

```rust
pub fn skills_config_path(&self) -> PathBuf {
    self.base.join("skillsConfig.json")
}
```

Extend the existing `paths_snapshot_all_directories` test with:

```rust
assert_eq!(
    paths.skills_config_path(),
    base.join("skillsConfig.json")
);
```

- [ ] **Step 4: Implement `SkillEnablementStore`**

Create `src-tauri/src/plugin/skill/enablement.rs`:

```rust
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::storage::fs_atomic::write_atomic;
use crate::storage::{CurrentUserStorage, UserScopedPathResolver};

#[derive(Debug, Clone)]
pub struct SkillEnablementStore {
    current_user: Arc<CurrentUserStorage>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEnablementState {
    #[serde(default)]
    pub disabled_skill_ids: BTreeSet<String>,
}

impl SkillEnablementState {
    pub fn is_enabled(&self, skill_id: &str) -> bool {
        !self.disabled_skill_ids.contains(skill_id)
    }
}

impl SkillEnablementStore {
    pub fn new(current_user: Arc<CurrentUserStorage>) -> Self {
        Self { current_user }
    }

    pub fn load(&self) -> Result<SkillEnablementState> {
        let path = self.state_path()?;
        if !path.is_file() {
            return Ok(SkillEnablementState::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read skill enablement '{}'", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("parse skill enablement '{}'", path.display()))
    }

    pub fn load_or_default(&self) -> SkillEnablementState {
        match self.load() {
            Ok(state) => state,
            Err(error) => {
                log::warn!("[skill-enablement] load failed, default enabled: {error}");
                SkillEnablementState::default()
            }
        }
    }

    pub fn set_enabled(&self, skill_id: &str, enabled: bool) -> Result<SkillEnablementState> {
        let mut state = self.load_or_default();
        if enabled {
            state.disabled_skill_ids.remove(skill_id);
        } else {
            state.disabled_skill_ids.insert(skill_id.to_string());
        }
        self.save(&state)?;
        Ok(state)
    }

    pub fn clear_override(&self, skill_id: &str) -> Result<SkillEnablementState> {
        let mut state = self.load_or_default();
        state.disabled_skill_ids.remove(skill_id);
        self.save(&state)?;
        Ok(state)
    }

    fn save(&self, state: &SkillEnablementState) -> Result<()> {
        let path = self.state_path()?;
        let bytes = serde_json::to_vec_pretty(state).context("encode skill enablement")?;
        write_atomic(&path, &bytes)
    }

    fn state_path(&self) -> Result<PathBuf> {
        self.current_user
            .resolve_paths()
            .map(|paths| paths.skills_config_path())
            .ok_or_else(|| anyhow::anyhow!("未登录，无法读取技能配置"))
    }
}
```

Add the module export in `src-tauri/src/plugin/skill/mod.rs`:

```rust
pub mod enablement;
```

- [ ] **Step 5: Add failing tests for enabled registry view**

In `src-tauri/src/plugin/skill/registry.rs`, add tests under `replace_all_tests`:

```rust
#[test]
fn enabled_skill_ids_excludes_disabled_ids() {
    let reg = SkillRegistry::from_skills(vec![skill("a"), skill("b")]);
    let mut state = crate::plugin::skill::enablement::SkillEnablementState::default();
    state.disabled_skill_ids.insert("b".to_string());

    assert_eq!(reg.enabled_skill_ids(&state), vec!["a".to_string()]);
    assert!(reg.get_enabled("a", &state).is_some());
    assert!(reg.get_enabled("b", &state).is_none());
}

#[test]
fn format_enabled_catalog_excludes_disabled_skills_but_full_catalog_keeps_all() {
    let reg = SkillRegistry::from_skills(vec![skill("a"), skill("b")]);
    let mut state = crate::plugin::skill::enablement::SkillEnablementState::default();
    state.disabled_skill_ids.insert("b".to_string());

    let enabled = reg.format_enabled_catalog(&state, 100_000);
    assert!(enabled.contains("`a`"));
    assert!(!enabled.contains("`b`"));

    let full = reg.format_full_catalog(100_000);
    assert!(full.contains("`a`"));
    assert!(full.contains("`b`"));
}
```

- [ ] **Step 6: Run the failing registry tests**

Run:

```powershell
cd src-tauri
cargo test enabled_skill_ids_excludes_disabled_ids --lib
cargo test format_enabled_catalog_excludes_disabled_skills_but_full_catalog_keeps_all --lib
```

Expected: FAIL because `enabled_skill_ids`, `get_enabled`, and `format_enabled_catalog` are not implemented.

- [ ] **Step 7: Implement registry enabled helpers**

In `src-tauri/src/plugin/skill/registry.rs`, import the state:

```rust
use super::enablement::SkillEnablementState;
```

Add methods to `impl SkillRegistry`:

```rust
pub fn enabled_skill_ids(&self, state: &SkillEnablementState) -> Vec<String> {
    let mut ids = self
        .skills
        .keys()
        .filter(|id| state.is_enabled(id))
        .cloned()
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

pub fn get_enabled(&self, id: &str, state: &SkillEnablementState) -> Option<&DiskSkill> {
    if state.is_enabled(id) {
        self.skills.get(id)
    } else {
        None
    }
}

pub fn format_enabled_catalog(
    &self,
    state: &SkillEnablementState,
    context_window_tokens: usize,
) -> String {
    let mut skills = self
        .skills
        .values()
        .filter(|skill| state.is_enabled(&skill.id))
        .cloned()
        .collect::<Vec<_>>();
    skills.sort_by(|a, b| a.id.cmp(&b.id));
    format_skill_catalog_with_budget(&skills, context_window_tokens)
}
```

- [ ] **Step 8: Run Task 1 tests**

Run:

```powershell
cd src-tauri
cargo test skill_enablement --lib
cargo test enabled_skill_ids_excludes_disabled_ids --lib
cargo test format_enabled_catalog_excludes_disabled_skills_but_full_catalog_keeps_all --lib
```

Expected: PASS.

- [ ] **Step 9: Commit Task 1**

```powershell
git add src-tauri/src/plugin/skill/enablement.rs src-tauri/src/plugin/skill/mod.rs src-tauri/src/storage/user_scoped_paths.rs src-tauri/src/plugin/skill/registry.rs
git commit -m "feat: add skill enablement state"
```

---

### Task 2: Backend IPC For `enabled` State And Full Skill Listing

**Files:**

- Modify: `src-tauri/src/commands/skill_management.rs`
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/commands/skill_management.rs`

- [ ] **Step 1: Add failing tests for `SkillInfo.enabled` merge**

In `src-tauri/src/commands/skill_management.rs`, add tests in the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn list_skills_merges_enabled_state_without_filtering_disabled() {
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(vec![
        test_disk_skill("enabled-skill", crate::plugin::skill::types::SkillSource::User),
        test_disk_skill("disabled-skill", crate::plugin::skill::types::SkillSource::User),
    ])));
    let mut state = crate::plugin::skill::enablement::SkillEnablementState::default();
    state.disabled_skill_ids.insert("disabled-skill".to_string());

    let skills = list_skills_from_registry_with_enablement(&registry, &state);

    assert_eq!(skills.len(), 2);
    assert_eq!(
        skills.iter().find(|s| s.id == "enabled-skill").unwrap().enabled,
        true
    );
    assert_eq!(
        skills.iter().find(|s| s.id == "disabled-skill").unwrap().enabled,
        false
    );
}
```

If no helper exists for building a `DiskSkill` in this file, add this test helper inside the test module:

```rust
fn test_disk_skill(
    id: &str,
    source: crate::plugin::skill::types::SkillSource,
) -> crate::plugin::skill::types::DiskSkill {
    use crate::plugin::skill::types::{DiskSkill, SkillFrontmatter};
    DiskSkill {
        id: id.to_string(),
        root: std::path::PathBuf::from(format!("/tmp/{id}")),
        frontmatter: SkillFrontmatter {
            name: id.to_string(),
            description: format!("description for {id}"),
            ..Default::default()
        },
        body: String::new(),
        source,
    }
}
```

- [ ] **Step 2: Run the failing SkillInfo merge test**

Run:

```powershell
cd src-tauri
cargo test list_skills_merges_enabled_state_without_filtering_disabled --lib
```

Expected: FAIL because `SkillInfo.enabled` and `list_skills_from_registry_with_enablement` do not exist.

- [ ] **Step 3: Add `enabled` to backend `SkillInfo`**

In `src-tauri/src/commands/skill_management.rs`, extend `SkillInfo`:

```rust
pub enabled: bool,
```

Refactor list helpers so existing callers can pass a state:

```rust
pub fn list_skills_from_registry_with_enablement(
    registry: &Arc<Mutex<SkillRegistry>>,
    enablement: &crate::plugin::skill::enablement::SkillEnablementState,
) -> Vec<SkillInfo> {
    use crate::plugin::skill::updated_at::DirMtimeResolver;
    list_skills_from_registry_with_resolver_and_enablement(
        registry,
        &DirMtimeResolver,
        enablement,
    )
}

pub fn list_skills_from_registry_with_resolver_and_enablement(
    registry: &Arc<Mutex<SkillRegistry>>,
    resolver: &dyn crate::plugin::skill::updated_at::SkillUpdatedAtResolver,
    enablement: &crate::plugin::skill::enablement::SkillEnablementState,
) -> Vec<SkillInfo> {
    let guard = registry.lock().unwrap();
    guard
        .skill_ids()
        .into_iter()
        .filter_map(|id| {
            guard.get(&id).map(|skill| SkillInfo {
                id: skill.id.clone(),
                display_name: skill
                    .frontmatter
                    .metadata
                    .label
                    .clone()
                    .unwrap_or_else(|| skill.frontmatter.name.clone()),
                display_name_en: skill
                    .frontmatter
                    .metadata
                    .display_i18n
                    .get("en-US")
                    .cloned()
                    .unwrap_or_default()
                    .name
                    .unwrap_or_default(),
                description: skill.frontmatter.description.clone(),
                short_description_en: skill
                    .frontmatter
                    .metadata
                    .display_i18n
                    .get("en-US")
                    .cloned()
                    .unwrap_or_default()
                    .description
                    .unwrap_or_default(),
                icon: None,
                category: skill.frontmatter.category.clone(),
                source: match skill.source {
                    crate::plugin::skill::types::SkillSource::User => "user".to_string(),
                    crate::plugin::skill::types::SkillSource::Tenant => "tenant".to_string(),
                    crate::plugin::skill::types::SkillSource::Global => "global".to_string(),
                },
                updated_at: resolver.resolve(skill),
                version: skill.frontmatter.version.clone(),
                enabled: enablement.is_enabled(&skill.id),
            })
        })
        .collect()
}
```

Keep the old `list_skills_from_registry` as a compatibility wrapper using `SkillEnablementState::default()` only for tests that do not have app state:

```rust
pub fn list_skills_from_registry(registry: &Arc<Mutex<SkillRegistry>>) -> Vec<SkillInfo> {
    let enablement = crate::plugin::skill::enablement::SkillEnablementState::default();
    list_skills_from_registry_with_enablement(registry, &enablement)
}
```

- [ ] **Step 4: Add `set_skill_enabled` command**

In `src-tauri/src/commands/skill_management.rs`, add:

```rust
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEnablementChangedPayload {
    pub skill_id: String,
    pub enabled: bool,
}

#[tauri::command]
pub async fn set_skill_enabled(
    app: AppHandle,
    skill_id: String,
    enabled: bool,
) -> Result<SkillInfo, String> {
    let registry = app.state::<Arc<Mutex<SkillRegistry>>>();
    let exists = registry
        .lock()
        .map_err(|e| format!("registry lock poisoned: {e}"))?
        .get(&skill_id)
        .is_some();

    if !exists {
        refresh_skill_registry(&app)?;
        let exists_after_refresh = registry
            .lock()
            .map_err(|e| format!("registry lock poisoned: {e}"))?
            .get(&skill_id)
            .is_some();
        if !exists_after_refresh {
            return Err(format!("Unknown skill: {skill_id}"));
        }
    }

    let store = app.state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>();
    let state = store
        .set_enabled(&skill_id, enabled)
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "skill:enablement-changed",
        SkillEnablementChangedPayload {
            skill_id: skill_id.clone(),
            enabled,
        },
    );
    let _ = app.emit("skill:registry-refreshed", ());

    list_skills_from_registry_with_enablement(registry.inner(), &state)
        .into_iter()
        .find(|skill| skill.id == skill_id)
        .ok_or_else(|| format!("Skill disappeared after enablement change: {skill_id}"))
}
```

- [ ] **Step 5: Manage store and register command**

In `src-tauri/src/lib.rs`, after `current_user_storage` is available, manage:

```rust
let skill_enablement_store = Arc::new(plugin::skill::enablement::SkillEnablementStore::new(
    current_user_storage.clone(),
));
app.manage(skill_enablement_store);
```

Register the command near skill management commands:

```rust
commands::skill_management::set_skill_enabled,
```

- [ ] **Step 6: Update plugin list commands to use enablement**

In `src-tauri/src/commands/plugin.rs`, import the store and state:

```rust
use crate::plugin::skill::enablement::SkillEnablementStore;
```

Update `list_skills`:

```rust
#[tauri::command]
pub fn list_skills(
    registry: State<'_, Arc<Mutex<SkillRegistry>>>,
    enablement: State<'_, Arc<SkillEnablementStore>>,
) -> Result<Vec<SkillInfo>, String> {
    let state = enablement.load_or_default();
    Ok(crate::commands::skill_management::list_skills_from_registry_with_enablement(
        registry.inner(),
        &state,
    ))
}
```

Update `get_plugin_info` the same way so `skills` includes `enabled`.

- [ ] **Step 7: Clear enablement overrides on local install/uninstall**

In `install_custom_skill`, after install succeeds and before/after `refresh_skill_registry`, clear the disabled override for the installed id. Use the installed folder name as the id:

```rust
if let Some(skill_id) = PathBuf::from(&dest).file_name().and_then(|name| name.to_str()) {
    if let Some(store) = app.try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>() {
        let _ = store.clear_override(skill_id);
    }
}
```

In `uninstall_custom_skill`, after removing the directory, clear:

```rust
if let Some(store) = app.try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>() {
    let _ = store.clear_override(&skill_id);
}
```

- [ ] **Step 8: Run Task 2 tests**

Run:

```powershell
cd src-tauri
cargo test list_skills_merges_enabled_state_without_filtering_disabled --lib
cargo check
```

Expected: PASS.

- [ ] **Step 9: Commit Task 2**

```powershell
git add src-tauri/src/commands/skill_management.rs src-tauri/src/commands/plugin.rs src-tauri/src/lib.rs
git commit -m "feat: expose skill enablement ipc"
```

---

### Task 3: Filter Model Catalog And `Skill` Runtime Tool

**Files:**

- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/plugin/context.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/load_skill.rs`
- Test: `src-tauri/src/plugin/skill/registry.rs`
- Test: `src-tauri/tests/load_skill_skill_md_test.rs`

- [ ] **Step 1: Add a focused test for `LoadSkillRuntimeTool` disabled behavior**

In `src-tauri/tests/load_skill_skill_md_test.rs`, add:

```rust
use app_lib::plugin::skill::enablement::SkillEnablementState;
use app_lib::plugin::skill::registry::SkillRegistry;
use app_lib::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillSource};
use app_lib::runtime::tools::builtin::load_skill::LoadSkillRuntimeTool;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::tools::RuntimeTool;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn disk_skill(id: &str) -> DiskSkill {
    DiskSkill {
        id: id.to_string(),
        root: PathBuf::from(format!("/tmp/{id}")),
        frontmatter: SkillFrontmatter {
            name: id.to_string(),
            description: format!("description for {id}"),
            ..Default::default()
        },
        body: format!("body for {id}"),
        source: SkillSource::User,
    }
}

#[tokio::test]
async fn load_skill_rejects_disabled_skill() {
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(vec![disk_skill("disabled-skill")])));
    let mut state = SkillEnablementState::default();
    state.disabled_skill_ids.insert("disabled-skill".to_string());
    let tool = LoadSkillRuntimeTool::new_with_enablement_state_for_test(registry, state);

    let err = tool
        .execute(
            json!({ "skill_id": "disabled-skill" }),
            app_lib::runtime::tools::context::ToolExecutionContext::new(
                SessionId::new("conv-test"),
                RunId::new("run-test"),
                None,
                "tool-call-test",
                CancellationToken::new(),
            ),
        )
        .await
        .unwrap_err();

    assert!(
        format!("{err:?}").contains("disabled") || format!("{err:?}").contains("unavailable"),
        "error should mention disabled or unavailable: {err:?}"
    );
}
```

- [ ] **Step 2: Run the failing runtime tool test**

Run:

```powershell
cd src-tauri
cargo test load_skill_rejects_disabled_skill --test load_skill_skill_md_test -- --nocapture
```

Expected: FAIL because the test constructor and enablement-aware tool constructor do not exist.

- [ ] **Step 3: Thread enablement into `LoadSkillRuntimeTool`**

In `src-tauri/src/runtime/tools/builtin/load_skill.rs`, add:

```rust
use crate::plugin::skill::enablement::{SkillEnablementState, SkillEnablementStore};
```

Extend the struct:

```rust
enablement_store: Option<Arc<SkillEnablementStore>>,
test_enablement_state: Option<SkillEnablementState>,
```

Add constructors:

```rust
pub fn with_refresher_and_enablement(
    skill_registry: Arc<Mutex<SkillRegistry>>,
    refresher: Arc<dyn SkillRegistryRefresher>,
    enablement_store: Arc<SkillEnablementStore>,
) -> Self {
    Self {
        skill_registry,
        refresher: Some(refresher),
        last_refresh: Arc::new(Mutex::new(None)),
        enablement_store: Some(enablement_store),
        test_enablement_state: None,
    }
}

#[cfg(test)]
pub fn new_with_enablement_state_for_test(
    skill_registry: Arc<Mutex<SkillRegistry>>,
    state: SkillEnablementState,
) -> Self {
    Self {
        skill_registry,
        refresher: None,
        last_refresh: Arc::new(Mutex::new(None)),
        enablement_store: None,
        test_enablement_state: Some(state),
    }
}

fn enablement_state(&self) -> SkillEnablementState {
    if let Some(state) = self.test_enablement_state.clone() {
        return state;
    }
    self.enablement_store
        .as_ref()
        .map(|store| store.load_or_default())
        .unwrap_or_default()
}
```

Update existing constructors to set both new fields to `None`.

- [ ] **Step 4: Filter tool definition and execute**

In `definition()`, replace:

```rust
reg.skill_ids().join(", ")
```

with:

```rust
let state = self.enablement_state();
reg.enabled_skill_ids(&state).join(", ")
```

In `execute()`, after parsing `skill_id`, add:

```rust
let state = self.enablement_state();
if !state.is_enabled(&skill_id) {
    return Err(ToolError::ExecutionFailed(format!(
        "Skill disabled or unavailable: {skill_id}"
    )));
}
```

When retrying after refresh, use `reg.get_enabled(&skill_id, &state)` and build the available list with `reg.enabled_skill_ids(&state).join(", ")`.

- [ ] **Step 5: Thread enablement through request-scoped tool construction**

In `src-tauri/src/plugin/context.rs`, add:

```rust
pub skill_enablement:
    Option<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>,
```

In `RequestScopedRuntimeDeps`, add the same field and copy it in `from_plugin_context`.

In `src-tauri/src/plugin/registry.rs`, when constructing `"Skill"`, use:

```rust
let enablement = ctx.skill_enablement.clone();
let tool = match (ctx.app_handle.as_ref(), enablement) {
    (Some(app), Some(enablement_store)) => builtin::load_skill::LoadSkillRuntimeTool::with_refresher_and_enablement(
        registry,
        Arc::new(AppSkillRegistryRefresher { app: app.clone() }),
        enablement_store,
    ),
    (Some(app), None) => builtin::load_skill::LoadSkillRuntimeTool::with_refresher(
        registry,
        Arc::new(AppSkillRegistryRefresher { app: app.clone() }),
    ),
    (None, _) => builtin::load_skill::LoadSkillRuntimeTool::new(registry),
};
```

Update every `PluginContext { ... }` literal with `skill_enablement: None` unless app state is available. In `src-tauri/src/transport/tauri_commands/chat.rs`, set it from `app.state::<Arc<SkillEnablementStore>>()`.

- [ ] **Step 6: Filter chat catalog injection**

In `TauriChatServices`, add:

```rust
skill_enablement: Arc<crate::plugin::skill::enablement::SkillEnablementStore>,
```

In `TauriChatCommandAdapter::new_with_channel_sessions`, before constructing `services`, get:

```rust
let skill_enablement = app
    .state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>()
    .inner()
    .clone();
```

Set the field in `TauriChatServices`.

In `get_skill_catalog`, replace full catalog:

```rust
let state = self.services.skill_enablement.load_or_default();
self.services
    .skill_registry
    .lock()
    .map(|reg| reg.format_enabled_catalog(&state, 200_000))
    .unwrap_or_default()
```

- [ ] **Step 7: Run Task 3 tests**

Run:

```powershell
cd src-tauri
cargo test load_skill_rejects_disabled_skill --test load_skill_skill_md_test -- --nocapture
cargo test format_enabled_catalog_excludes_disabled_skills_but_full_catalog_keeps_all --lib
cargo check
```

Expected: PASS.

- [ ] **Step 8: Commit Task 3**

```powershell
git add src-tauri/src/transport/tauri_commands/chat.rs src-tauri/src/plugin/context.rs src-tauri/src/plugin/registry.rs src-tauri/src/runtime/tools/builtin/load_skill.rs src-tauri/tests/load_skill_skill_md_test.rs
git commit -m "feat: filter disabled skills from runtime"
```

---

### Task 4: Ensure Required Builtins And Split Marketplace Directory From Auto Install Sync

**Files:**

- Create: `src-tauri/src/plugin/skill/required_builtin.rs`
- Modify: `src-tauri/src/plugin/skill/global_sync.rs`
- Modify: `src-tauri/src/plugin/skill/sync_command.rs`
- Modify: `src-tauri/src/commands/skill_management.rs`
- Modify: `src-tauri/src/plugin/skill/mod.rs`
- Test: `src-tauri/src/plugin/skill/global_sync.rs`
- Test: `src-tauri/src/plugin/skill/required_builtin.rs`

- [ ] **Step 1: Add sync filtering tests**

In `src-tauri/src/plugin/skill/global_sync.rs`, add pure tests for deciding which remote packages are installable during sync:

```rust
#[cfg(test)]
mod sync_filter_tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn item(plugin_id: &str, version: &str) -> SkillPackageItem {
        SkillPackageItem {
            id: 1,
            plugin_id: plugin_id.to_string(),
            name: plugin_id.to_string(),
            version: version.to_string(),
            package_url: "https://example.invalid/pkg.zip".to_string(),
            package_size: 1,
            scope: "tenant".to_string(),
            category: None,
            display_i18n: None,
        }
    }

    #[test]
    fn sync_targets_include_already_installed_and_required_builtin_packages() {
        let local_state = GlobalSkillsState {
            installed: HashMap::from([("already-installed".to_string(), "1.0".to_string())]),
            updated_at_unix_seconds: 0,
        };
        let disk_installed = HashSet::from(["manual-local".to_string()]);
        let required_builtin = HashSet::from(["skill-creator".to_string(), "dingtalk-workspace".to_string()]);
        let remote = vec![
            item("already-installed", "1.1"),
            item("skill-creator", "1.0"),
            item("dingtalk-workspace", "1.0"),
            item("brand-new", "1.0"),
        ];

        let targets = select_packages_for_update(&remote, &local_state, &disk_installed, &required_builtin);

        assert_eq!(
            targets.iter().map(|i| i.plugin_id.as_str()).collect::<Vec<_>>(),
            vec!["already-installed", "skill-creator", "dingtalk-workspace"]
        );
    }

    #[test]
    fn first_login_with_no_local_state_does_not_auto_install_non_required_remote_packages() {
        let local_state = GlobalSkillsState::default();
        let disk_installed = HashSet::new();
        let required_builtin = HashSet::from(["skill-creator".to_string(), "dingtalk-workspace".to_string()]);
        let remote = vec![item("remote-default", "1.0")];

        let targets = select_packages_for_update(&remote, &local_state, &disk_installed, &required_builtin);

        assert!(targets.is_empty());
    }

    #[test]
    fn first_login_installs_required_builtin_packages_only() {
        let local_state = GlobalSkillsState::default();
        let disk_installed = HashSet::new();
        let required_builtin = HashSet::from(["skill-creator".to_string(), "dingtalk-workspace".to_string()]);
        let remote = vec![
            item("skill-creator", "1.0"),
            item("dingtalk-workspace", "1.0"),
            item("tenant-workflow", "1.0"),
        ];

        let targets = select_packages_for_update(&remote, &local_state, &disk_installed, &required_builtin);

        assert_eq!(
            targets.iter().map(|i| i.plugin_id.as_str()).collect::<Vec<_>>(),
            vec!["skill-creator", "dingtalk-workspace"]
        );
    }
}
```

- [ ] **Step 2: Run failing sync tests**

Run:

```powershell
cd src-tauri
cargo test sync_targets_include_already_installed_and_required_builtin_packages --lib
cargo test first_login_with_no_local_state_does_not_auto_install_non_required_remote_packages --lib
cargo test first_login_installs_required_builtin_packages_only --lib
```

Expected: FAIL because `select_packages_for_update` does not exist.

- [ ] **Step 3: Add required builtin allowlist**

Create `src-tauri/src/plugin/skill/required_builtin.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredBuiltinSkill {
    pub id: &'static str,
    pub display_alias: &'static str,
    pub default_enabled: bool,
}

pub const REQUIRED_BUILTIN_SKILLS: &[RequiredBuiltinSkill] = &[
    RequiredBuiltinSkill {
        id: "skill-creator",
        display_alias: "create-skill",
        default_enabled: true,
    },
    RequiredBuiltinSkill {
        id: "dingtalk-workspace",
        display_alias: "dws",
        default_enabled: true,
    },
];

pub fn required_builtin_ids() -> std::collections::HashSet<String> {
    REQUIRED_BUILTIN_SKILLS
        .iter()
        .map(|skill| skill.id.to_string())
        .collect()
}

pub fn is_required_builtin_skill(id: &str) -> bool {
    REQUIRED_BUILTIN_SKILLS.iter().any(|skill| skill.id == id)
}
```

Export it from `src-tauri/src/plugin/skill/mod.rs`.

`dingtalk-workspace` is the SKILL.md wrapper id; `dws` is the CLI/display shorthand and `src-tauri/resources/dws` remains the bundled binary. If the published package id differs, update this allowlist to the real `plugin_id` and keep UI display mapping separate.

- [ ] **Step 4: Implement package update selector**

In `global_sync.rs`, add:

```rust
pub fn select_packages_for_update<'a>(
    remote: &'a [SkillPackageItem],
    local_state: &GlobalSkillsState,
    disk_installed_ids: &HashSet<String>,
    required_builtin_ids: &HashSet<String>,
) -> Vec<&'a SkillPackageItem> {
    remote
        .iter()
        .filter(|item| {
            local_state.installed.contains_key(&item.plugin_id)
                || disk_installed_ids.contains(&item.plugin_id)
                || required_builtin_ids.contains(&item.plugin_id)
        })
        .collect()
}
```

Add a helper to read installed ids from disk:

```rust
fn installed_skill_ids_in_dir(dir: &Path) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    if !dir.is_dir() {
        return Ok(ids);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read skills dir '{}'", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join("SKILL.md").is_file() {
            if let Some(id) = path.file_name().and_then(|name| name.to_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    Ok(ids)
}
```

- [ ] **Step 5: Change `sync_skill_packages_from_server` loop**

After fetching remote list and reading local state, compute:

```rust
let disk_installed_ids = installed_skill_ids_in_dir(&config.global_skills_dir)?;
let required_builtin_ids = crate::plugin::skill::required_builtin::required_builtin_ids();
let update_targets = select_packages_for_update(
    &list.data,
    &local_state,
    &disk_installed_ids,
    &required_builtin_ids,
);
```

Iterate `for item in update_targets` instead of `for item in &list.data`.

Keep `remote_ids` based on the full remote list so cleanup still knows which tracked packages disappeared.

When `local_state.installed` is empty on first login, only required builtin packages whose `plugin_id` is in `REQUIRED_BUILTIN_SKILLS` are installed automatically. All other remote packages remain market-only until the user clicks `+`.

Do not call `clear_override` for required builtin sync installs or updates. Required builtins are default enabled because absent disabled override means enabled; if the user previously disabled `dingtalk-workspace`, sync must preserve that local choice.

- [ ] **Step 6: Define sync result and missing-required behavior**

Keep `sync_builtin_skills` as the login-time command name for compatibility with `AuthGate`, but document and implement the new semantics:

- `installed`: required builtin packages installed for the first time, plus already-installed packages that were updated.
- `skipped`: targeted packages that could not be installed or updated. This can include a required builtin id that the server did not return or failed to download.
- Non-required remote packages that are merely visible in the market are not counted as skipped; otherwise first login would log hundreds of harmless skips.

If a required builtin id is not present in `/v1/skill-packages`, append that id to `skipped` and log a warning:

```rust
for required_id in &required_builtin_ids {
    if !list.data.iter().any(|item| item.plugin_id == *required_id) {
        report.skipped.push(required_id.clone());
        log::warn!("[skill-sync] required builtin '{}' missing from remote package list", required_id);
    }
}
```

Do not block login if a required builtin is missing or fails to download. The next login sync or manual update can retry.

- [ ] **Step 7: Ensure marketplace install refreshes registry and enables the skill**

In `install_marketplace_skill`, after extraction succeeds:

```rust
if let Some(store) = app.try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>() {
    let _ = store.clear_override(&plugin_id);
}
refresh_skill_registry(&app)?;
```

Return a message that no longer says restart is required:

```rust
Ok(format!("Installed '{}'", plugin_id))
```

- [ ] **Step 8: Run Task 4 tests**

Run:

```powershell
cd src-tauri
cargo test sync_targets_include_already_installed_and_required_builtin_packages --lib
cargo test first_login_with_no_local_state_does_not_auto_install_non_required_remote_packages --lib
cargo test first_login_installs_required_builtin_packages_only --lib
cargo check
```

Expected: PASS.

- [ ] **Step 9: Commit Task 4**

```powershell
git add src-tauri/src/plugin/skill/global_sync.rs src-tauri/src/plugin/skill/sync_command.rs src-tauri/src/plugin/skill/required_builtin.rs src-tauri/src/plugin/skill/mod.rs src-tauri/src/commands/skill_management.rs
git commit -m "feat: install only required builtin skills by default"
```

---

### Task 5: Frontend API, Store, And Event Plumbing

**Files:**

- Modify: `src/lib/tauri.ts`
- Modify: `src/stores/skillStore.ts`
- Modify: `src/stores/skillStore.test.ts`
- Modify: `src/components/auth/AuthGate.tsx`
- Modify: `src/features/auth/AuthGate.integration.test.tsx`

- [ ] **Step 1: Add failing store tests**

In `src/stores/skillStore.test.ts`, update mocked skills to include enabled states, and add:

```ts
it('enabledSkills 只返回开启的技能', async () => {
  tauriMock.listSkills.mockResolvedValueOnce([
    { id: 'enabled-one', displayName: '开启技能', description: 'desc', source: 'user', hasWorkflow: true, icon: '', category: 'general', triggerText: '', shortDescription: '', displayNameEn: '', shortDescriptionEn: '', updatedAt: null, enabled: true },
    { id: 'disabled-one', displayName: '关闭技能', description: 'desc', source: 'user', hasWorkflow: true, icon: '', category: 'general', triggerText: '', shortDescription: '', displayNameEn: '', shortDescriptionEn: '', updatedAt: null, enabled: false },
  ])

  await useSkillStore.getState().reload()

  expect(useSkillStore.getState().skills).toHaveLength(2)
  expect(useSkillStore.getState().enabledSkills.map((s) => s.id)).toEqual(['enabled-one'])
})

it('setSkillEnabled 写后端并刷新列表', async () => {
  await useSkillStore.getState().setSkillEnabled('disabled-one', true)

  expect(tauriMock.setSkillEnabled).toHaveBeenCalledWith('disabled-one', true)
  expect(tauriMock.listSkills).toHaveBeenCalled()
})
```

Extend `tauriMock` with:

```ts
setSkillEnabled: vi.fn().mockResolvedValue({ id: 'disabled-one', enabled: true }),
installMarketplaceSkill: vi.fn().mockResolvedValue('installed'),
```

- [ ] **Step 2: Run failing store tests**

Run:

```powershell
pnpm exec vitest run src/stores/skillStore.test.ts
```

Expected: FAIL because `enabledSkills` and `setSkillEnabled` are not implemented.

- [ ] **Step 3: Update frontend Tauri types and functions**

In `src/lib/tauri.ts`, add event:

```ts
SKILL_ENABLEMENT_CHANGED: 'skill:enablement-changed',
```

Extend `SkillInfo`:

```ts
enabled: boolean
installed?: boolean
```

Add:

```ts
export interface SkillEnablementChangedPayload {
  skillId: string
  enabled: boolean
}

export function setSkillEnabled(skillId: string, enabled: boolean): Promise<SkillInfo> {
  return invoke<SkillInfo>('set_skill_enabled', { skillId, enabled })
}
```

- [ ] **Step 4: Update `skillStore`**

In `src/stores/skillStore.ts`, import:

```ts
import { installMarketplaceSkill, setSkillEnabled as setSkillEnabledCommand } from '@/lib/tauri'
```

Update `normalizeSkill` to default enabled to true:

```ts
enabled: skill.enabled ?? true,
```

Extend state:

```ts
enabledSkills: SkillInfo[]
setSkillEnabled: (id: string, enabled: boolean) => Promise<void>
installMarketplace: (packageId: number, pluginId: string) => Promise<void>
```

Set derived values after reload:

```ts
const skills = (await listSkills()).map(normalizeSkill)
set({ skills, enabledSkills: skills.filter((skill) => skill.enabled), isLoading: false })
```

Reset both arrays:

```ts
reset: () => set({ skills: [], enabledSkills: [], isLoading: false }),
```

Add actions:

```ts
async setSkillEnabled(id, enabled) {
  await setSkillEnabledCommand(id, enabled)
  await get().reload()
},
async installMarketplace(packageId, pluginId) {
  await installMarketplaceSkill(packageId, pluginId)
  await get().reload()
},
```

- [ ] **Step 5: Listen for enablement event**

In `src/components/auth/AuthGate.tsx`, update the event listener effect to register both events:

```ts
const handles = await Promise.all([
  listen(TAURI_EVENTS.SKILL_REGISTRY_REFRESHED, reloadSkills),
  listen(TAURI_EVENTS.SKILL_ENABLEMENT_CHANGED, reloadSkills),
])
```

Use one `reloadSkills` callback:

```ts
const reloadSkills = () => {
  void useSkillStore.getState().reload().catch((err) => {
    console.warn('[skill-refresh] skillStore reload failed:', err)
  })
}
```

Cleanup all handles:

```ts
handles.forEach((handle) => handle())
```

In `AuthGate.integration.test.tsx`, add `SKILL_ENABLEMENT_CHANGED` to the mocked `TAURI_EVENTS`.

- [ ] **Step 6: Run Task 5 tests**

Run:

```powershell
pnpm exec vitest run src/stores/skillStore.test.ts src/features/auth/AuthGate.integration.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit Task 5**

```powershell
git add src/lib/tauri.ts src/stores/skillStore.ts src/stores/skillStore.test.ts src/components/auth/AuthGate.tsx src/features/auth/AuthGate.integration.test.tsx
git commit -m "feat: wire skill enablement store"
```

---

### Task 6: Skill Center Tabs, Market Cards, And Enable Switches

**Files:**

- Modify: `src/features/skill-center/SkillCenterPage.tsx`
- Modify: `src/components/skills/SkillCard.tsx` only if action layout needs a small reusable prop.
- Test: add or update `src/features/skill-center/SkillCenterPage.integration.test.tsx`

- [ ] **Step 1: Add failing UI tests for tabs and clean market cards**

Create or update `src/features/skill-center/SkillCenterPage.integration.test.tsx` with:

```tsx
it('市场卡片只展示添加或已添加，不展示关闭开关和去对话', async () => {
  render(<SkillCenterPage />)

  fireEvent.click(await screen.findByRole('tab', { name: '市场' }))

  expect(screen.getByText('深入研究')).toBeInTheDocument()
  expect(screen.queryByText('已关闭')).not.toBeInTheDocument()
  expect(screen.queryByText('去对话')).not.toBeInTheDocument()
  expect(screen.queryByRole('switch')).not.toBeInTheDocument()
})

it('已安装页展示开关并调用 setSkillEnabled', async () => {
  render(<SkillCenterPage />)

  fireEvent.click(await screen.findByRole('tab', { name: /已安装/ }))
  fireEvent.click(screen.getByRole('switch', { name: /商业方案/ }))

  await waitFor(() => {
    expect(useSkillStore.getState().setSkillEnabled).toHaveBeenCalledWith('biz-plan', false)
  })
})
```

Mock `listMarketplaceSkills` to return one not-installed item and one installed item:

```ts
listMarketplaceSkills: vi.fn().mockResolvedValue({
  items: [
    { id: 1, pluginId: 'deep-research', name: '深入研究', description: '通过来源验证、三角测量和引用支持的报告。', category: 'research', icon: '', version: '1.0', scope: 'tenant', status: 'published', downloads: 0, featured: false, packageSize: 1, tenantName: '', createdAt: '' },
    { id: 2, pluginId: 'biz-plan', name: '商业方案', description: '商业方案撰写', category: 'general', icon: '', version: '1.0', scope: 'tenant', status: 'published', downloads: 0, featured: false, packageSize: 1, tenantName: '', createdAt: '' },
  ],
  total: 2,
  page: 1,
  size: 20,
}),
```

- [ ] **Step 2: Run failing skill center test**

Run:

```powershell
pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx
```

Expected: FAIL because tabs and market behavior are not implemented.

- [ ] **Step 3: Add page tab state**

In `SkillCenterPage.tsx`, add:

```ts
type SkillCenterTab = 'market' | 'builtIn' | 'installed'
const [activeTab, setActiveTab] = useState<SkillCenterTab>('market')
```

Render top tabs near the existing heading:

```tsx
<div role="tablist" aria-label="技能中心分类" className="flex items-center gap-6">
  {[
    { id: 'market', label: '市场' },
    { id: 'builtIn', label: '内置' },
    { id: 'installed', label: `已安装 ${skills.length}` },
  ].map((tab) => (
    <button
      key={tab.id}
      type="button"
      role="tab"
      aria-selected={activeTab === tab.id}
      onClick={() => setActiveTab(tab.id as SkillCenterTab)}
      className={activeTab === tab.id ? 'text-xl font-semibold text-foreground' : 'text-xl font-semibold text-muted-foreground'}
    >
      {tab.label}
    </button>
  ))}
</div>
```

- [ ] **Step 4: Load market items without installing**

Import `listMarketplaceSkills` and `MarketplaceSkillItem`.

Add state:

```ts
const [marketItems, setMarketItems] = useState<MarketplaceSkillItem[]>([])
const [marketLoading, setMarketLoading] = useState(false)
```

Load on market tab:

```ts
useEffect(() => {
  if (activeTab !== 'market' || !isLoggedIn) return
  let cancelled = false
  setMarketLoading(true)
  listMarketplaceSkills(1, 100, category === 'recommended' ? undefined : category, query || undefined)
    .then((response) => {
      if (!cancelled) setMarketItems(response.items)
    })
    .catch(handleLoadError)
    .finally(() => {
      if (!cancelled) setMarketLoading(false)
    })
  return () => {
    cancelled = true
  }
}, [activeTab, isLoggedIn, category, query])
```

- [ ] **Step 5: Render market card actions**

Build installed id set:

```ts
const installedIds = useMemo(() => new Set(skills.map((skill) => skill.id)), [skills])
```

For market card:

```tsx
const isInstalled = installedIds.has(item.pluginId)
const action = isInstalled ? (
  <span className="rounded-md bg-muted px-2 py-1 text-xs font-medium text-muted-foreground">已添加</span>
) : (
  <button
    type="button"
    aria-label={`添加 ${item.name}`}
    className="flex h-8 w-8 items-center justify-center rounded-md text-foreground hover:bg-muted"
    onClick={() => void handleInstallMarket(item)}
  >
    <Plus className="h-5 w-5" />
  </button>
)
```

Do not render switch, `已关闭`, or `去对话` in market cards.

- [ ] **Step 6: Render installed/built-in switches**

Use source grouping:

```ts
const builtInSkills = filteredSkills.filter((skill) => skill.source === 'global' || skill.source === 'tenant')
const installedSkills = filteredSkills
```

For built-in and installed cards, render a switch action:

```tsx
<Switch
  aria-label={`${localized.name} ${skill.enabled ? '关闭' : '开启'}`}
  checked={skill.enabled}
  onCheckedChange={(checked) => void setSkillEnabled(skill.id, checked)}
/>
```

Use the existing card grid and category chips. Do not convert to a table/list.

- [ ] **Step 7: Install market item and route to conversation**

Add:

```ts
const installMarketplace = useSkillStore((s) => s.installMarketplace)
const setPendingSkill = useUiStore((s) => s.setPendingSkill)

const handleInstallMarket = async (item: MarketplaceSkillItem) => {
  await installMarketplace(item.id, item.pluginId)
  setPendingSkill({
    id: item.pluginId,
    label: item.name,
    trigger: `/${item.pluginId}`,
  })
  setRoute({ kind: 'home' })
}
```

- [ ] **Step 8: Run Task 6 tests**

Run:

```powershell
pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Commit Task 6**

```powershell
git add src/features/skill-center/SkillCenterPage.tsx src/features/skill-center/SkillCenterPage.integration.test.tsx src/components/skills/SkillCard.tsx
git commit -m "feat: add skill center enablement tabs"
```

---

### Task 7: Skill Detail And Chat Entrypoints Use Enabled Skills

**Files:**

- Modify: `src/features/skill-detail/SkillDetailPage.tsx`
- Modify: `src/features/skill-detail/SkillDetailPage.test.tsx`
- Modify: `src/components/chat/SkillPopover.tsx`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/home/HomeTaskComposerCard.tsx`
- Modify: `src/features/welcome/WelcomeScreen.tsx` or the actual welcome component if the path differs.
- Test: existing focused component tests.

- [ ] **Step 1: Add failing detail tests**

In `SkillDetailPage.test.tsx`, include `enabled` in existing mock skills and add:

```tsx
it('关闭技能后不允许直接使用，只显示开启并使用', () => {
  useSkillStore.setState({
    ...useSkillStore.getState(),
    skills: [
      {
        id: 'biz-proposal',
        displayName: '商业方案撰写',
        displayNameEn: 'Business Proposal',
        description: '依据业务数据生成商业方案。',
        source: 'user',
        hasWorkflow: true,
        icon: 'sparkles',
        shortDescription: '商业方案撰写',
        shortDescriptionEn: 'Business proposal writing',
        triggerText: '/biz-proposal',
        category: 'general',
        updatedAt: null,
        enabled: false,
      },
    ],
    enabledSkills: [],
    setSkillEnabled: vi.fn().mockResolvedValue(undefined),
  })

  render(<SkillDetailPage skillId="biz-proposal" />)

  expect(screen.getByRole('button', { name: '开启并使用' })).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: '使用' })).toBeNull()
})
```

Add another test:

```tsx
it('点击关闭会调用 setSkillEnabled 且不会进入对话', async () => {
  const setSkillEnabled = vi.fn().mockResolvedValue(undefined)
  useSkillStore.setState({ ...useSkillStore.getState(), setSkillEnabled })

  render(<SkillDetailPage skillId="biz-proposal" />)
  fireEvent.click(screen.getByRole('button', { name: '关闭' }))

  expect(setSkillEnabled).toHaveBeenCalledWith('biz-proposal', false)
  expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'biz-proposal' })
})
```

- [ ] **Step 2: Run failing detail tests**

Run:

```powershell
pnpm exec vitest run src/features/skill-detail/SkillDetailPage.test.tsx
```

Expected: FAIL because detail actions do not branch by `enabled`.

- [ ] **Step 3: Implement detail state actions**

In `SkillDetailPage.tsx`, read:

```ts
const setSkillEnabled = useSkillStore((s) => s.setSkillEnabled)
```

Add:

```ts
const useSkill = () => {
  if (!skill || !skill.enabled) return
  const localized = localizeSkill(skill)
  setPendingSkill({
    id: skill.id,
    label: localized.name,
    trigger: skill.triggerText?.trim() || `/${skill.id}`,
  })
  setRoute({ kind: 'home' })
}

const enableAndUse = async () => {
  if (!skill) return
  await setSkillEnabled(skill.id, true)
  const localized = localizeSkill(skill)
  setPendingSkill({
    id: skill.id,
    label: localized.name,
    trigger: skill.triggerText?.trim() || `/${skill.id}`,
  })
  setRoute({ kind: 'home' })
}

const closeSkill = async () => {
  if (!skill) return
  await setSkillEnabled(skill.id, false)
}
```

Render action bar:

```tsx
<SkillActionBar
  primaryLabel={skill.enabled ? '使用' : '开启并使用'}
  onPrimary={skill.enabled ? useSkill : enableAndUse}
  secondaryLabel={skill.enabled ? '关闭' : '保持关闭'}
  onSecondary={skill.enabled ? closeSkill : goToSkillCenter}
/>
```

For disabled skills, add a small warning near the hero subtitle:

```tsx
{!skill.enabled ? (
  <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">
    当前已关闭。关闭后不会出现在聊天输入框，也不会进入模型上下文。
  </div>
) : null}
```

- [ ] **Step 4: Filter chat skill popover and composer tokens**

In `SkillPopover.tsx`, change:

```ts
const skills = useSkillStore((s) => s.enabledSkills)
```

In `ChatBottomArea.tsx`, change:

```ts
const skills = useSkillStore((s) => s.enabledSkills)
```

Keep `getSkillById` using full list so previously inserted chips can still localize safely, but do not offer disabled skills in picker or slash tokens.

- [ ] **Step 5: Filter other skill entrypoints**

Run:

```powershell
rg -n "useSkillStore\\(\\(s\\) => s\\.skills|\\.skills\\)" src\\components src\\features src\\stores
```

For user-facing pickers or quick-start suggestions, use `enabledSkills`. Management pages keep full `skills`.

Specific expected changes:

- `src/components/home/HomeTaskComposerCard.tsx`: use enabled skills for skill suggestions.
- Welcome screen component: use enabled skills for any skill shortcuts.
- Skill center and detail keep full `skills`.

- [ ] **Step 6: Run Task 7 tests**

Run:

```powershell
pnpm exec vitest run src/features/skill-detail/SkillDetailPage.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
rg -n "useSkillStore\\(\\(s\\) => s\\.skills\\)" src\\components src\\features
```

Expected:

- Tests PASS.
- `rg` output only includes management/detail views where full skill list is intentional.

- [ ] **Step 7: Commit Task 7**

```powershell
git add src/features/skill-detail/SkillDetailPage.tsx src/features/skill-detail/SkillDetailPage.test.tsx src/components/chat/SkillPopover.tsx src/components/chat-scene/ChatBottomArea.tsx src/components/home/HomeTaskComposerCard.tsx
git commit -m "feat: hide disabled skills from chat entrypoints"
```

---

### Task 8: Final Verification, Browser Check, And Plan Follow-Through

**Files:**

- Verify all changed files.
- Update docs only if implementation discovers a real design correction.

- [ ] **Step 1: Run focused frontend tests**

Run:

```powershell
pnpm exec vitest run src/stores/skillStore.test.ts src/features/skill-center/SkillCenterPage.integration.test.tsx src/features/skill-detail/SkillDetailPage.test.tsx src/features/auth/AuthGate.integration.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
```

Expected: PASS.

- [ ] **Step 2: Run focused backend tests**

Run:

```powershell
cd src-tauri
cargo test skill_enablement --lib
cargo test enabled_skill_ids_excludes_disabled_ids --lib
cargo test format_enabled_catalog_excludes_disabled_skills_but_full_catalog_keeps_all --lib
cargo test load_skill_rejects_disabled_skill --test load_skill_skill_md_test -- --nocapture
cargo test sync_targets_include_already_installed_and_required_builtin_packages --lib
cargo test first_login_with_no_local_state_does_not_auto_install_non_required_remote_packages --lib
cargo test first_login_installs_required_builtin_packages_only --lib
cargo check
```

Expected: PASS.

- [ ] **Step 3: Run broader build check**

Run from repo root:

```powershell
pnpm test
pnpm build
```

Expected:

- `pnpm test` passes or reports only known unrelated failures with exact test names captured.
- `pnpm build` exits 0.

- [ ] **Step 4: Browser verification**

Start or reuse the local app server. In the in-app browser verify:

- Skill Center has `市场 / 内置 / 已安装`.
- Market cards show `+` or `已添加`.
- Market cards do not show switch, `已关闭`, or `去对话`.
- Installed/Built-in tabs show switches.
- Turning a skill off removes it from the chat skill picker.
- Disabled skill detail shows `开启并使用` and does not offer direct `使用`.

- [ ] **Step 5: Runtime verification with logs or tests**

Use a disabled skill id such as `biz-plan` and confirm:

- `list_skills` still returns it with `enabled: false`.
- Chat catalog generated by backend does not contain that id.
- `Skill` runtime tool returns unavailable for that id.

The Task 3 cargo test is the minimum proof for this item when no manual runtime harness is available in the current environment.

- [ ] **Step 6: Final commit**

If Task 8 required test-only or wiring adjustments, commit them:

```powershell
git add .
git commit -m "test: verify skill enablement flow"
```

If no files changed during Task 8, do not create an empty commit.

---

## Implementation Notes

- Keep `SkillRegistry::skill_ids()` and `format_full_catalog()` full-list semantics. Only enabled-specific methods should filter.
- Do not remove disabled skills from `list_skills`; management pages need to display and re-enable them.
- Do not show disabled state in market cards. Market only shows `+` or `已添加`.
- Do not add a `去对话` button to cards.
- Do not rely on frontend filtering for security. Runtime catalog and `Skill` tool must both enforce enablement.
- New login sync must not increase installed skill count by installing newly published remote packages.

## Handoff Checklist

- [ ] Backend state file exists and survives app restart.
- [ ] Toggle event causes `skillStore.reload()`.
- [ ] Chat picker and slash tokens use `enabledSkills`.
- [ ] Model catalog excludes disabled skills.
- [ ] `Skill` tool rejects disabled skills.
- [ ] Market install explicitly installs and enables one skill.
- [ ] Login sync no longer installs every remote enterprise/platform skill.
