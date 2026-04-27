# User-Scoped Storage Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 切换租户/账号时，所有用户业务数据（聊天、MCP、schedules、permissions、skills、browser profile 等）完全隔离，互不可见。

**Architecture:** 在 `~/.renlijia/` 下新增 `global/` 和 `users/t_{tenantId}__u_{userId}/` 两层目录。auth bootstrap 走 GlobalConfigStore，业务数据走 CurrentUserStorage（内部 `RwLock<Option<Inner>>`），所有用户态服务通过 `UserScopedPathResolver` 获取当前用户路径快照，启动时同步迁移 legacy 数据。

**Tech Stack:** Rust (Tauri 2.x), TypeScript/React (Zustand), serde_json, tokio

**Spec:** `docs/superpowers/specs/2026-04-27-user-scoped-storage-design.md`

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `src-tauri/src/storage/user_scope.rs` | `UserScope` type + `key()` |
| `src-tauri/src/storage/global_config_store.rs` | `GlobalConfigStore`：读写 `global/config.json` 和 `global/auth/` |
| `src-tauri/src/storage/user_scoped_paths.rs` | `UserScopedPaths` 快照 + `UserScopedPathResolver` trait，统一暴露当前用户路径 |
| `src-tauri/src/storage/current_user_storage.rs` | `CurrentUserStorage`：`RwLock<Option<Inner>>` 包装，提供 `get()` / `require()` / `reload_for_scope()`，并实现 `UserScopedPathResolver` |
| `src-tauri/src/storage/migration_user_scope.rs` | `migrate_legacy_to_user_scope_if_needed()` + `migrate_legacy_config_if_needed()` |
| `src-tauri/tests/user_scope_migration_test.rs` | 迁移集成测试 |

### Modified files

| File | Changes |
|---|---|
| `src-tauri/src/storage/aijia_home.rs` | 新增 `global_dir()`, `auth_dir()`, `users_dir()`, `user_dir(scope)`, `user_*_path(scope)` 等 20+ path helpers |
| `src-tauri/src/storage/mod.rs` | 导出新模块 |
| `src-tauri/src/auth/mod.rs` | `AuthManager::new()` 改为接收 `GlobalConfigStore` 替代 `Arc<AppStorage>` |
| `src-tauri/src/lib.rs` | 启动顺序重写：先 auth → derive scope → migration → CurrentUserStorage → services |
| `src-tauri/src/storage/migration.rs` | 导出 `copy_dir`、`read_state_json`、`write_state_json` 为 `pub(crate)` |
| `src-tauri/src/runtime/schedule.rs` | `ScheduleStore::new()` 改为接收 user scope 路径 |
| `src-tauri/src/runtime/schedule_runner.rs` | `spawn_schedule_runner()` 改为接收 `Arc<CurrentUserStorage>` |
| `src-tauri/src/storage/mcp_config_store.rs` | `McpConfigStore::new()` 路径改为 user scope |
| `src-tauri/src/runtime/store/permission_store.rs` | user layer 路径改为 user scope |
| `src-tauri/src/runtime/agent/agent_runtime.rs` | 路径改为 user scope |
| `src-tauri/src/commands/auth.rs` | `cloud_login` 新增 scope 切换逻辑；`cloud_logout` 新增 storage 清理 |
| `src-tauri/src/commands/schedules.rs` | `schedule_store()` 改为从 `CurrentUserStorage` 取路径 |
| `src/stores/authStore.ts` | `login()` / `logout()` 新增清理 chatStore 等全部缓存 |
| `src/stores/chatStore.ts` | 新增 `resetAll()` 方法 |

---

## Task 1: UserScope 类型

**Files:**
- Create: `src-tauri/src/storage/user_scope.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Test: `src-tauri/src/storage/user_scope.rs` (内联 `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

```rust
// src-tauri/src/storage/user_scope.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_format_stable() {
        let scope = UserScope { tenant_id: 1, user_id: 2 };
        assert_eq!(scope.key(), "t_1__u_2");
    }

    #[test]
    fn key_format_large_ids() {
        let scope = UserScope { tenant_id: 123456, user_id: 789012 };
        assert_eq!(scope.key(), "t_123456__u_789012");
    }

    #[test]
    fn from_cloud_auth_extracts_ids() {
        // Will test after CloudAuth integration
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test storage::user_scope --no-fail-fast -- --nocapture`
Expected: compile error — module not found

- [ ] **Step 3: Write UserScope implementation**

```rust
// src-tauri/src/storage/user_scope.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserScope {
    pub tenant_id: i64,
    pub user_id: i64,
}

impl UserScope {
    pub fn new(tenant_id: i64, user_id: i64) -> Self {
        Self { tenant_id, user_id }
    }

    /// Stable directory name: `t_{tenant_id}__u_{user_id}`
    pub fn key(&self) -> String {
        format!("t_{}__u_{}", self.tenant_id, self.user_id)
    }
}
```

- [ ] **Step 4: Export from mod.rs**

Add to `src-tauri/src/storage/mod.rs`:
```rust
pub mod user_scope;
pub use user_scope::UserScope;
```

- [ ] **Step 5: Run tests and verify pass**

Run: `cd src-tauri && cargo test storage::user_scope -- --nocapture`
Expected: 2 tests pass

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage/user_scope.rs src-tauri/src/storage/mod.rs
git commit -m "feat(storage): add UserScope type with stable directory key format"
```

---

## Task 2: AiJiaHome 分层 path helper

**Files:**
- Modify: `src-tauri/src/storage/aijia_home.rs`

- [ ] **Step 1: Write failing tests for new path helpers**

在 `aijia_home.rs` 底部 `#[cfg(test)]` 模块中添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::UserScope;

    #[test]
    fn global_dir_under_root() {
        let home = AiJiaHome::from_home();
        assert_eq!(home.global_dir(), home.root().join("global"));
    }

    #[test]
    fn auth_dir_under_global() {
        let home = AiJiaHome::from_home();
        assert_eq!(home.auth_dir(), home.root().join("global").join("auth"));
    }

    #[test]
    fn user_dir_uses_scope_key() {
        let home = AiJiaHome::from_home();
        let scope = UserScope::new(1, 2);
        assert_eq!(
            home.user_dir(&scope),
            home.root().join("users").join("t_1__u_2")
        );
    }

    #[test]
    fn user_config_path() {
        let home = AiJiaHome::from_home();
        let scope = UserScope::new(1, 2);
        assert_eq!(
            home.user_config_path(&scope),
            home.user_dir(&scope).join("config.json")
        );
    }

    #[test]
    fn user_mcp_config_path() {
        let home = AiJiaHome::from_home();
        let scope = UserScope::new(1, 2);
        assert_eq!(
            home.user_mcp_config_path(&scope),
            home.user_dir(&scope).join("mcp_servers.json")
        );
    }

    #[test]
    fn user_skills_dir() {
        let home = AiJiaHome::from_home();
        let scope = UserScope::new(1, 2);
        assert_eq!(
            home.user_skills_dir(&scope),
            home.user_dir(&scope).join("skills")
        );
    }

    #[test]
    fn global_state_path() {
        let home = AiJiaHome::from_home();
        assert_eq!(
            home.global_state_path(),
            home.global_dir().join("state.json")
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test storage::aijia_home::tests -- --nocapture`
Expected: compile errors — methods not found

- [ ] **Step 3: Implement all new path helpers**

在 `AiJiaHome` impl block 中添加（保留所有现有方法不变）：

```rust
use crate::storage::UserScope;

// ---- global layer ----

pub fn global_dir(&self) -> PathBuf {
    self.root.join("global")
}

pub fn global_config_path(&self) -> PathBuf {
    self.global_dir().join("config.json")
}

pub fn global_state_path(&self) -> PathBuf {
    self.global_dir().join("state.json")
}

pub fn auth_dir(&self) -> PathBuf {
    self.global_dir().join("auth")
}

pub fn cloud_auth_path(&self) -> PathBuf {
    self.auth_dir().join("cloud_auth")
}

pub fn active_account_path(&self) -> PathBuf {
    self.auth_dir().join("active_account.json")
}

pub fn users_dir(&self) -> PathBuf {
    self.root.join("users")
}

// ---- user scope layer ----

pub fn user_dir(&self, scope: &UserScope) -> PathBuf {
    self.users_dir().join(scope.key())
}

pub fn user_config_path(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("config.json")
}

pub fn user_scope_json_path(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("scope.json")
}

pub fn user_conversations_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("conversations")
}

pub fn user_schedules_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("schedules")
}

pub fn user_permissions_path(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("permissions.json")
}

pub fn user_mcp_config_path(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("mcp_servers.json")
}

pub fn user_agent_invocations_path(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("agent_invocations.json")
}

pub fn user_subagent_transcripts_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("subagent_transcripts")
}

pub fn user_skills_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("skills")
}

pub fn user_playwright_profile_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("playwright-profile")
}

pub fn user_api_data_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("api-data")
}

pub fn user_screenshots_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("screenshots")
}

pub fn user_site_profiles_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("site-profiles")
}

pub fn user_audit_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("audit")
}

pub fn user_logs_dir(&self, scope: &UserScope) -> PathBuf {
    self.user_dir(scope).join("logs")
}
```

- [ ] **Step 4: Add `ensure_global_dirs()` method**

```rust
/// Ensure global-layer directories exist (called before auth restore).
pub fn ensure_global_dirs(&self) -> std::io::Result<()> {
    std::fs::create_dir_all(self.global_dir())?;
    std::fs::create_dir_all(self.auth_dir())?;
    std::fs::create_dir_all(self.users_dir())?;
    Ok(())
}

/// Ensure user-scope directories exist (called after scope is known).
pub fn ensure_user_dirs(&self, scope: &UserScope) -> std::io::Result<()> {
    let ud = self.user_dir(scope);
    std::fs::create_dir_all(ud.join("conversations"))?;
    std::fs::create_dir_all(ud.join("shared").join("memory"))?;
    std::fs::create_dir_all(ud.join("shared").join("cache"))?;
    std::fs::create_dir_all(ud.join("audit"))?;
    std::fs::create_dir_all(ud.join("schedules"))?;
    std::fs::create_dir_all(ud.join("skills"))?;
    std::fs::create_dir_all(ud.join("subagent_transcripts"))?;
    std::fs::create_dir_all(ud.join("playwright-profile"))?;
    std::fs::create_dir_all(ud.join("api-data"))?;
    std::fs::create_dir_all(ud.join("screenshots"))?;
    std::fs::create_dir_all(ud.join("site-profiles"))?;
    std::fs::create_dir_all(ud.join("logs"))?;
    Ok(())
}
```

- [ ] **Step 5: Run tests and verify pass**

Run: `cd src-tauri && cargo test storage::aijia_home -- --nocapture`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage/aijia_home.rs
git commit -m "feat(storage): add global/user-scoped path helpers to AiJiaHome"
```

---

## Task 2.5: UserScopedPaths 快照 + UserScopedPathResolver trait

**Files:**
- Create: `src-tauri/src/storage/user_scoped_paths.rs`
- Modify: `src-tauri/src/storage/mod.rs`

设计思想：**高内聚低耦合**。`AiJiaHome` 负责路径规则（纯函数），`UserScopedPaths` 是"当前用户所有路径"的不可变快照，`UserScopedPathResolver` trait 是业务服务获取路径的唯一接口。业务模块不依赖 `AiJiaHome`、不知道 `UserScope`，只依赖 trait。

- [ ] **Step 1: Write failing tests**

```rust
// src-tauri/src/storage/user_scoped_paths.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn paths_snapshot_consistent() {
        let root = PathBuf::from("/tmp/test-renlijia");
        let paths = UserScopedPaths::new(&root, "t_1__u_2");

        assert_eq!(paths.base_dir(), root.join("users/t_1__u_2"));
        assert_eq!(paths.conversations_dir(), root.join("users/t_1__u_2/conversations"));
        assert_eq!(paths.mcp_config_path(), root.join("users/t_1__u_2/mcp_servers.json"));
        assert_eq!(paths.schedules_dir(), root.join("users/t_1__u_2/schedules"));
        assert_eq!(paths.permissions_path(), root.join("users/t_1__u_2/permissions.json"));
        assert_eq!(paths.skills_dir(), root.join("users/t_1__u_2/skills"));
        assert_eq!(paths.agent_invocations_path(), root.join("users/t_1__u_2/agent_invocations.json"));
        assert_eq!(paths.subagent_transcripts_dir(), root.join("users/t_1__u_2/subagent_transcripts"));
        assert_eq!(paths.playwright_profile_dir(), root.join("users/t_1__u_2/playwright-profile"));
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cd src-tauri && cargo test storage::user_scoped_paths -- --nocapture`
Expected: compile error — module not found

- [ ] **Step 3: Implement UserScopedPaths and UserScopedPathResolver**

```rust
// src-tauri/src/storage/user_scoped_paths.rs
use std::path::{Path, PathBuf};

/// Immutable snapshot of all paths for the current user scope.
/// Constructed once per scope activation, cloned freely, never mutated.
#[derive(Debug, Clone)]
pub struct UserScopedPaths {
    base: PathBuf,
}

impl UserScopedPaths {
    pub fn new(root: &Path, scope_key: &str) -> Self {
        Self {
            base: root.join("users").join(scope_key),
        }
    }

    pub fn base_dir(&self) -> PathBuf { self.base.clone() }
    pub fn config_path(&self) -> PathBuf { self.base.join("config.json") }
    pub fn scope_json_path(&self) -> PathBuf { self.base.join("scope.json") }
    pub fn index_path(&self) -> PathBuf { self.base.join("index.json") }
    pub fn conversations_dir(&self) -> PathBuf { self.base.join("conversations") }
    pub fn shared_dir(&self) -> PathBuf { self.base.join("shared") }
    pub fn memory_dir(&self) -> PathBuf { self.base.join("shared").join("memory") }
    pub fn cognitive_dir(&self) -> PathBuf { self.base.join("shared").join("cognitive") }
    pub fn cache_dir(&self) -> PathBuf { self.base.join("shared").join("cache") }
    pub fn schedules_dir(&self) -> PathBuf { self.base.join("schedules") }
    pub fn permissions_path(&self) -> PathBuf { self.base.join("permissions.json") }
    pub fn mcp_config_path(&self) -> PathBuf { self.base.join("mcp_servers.json") }
    pub fn skills_dir(&self) -> PathBuf { self.base.join("skills") }
    pub fn agent_invocations_path(&self) -> PathBuf { self.base.join("agent_invocations.json") }
    pub fn subagent_transcripts_dir(&self) -> PathBuf { self.base.join("subagent_transcripts") }
    pub fn project_memories_dir(&self) -> PathBuf { self.base.join("project_memories") }
    pub fn playwright_profile_dir(&self) -> PathBuf { self.base.join("playwright-profile") }
    pub fn api_data_dir(&self) -> PathBuf { self.base.join("api-data") }
    pub fn screenshots_dir(&self) -> PathBuf { self.base.join("screenshots") }
    pub fn site_profiles_dir(&self) -> PathBuf { self.base.join("site-profiles") }
    pub fn audit_dir(&self) -> PathBuf { self.base.join("audit") }
    pub fn logs_dir(&self) -> PathBuf { self.base.join("logs") }
    pub fn downloads_dir(&self) -> PathBuf { self.base.join("downloads") }
}

/// Trait for services that need user-scoped paths.
/// Services depend on this trait, not on AiJiaHome or UserScope directly.
pub trait UserScopedPathResolver: Send + Sync {
    /// Returns a paths snapshot if a user is logged in, None otherwise.
    fn resolve_paths(&self) -> Option<UserScopedPaths>;

    /// Returns a paths snapshot or error if not logged in.
    fn require_paths(&self) -> anyhow::Result<UserScopedPaths> {
        self.resolve_paths().ok_or_else(|| anyhow::anyhow!("未登录"))
    }
}
```

- [ ] **Step 4: Export from mod.rs**

```rust
pub mod user_scoped_paths;
pub use user_scoped_paths::{UserScopedPaths, UserScopedPathResolver};
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test storage::user_scoped_paths -- --nocapture`
Expected: pass

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage/user_scoped_paths.rs src-tauri/src/storage/mod.rs
git commit -m "feat(storage): add UserScopedPaths snapshot and UserScopedPathResolver trait"
```

---

## Task 3: GlobalConfigStore

**Files:**
- Create: `src-tauri/src/storage/global_config_store.rs`
- Modify: `src-tauri/src/storage/mod.rs`

`GlobalConfigStore` 负责 `global/config.json` 读写和 `global/auth/cloud_auth` 的加密存储。AuthManager 将改为依赖它而不是 AppStorage。

- [ ] **Step 1: Write failing test**

```rust
// src-tauri/src/storage/global_config_store.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn set_and_get_setting() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("key1", "value1").unwrap();
        assert_eq!(store.get_setting("key1").unwrap(), Some("value1".to_string()));
    }

    #[test]
    fn delete_setting() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("key1", "value1").unwrap();
        store.delete_setting("key1").unwrap();
        assert_eq!(store.get_setting("key1").unwrap(), None);
    }

    #[test]
    fn cloud_auth_stored_in_dedicated_file_not_config_json() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("cloud_auth", "encrypted_blob").unwrap();

        // Should be in auth/cloud_auth file, NOT in config.json
        assert!(tmp.path().join("auth/cloud_auth").exists());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("auth/cloud_auth")).unwrap(),
            "encrypted_blob"
        );

        // config.json should NOT contain cloud_auth
        let config_text = std::fs::read_to_string(tmp.path().join("config.json")).unwrap_or_default();
        assert!(!config_text.contains("cloud_auth"));

        // get_setting should still return it
        assert_eq!(store.get_setting("cloud_auth").unwrap(), Some("encrypted_blob".to_string()));
    }

    #[test]
    fn delete_cloud_auth_removes_file() {
        let tmp = TempDir::new().unwrap();
        let store = GlobalConfigStore::new(tmp.path().to_path_buf());
        store.set_setting("cloud_auth", "blob").unwrap();
        store.delete_setting("cloud_auth").unwrap();
        assert!(!tmp.path().join("auth/cloud_auth").exists());
        assert_eq!(store.get_setting("cloud_auth").unwrap(), None);
    }
}
```

- [ ] **Step 2: Run to verify fail**

Run: `cd src-tauri && cargo test storage::global_config_store -- --nocapture`

- [ ] **Step 3: Implement GlobalConfigStore**

```rust
// src-tauri/src/storage/global_config_store.rs
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
struct SettingsMap(HashMap<String, String>);

pub struct GlobalConfigStore {
    global_dir: PathBuf,
    write_lock: Mutex<()>,
}

impl GlobalConfigStore {
    pub fn new(global_dir: PathBuf) -> Self {
        fs::create_dir_all(&global_dir).ok();
        fs::create_dir_all(global_dir.join("auth")).ok();
        Self {
            global_dir,
            write_lock: Mutex::new(()),
        }
    }

    fn config_path(&self) -> PathBuf {
        self.global_dir.join("config.json")
    }

    fn cloud_auth_path(&self) -> PathBuf {
        self.global_dir.join("auth").join("cloud_auth")
    }

    pub fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        if key == "cloud_auth" {
            let path = self.cloud_auth_path();
            if !path.exists() {
                return Ok(None);
            }
            return Ok(Some(fs::read_to_string(path)?));
        }
        let map = self.read_map()?;
        Ok(map.0.get(key).cloned())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        if key == "cloud_auth" {
            let path = self.cloud_auth_path();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, value)?;
            return Ok(());
        }
        let mut map = self.read_map()?;
        map.0.insert(key.to_string(), value.to_string());
        self.write_map(&map)
    }

    pub fn delete_setting(&self, key: &str) -> anyhow::Result<()> {
        let _lock = self.write_lock.lock().unwrap();
        if key == "cloud_auth" {
            let path = self.cloud_auth_path();
            if path.exists() {
                fs::remove_file(path)?;
            }
            return Ok(());
        }
        let mut map = self.read_map()?;
        map.0.remove(key);
        self.write_map(&map)
    }

    fn read_map(&self) -> anyhow::Result<SettingsMap> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(SettingsMap::default());
        }
        let text = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    fn write_map(&self, map: &SettingsMap) -> anyhow::Result<()> {
        let path = self.config_path();
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(map)?;
        fs::write(&tmp, &text)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}
```

- [ ] **Step 4: Export from mod.rs**

```rust
pub mod global_config_store;
pub use global_config_store::GlobalConfigStore;
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test storage::global_config_store -- --nocapture`
Expected: pass

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage/global_config_store.rs src-tauri/src/storage/mod.rs
git commit -m "feat(storage): add GlobalConfigStore for auth bootstrap decoupling"
```

---

## Task 4: AuthManager 改为依赖 GlobalConfigStore

**Files:**
- Modify: `src-tauri/src/auth/mod.rs`

当前 AuthManager 持有 `Arc<AppStorage>`，只用它做 `get_setting("cloud_auth")` / `set_setting` / `delete_setting`。改为持有 `Arc<GlobalConfigStore>`，接口完全一致。

- [ ] **Step 1: 修改 AuthManager struct**

`src-tauri/src/auth/mod.rs`，将：
```rust
pub struct AuthManager {
    client: AuthClient,
    state: RwLock<Option<CloudAuth>>,
    storage: Arc<AppStorage>,
    secure_storage: Option<Arc<SecureStorage>>,
}
```

改为：
```rust
use crate::storage::GlobalConfigStore;

pub struct AuthManager {
    client: AuthClient,
    state: RwLock<Option<CloudAuth>>,
    global_store: Arc<GlobalConfigStore>,
    secure_storage: Option<Arc<SecureStorage>>,
}
```

- [ ] **Step 2: 修改 `new()` 签名**

将 `pub fn new(storage: Arc<AppStorage>, ...)` 改为：
```rust
pub fn new(global_store: Arc<GlobalConfigStore>, secure_storage: Option<Arc<SecureStorage>>) -> Self {
    Self {
        client: AuthClient::new(),
        state: RwLock::new(None),
        global_store,
        secure_storage,
    }
}
```

- [ ] **Step 3: 修改 `persist_auth` / `load_persisted_auth` / `clear_persisted_auth`**

将所有 `self.storage.set_setting(...)` 改为 `self.global_store.set_setting(...)`。
将所有 `self.storage.get_setting(...)` 改为 `self.global_store.get_setting(...)`。
将所有 `self.storage.delete_setting(...)` 改为 `self.global_store.delete_setting(...)`。

搜索 `self.storage` 确保没有遗漏——AuthManager 不应再持有任何 `AppStorage` 引用。

- [ ] **Step 4: cargo check 确认编译**

Run: `cd src-tauri && cargo check 2>&1 | head -50`
Expected: lib.rs 编译错误（因为 lib.rs 还传的是 `db.clone()`），这是预期的，Task 8 修复。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/auth/mod.rs
git commit -m "refactor(auth): decouple AuthManager from AppStorage, use GlobalConfigStore"
```

---

## Task 5: CurrentUserStorage

**Files:**
- Create: `src-tauri/src/storage/current_user_storage.rs`
- Modify: `src-tauri/src/storage/mod.rs`

核心设计：`RwLock<Option<Inner>>`，Tauri `app.manage()` 注册后外层 Arc 不变，切 scope 时替换 inner。

- [ ] **Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::storage::UserScope;

    #[test]
    fn not_logged_in_returns_none() {
        let tmp = TempDir::new().unwrap();
        let home = crate::storage::AiJiaHome::from_path(tmp.path());
        let cus = CurrentUserStorage::new(Arc::new(home));
        assert!(cus.get().is_none());
    }

    #[test]
    fn activate_scope_creates_storage() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path()));
        let cus = CurrentUserStorage::new(home.clone());
        let scope = UserScope::new(1, 2);
        cus.activate_scope(scope.clone()).unwrap();
        assert!(cus.get().is_some());
    }

    #[test]
    fn deactivate_clears_storage() {
        let tmp = TempDir::new().unwrap();
        let home = Arc::new(crate::storage::AiJiaHome::from_path(tmp.path()));
        let cus = CurrentUserStorage::new(home);
        let scope = UserScope::new(1, 2);
        cus.activate_scope(scope).unwrap();
        cus.deactivate();
        assert!(cus.get().is_none());
    }
}
```

- [ ] **Step 2: Add `AiJiaHome::from_path()` for testing**

在 `aijia_home.rs` 中添加：
```rust
/// For testing: create AiJiaHome from a custom path.
#[cfg(test)]
pub fn from_path(root: &Path) -> Self {
    Self { root: root.to_path_buf() }
}
```

- [ ] **Step 3: Implement CurrentUserStorage**

```rust
// src-tauri/src/storage/current_user_storage.rs
use std::sync::{Arc, RwLock};
use crate::storage::{AiJiaHome, UserScope, UserScopedPaths, UserScopedPathResolver};
use crate::storage::file_store::AppStorage;

struct Inner {
    scope: UserScope,
    paths: UserScopedPaths,
    storage: Arc<AppStorage>,
}

pub struct CurrentUserStorage {
    home: Arc<AiJiaHome>,
    inner: RwLock<Option<Inner>>,
}

impl CurrentUserStorage {
    pub fn new(home: Arc<AiJiaHome>) -> Self {
        Self {
            home,
            inner: RwLock::new(None),
        }
    }

    /// Activate a user scope: create UserScopedPaths snapshot + AppStorage at user base dir.
    pub fn activate_scope(&self, scope: UserScope) -> anyhow::Result<()> {
        self.home.ensure_user_dirs(&scope)?;
        let paths = UserScopedPaths::new(self.home.root(), &scope.key());
        let storage = Arc::new(AppStorage::new(&paths.base_dir())?);
        let mut guard = self.inner.write().unwrap();
        *guard = Some(Inner { scope, paths, storage });
        Ok(())
    }

    /// Deactivate (on logout).
    pub fn deactivate(&self) {
        let mut guard = self.inner.write().unwrap();
        *guard = None;
    }

    /// Get current AppStorage if logged in.
    pub fn get(&self) -> Option<Arc<AppStorage>> {
        self.inner.read().unwrap().as_ref().map(|i| i.storage.clone())
    }

    /// Get current AppStorage or error.
    pub fn require(&self) -> anyhow::Result<Arc<AppStorage>> {
        self.get().ok_or_else(|| anyhow::anyhow!("未登录"))
    }

    /// Get current UserScope if logged in.
    pub fn scope(&self) -> Option<UserScope> {
        self.inner.read().unwrap().as_ref().map(|i| i.scope.clone())
    }

    /// Get AiJiaHome reference for global/root paths only.
    pub fn home(&self) -> &AiJiaHome {
        &self.home
    }
}

impl UserScopedPathResolver for CurrentUserStorage {
    fn resolve_paths(&self) -> Option<UserScopedPaths> {
        self.inner.read().unwrap().as_ref().map(|i| i.paths.clone())
    }
}
```

- [ ] **Step 4: Export from mod.rs**

```rust
pub mod current_user_storage;
pub use current_user_storage::CurrentUserStorage;
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test storage::current_user_storage -- --nocapture`

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage/current_user_storage.rs src-tauri/src/storage/mod.rs src-tauri/src/storage/aijia_home.rs
git commit -m "feat(storage): add CurrentUserStorage with RwLock<Option<Inner>> pattern"
```

---

## Task 6: Legacy 数据迁移函数

**Files:**
- Modify: `src-tauri/src/storage/migration.rs` — 导出 helpers 为 `pub(crate)`
- Create: `src-tauri/src/storage/migration_user_scope.rs`

- [ ] **Step 1: 将 migration.rs 中的 helpers 改为 pub(crate)**

在 `src-tauri/src/storage/migration.rs` 中，将：
```rust
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()>
fn read_state_json(path: &Path) -> std::io::Result<Value>
fn write_state_json(path: &Path, state: &Value) -> std::io::Result<()>
```
改为：
```rust
pub(crate) fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()>
pub(crate) fn read_state_json(path: &Path) -> std::io::Result<Value>
pub(crate) fn write_state_json(path: &Path, state: &Value) -> std::io::Result<()>
```

- [ ] **Step 2: Write failing integration test**

```rust
// src-tauri/tests/user_scope_migration_test.rs
use tempfile::TempDir;
use std::fs;

#[test]
fn migrate_legacy_conversations_to_user_scope() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    // Setup legacy data
    fs::create_dir_all(root_path.join("conversations/conv1")).unwrap();
    fs::write(root_path.join("conversations/conv1/conv.json"), r#"{"id":"conv1"}"#).unwrap();
    fs::write(root_path.join("conversations/conv1/messages.jsonl"), "{}").unwrap();
    fs::write(root_path.join("index.json"), r#"{"conversations":[{"id":"conv1"}]}"#).unwrap();
    fs::create_dir_all(root_path.join("shared/memory")).unwrap();
    fs::write(root_path.join("shared/memory/memory.jsonl"), "[]").unwrap();
    fs::write(root_path.join("mcp_servers.json"), "[]").unwrap();
    fs::write(root_path.join("permissions.json"), "{}").unwrap();
    fs::create_dir_all(root_path.join("global")).unwrap();

    let scope_key = "t_1__u_2";
    let user_dir = root_path.join("users").join(scope_key);

    aijia::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path, &user_dir, scope_key,
        &root_path.join("global/state.json"),
    ).unwrap();

    // Verify conversations migrated
    assert!(user_dir.join("conversations/conv1/conv.json").exists());
    assert!(user_dir.join("index.json").exists());
    assert!(user_dir.join("shared/memory/memory.jsonl").exists());
    assert!(user_dir.join("mcp_servers.json").exists());
    assert!(user_dir.join("permissions.json").exists());

    // Verify legacy preserved
    assert!(root_path.join("conversations/conv1/conv.json").exists());

    // Verify claim marker
    let state_text = fs::read_to_string(root_path.join("global/state.json")).unwrap();
    assert!(state_text.contains("claimedBy"));
    assert!(state_text.contains(scope_key));
}

#[test]
fn second_scope_blocked_from_auto_migration() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    // Setup legacy data
    fs::create_dir_all(root_path.join("conversations/conv1")).unwrap();
    fs::write(root_path.join("conversations/conv1/conv.json"), "data").unwrap();
    fs::write(root_path.join("index.json"), r#"{"conversations":[]}"#).unwrap();
    fs::create_dir_all(root_path.join("global")).unwrap();

    let state_path = root_path.join("global/state.json");

    // First scope claims legacy root
    let scope_a = "t_1__u_2";
    let user_dir_a = root_path.join("users").join(scope_a);
    aijia::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path, &user_dir_a, scope_a, &state_path,
    ).unwrap();
    assert!(user_dir_a.join("conversations/conv1/conv.json").exists());

    // Second scope should NOT get legacy data
    let scope_b = "t_3__u_4";
    let user_dir_b = root_path.join("users").join(scope_b);
    aijia::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path, &user_dir_b, scope_b, &state_path,
    ).unwrap();
    assert!(!user_dir_b.join("conversations/conv1/conv.json").exists());
}

#[test]
fn new_user_no_legacy_no_marker() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    fs::create_dir_all(root_path.join("global")).unwrap();

    let scope_key = "t_1__u_2";
    let user_dir = root_path.join("users").join(scope_key);
    let state_path = root_path.join("global/state.json");

    aijia::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        root_path, &user_dir, scope_key, &state_path,
    ).unwrap();

    // No state.json should be created for new users
    assert!(!state_path.exists());
}
```

- [ ] **Step 3: Implement migration function**

```rust
// src-tauri/src/storage/migration_user_scope.rs
use std::path::Path;
use std::fs;
use serde_json::json;

use super::migration::{copy_dir, read_state_json, write_state_json};

/// Items to migrate from legacy root to user scope directory.
const LEGACY_ITEMS: &[(&str, &str)] = &[
    ("index.json",              "index.json"),
    ("conversations",           "conversations"),
    ("shared",                  "shared"),
    ("audit",                   "audit"),
    ("mcp_servers.json",        "mcp_servers.json"),
    ("permissions.json",        "permissions.json"),
    ("agent_invocations.json",  "agent_invocations.json"),
    ("subagent_transcripts",    "subagent_transcripts"),
    ("schedules",               "schedules"),
    ("project_memories",        "project_memories"),
    ("playwright-profile",      "playwright-profile"),
    ("api-data",                "api-data"),
    ("screenshots",             "screenshots"),
    ("site-profiles",           "site-profiles"),
];

pub fn migrate_legacy_to_user_scope_if_needed(
    root: &Path,
    user_dir: &Path,
    scope_key: &str,
    global_state_path: &Path,
) -> std::io::Result<()> {
    // 1. Check if legacy data exists
    let has_index = root.join("index.json").exists();
    let has_conversations = root.join("conversations").exists();
    if !has_index && !has_conversations {
        return Ok(()); // New user, no legacy data, no marker
    }

    // 2. Check global legacy claim marker
    let mut state = read_state_json(global_state_path)?;
    let claimed_by = state
        .get("migrations")
        .and_then(|m| m.get("legacyRootClaim"))
        .and_then(|c| c.get("claimedBy"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    match claimed_by.as_deref() {
        Some(existing) if existing == scope_key => return Ok(()),
        Some(existing) => {
            log::info!(
                "[migration:user-scope] legacy root already claimed by {}, skip auto migration for {}",
                existing,
                scope_key
            );
            return Ok(());
        }
        None => {}
    }

    log::info!("[migration:user-scope] migrating legacy data to {:?}", user_dir);

    // 3. Ensure user directory
    fs::create_dir_all(user_dir)?;

    // 4. Copy each item
    for (src_rel, dst_rel) in LEGACY_ITEMS {
        let src = root.join(src_rel);
        let dst = user_dir.join(dst_rel);
        if !src.exists() {
            continue;
        }
        if dst.exists() {
            log::info!("[migration:user-scope] skip (exists): {}", dst_rel);
            continue;
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)?;
        }
        log::info!("[migration:user-scope] copied: {} -> {}", src_rel, dst_rel);
    }

    // 5. Handle skills: copy non-builtin skills (_drafts/ and custom skills)
    let legacy_skills = root.join("skills");
    let user_skills = user_dir.join("skills");
    if legacy_skills.exists() {
        fs::create_dir_all(&user_skills)?;
        if let Ok(entries) = fs::read_dir(&legacy_skills) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // _drafts and any non-builtin skill directories
                if name_str.starts_with('_') || entry.path().is_dir() {
                    let dst = user_skills.join(&name);
                    if !dst.exists() {
                        copy_dir(&entry.path(), &dst)?;
                        log::info!("[migration:user-scope] copied skill: {}", name_str);
                    }
                }
            }
        }
    }

    // 6. Write global legacy claim marker
    state["migrations"]["legacyRootClaim"] = json!({
        "claimedBy": scope_key,
        "claimedAt": chrono::Utc::now().to_rfc3339(),
    });
    write_state_json(global_state_path, &state)?;

    log::info!("[migration:user-scope] completed for scope={}", scope_key);
    Ok(())
}
```

- [ ] **Step 4: Export from mod.rs**

```rust
pub mod migration_user_scope;
```

- [ ] **Step 5: Run integration tests**

Run: `cd src-tauri && cargo test --test user_scope_migration_test -- --nocapture`

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/storage/migration.rs src-tauri/src/storage/migration_user_scope.rs \
       src-tauri/src/storage/mod.rs src-tauri/tests/user_scope_migration_test.rs
git commit -m "feat(storage): add legacy-to-user-scope migration with idempotent markers"
```

---

## Task 7: Config 拆分函数

**Files:**
- Add to: `src-tauri/src/storage/migration_user_scope.rs`

从 legacy `config.json` 拆分 `cloud_auth` → global，`workspacePath` / 模型偏好 → user scope。

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn migrate_config_splits_keys() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();

    // Legacy config.json with mixed keys
    let legacy_config = serde_json::json!({
        "cloud_auth": "encrypted_blob",
        "workspacePath": "/some/path",
        "primaryModel": "gpt-4",
        "theme": "dark",
        "autoModelRouting": "true"
    });
    fs::write(root_path.join("config.json"), legacy_config.to_string()).unwrap();
    fs::create_dir_all(root_path.join("global/auth")).unwrap();

    let user_dir = root_path.join("users/t_1__u_2");
    fs::create_dir_all(&user_dir).unwrap();

    migrate_legacy_config_if_needed(
        root_path,
        &user_dir,
        &root_path.join("global"),
    ).unwrap();

    // cloud_auth should be in global/auth/cloud_auth
    assert!(root_path.join("global/auth/cloud_auth").exists());

    // user settings should be in user config.json
    let user_cfg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(user_dir.join("config.json")).unwrap()
    ).unwrap();
    assert_eq!(user_cfg["workspacePath"], "/some/path");
    assert_eq!(user_cfg["primaryModel"], "gpt-4");

    // global config should have remaining keys
    let global_cfg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root_path.join("global/config.json")).unwrap()
    ).unwrap();
    assert_eq!(global_cfg.get("cloud_auth"), None);
}
```

- [ ] **Step 2: Implement migrate_legacy_config_if_needed**

```rust
/// Keys that should go to user scope config.
const USER_CONFIG_KEYS: &[&str] = &[
    "workspacePath", "primaryModel", "primaryApiKey",
    "autoModelRouting", "cloudModel", "cloudModelType",
    "dataMaskingLevel", "autoCleanupEnabled", "tempFileRetentionDays",
    "keepOldVersions", "tavilyApiKey", "bochaApiKey",
    "customModelEndpoint", "customModelName", "useCloud",
    "personaOnboardingDone", "thinkingType", "thinkingBudgetTokens",
    "analysisThreshold", "enableTaorTracking",
];

/// Keys that are auth bootstrap (go to global/auth/).
const AUTH_KEYS: &[&str] = &["cloud_auth"];

pub fn migrate_legacy_config_if_needed(
    root: &Path,
    user_dir: &Path,
    global_dir: &Path,
) -> std::io::Result<()> {
    let legacy_config = root.join("config.json");
    let global_config = global_dir.join("config.json");

    // Skip if global config already exists (already split)
    if global_config.exists() {
        return Ok(());
    }

    if !legacy_config.exists() {
        return Ok(());
    }

    let text = fs::read_to_string(&legacy_config)?;
    let map: std::collections::HashMap<String, String> =
        serde_json::from_str(&text).unwrap_or_default();

    let mut user_map = std::collections::HashMap::new();
    let mut global_map = std::collections::HashMap::new();

    for (key, value) in &map {
        if AUTH_KEYS.contains(&key.as_str()) {
            // cloud_auth is bootstrapped before AuthManager::restore(). Do not overwrite it here.
            let auth_dir = global_dir.join("auth");
            fs::create_dir_all(&auth_dir)?;
            let cloud_auth_path = auth_dir.join("cloud_auth");
            if !cloud_auth_path.exists() {
                fs::write(cloud_auth_path, value)?;
            }
        } else if USER_CONFIG_KEYS.contains(&key.as_str()) || key.starts_with("apiKey:") {
            user_map.insert(key.clone(), value.clone());
        } else {
            global_map.insert(key.clone(), value.clone());
        }
    }

    // Write user config
    if !user_map.is_empty() {
        let user_config_path = user_dir.join("config.json");
        if !user_config_path.exists() {
            let text = serde_json::to_string_pretty(&user_map)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            fs::write(&user_config_path, text)?;
        }
    }

    // Write global config
    let text = serde_json::to_string_pretty(&global_map)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(&global_config, text)?;

    log::info!("[migration:config-split] legacy config.json split complete");
    Ok(())
}
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test migration_user_scope -- --nocapture`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/storage/migration_user_scope.rs
git commit -m "feat(storage): add legacy config.json split migration"
```

---

## Task 7.5: Pre-auth cloud_auth bootstrap migration

**Files:**
- Add to: `src-tauri/src/storage/migration_user_scope.rs`

老用户的 `cloud_auth` 在 legacy `~/.renlijia/config.json`。新版 `AuthManager::restore()` 从 `global/auth/cloud_auth` 读取，所以必须在 restore 之前先原样复制 `cloud_auth`。

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn bootstrap_cloud_auth_before_restore() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    let legacy_config = serde_json::json!({
        "cloud_auth": "encrypted_or_plaintext_blob",
        "workspacePath": "/legacy/workspace"
    });
    fs::write(root_path.join("config.json"), legacy_config.to_string()).unwrap();

    let global_dir = root_path.join("global");
    bootstrap_cloud_auth_if_needed(root_path, &global_dir).unwrap();

    assert_eq!(
        fs::read_to_string(global_dir.join("auth/cloud_auth")).unwrap(),
        "encrypted_or_plaintext_blob"
    );
}

#[test]
fn bootstrap_cloud_auth_does_not_overwrite_existing_global_auth() {
    let root = TempDir::new().unwrap();
    let root_path = root.path();
    fs::write(root_path.join("config.json"), r#"{"cloud_auth":"legacy"}"#).unwrap();
    let global_dir = root_path.join("global");
    fs::create_dir_all(global_dir.join("auth")).unwrap();
    fs::write(global_dir.join("auth/cloud_auth"), "existing").unwrap();

    bootstrap_cloud_auth_if_needed(root_path, &global_dir).unwrap();

    assert_eq!(
        fs::read_to_string(global_dir.join("auth/cloud_auth")).unwrap(),
        "existing"
    );
}
```

- [ ] **Step 2: Implement bootstrap function**

```rust
pub fn bootstrap_cloud_auth_if_needed(root: &Path, global_dir: &Path) -> std::io::Result<()> {
    let target = global_dir.join("auth").join("cloud_auth");
    if target.exists() {
        return Ok(());
    }

    let legacy_config = root.join("config.json");
    if !legacy_config.exists() {
        return Ok(());
    }

    let text = fs::read_to_string(&legacy_config)?;
    let map: std::collections::HashMap<String, String> =
        serde_json::from_str(&text).unwrap_or_default();

    if let Some(value) = map.get("cloud_auth") {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // Important: do not decrypt or parse here; AuthManager::restore owns that logic.
        fs::write(&target, value)?;
        log::info!("[migration:bootstrap-auth] copied legacy cloud_auth to global/auth/cloud_auth");
    }

    Ok(())
}
```

- [ ] **Step 3: Run tests**

Run: `cd src-tauri && cargo test migration_user_scope -- --nocapture`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/storage/migration_user_scope.rs
git commit -m "feat(storage): bootstrap cloud_auth before AuthManager restore"
```

---

## Task 8: lib.rs 启动顺序重写

**Files:**
- Modify: `src-tauri/src/lib.rs`

这是最关键也最大的改动。改变启动顺序为：`AiJiaHome → ensure_global_dirs → SecureStorage → GlobalConfigStore → AuthManager(global) → restore → derive scope → CurrentUserStorage → migrations → FileManager → services`。

- [ ] **Step 1: 重写 lib.rs setup 前半段（auth bootstrap）**

将 `lib.rs` 中从 `AiJiaHome::from_home()` 到 `AuthManager::restore()` 之间的代码替换。关键变化：

```rust
// 1. AiJiaHome
let aijia_home = Arc::new(storage::AiJiaHome::from_home());
aijia_home.ensure_dirs().expect("Failed to ensure AiJia directories");
aijia_home.ensure_global_dirs().expect("Failed to ensure global directories");
app.manage(aijia_home.clone());

// 2. Legacy migrations (old app_data_dir → ~/.renlijia/)
let app_data_dir = app.path().app_data_dir()?;
std::fs::create_dir_all(&app_data_dir)?;
if let Err(e) = storage::migration::migrate_if_needed(&app_data_dir, aijia_home.root()) { ... }
if let Err(e) = storage::migration::reconcile_legacy_conversations_if_needed(&app_data_dir, aijia_home.root()) { ... }
if let Err(e) = storage::migration::migrate_message_shards_to_single_file_if_needed(aijia_home.root()) { ... }

// 3. SecureStorage (global crypto)
let secure_storage: Option<Arc<SecureStorage>> = ...same as before...

// 4. GlobalConfigStore (replaces AppStorage for auth)
let global_store = Arc::new(storage::GlobalConfigStore::new(aijia_home.global_dir()));

// 4.5 Pre-auth bootstrap: legacy cloud_auth must be copied before restore()
if let Err(e) = storage::migration_user_scope::bootstrap_cloud_auth_if_needed(
    aijia_home.root(),
    &aijia_home.global_dir(),
) {
    log::warn!("[setup] cloud_auth bootstrap warning: {}", e);
}

// 5. AuthManager (now depends on GlobalConfigStore, not AppStorage)
let auth_manager = Arc::new(auth::AuthManager::new(global_store.clone(), secure_storage.clone()));
tauri::async_runtime::block_on(auth_manager.restore());

// 6. Derive UserScope
let user_scope: Option<storage::UserScope> = {
    let info = tauri::async_runtime::block_on(auth_manager.get_auth_info());
    if info.logged_in {
        info.user.as_ref().zip(info.tenant.as_ref()).map(|(u, t)| {
            storage::UserScope::new(t.id, u.id)
        })
    } else {
        None
    }
};

// 7. CurrentUserStorage
let current_user_storage = Arc::new(storage::CurrentUserStorage::new(aijia_home.clone()));
if let Some(ref scope) = user_scope {
    // Run user-scope migration (synchronous, blocking)
    let user_dir = aijia_home.user_dir(scope);
    if let Err(e) = storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
        aijia_home.root(), &user_dir, &scope.key(), &aijia_home.global_state_path(),
    ) {
        log::warn!("[setup] user-scope migration warning: {}", e);
    }
    if let Err(e) = storage::migration_user_scope::migrate_legacy_config_if_needed(
        aijia_home.root(), &user_dir, &aijia_home.global_dir(),
    ) {
        log::warn!("[setup] config split warning: {}", e);
    }
    // Activate user storage
    current_user_storage.activate_scope(scope.clone())
        .expect("Failed to activate user storage");
}

// 8. db = user-scoped AppStorage (or fallback for backward compat)
let db: Arc<AppStorage> = current_user_storage.get()
    .expect("AppStorage not available — user must be logged in at startup");
```

- [ ] **Step 2: 修改 FileManager 初始化**

```rust
// FileManager now reads from user-scoped config
let workspace_path = db.get_setting("workspacePath").ok().flatten().unwrap_or_default();
// ...rest stays the same...
```

- [ ] **Step 3: 修改所有 service 初始化以使用新路径**

各 service 的路径改为从 `UserScopedPathResolver::resolve_paths()` 返回的 `UserScopedPaths` 快照取得。详见后续 Tasks 9-13。

- [ ] **Step 4: 注册新的 managed state**

```rust
app.manage(global_store.clone());
app.manage(current_user_storage.clone());
// db, auth_manager, etc. 保持注册
```

- [ ] **Step 5: cargo check 确认编译通过**

Run: `cd src-tauri && cargo check`

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "refactor(lib): rewrite startup order for user-scoped storage"
```

---

## Task 9: ScheduleStore / schedule_runner 绑定 user scope（通过 UserScopedPathResolver）

**Files:**
- Modify: `src-tauri/src/commands/schedules.rs`
- Modify: `src-tauri/src/runtime/schedule_runner.rs`

- [ ] **Step 1: 修改 schedules.rs 中的 schedule_store()**

```rust
fn schedule_store(app: &AppHandle) -> ScheduleStore {
    let cus = app.state::<Arc<CurrentUserStorage>>();
    let paths = cus.require_paths().expect("Must be logged in to access schedules");
    ScheduleStore::new(paths.base_dir())
}
```

- [ ] **Step 2: 修改 spawn_schedule_runner**

将 `spawn_schedule_runner(aijia_home, dispatcher)` 改为接收 `Arc<dyn UserScopedPathResolver>`：

```rust
pub fn spawn_schedule_runner(
    path_resolver: Arc<dyn UserScopedPathResolver>,
    dispatcher: Arc<dyn ScheduleRunDispatcher>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Some(paths) = path_resolver.resolve_paths() {
                let store = ScheduleStore::new(paths.base_dir());
                match run_due_schedules_once(&store, dispatcher.as_ref(), Utc::now()).await {
                    Ok(n) => { if n > 0 { log::info!("[scheduler] ran {} due schedules", n); } }
                    Err(e) => log::error!("[scheduler] error: {}", e),
                }
            }
        }
    });
}
```

- [ ] **Step 3: 更新 lib.rs 中的调用**

```rust
runtime::schedule_runner::spawn_schedule_runner(
    current_user_storage.clone() as Arc<dyn UserScopedPathResolver>,
    app.state::<Arc<TauriChatCommandAdapter>>().inner().clone(),
);
```

- [ ] **Step 4: cargo check**

Run: `cd src-tauri && cargo check`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/schedules.rs src-tauri/src/runtime/schedule_runner.rs src-tauri/src/lib.rs
git commit -m "refactor(schedule): bind ScheduleStore to user scope via UserScopedPathResolver"
```

---

## Task 10: MCP 绑定 user scope（通过 UserScopedPathResolver）

**Files:**
- Modify: `src-tauri/src/lib.rs` (MCP init section)

- [ ] **Step 1: 修改 McpConfigStore 路径**

在 lib.rs MCP 初始化中，改为通过 paths 快照取路径：

```rust
let mcp_config_path = if let Some(ref paths) = current_user_storage.resolve_paths() {
    paths.mcp_config_path()
} else {
    aijia_home.mcp_config_path() // fallback for backward compat
};
let mcp_config_store = Arc::new(storage::mcp_config_store::McpConfigStore::new(mcp_config_path));
```

- [ ] **Step 2: cargo check**

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "refactor(mcp): bind McpConfigStore to user scope path"
```

---

## Task 11: PermissionStore 绑定 user scope（通过 UserScopedPathResolver）

**Files:**
- Modify: `src-tauri/src/lib.rs` (PermissionStore init section)

- [ ] **Step 1: 修改 user layer 路径**

```rust
let user_permission_path = current_user_storage
    .resolve_paths()
    .map(|paths| paths.permissions_path())
    .or_else(|| Some(aijia_home.permissions_path()));

let permission_store = Arc::new(runtime::store::PermissionStore::with_layer_files(
    Some(file_mgr.workspace_path().join(".aijia").join("permissions.json")),
    user_permission_path,
));
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "refactor(permissions): bind user layer to user scope path"
```

---

## Task 12: AgentRuntime 绑定 user scope（通过 UserScopedPathResolver）

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 修改 AgentRuntime 路径**

```rust
let (agent_store_path, subagent_dir) = if let Some(ref paths) = current_user_storage.resolve_paths() {
    (paths.agent_invocations_path(), paths.subagent_transcripts_dir())
} else {
    (aijia_home.agent_invocations_path(), aijia_home.subagent_transcripts_dir())
};
let agent_runtime = Arc::new(runtime::agent::AgentRuntime::from_storage(agent_store_path, subagent_dir)?);
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "refactor(agent): bind AgentRuntime to user scope paths"
```

---

## Task 13: Skills 加载路径分离

**Files:**
- Modify: `src-tauri/src/lib.rs` (skills scan section)

- [ ] **Step 1: 用户自定义 skills 改为从 user scope 加载**

```rust
// Built-in skills: still from global
let skills_dir = aijia_home.skills_dir(); // ~/.renlijia/skills/
if skills_dir.is_dir() {
    // Only scan built-in (non-underscore) skills from global dir
    scan_external_plugins(&skills_dir, &tool_registry, &skill_registry,
        file_mgr.workspace_path(), "builtin").await;
}

// User-installed skills: from user scope
if let Some(ref paths) = current_user_storage.resolve_paths() {
    let user_skills = paths.skills_dir();
    if user_skills.is_dir() {
        scan_external_plugins(&user_skills, &tool_registry, &skill_registry,
            file_mgr.workspace_path(), "custom").await;
    }
}
```

- [ ] **Step 2: 修改 skill_management 的 drafts_dir 引用**

确保 `skill_smith` 和 `skill_management` 中创建/读取 `_drafts/` 时使用 user scope 路径。搜索 `aijia_home.drafts_dir()` 并改为通过 `UserScopedPathResolver`：`resolver.require_paths()?.skills_dir().join("_drafts")`。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/commands/skill_management.rs
git commit -m "refactor(skills): split built-in (global) and user-installed (user scope) skill loading"
```

---

## Task 14: 前端 login/logout 清理全部状态

**Files:**
- Modify: `src/stores/chatStore.ts`
- Modify: `src/stores/authStore.ts`

- [ ] **Step 1: 在 chatStore 添加 resetAll()**

在 `sessionStore.ts` 中（或 chatStore 的 session slice）添加：

```typescript
resetAll: () => set({
    conversations: [],
    activeConversationId: null,
    messages: [],
}),
```

在 `streamingStore.ts` 中添加：

```typescript
resetStreaming: () => set({
    busyConversations: new Set(),
    streamStates: {},
    taskStates: {},
    pendingAsks: new Map(),
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],
}),
```

- [ ] **Step 2: 修改 authStore.logout()**

```typescript
async logout() {
    set({ isAuthPending: true })
    await cloudLogout()
    // Clear all user-scoped UI state
    useChatStore.getState().resetAll()
    useChatStore.getState().resetStreaming()
    // Also clear any other stores that cache user data
    // (schedulesStore, mcpStore, settingsStore if they exist)
    set({ ...EMPTY_AUTH_STATE, redirectFrom: null, isAuthPending: false })
},
```

- [ ] **Step 3: 修改 authStore.login() — 登录后重新加载**

`login()` 成功后调用后端的 scope 切换（后端 `cloud_login` 返回后 `CurrentUserStorage` 已激活新 scope），然后前端重新加载 conversations：

```typescript
async login(username, password) {
    set({ isAuthPending: true })
    // Reset stale state first
    useChatStore.getState().resetAll()
    useChatStore.getState().resetStreaming()

    const info = await cloudLogin(username.trim(), password)
    const models = info.models.length > 0 ? info.models : await getCloudModels()
    set({ ...mapAuthState(info, models), isAuthPending: false })
    // Conversations will be reloaded by the UI when isLoggedIn becomes true
},
```

- [ ] **Step 4: Commit**

```bash
git add src/stores/chatStore.ts src/stores/authStore.ts src/stores/sessionStore.ts src/stores/streamingStore.ts
git commit -m "feat(frontend): clear all user state on logout, reset on login"
```

---

## Task 15: cloud_login / cloud_logout 后端 scope 切换

**Files:**
- Modify: `src-tauri/src/commands/auth.rs`

- [ ] **Step 1: 修改 cloud_login — 登录后激活 user scope**

```rust
#[tauri::command]
pub async fn cloud_login(
    auth: State<'_, Arc<AuthManager>>,
    cus: State<'_, Arc<CurrentUserStorage>>,
    home: State<'_, Arc<AiJiaHome>>,
    username: String,
    password: String,
) -> Result<CloudAuthInfo, String> {
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return Err("请输入用户名和密码".to_string());
    }
    let result = auth.login(username, &password).await.map_err(format_auth_error)?;

    // Activate user scope
    if let (Some(user), Some(tenant)) = (&result.user, &result.tenant) {
        let scope = crate::storage::UserScope::new(tenant.id, user.id);

        // Run migration if needed
        let user_dir = home.user_dir(&scope);
        if let Err(e) = crate::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
            home.root(), &user_dir, &scope.key(), &home.global_state_path(),
        ) {
            log::warn!("[cloud_login] migration warning: {}", e);
        }

        cus.activate_scope(scope).map_err(|e| format!("Failed to activate scope: {}", e))?;
    }

    Ok(result)
}
```

- [ ] **Step 2: 修改 cloud_logout — 清除 user scope**

```rust
#[tauri::command]
pub async fn cloud_logout(
    auth: State<'_, Arc<AuthManager>>,
    cus: State<'_, Arc<CurrentUserStorage>>,
) -> Result<(), String> {
    auth.logout().await;
    cus.deactivate();
    Ok(())
}
```

- [ ] **Step 3: cargo check**

Run: `cd src-tauri && cargo check`

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/auth.rs
git commit -m "feat(auth): activate/deactivate user scope on login/logout"
```

---

## Task 16: 修复 session_runtime / chat_runtime_impl 中的直接 AiJiaHome::from_home()

**Files:**
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

这两个文件直接调用 `AiJiaHome::from_home()` 绕过了 managed state。

- [ ] **Step 1: 搜索并替换**

Run: `cd src-tauri && grep -rn "AiJiaHome::from_home" src/`

对每个出现的地方，改为从函数参数/上下文中接收 `Arc<AiJiaHome>` 或 `Arc<CurrentUserStorage>`，而不是直接构造新实例。

具体改法取决于调用上下文——每个调用点需要单独检查并将 `AiJiaHome` 作为参数传入。

- [ ] **Step 2: cargo check 确认无遗漏**

Run: `cd src-tauri && cargo check && grep -rn "AiJiaHome::from_home" src/ | grep -v test | grep -v "fn from_home"`
Expected: 0 匹配（除了 `from_home()` 定义本身和测试代码）

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/runtime/session_runtime.rs src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs
git commit -m "fix: replace direct AiJiaHome::from_home() calls with injected state"
```

---

## Task 17: 写 scope.json 和 active_account.json

**Files:**
- Modify: `src-tauri/src/commands/auth.rs`（在 cloud_login 中）

- [ ] **Step 1: 登录成功后写 scope.json**

在 `cloud_login` 的 scope 激活后：

```rust
// Write scope.json
let scope_json = serde_json::json!({
    "tenantId": tenant.id,
    "userId": user.id,
    "name": user.name,
    "username": user.username,
    "tenantName": tenant.name,
    "createdAt": chrono::Utc::now().to_rfc3339(),
    "lastSeenAt": chrono::Utc::now().to_rfc3339(),
});
let scope_path = home.user_scope_json_path(&scope);
std::fs::write(&scope_path, serde_json::to_string_pretty(&scope_json).unwrap()).ok();

// Write active_account.json
let active = serde_json::json!({
    "scopeKey": scope.key(),
    "tenantId": tenant.id,
    "userId": user.id,
});
std::fs::write(&home.active_account_path(), serde_json::to_string_pretty(&active).unwrap()).ok();
```

- [ ] **Step 2: Commit**

```bash
git add src-tauri/src/commands/auth.rs
git commit -m "feat(auth): write scope.json and active_account.json on login"
```

---

## Verification

### 自动化测试

```bash
# Rust 单测
cd src-tauri && cargo test storage::user_scope -- --nocapture
cd src-tauri && cargo test storage::aijia_home -- --nocapture
cd src-tauri && cargo test storage::global_config_store -- --nocapture
cd src-tauri && cargo test storage::current_user_storage -- --nocapture

# 迁移集成测试
cd src-tauri && cargo test --test user_scope_migration_test -- --nocapture

# 全量编译检查
cd src-tauri && cargo check

# 前端测试
pnpm test -- authStore
pnpm test -- chatStore
```

### 手工验收

```text
1. 清理或备份 ~/.renlijia
2. 启动 app → 登录用户 A
3. 确认 ~/.renlijia/users/t_A__u_A/ 目录存在
4. 确认 users/t_A__u_A/config.json 包含 workspacePath
5. 确认 users/t_A__u_A/scope.json 存在
6. 创建会话 A1、发送消息
7. 确认 conversations 在 users/t_A__u_A/conversations/ 下
8. 登出 → 确认前端会话列表已清空
9. 登录用户 B → 确认会话列表为空
10. 创建会话 B1 → 确认在 users/t_B__u_B/ 下
11. 登出 → 登录用户 A → 确认 A1 存在、B1 不可见
12. 确认 legacy 根目录数据保留未删除
13. 确认 global/state.json 有迁移标记
14. 确认 MCP、schedules、permissions 读写都在 user scope 下
```
