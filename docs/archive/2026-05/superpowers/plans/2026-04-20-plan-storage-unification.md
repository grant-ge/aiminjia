# Storage Unification：统一到 ~/.renlijia/ 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把所有运行时数据从 `~/Library/Application Support/com.aijia.app/`（app_data_dir）统一到 `~/.renlijia/`，使 AIjia 的数据目录结构与 claude-code-best 的 `~/.claude/` 保持对齐——单一根目录、用户可见、易备份迁移。

**Architecture:** 在 `lib.rs` 的 setup 阶段将 `app_data_dir` 全部替换为 `~/.renlijia/`（`aijia_home`），不做迁移兼容（硬切换）；所有调用 `app.path().app_data_dir()` 的 commands 改为从 `AppHandle` state 读取注入的 `aijia_home`。macOS 和 Windows 统一使用 `~/.renlijia/`（与 claude-code-best 使用 `~/.claude/` 的跨平台策略一致，不区分平台路径）。

**Tech Stack:** Rust / Tauri v2 / dirs crate（已有）

---

## 目录结构对比

### claude-code-best（对标基线）

```
~/.claude/                          ← 单一根目录
├── settings.json                   ← 全局配置
├── managed-settings.json           ← 管理员配置
├── history.jsonl                   ← 会话历史
├── skills/                         ← 用户全局 skill
│   └── <skill-name>/SKILL.md
├── teams/<team-name>/config.json   ← 团队配置
├── tasks/<task-list-id>/           ← 任务列表
└── projects/<project-slug>/
    └── memory/                     ← 项目隔离记忆
        ├── MEMORY.md
        └── logs/YYYY/MM/YYYY-MM-DD.md

./.claude/                          ← 项目级目录（随仓库）
├── settings.json
├── settings.local.json             ← gitignore
├── skills/<skill-name>/SKILL.md
└── agents/<agent-name>.md
```

### lotus-app 当前（问题：分散两处）

```
~/Library/Application Support/com.aijia.app/   ← 系统隐藏目录（用户不可见）
├── config.json                     ← 应用设置（含 workspacePath）
├── index.json                      ← 会话索引
├── conversations/{id}/             ← 聊天记录
├── shared/memory/                  ← 记忆
├── shared/cognitive/               ← 认知记忆
├── permissions.json                ← 全局权限
├── mcp_servers.json                ← MCP 配置
├── custom_plugins/                 ← 用户安装的 skill
├── agent_invocations.json
├── subagent_transcripts/
├── playwright-profile/
├── api-data/ / screenshots/
└── .lotus-key                      ← 加密密钥

~/.renlijia/                        ← workspace（默认，用户可见）
├── conversations/uploads/
├── conversations/generated/
├── logs/
└── temp/
```

### lotus-app 目标（对齐 claude-code-best）

```
~/.renlijia/                        ← 单一根目录（等价于 ~/.claude/）
├── config.json                     ← 应用设置
├── permissions.json                ← 全局权限
├── mcp_servers.json                ← MCP 配置
├── agent_invocations.json
├── subagent_transcripts/
├── skills/                         ← 用户全局 skill（等价于 ~/.claude/skills/）
│   └── <skill-id>/
├── conversations/                  ← 聊天记录 + 附件
│   └── {id}/
│       ├── conv.json
│       ├── messages.N.jsonl
│       ├── uploads/
│       └── generated/
├── shared/
│   ├── memory/memory.jsonl
│   └── cognitive/
│       ├── mem.md
│       ├── index.json
│       └── daily/
├── playwright-profile/
├── api-data/
├── screenshots/
├── logs/
└── temp/

./.aijia/                           ← 项目级目录（等价于 ./.claude/）  ← 未来扩展
├── settings.json                   ← 项目级设置（未来）
└── skills/                         ← 项目级 skill（未来）
```

### 核心差距总结

| 维度 | claude-code-best | lotus-app 当前 | lotus-app 目标 |
|------|-----------------|---------------|---------------|
| 存储根目录 | `~/.claude/`（单一） | 两处分散 | `~/.renlijia/`（单一） |
| skill 存放 | `~/.claude/skills/` | `app_data_dir/custom_plugins/` | `~/.renlijia/skills/` |
| 配置分层 | global → project → local | 单层 config.json | 单层（暂不做分层）|
| 项目级目录 | `./.claude/` | 无 | `./.aijia/`（本计划不实现，留桩） |
| 用户可见性 | 可见（home dir） | 不可见（Library） | 可见（home dir） |
| 旧数据迁移 | N/A | 需要迁移 | 首次启动自动迁移 |

---

## 文件改动清单

| 文件 | 操作 | 改动内容 |
|------|------|---------|
| `src-tauri/src/lib.rs` | 修改 | `app_data_dir` → `aijia_home`（`~/.renlijia/`）；`.lotus` → `.aijia`；`init_prompts` 路径更新；注册 `AiJiaHome` state；添加首次迁移逻辑 |
| `src-tauri/src/storage/file_store/mod.rs` | 修改 | `AppStorage::new` 路径不变，base_dir 由调用方传入，无需修改 |
| `src-tauri/src/storage/file_store/workspace_settings.rs` | 修改 | `.lotus/settings.json` → `.aijia/settings.json` |
| `src-tauri/src/commands/skill_management.rs` | 修改 | `app_data_dir().join("custom_plugins")` → `aijia_home.skills_dir()` |
| `src-tauri/src/commands/skill_smith/commit.rs` | 修改 | 同上 |
| `src-tauri/src/commands/skill_smith/mod.rs` | 修改 | 同上 |
| `src-tauri/src/commands/project_memory.rs` | 修改 | `app_data_dir` → `aijia_home` |
| `src-tauri/src/commands/chat.rs` | 修改 | 临时目录前缀 `lotus-workspace-first-` → `aijia-workspace-first-` |
| `src-tauri/src/connector/playwright_browser.rs` | 修改 | `get_app_data_dir()` → `aijia_home`（通过 state 注入） |
| `src-tauri/src/runtime/mcp/connection.rs` | 修改 | `"name": "lotus-app"` → `"name": "aijia"` |
| `src-tauri/src/storage/crypto.rs` | 修改 | key 目录改到 `aijia_home.crypto_dir()`（key 文件名 `master.key` 不变） |
| `src-tauri/src/storage/migration.rs` | 新增 | 启动时一次性迁移：旧 `app_data_dir` → `~/.renlijia/`，写 `.migrated` 标记后跳过 |
| `src-tauri/src/runtime/claude_md.rs` | 重命名→ `renlijia_md.rs` | struct/type 名改为 `RenlijiaMd*`；只读取 `RENLIJIA.md` / `.aijia/RENLIJIA.md` / `RENLIJIA.local.md`，**不兼容** `.claude/CLAUDE.md` |
| `src-tauri/src/runtime/mod.rs` | 修改 | `pub mod claude_md` → `pub mod renlijia_md` |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 修改 | 函数名、注释、context 标签 `# claudeMd` → `# renlijiaMd` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | 修改 | 字段名 `claude_md_loader` → `renlijia_md_loader`，类型引用更新 |
| `src-tauri/tests/plan_ac_claude_md_test.rs` | 修改 | 文件名字面量、路径、断言全部更新为 `RENLIJIA.md` / `.aijia` |
| `src-tauri/tests/plan_ae_config_layers_test.rs` | 修改 | `.lotus` → `.aijia` |
| `src-tauri/tests/plan_u4_memory_runtime_native_test.rs` | 修改 | `"# claudeMd"` → `"# renlijiaMd"` |
| `src/hooks/useAuthorizedWorkspace.ts` | 修改 | 事件名 `lotus:authorized-workspace-changed` → `aijia:authorized-workspace-changed` |

---

## Task 1：定义 AiJiaHome 结构体，注册为 Tauri state

**Files:**
- Create: `src-tauri/src/storage/aijia_home.rs`
- Modify: `src-tauri/src/storage/mod.rs`

- [ ] **Step 1: 编写 AiJiaHome 结构体**

```rust
// src-tauri/src/storage/aijia_home.rs
use std::path::{Path, PathBuf};

/// `~/.renlijia/` — AIjia 的单一数据根目录。
/// 等价于 claude-code-best 的 `~/.claude/`。
#[derive(Debug, Clone)]
pub struct AiJiaHome {
    root: PathBuf,
}

impl AiJiaHome {
    /// 从 home dir 构建，默认 `~/.renlijia/`。
    pub fn from_home() -> Self {
        let root = dirs::home_dir()
            .map(|h| h.join(".renlijia"))
            .expect("Cannot determine home directory");
        Self { root }
    }

    /// 用于测试，传入任意路径。
    #[cfg(test)]
    pub fn from_path(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    // ── 子目录 ──────────────────────────────────────

    /// 用户全局 skill 目录（等价于 ~/.claude/skills/）。
    pub fn skills_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    /// MCP 配置文件。
    pub fn mcp_config_path(&self) -> PathBuf {
        self.root.join("mcp_servers.json")
    }

    /// 全局权限文件。
    pub fn permissions_path(&self) -> PathBuf {
        self.root.join("permissions.json")
    }

    /// Agent 调用记录文件。
    pub fn agent_invocations_path(&self) -> PathBuf {
        self.root.join("agent_invocations.json")
    }

    /// Subagent transcript 目录。
    pub fn subagent_transcripts_dir(&self) -> PathBuf {
        self.root.join("subagent_transcripts")
    }

    /// Playwright 浏览器 profile 目录。
    pub fn playwright_profile_dir(&self) -> PathBuf {
        self.root.join("playwright-profile")
    }

    /// Browser API data 目录。
    pub fn api_data_dir(&self) -> PathBuf {
        self.root.join("api-data")
    }

    /// Browser screenshots 目录。
    pub fn screenshots_dir(&self) -> PathBuf {
        self.root.join("screenshots")
    }

    /// 加密密钥文件目录。
    pub fn crypto_dir(&self) -> PathBuf {
        self.root.join("crypto")
    }

    /// 确保所有必需子目录存在。
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(self.skills_dir())?;
        std::fs::create_dir_all(self.subagent_transcripts_dir())?;
        std::fs::create_dir_all(self.playwright_profile_dir())?;
        std::fs::create_dir_all(self.api_data_dir())?;
        std::fs::create_dir_all(self.screenshots_dir())?;
        std::fs::create_dir_all(self.crypto_dir())?;
        Ok(())
    }
}
```

- [ ] **Step 2: 注册到 storage/mod.rs**

```rust
// storage/mod.rs 末尾添加：
pub mod aijia_home;
pub use aijia_home::AiJiaHome;
```

- [ ] **Step 3: 编写单元测试**

```rust
// src-tauri/src/storage/aijia_home.rs 末尾
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_paths_under_root() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        assert_eq!(home.skills_dir(), tmp.path().join("skills"));
        assert_eq!(home.mcp_config_path(), tmp.path().join("mcp_servers.json"));
        assert_eq!(home.permissions_path(), tmp.path().join("permissions.json"));
    }

    #[test]
    fn test_ensure_dirs_creates_subdirs() {
        let tmp = TempDir::new().unwrap();
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());
        home.ensure_dirs().unwrap();
        assert!(home.skills_dir().exists());
        assert!(home.subagent_transcripts_dir().exists());
    }
}
```

- [ ] **Step 4: 编译通过**

```bash
cd src-tauri && cargo test storage::aijia_home -- --nocapture
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/aijia_home.rs src-tauri/src/storage/mod.rs
git commit -m "feat(storage): add AiJiaHome struct for unified ~/.renlijia root"
```

---

## Task 2：lib.rs — 将 app_data_dir 替换为 aijia_home，注册 state

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 在 lib.rs setup 顶部，构建 aijia_home 替换 app_data_dir 用途**

在 `lib.rs` setup 函数的 `app_data_dir` 定义之后（约 33 行），插入：

```rust
// lib.rs — 在 app_data_dir 创建之后插入
let aijia_home = Arc::new(storage::AiJiaHome::from_home());
aijia_home.ensure_dirs().expect("Failed to create ~/.renlijia dirs");
app.manage(aijia_home.clone());
```

- [ ] **Step 2: 替换 AppStorage::new 的路径**

```rust
// 原来：
let db = Arc::new(
    storage::file_store::AppStorage::new(&app_data_dir)
        .expect("Failed to initialize file storage"),
);

// 改为：
let db = Arc::new(
    storage::file_store::AppStorage::new(aijia_home.root())
        .expect("Failed to initialize file storage"),
);
```

- [ ] **Step 3: 替换 agent_invocations.json 和 subagent_transcripts 路径**

```rust
// 原来：
let agent_store_path = app_data_dir.join("agent_invocations.json");
let subagent_transcript_store_dir = app_data_dir.join("subagent_transcripts");

// 改为：
let agent_store_path = aijia_home.agent_invocations_path();
let subagent_transcript_store_dir = aijia_home.subagent_transcripts_dir();
```

- [ ] **Step 4: 替换 permissions.json（全局层）路径**

```rust
// 原来：
Some(app_data_dir.join("permissions.json")),

// 改为：
Some(aijia_home.permissions_path()),
```

- [ ] **Step 5: 替换 mcp_servers.json 路径**

```rust
// 原来：
app_config_dir.join("mcp_servers.json"),

// 改为：
aijia_home.mcp_config_path(),
```

- [ ] **Step 6: 替换 custom_plugins 路径**

```rust
// 原来：
let custom_plugins_dir = app_data_dir.join("custom_plugins");

// 改为：
let custom_plugins_dir = aijia_home.skills_dir();
```

- [ ] **Step 7: 替换 crypto::SecureStorage 路径**

```rust
// 原来：
storage::crypto::SecureStorage::new(&app_data_dir)

// 改为：
storage::crypto::SecureStorage::new(&aijia_home.crypto_dir())
```

- [ ] **Step 8: 更新 init_prompts 调用路径**

```rust
// lib.rs:46 原来：
llm::prompts::init_prompts(&resource_dir, &app_data_dir);

// 改为：
llm::prompts::init_prompts(&resource_dir, aijia_home.root());
```

- [ ] **Step 9: 删除不再使用的 app_config_dir（如果只剩 mcp_servers.json 用它）**

确认 `app_config_dir` 不再被使用，删除其声明（第 35-39 行）：

```rust
// 删除这段：
let app_config_dir = app
    .path()
    .app_config_dir()
    .unwrap_or_else(|_| app_data_dir.clone());
std::fs::create_dir_all(&app_config_dir)?;
```

- [ ] **Step 10: 编译通过**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```
Expected: 无 error 输出

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(lib): route all app_data_dir paths to ~/.renlijia via AiJiaHome"
```

---

## Task 3：commands — 替换各 command 中的 app_data_dir 调用

**Files:**
- Modify: `src-tauri/src/commands/skill_management.rs`
- Modify: `src-tauri/src/commands/skill_smith/commit.rs`
- Modify: `src-tauri/src/commands/skill_smith/mod.rs`
- Modify: `src-tauri/src/commands/project_memory.rs`

- [ ] **Step 1: skill_management.rs — 替换所有 app_data_dir 调用**

`skill_management.rs` 共有 5 处 `app.path().app_data_dir()` → `join("custom_plugins")`，全部替换为读取 state：

```rust
// 原来的模式（4处）：
let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
let custom_dir = app_data.join("custom_plugins");

// 改为：
let aijia_home = app.state::<std::sync::Arc<crate::storage::AiJiaHome>>();
let custom_dir = aijia_home.skills_dir();
```

涉及的函数（逐一替换）：
- `list_custom_skills`（第 26 行）
- `install_skill_from_dir`（第 96 行）
- `uninstall_skill`（第 117 行）
- `download_and_install_skill`（第 625 行）

- [ ] **Step 2: skill_smith/commit.rs — 替换 custom_plugins 路径**

```rust
// 原来（第 105 行）：
let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
let custom_dir = app_data.join("custom_plugins");

// 改为：
let aijia_home = app.state::<std::sync::Arc<crate::storage::AiJiaHome>>();
let custom_dir = aijia_home.skills_dir();
```

- [ ] **Step 3: skill_smith/mod.rs — 替换 draft dir（如使用 app_data_dir）**

检查 `mod.rs:130` 处：

```rust
// 原来：
let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;

// 改为：
let aijia_home = app.state::<std::sync::Arc<crate::storage::AiJiaHome>>();
// draft 目录可放在 skills_dir() 下的 .draft/ 子目录
let draft_dir = aijia_home.skills_dir().join(".draft");
```

- [ ] **Step 4: project_memory.rs — 替换 app_data_dir**

```rust
// 原来（两处）：
let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

// 改为：
let aijia_home = app.state::<std::sync::Arc<crate::storage::AiJiaHome>>();
// project_memory 存放在 ~/.renlijia/ 下（由 AppStorage 管理，无需单独路径）
```

> 注：`project_memory.rs` 如果只是读写 `AppStorage`，则不需要 `app_data_dir`，直接删除该行即可。若有独立文件路径，用 `aijia_home.root()` 替换。

- [ ] **Step 5: 编译通过**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```
Expected: 无 error

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/skill_management.rs \
        src-tauri/src/commands/skill_smith/commit.rs \
        src-tauri/src/commands/skill_smith/mod.rs \
        src-tauri/src/commands/project_memory.rs
git commit -m "feat(commands): replace app_data_dir with AiJiaHome in skill/memory commands"
```

---

## Task 4：playwright_browser — 替换 get_app_data_dir

**Files:**
- Modify: `src-tauri/src/connector/playwright_browser.rs`

- [ ] **Step 1: 将 AiJiaHome 注入 PlaywrightBrowser**

`playwright_browser.rs` 有 `get_app_data_dir()` 方法（第 912 行），通过 `AppHandle` 拿 `app_data_dir`。改为读取 state：

```rust
// 原来（第 912-916 行）：
fn get_app_data_dir(&self) -> PathBuf {
    self.app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| dirs::home_dir().unwrap().join(".renlijia"))
}

// 改为：
fn get_app_data_dir(&self) -> PathBuf {
    self.app_handle
        .try_state::<std::sync::Arc<crate::storage::AiJiaHome>>()
        .map(|h| h.root().to_path_buf())
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".renlijia"))
}
```

这样 `playwright-profile/`、`api-data/`、`screenshots/` 都自动落到 `~/.renlijia/` 下。

- [ ] **Step 2: 编译通过**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/connector/playwright_browser.rs
git commit -m "feat(connector): route playwright data dirs to ~/.renlijia via AiJiaHome"
```

---

## Task 5：启动时一次性迁移旧数据

**Files:**
- Create: `src-tauri/src/storage/migration.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 编写迁移函数**

```rust
// src-tauri/src/storage/migration.rs
use std::path::Path;

/// 启动时一次性迁移：把旧 app_data_dir 的数据复制到 ~/.renlijia/。
/// 完成后写 .migrated 标记，下次启动直接跳过。
/// 已存在的文件不覆盖，保护用户数据。
pub fn migrate_if_needed(old_dir: &Path, new_dir: &Path) -> std::io::Result<()> {
    let marker = new_dir.join(".migrated");
    if marker.exists() || !old_dir.exists() {
        if !marker.exists() {
            let _ = std::fs::write(&marker, "1");
        }
        return Ok(());
    }

    log::info!("[migration] {:?} → {:?}", old_dir, new_dir);

    // (旧路径, 新路径)
    let items: &[(&str, &str)] = &[
        ("config.json",            "config.json"),
        ("index.json",             "index.json"),
        ("conversations",          "conversations"),
        ("shared",                 "shared"),
        ("project_memories",       "project_memories"),
        ("audit",                  "audit"),
        ("permissions.json",       "permissions.json"),
        ("mcp_servers.json",       "mcp_servers.json"),
        ("agent_invocations.json", "agent_invocations.json"),
        ("subagent_transcripts",   "subagent_transcripts"),
        ("custom_plugins",         "skills"),      // custom_plugins → skills
        ("playwright-profile",     "playwright-profile"),
        ("api-data",               "api-data"),
        ("screenshots",            "screenshots"),
        ("master.key",             "crypto/master.key"),  // 加密密钥
    ];

    for (old_rel, new_rel) in items {
        let src = old_dir.join(old_rel);
        let dst = new_dir.join(new_rel);
        if !src.exists() || dst.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
        log::info!("[migration] {} → {}", old_rel, new_rel);
    }

    std::fs::write(&marker, "1")?;
    log::info!("[migration] done");
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_symlink() { continue; }
        if s.is_dir() { copy_dir(&s, &d)?; } else { std::fs::copy(&s, &d)?; }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_copies_and_renames_custom_plugins() {
        let old = TempDir::new().unwrap();
        let new = TempDir::new().unwrap();

        std::fs::write(old.path().join("config.json"), r#"{"k":"v"}"#).unwrap();
        // conversations 目录
        std::fs::create_dir_all(old.path().join("conversations/conv1")).unwrap();
        std::fs::write(old.path().join("conversations/conv1/conv.json"), "{}").unwrap();
        // custom_plugins → skills
        let skill = old.path().join("custom_plugins").join("my-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("plugin.toml"), "[plugin]").unwrap();
        // master.key
        std::fs::write(old.path().join("master.key"), "key-data").unwrap();

        migrate_if_needed(old.path(), new.path()).unwrap();

        assert!(new.path().join("config.json").exists());
        assert!(new.path().join("conversations/conv1/conv.json").exists());
        assert!(new.path().join("skills/my-skill/plugin.toml").exists());
        assert!(new.path().join("crypto/master.key").exists());
        assert!(new.path().join(".migrated").exists());
    }

    #[test]
    fn test_idempotent() {
        let old = TempDir::new().unwrap();
        let new = TempDir::new().unwrap();
        std::fs::write(old.path().join("config.json"), "old").unwrap();

        migrate_if_needed(old.path(), new.path()).unwrap();
        std::fs::write(new.path().join("config.json"), "new").unwrap();
        migrate_if_needed(old.path(), new.path()).unwrap(); // 第二次不覆盖

        let c = std::fs::read_to_string(new.path().join("config.json")).unwrap();
        assert_eq!(c, "new");
    }
}
```

- [ ] **Step 2: 注册到 storage/mod.rs**

```rust
pub mod migration;
```

- [ ] **Step 3: 在 lib.rs 中调用（在 aijia_home.ensure_dirs() 之后）**

```rust
// aijia_home.ensure_dirs() 之后插入：
if let Err(e) = storage::migration::migrate_if_needed(
    &app_data_dir,
    aijia_home.root(),
) {
    log::warn!("[setup] migration warning (non-fatal): {}", e);
}
```

- [ ] **Step 4: 运行迁移测试**

```bash
cd src-tauri && cargo test storage::migration -- --nocapture
```
Expected: `test result: ok. 2 passed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/migration.rs src-tauri/src/storage/mod.rs src-tauri/src/lib.rs
git commit -m "feat(storage): migrate existing app_data_dir to ~/.renlijia on first launch"
```

---

## Task 6：crypto — 更新加密密钥路径

**Files:**
- Modify: `src-tauri/src/storage/crypto.rs`

- [ ] **Step 1: 查看 SecureStorage::new 的路径逻辑**

```bash
grep -n "\.lotus-key\|key_path\|new(" src-tauri/src/storage/crypto.rs | head -20
```

- [ ] **Step 2: 更新 key file 路径到 crypto 子目录**

找到 key 文件路径构建处，确认 `SecureStorage::new(dir)` 里 key 文件名是 `master.key`（不是 `.lotus-key`）：

```rust
// crypto.rs 中：
const KEY_FILE_NAME: &str = "master.key";

// 保持 key 文件名不变，只改变传入目录：
// SecureStorage::new(&aijia_home.crypto_dir())
// => ~/.renlijia/crypto/master.key
```

- [ ] **Step 3: 编译并运行全量测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```
Expected: 无新增 test failures

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/storage/crypto.rs
git commit -m "fix(crypto): confirm key file path resolves under ~/.renlijia/crypto/"
```

---

## Task 7：将 claude_md 重命名为 renlijia_md，更新读取路径

**Files:**
- Rename: `src-tauri/src/runtime/claude_md.rs` → `src-tauri/src/runtime/renlijia_md.rs`
- Modify: `src-tauri/src/runtime/mod.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/tests/plan_ac_claude_md_test.rs`
- Modify: `src-tauri/tests/plan_u4_memory_runtime_native_test.rs`

- [ ] **Step 1: 新建 renlijia_md.rs，内容为重命名后的版本**

```rust
// src-tauri/src/runtime/renlijia_md.rs
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenlijiaMdFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Default)]
pub struct RenlijiaMdLoader {
    cache: HashMap<PathBuf, (SystemTime, String)>,
}

impl RenlijiaMdLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn load(&mut self, workspace_path: &Path) -> Vec<RenlijiaMdFile> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        // 全局：~/.renlijia/RENLIJIA.md
        if let Some(home) = Self::home_dir() {
            self.try_add_file(
                &home.join(".renlijia").join("RENLIJIA.md"),
                &mut seen,
                &mut result,
            );
        }

        if workspace_path.as_os_str().is_empty() {
            return result;
        }

        let mut dirs = Vec::new();
        let mut current = Some(workspace_path);
        while let Some(dir) = current {
            dirs.push(dir.to_path_buf());
            current = dir.parent();
        }
        dirs.reverse();

        for dir in dirs {
            // {workspace}/RENLIJIA.md
            self.try_add_file(&dir.join("RENLIJIA.md"), &mut seen, &mut result);
            // {workspace}/.aijia/RENLIJIA.md
            self.try_add_file(&dir.join(".aijia").join("RENLIJIA.md"), &mut seen, &mut result);
            // {workspace}/RENLIJIA.local.md（本地覆盖，gitignore）
            self.try_add_file(&dir.join("RENLIJIA.local.md"), &mut seen, &mut result);
        }

        result
    }

    fn try_add_file(
        &mut self,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        result: &mut Vec<RenlijiaMdFile>,
    ) {
        let dedupe_key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(dedupe_key) {
            return;
        }
        if let Some(content) = self.read_with_cache(path) {
            result.push(RenlijiaMdFile {
                path: path.to_path_buf(),
                content,
            });
        }
    }

    fn read_with_cache(&mut self, path: &Path) -> Option<String> {
        let metadata = fs::metadata(path).ok()?;
        let mtime = metadata.modified().ok()?;
        let cache_key = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some((cached_mtime, cached_content)) = self.cache.get(&cache_key) {
            if *cached_mtime == mtime {
                return Some(cached_content.clone());
            }
        }
        let content = fs::read_to_string(path).ok()?;
        self.cache.insert(cache_key, (mtime, content.clone()));
        Some(content)
    }

    fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
```

- [ ] **Step 2: 删除旧文件**

```bash
rm src-tauri/src/runtime/claude_md.rs
```

- [ ] **Step 3: 更新 runtime/mod.rs**

```rust
// 原来：
pub mod claude_md;

// 改为：
pub mod renlijia_md;
```

- [ ] **Step 4: 批量替换引用**

```bash
# chat.rs、chat_turn_driver.rs、测试文件：符号名替换
sed -i '' 's/claude_md/renlijia_md/g; s/ClaudeMdFile/RenlijiaMdFile/g; s/ClaudeMdLoader/RenlijiaMdLoader/g' \
  src-tauri/src/transport/tauri_commands/chat.rs \
  src-tauri/src/runtime/chat/chat_turn_driver.rs \
  src-tauri/tests/plan_ac_claude_md_test.rs \
  src-tauri/tests/plan_u4_memory_runtime_native_test.rs
```

- [ ] **Step 5: 替换其余"去 claude/lotus 化"路径字符串**

```bash
# workspace_settings.rs：.lotus → .aijia
sed -i '' 's/\.lotus/\.aijia/g' \
  src-tauri/src/storage/file_store/workspace_settings.rs

# lib.rs：.lotus → .aijia（permissions 路径）
sed -i '' 's/join("\.lotus")/join(".aijia")/g' \
  src-tauri/src/lib.rs

# commands/chat.rs：临时目录前缀
sed -i '' 's/lotus-workspace-first-/aijia-workspace-first-/g' \
  src-tauri/src/commands/chat.rs

# mcp/connection.rs：client name
sed -i '' 's/"name": "lotus-app"/"name": "aijia"/g' \
  src-tauri/src/runtime/mcp/connection.rs

# chat_turn_driver.rs：注入给 LLM 的 context 标签
sed -i '' 's/# claudeMd/# renlijiaMd/g' \
  src-tauri/src/runtime/chat/chat_turn_driver.rs

# 前端事件名
sed -i '' "s/lotus:authorized-workspace-changed/aijia:authorized-workspace-changed/g" \
  src/hooks/useAuthorizedWorkspace.ts
```

- [ ] **Step 6: 手动更新 chat_turn_driver.rs 中的注释和函数名**

找到以下内容并更新（`sed` 不处理注释和函数名）：

```rust
// 原来：
/// 加载 CLAUDE.md user-context 文件。
async fn load_claude_md(

fn build_claude_md_context_message(

// 改为：
/// 加载 RENLIJIA.md user-context 文件。
async fn load_renlijia_md(

fn build_renlijia_md_context_message(
```

同步更新 `RuntimeLlmExecutor` trait 中的方法名 `load_claude_md` → `load_renlijia_md`（涉及 `chat_turn_driver.rs` 和所有实现该 trait 的文件）：

```bash
grep -rn "load_claude_md\|build_claude_md" src-tauri/src/ src-tauri/tests/ | grep -v ".rs:#"
```
逐一替换为 `load_renlijia_md` / `build_renlijia_md_context_message`。

- [ ] **Step 7: 更新测试文件中的路径字面量**

```bash
# plan_ac_claude_md_test.rs：文件名字面量、路径、断言
sed -i '' \
  's/CLAUDE\.md/RENLIJIA.md/g; s/CLAUDE\.local\.md/RENLIJIA.local.md/g; s/\.claude/.aijia/g; s/# claudeMd/# renlijiaMd/g; s/claude_md\.rs/renlijia_md.rs/g; s/claude-md-context/renlijia-md-context/g' \
  src-tauri/tests/plan_ac_claude_md_test.rs

# plan_ae_config_layers_test.rs：.lotus → .aijia
sed -i '' 's/\.lotus/.aijia/g' \
  src-tauri/tests/plan_ae_config_layers_test.rs

# plan_u4_memory_runtime_native_test.rs：# claudeMd → # renlijiaMd
sed -i '' 's/# claudeMd/# renlijiaMd/g; s/CLAUDE\.md/RENLIJIA.md/g' \
  src-tauri/tests/plan_u4_memory_runtime_native_test.rs
```

- [ ] **Step 8: 编译通过**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -20
```
Expected: 无 error

- [ ] **Step 9: 测试通过**

```bash
cd src-tauri && cargo test renlijia_md -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/runtime/renlijia_md.rs \
        src-tauri/src/runtime/mod.rs \
        src-tauri/src/transport/tauri_commands/chat.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/tests/plan_ac_claude_md_test.rs \
        src-tauri/tests/plan_ae_config_layers_test.rs \
        src-tauri/tests/plan_u4_memory_runtime_native_test.rs \
        src/hooks/useAuthorizedWorkspace.ts
git rm src-tauri/src/runtime/claude_md.rs
git commit -m "feat(runtime): rename claude_md → renlijia_md, add RENLIJIA.md path support"
```

---

## Task 8：验证回归测试全部通过

- [ ] **Step 1: 运行 review_ 系列架构约束测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```
Expected: 所有 `review_*` 测试通过

- [ ] **Step 2: 运行完整 Rust 测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```
Expected: `test result: ok`，无新增失败

- [ ] **Step 3: 运行前端测试**

```bash
pnpm test 2>&1 | tail -20
```
Expected: 通过

- [ ] **Step 4: 启动开发模式验证**

```bash
pnpm tauri:dev
```

验证清单：
- [ ] 应用正常启动，无启动错误
- [ ] `~/.renlijia/` 下存在 `skills/`、`crypto/`、`playwright-profile/` 等新目录
- [ ] 发送一条消息，检查 `~/.renlijia/conversations/` 下生成新文件
- [ ] skill 管理功能正常（安装/卸载 skill 写入 `~/.renlijia/skills/`）

- [ ] **Step 5: 最终 Commit**

```bash
git add -A
git commit -m "feat(storage): unify all data paths under ~/.renlijia — storage consolidation complete"
```

---

## 自检

**Spec 覆盖检查：**
- ✅ `app_data_dir/AppStorage` → `~/.renlijia/`（Task 2）
- ✅ `init_prompts` 路径更新（Task 2 Step 8）
- ✅ `agent_invocations.json` / `subagent_transcripts/`（Task 2）
- ✅ `permissions.json` / `mcp_servers.json`（Task 2）
- ✅ `custom_plugins/` → `skills/`（Task 2 + Task 3）
- ✅ `playwright-profile/` 等 browser 数据（Task 4）
- ✅ 启动时迁移旧数据，含 `conversations/`、`shared/`、`master.key` → `crypto/master.key`（Task 5）
- ✅ crypto key 目录 `aijia_home.crypto_dir()`，key 文件名 `master.key` 不变（Task 6）
- ✅ `claude_md.rs` → `renlijia_md.rs`，读取路径改为 `RENLIJIA.md` / `.aijia/RENLIJIA.md`（Task 7）
- ✅ `.lotus` → `.aijia`（`workspace_settings.rs`、`lib.rs`、测试文件）（Task 7）
- ✅ `# claudeMd` → `# renlijiaMd`（Task 7）
- ✅ 事件名 `lotus:authorized-workspace-changed` → `aijia:authorized-workspace-changed`（Task 7）
- ✅ 测试文件 `plan_ac_claude_md_test.rs`、`plan_ae_config_layers_test.rs`、`plan_u4` 全部同步更新（Task 7）
- ✅ AiJiaHome 结构体统一管理路径（Task 1）
- ✅ macOS / Windows 统一使用 `~/.renlijia/`，不区分平台

**与 claude-code-best 对齐程度（本计划完成后）：**
- ✅ 单一根目录 `~/.renlijia/`（等价于 `~/.claude/`）
- ✅ skills/ 目录（等价于 `~/.claude/skills/`）
- ⬜ 项目级 `.aijia/` 目录——本计划不实现，留作下一期
- ⬜ settings 分层（global/project/local）——本计划不实现
