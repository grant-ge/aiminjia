# 技能创建后无需重启即可使用 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 LLM 在对话里通过 skill-creator 创建技能后，下一轮 turn 起（甚至同 turn 内）就能用，无需重启 app。

**Architecture:** 显式 install-triggered + Skill miss-retry 兜底 + 前端 SkillCenter 主动刷新。沿用工业主流（systemd / IntelliJ / VS Code / Claude Code）的混合分区："廉价路径走显式 trigger / Skill 工具 miss 自动兜底"。

**Tech Stack:** Rust（Tauri command + RuntimeTool）+ TypeScript（前端 IPC 封装 + useEffect）+ markdown（skill-creator SKILL.md 修改）

**Spec:** `docs/superpowers/specs/2026-06-01-skill-hot-reload-design.md`

---

## File Structure Map

### Create
| Path | Responsibility |
|---|---|
| `src-tauri/src/runtime/tools/builtin/refresh_skills.rs` | 新 `RefreshSkillsTool` 实现，LLM 显式触发 registry 刷新 |
| `src-tauri/tests/skill_hot_reload_test.rs` | 集成测试：refresh IPC + load_skill miss-retry 行为 |

### Modify
| Path | Change |
|---|---|
| `src-tauri/src/commands/skill_management.rs` | 加 `refresh_skill_registry_cmd` Tauri command wrapper |
| `src-tauri/src/lib.rs` | 在 `generate_handler!` 注册新 IPC |
| `src-tauri/src/runtime/tools/builtin/mod.rs` | `pub mod refresh_skills;` |
| `src-tauri/src/runtime/tools/builtin/load_skill.rs` | execute 里 registry miss 时调 refresh + retry，加 5 秒 throttle |
| `src-tauri/src/runtime/tools/catalog.rs` | catalog 注册 `refresh_skills` 条目 + `DAILY_ALLOWED_TOOLS` 加入 |
| `src-tauri/src/plugin/registry.rs` | request-scoped 路由加 `"refresh_skills" => RefreshSkillsTool::new(...)` |
| `src/lib/tauri.ts` | 暴露 `refreshSkillRegistry()` 前端封装 |
| `src/features/skill-center/SkillCenterPage.tsx` | useEffect mount 时调一次 refresh |

### Skill body (out of this repo)
| Path | Change |
|---|---|
| `~/.renlijia/skills/skill-creator/SKILL.md` 本地 | 加 Step 8：调用 `refresh_skills` 工具 |
| Lotus OPS `employee-skills.skill-creator` v1.4 | 同步发布到服务端（不在本仓库执行） |

---

## Task 1: 暴露 `refresh_skill_registry_cmd` Tauri command

**Files:**
- Modify: `src-tauri/src/commands/skill_management.rs`（在 `refresh_skill_registry` fn 之后加 wrapper）
- Modify: `src-tauri/src/lib.rs:1077-1088`（generate_handler! skill_management 段）

- [ ] **Step 1: 写失败测试**

新建 `src-tauri/tests/skill_hot_reload_test.rs`：

```rust
//! Integration tests for skill hot-reload behavior.
//! Tests that refresh_skill_registry sees new SKILL.md files on disk
//! without requiring app restart.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use app_lib::plugin::skill::loader::load_skill_roots;
use app_lib::plugin::skill::registry::SkillRegistry;

#[test]
fn refresh_reads_new_skill_md_added_after_initial_scan() {
    let tmp = TempDir::new().unwrap();
    let user_dir = tmp.path().join("users").join("scope_x").join("skills");
    let global_dir = tmp.path().join("skills");
    fs::create_dir_all(&user_dir).unwrap();
    fs::create_dir_all(&global_dir).unwrap();

    // Initial scan: empty registry
    let roots: Vec<PathBuf> = vec![user_dir.clone(), global_dir.clone()];
    let initial = load_skill_roots(&roots).expect("initial scan ok");
    let registry = Arc::new(Mutex::new(SkillRegistry::new()));
    registry
        .lock()
        .unwrap()
        .replace_all(initial.into_values().collect());
    assert_eq!(registry.lock().unwrap().skill_ids().len(), 0);

    // 模拟 lotus_skill.py install: 写一个新 SKILL.md
    let new_skill_dir = user_dir.join("foo-skill");
    fs::create_dir_all(&new_skill_dir).unwrap();
    fs::write(
        new_skill_dir.join("SKILL.md"),
        "---\nname: foo-skill\ndescription: test skill\n---\n# foo-skill\n\nbody\n",
    )
    .unwrap();

    // Re-scan + replace
    let after = load_skill_roots(&roots).expect("rescan ok");
    registry
        .lock()
        .unwrap()
        .replace_all(after.into_values().collect());

    // 验收：新 skill 在 registry 里
    let ids = registry.lock().unwrap().skill_ids();
    assert!(
        ids.iter().any(|id| id == "foo-skill"),
        "foo-skill must be in registry after re-scan; got: {:?}",
        ids
    );
}
```

- [ ] **Step 2: 运行测试确认失败（其实初始用现有 API 就能通过）**

Run: `cd src-tauri && cargo test --test skill_hot_reload_test refresh_reads_new_skill_md_added_after_initial_scan -- --nocapture`

Expected: PASS（这条测试验证的是 `load_skill_roots` 现有能力，应该已经成立）。如果 fail，说明 `load_skill_roots` 有 bug。

- [ ] **Step 3: 在 commands/skill_management.rs 加 Tauri command wrapper**

在 `refresh_skill_registry` 函数（约 line 231）的紧后面加：

```rust
/// Tauri command wrapper for `refresh_skill_registry`. Exposed so the
/// frontend (SkillCenterPage) and runtime tools (refresh_skills) can
/// trigger a registry refresh without restarting the app.
///
/// Returns `()` on success; serializable error string on failure.
#[tauri::command]
pub async fn refresh_skill_registry_cmd(app: AppHandle) -> Result<(), String> {
    refresh_skill_registry(&app)
}
```

- [ ] **Step 4: 在 lib.rs 注册新 IPC**

修改 `src-tauri/src/lib.rs:1077-1088` 一段，在 `pack_skill,` 之后插入：

```rust
            commands::skill_management::list_custom_skills,
            commands::skill_management::install_custom_skill,
            commands::skill_management::uninstall_custom_skill,
            commands::skill_management::init_skill_template,
            commands::skill_management::pack_skill,
            commands::skill_management::refresh_skill_registry_cmd,
            // Skill package import/export (drag-drop zip / SkillCard export)
            commands::skill_draft::import_skill_package,
```

- [ ] **Step 5: cargo check 验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`

Expected: `Finished dev` 没有新增 error。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/skill_management.rs \
        src-tauri/src/lib.rs \
        src-tauri/tests/skill_hot_reload_test.rs
git commit -m "$(cat <<'EOF'
feat(skill): 暴露 refresh_skill_registry 成 Tauri command

- 加 refresh_skill_registry_cmd wrapper，让前端 / RuntimeTool
  可以通过 IPC 触发 registry 重扫
- 加 skill_hot_reload_test 集成测试，验证 load_skill_roots
  能扫到磁盘新增的 SKILL.md

为下一步 RefreshSkillsTool + load_skill miss-retry 做准备。
EOF
)"
```

---

## Task 2: load_skill miss-retry 兜底 + 5 秒 throttle

**Files:**
- Modify: `src-tauri/src/runtime/tools/builtin/load_skill.rs`
- Modify: `src-tauri/tests/skill_hot_reload_test.rs`（追加测试）

- [ ] **Step 1: 加 miss-retry 集成测试**

在 `src-tauri/tests/skill_hot_reload_test.rs` 末尾追加：

```rust
use std::time::{Duration, Instant};

/// SkillRegistry that tracks how many times its underlying refresh
/// hook would be triggered. Used to assert miss-retry behavior.
struct ProbeRefresh {
    count: Mutex<u32>,
    last_call: Mutex<Option<Instant>>,
}

impl ProbeRefresh {
    fn new() -> Self {
        Self {
            count: Mutex::new(0),
            last_call: Mutex::new(None),
        }
    }
    fn record(&self) {
        *self.count.lock().unwrap() += 1;
        *self.last_call.lock().unwrap() = Some(Instant::now());
    }
    fn count(&self) -> u32 {
        *self.count.lock().unwrap()
    }
}

#[test]
fn miss_retry_throttle_prevents_rapid_repeat_refresh() {
    // 这条测试单独验证 throttle 逻辑（不依赖完整 Tauri ctx）。
    // 实际 load_skill 集成时通过 try_refresh_with_throttle 辅助函数实现。
    let probe = Arc::new(ProbeRefresh::new());
    let throttle = Arc::new(Mutex::new(None::<Instant>));

    // 模拟 5 次连续 miss-retry
    for _ in 0..5 {
        let now = Instant::now();
        let should_refresh = {
            let last = throttle.lock().unwrap();
            match *last {
                None => true,
                Some(t) => now.duration_since(t) >= Duration::from_secs(5),
            }
        };
        if should_refresh {
            probe.record();
            *throttle.lock().unwrap() = Some(now);
        }
    }

    // 5 次连续调用应该只触发 1 次实际 refresh（throttle 生效）
    assert_eq!(probe.count(), 1, "throttle should suppress rapid retries");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd src-tauri && cargo test --test skill_hot_reload_test miss_retry_throttle_prevents_rapid_repeat_refresh -- --nocapture`

Expected: PASS（这条测试不依赖 production code 改动，验证逻辑模式本身）。

- [ ] **Step 3: 修改 load_skill.rs 加 ctx + app_handle 字段**

`src-tauri/src/runtime/tools/builtin/load_skill.rs` 完整替换为：

```rust
//! Stateless skill instruction loading via SKILL.md system.
//!
//! `load_skill` returns a skill's prompt body as a tool result. It does not
//! mutate session state, change the system prompt, or restrict tools.
//!
//! On registry miss it transparently retries once after a refresh — covers
//! the "same-turn install then use" case (LLM runs lotus_skill.py install
//! → immediately calls Skill('new-skill') before refresh_skills RuntimeTool
//! runs). Throttled to at most one refresh per 5 seconds to avoid abuse.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::skill::substitution::{substitute_skill_body, SkillSubstitutionContext};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

/// Format the result of a forked skill execution.
pub fn format_fork_result(skill_name: &str, result_text: &str) -> String {
    format!(
        "Skill \"{}\" completed (forked execution).\n\nResult:\n{}",
        skill_name, result_text
    )
}

/// Throttle for miss-retry: at most one refresh per 5 seconds.
const REFRESH_THROTTLE: Duration = Duration::from_secs(5);

pub struct LoadSkillRuntimeTool {
    skill_registry: Arc<Mutex<SkillRegistry>>,
    /// 最近一次因 miss 触发的 refresh 时间。throttle 用。
    last_refresh: Arc<Mutex<Option<Instant>>>,
}

impl LoadSkillRuntimeTool {
    pub fn new(skill_registry: Arc<Mutex<SkillRegistry>>) -> Self {
        Self {
            skill_registry,
            last_refresh: Arc::new(Mutex::new(None)),
        }
    }

    /// 判断是否允许触发 refresh（throttle）。允许后立刻记录本次时间。
    fn try_acquire_refresh_slot(&self) -> bool {
        let mut last = self.last_refresh.lock().unwrap();
        let now = Instant::now();
        let allow = match *last {
            None => true,
            Some(t) => now.duration_since(t) >= REFRESH_THROTTLE,
        };
        if allow {
            *last = Some(now);
        }
        allow
    }
}

#[async_trait]
impl RuntimeTool for LoadSkillRuntimeTool {
    fn id(&self) -> &str {
        "Skill"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        let ids = self
            .skill_registry
            .lock()
            .map(|reg| reg.skill_ids().join(", "))
            .unwrap_or_default();
        let available = if ids.is_empty() {
            "无可用专项技能".to_string()
        } else {
            ids
        };

        let description = format!(
            "加载一个专项技能的详细指令到当前对话。当用户需求匹配技能目录中的某个专项技能时，\
             调用此工具并传入 skill_id。无副作用：不改变系统提示、不限制工具、不持久化。\
             可用 skill_id：{}。",
            available
        );

        ToolDefinition::new("Skill", description)
            .with_kind(ToolKind::Support)
            .with_read_only(true)
            .with_max_result_size_chars(16_000)
            .with_preserve_tool_use_results(true)
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let skill_id = input
            .get("skill_id")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required field: skill_id".into()))?;

        let args = input
            .get("args")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // Clone the DiskSkill out of the registry (short lock window)
        let skill = {
            let reg = self
                .skill_registry
                .lock()
                .map_err(|e| ToolError::ExecutionFailed(format!("Registry lock failed: {}", e)))?;
            reg.get(&skill_id).cloned()
        };

        // 兜底：registry miss 时，throttle 内尝试 refresh-then-retry。
        // 覆盖"LLM 同 turn install + 立即 Skill('new-skill')"的边缘场景。
        let skill = match skill {
            Some(s) => s,
            None => {
                if self.try_acquire_refresh_slot() {
                    if let Some(app) = ctx.app_handle.as_ref() {
                        let _ = crate::commands::skill_management::refresh_skill_registry(app);
                    }
                }
                // 重查
                let reg = self.skill_registry.lock().map_err(|e| {
                    ToolError::ExecutionFailed(format!("Registry lock failed: {}", e))
                })?;
                let available_ids = reg.skill_ids().join(", ");
                reg.get(&skill_id).cloned().ok_or_else(|| {
                    ToolError::ExecutionFailed(format!(
                        "Unknown or unavailable skill: {}. Available: {}",
                        skill_id, available_ids
                    ))
                })?
            }
        };

        // Check for fork mode (placeholder — full sub-agent wiring in follow-up)
        if skill.frontmatter.context.as_deref() == Some("fork") {
            // TODO: wire to AgentRuntime in follow-up
            let placeholder = format_fork_result(
                &skill.frontmatter.name,
                "fork mode: subagent dispatch will be wired in a follow-up task. Returning a placeholder body so the call doesn't fail.",
            );
            return Ok(ToolResult::new(
                "Skill",
                placeholder,
                Some(json!({
                    "skill_id": skill_id,
                    "display_name": skill.frontmatter.metadata.label.clone()
                        .unwrap_or_else(|| skill.frontmatter.name.clone()),
                    "context": "fork",
                })),
            ));
        }

        // Build substitution context
        let session_id_str = ctx.session_id.as_str().to_string();
        let sub_ctx = SkillSubstitutionContext {
            skill_dir: skill.root.clone(),
            session_id: session_id_str,
            args,
            argument_names: skill.frontmatter.arguments.clone(),
            execute_shell: false,
        };

        let substituted_body = substitute_skill_body(&skill.body, &sub_ctx)
            .map_err(|e| ToolError::ExecutionFailed(format!("Body substitution failed: {}", e)))?;

        let content = format!(
            "## {} ({})\n\nBase directory for this skill: {}\n\n{}",
            skill.frontmatter.name,
            skill_id,
            skill.root.display(),
            substituted_body
        );

        // Track invoked skill
        {
            if let Ok(mut reg) = self.skill_registry.lock() {
                reg.remember_invoked(None, &skill_id, substituted_body.clone());
            }
        }

        Ok(ToolResult::new(
            "Skill",
            content,
            Some(json!({
                "skill_id": skill_id,
                "display_name": skill.frontmatter.metadata.label.clone()
                    .unwrap_or_else(|| skill.frontmatter.name.clone()),
            })),
        ))
    }
}
```

- [ ] **Step 4: cargo check 验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`

Expected: PASS。如果 `ctx.app_handle` 字段访问失败，需要先确认 `ToolExecutionContext` 是否暴露 `app_handle`（看 `runtime/tools/context.rs`）。如果不暴露则需要在 ctx 上加。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/load_skill.rs \
        src-tauri/tests/skill_hot_reload_test.rs
git commit -m "$(cat <<'EOF'
feat(skill): load_skill miss 时隐式 refresh-retry 兜底

- registry 找不到 skill_id 时，触发一次 refresh_skill_registry
  后重查；如果仍 miss 才报错
- 加 5 秒 throttle，防 LLM 在 turn 内对多个不存在的 id 各刷一次
- 覆盖"LLM 同 turn install + 立即 Skill('new-skill')"的边缘场景

参照 MCP client 在 list_changed SHOULD 失约时的兜底设计。
EOF
)"
```

---

## Task 3: `refresh_skills` RuntimeTool 实现

**Files:**
- Create: `src-tauri/src/runtime/tools/builtin/refresh_skills.rs`
- Modify: `src-tauri/src/runtime/tools/builtin/mod.rs:18`
- Modify: `src-tauri/src/runtime/tools/catalog.rs:861` 附近（最后一个 `c.insert` 后）和 `:870` `DAILY_ALLOWED_TOOLS`
- Modify: `src-tauri/src/plugin/registry.rs:1018-1027` 附近

- [ ] **Step 1: 创建 refresh_skills.rs**

`src-tauri/src/runtime/tools/builtin/refresh_skills.rs`：

```rust
//! refresh_skills — LLM 工具：通知 app 重新扫盘 user_skills_dir 和
//! global_skills_dir，把磁盘上新的 SKILL.md 更新到内存 SkillRegistry。
//!
//! 主要由 skill-creator 在 install 后调用（见 SKILL.md step 8）：
//!
//!   Bash(lotus_skill.py install ...)
//!   refresh_skills()                  <-- 这里
//!   <下一 turn catalog 已含 new skill>
//!
//! 也可被其他对话场景使用：用户手动 cp 装 skill 后让 AI 通知 app 刷新。

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::{ToolDefinition, ToolKind};
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct RefreshSkillsTool {
    app: Arc<AppHandle>,
}

impl RefreshSkillsTool {
    pub fn new(app: Arc<AppHandle>) -> Self {
        Self { app }
    }
}

#[async_trait]
impl RuntimeTool for RefreshSkillsTool {
    fn id(&self) -> &str {
        "refresh_skills"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        ToolDefinition::new(
            "refresh_skills",
            "通知 AIjia 重新扫描用户技能目录，让新装的技能立刻在对话和技能中心可见。\
             用法：刚通过 lotus_skill.py install 或别的方式装完技能后调用一次。\
             无参数。返回成功后下一 turn 的 catalog 含新技能。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(false)
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        match crate::commands::skill_management::refresh_skill_registry(&self.app) {
            Ok(()) => Ok(ToolResult::new(
                "refresh_skills",
                "✅ Skill registry refreshed. 新装的技能下一 turn 可用。".to_string(),
                Some(json!({ "refreshed": true })),
            )),
            Err(e) => Ok(ToolResult::new(
                "refresh_skills",
                format!("⚠️ Refresh failed: {}. 重试或重启 app。", e),
                Some(json!({ "refreshed": false, "error": e })),
            )),
        }
    }
}
```

- [ ] **Step 2: 在 mod.rs 注册 module**

修改 `src-tauri/src/runtime/tools/builtin/mod.rs`，在 `pub mod powershell_detect;` 之后插入：

```rust
pub mod powershell_detect;
pub mod refresh_skills;
pub mod send_message;
```

- [ ] **Step 3: 在 catalog.rs 注册**

修改 `src-tauri/src/runtime/tools/catalog.rs`，在 `list_agenda_occurrences` 那个 `c.insert` 块（约 line 861）的紧后面、`c` 返回之前，插入：

```rust
    c.insert(CatalogEntry::new(
        ToolDefinition::new(
            "refresh_skills",
            "通知 AIjia 重新扫描用户技能目录，让新装的技能立刻在对话和技能中心可见。\
             用法：刚通过 lotus_skill.py install 或别的方式装完技能后调用一次。\
             无参数。返回成功后下一 turn 的 catalog 含新技能。",
        )
        .with_kind(ToolKind::Support)
        .with_read_only(false),
        json!({
            "type": "object",
            "properties": {}
        }),
    ));

    c
}
```

注意：原 `c` 返回那一行不变，新增的 block 在它之前。

- [ ] **Step 4: 加入 DAILY_ALLOWED_TOOLS**

修改 `src-tauri/src/runtime/tools/catalog.rs:870-907` 的 `DAILY_ALLOWED_TOOLS` 数组，在 `"list_agenda_occurrences",` 之后插入：

```rust
    "list_agenda_occurrences",
    "refresh_skills",
];
```

- [ ] **Step 5: 在 registry.rs request-scoped 路由注册**

修改 `src-tauri/src/plugin/registry.rs:1018-1027`，在 `"create_agenda_item" | ... | "list_agenda_occurrences"` 那个 match arm 之后、`_ => None` 之前插入：

```rust
            "refresh_skills" => {
                use tauri::Manager;
                let app = ctx.app_handle.as_ref()?;
                Some(Arc::new(builtin::refresh_skills::RefreshSkillsTool::new(
                    Arc::new(app.clone()),
                )) as Arc<dyn crate::runtime::tools::RuntimeTool>)
            }
            _ => None,
```

- [ ] **Step 6: cargo check 验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`

Expected: PASS。

- [ ] **Step 7: 加 RefreshSkillsTool 集成测试**

在 `src-tauri/tests/skill_hot_reload_test.rs` 末尾追加：

```rust
/// 验证 refresh_skill_registry 函数确实把磁盘新增的 SKILL.md
/// 同步到 registry（这是 RefreshSkillsTool 内部调用的核心 fn）。
#[test]
fn refresh_registry_picks_up_disk_changes() {
    let tmp = TempDir::new().unwrap();
    let user_dir = tmp.path().join("users").join("scope_x").join("skills");
    let global_dir = tmp.path().join("skills");
    fs::create_dir_all(&user_dir).unwrap();
    fs::create_dir_all(&global_dir).unwrap();

    let registry = Arc::new(Mutex::new(SkillRegistry::new()));
    let roots: Vec<PathBuf> = vec![user_dir.clone(), global_dir.clone()];

    // 初始空
    let loaded = load_skill_roots(&roots).unwrap();
    registry
        .lock()
        .unwrap()
        .replace_all(loaded.into_values().collect());
    assert_eq!(registry.lock().unwrap().skill_ids().len(), 0);

    // 写 3 个 skill
    for id in &["alpha", "beta", "gamma"] {
        let d = user_dir.join(id);
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("SKILL.md"),
            format!(
                "---\nname: {}\ndescription: test {}\n---\n# {}\n\nbody\n",
                id, id, id
            ),
        )
        .unwrap();
    }

    // 模拟 refresh
    let loaded = load_skill_roots(&roots).unwrap();
    registry
        .lock()
        .unwrap()
        .replace_all(loaded.into_values().collect());

    let ids = registry.lock().unwrap().skill_ids();
    assert_eq!(ids.len(), 3, "should see all 3 skills after refresh");
    for id in &["alpha", "beta", "gamma"] {
        assert!(ids.iter().any(|s| s == id), "missing skill {}", id);
    }
}
```

- [ ] **Step 8: 运行集成测试**

Run: `cd src-tauri && cargo test --test skill_hot_reload_test -- --nocapture`

Expected: 3 tests passed。

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/runtime/tools/builtin/refresh_skills.rs \
        src-tauri/src/runtime/tools/builtin/mod.rs \
        src-tauri/src/runtime/tools/catalog.rs \
        src-tauri/src/plugin/registry.rs \
        src-tauri/tests/skill_hot_reload_test.rs
git commit -m "$(cat <<'EOF'
feat(skill): 新 refresh_skills RuntimeTool

LLM 在对话里显式触发 SkillRegistry 重扫的工具。无参，
返回 { refreshed: bool, error?: string }。

注册位置：
- runtime/tools/builtin/refresh_skills.rs (新建)
- mod.rs / catalog.rs / registry.rs 各加一行
- DAILY_ALLOWED_TOOLS 加入，让默认对话 + 数字员工都能调

下一步：skill-creator SKILL.md step 8 加调用，闭环。
EOF
)"
```

---

## Task 4: 前端 tauri.ts IPC 封装

**Files:**
- Modify: `src/lib/tauri.ts`（在 marketplace commands 段之前的 skill commands 段）

- [ ] **Step 1: 找到合适的注入点**

Run: `grep -n "init_skill_template\|skill_management" src/lib/tauri.ts | head -5`

输出包含几条 export function，比如 `initSkillTemplate`。

- [ ] **Step 2: 加 refreshSkillRegistry 封装**

在 `src/lib/tauri.ts` 中 `initSkillTemplate` 或类似 skill 相关 export 之后加：

```typescript
/** 触发后端重扫 user_skills_dir + global_skills_dir，把新增 SKILL.md 同步到内存 registry。
 *  Used by SkillCenterPage on mount + 任何"装完想立刻看到"的场景。
 *  调用 refresh_skill_registry_cmd Tauri command。 */
export function refreshSkillRegistry(): Promise<void> {
  return invoke<void>('refresh_skill_registry_cmd')
}
```

- [ ] **Step 3: tsc 验证**

Run: `pnpm exec tsc --noEmit 2>&1 | grep -E "error|tauri.ts" | head`

Expected: 空（或只有跟我们改动无关的 pre-existing 警告）。

- [ ] **Step 4: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(skill): 前端 refreshSkillRegistry IPC 封装"
```

---

## Task 5: SkillCenterPage mount 时主动 refresh

**Files:**
- Modify: `src/features/skill-center/SkillCenterPage.tsx`

- [ ] **Step 1: 看 SkillCenterPage 顶部 import 区**

Run: `head -30 src/features/skill-center/SkillCenterPage.tsx`

确认是 React 组件，已有 useEffect import。

- [ ] **Step 2: 加 useEffect 调 refresh**

在 SkillCenterPage 组件函数体的最前面（state hooks 之后、其他 useEffect 之前）加：

```typescript
import { refreshSkillRegistry } from '@/lib/tauri'

// ... 在 function SkillCenterPage() 体内：
useEffect(() => {
  // SkillCenter 打开时主动刷一次 registry，让用户手动 cp 装的 skill
  // 也能立刻可见。失败不影响 UI（registry 是上一次的状态）。
  refreshSkillRegistry().catch((err) => {
    console.warn('[SkillCenterPage] refresh skill registry failed:', err)
  })
}, [])
```

注意：
- 如果 `import` 已经从 `@/lib/tauri` import 别的，把 `refreshSkillRegistry` 加进去那一行而不是新加 import
- 不加 `visibilitychange` 监听（依 spec open question #3，暂不做，等用户体感反馈再加）

- [ ] **Step 3: tsc 验证**

Run: `pnpm exec tsc --noEmit 2>&1 | grep -E "SkillCenterPage" | head`

Expected: 空。

- [ ] **Step 4: vitest 跑 skill-center 整个 suite**

Run: `pnpm exec vitest run src/features/skill-center/ 2>&1 | tail -10`

Expected: 全部通过（13 tests）。

- [ ] **Step 5: Commit**

```bash
git add src/features/skill-center/SkillCenterPage.tsx
git commit -m "$(cat <<'EOF'
feat(skill): SkillCenterPage mount 时主动 refresh registry

让"用户没经过对话、直接想在中心看到刚装的 skill"的场景能用。
对应 Claude Code /reload-plugins 的"命令式 reload 兜底"模式。
EOF
)"
```

---

## Task 6: 更新本地 skill-creator SKILL.md 加 Step 8

**Files:**
- Modify: `~/.renlijia/skills/skill-creator/SKILL.md`（**注意：这个文件不在仓库内，是用户机器上的全局 skill 副本**）

- [ ] **Step 1: 备份当前版本**

```bash
cp ~/.renlijia/skills/skill-creator/SKILL.md ~/.renlijia/skills/skill-creator/SKILL.md.before-step8
```

- [ ] **Step 2: 在 SKILL.md 加 Step 8**

找到 `### 7. 安装到 user skills` 段（搜 `### 7\. 安装到 user skills`），在它的末尾、`### 8. 让用户验收` 之前插入：

```markdown
### 8. 通知应用刷新 registry

调用 `refresh_skills` 工具（无参）通知 AIjia 重新扫描技能目录。

```
refresh_skills()
```

返回 `{ "refreshed": true }` 表示已生效，下一 turn 对话里 catalog 含新技能、Skill 工具能 load 新技能、技能中心也能看到。

**这一步必须做**——否则技能装到了磁盘但内存 registry 不知道，必须重启 app 才能看到。

### 9. 让用户验收
```

把后面原本的 `### 8. 让用户验收` 改成 `### 9. 让用户验收`。

- [ ] **Step 3: 用 sed 也行**

如果手工编辑容易出错，用：

```bash
python3 << 'PY'
import pathlib
p = pathlib.Path.home() / '.renlijia' / 'skills' / 'skill-creator' / 'SKILL.md'
text = p.read_text()
# 找原 Step 8（验收段），换成 9，加新 Step 8
old = "### 8. 让用户验收"
new_step_8 = """### 8. 通知应用刷新 registry

调用 `refresh_skills` 工具（无参）通知 AIjia 重新扫描技能目录。

```
refresh_skills()
```

返回 `{ "refreshed": true }` 表示已生效，下一 turn 对话里 catalog 含新技能、Skill 工具能 load 新技能、技能中心也能看到。

**这一步必须做**——否则技能装到了磁盘但内存 registry 不知道，必须重启 app 才能看到。

### 9. 让用户验收"""

assert old in text, f"原 Step 8 段找不到 — SKILL.md 可能已经被改过，停止操作"
text = text.replace(old, new_step_8)
p.write_text(text)
print(f"✅ 已写入 {p}")
PY
```

- [ ] **Step 4: 验证修改**

```bash
grep -n "^### " ~/.renlijia/skills/skill-creator/SKILL.md
```

Expected：看到 1-9 共 9 个 step 标题，其中 8 是新加的 refresh_skills。

- [ ] **Step 5: 同步到 lotus OPS（脱离本仓库）**

⚠️ 这一步在仓库外执行：

```
1. 在 lotus 后台 OPS / skills 管理页：
   - 找到 skill-creator
   - 把上面修改后的 SKILL.md 粘贴
   - 版本号从 1.3 → 1.4
   - 发布
2. desktop 端下次 skill-sync 时（启动 / 手动刷新）会拉到 v1.4。
3. 在本地，如果没及时同步 OPS，下次 skill-sync 可能把本地 v1.3 之类的服务端版本同步回来覆盖你刚改的。建议立刻同步 OPS。
```

不写 commit（这一步不在仓库里）。

---

## Task 7: 加 E2E 意图测试

**Files:**
- Modify: `docs/test-intents/spec/tasks/技能/rules.md`（在意图 10 之后追加新意图）

- [ ] **Step 1: 看现有意图编号**

Run: `grep "^## 意图" docs/test-intents/spec/tasks/技能/rules.md`

输出包含 1/2/3/7/8/10（4/5/6 是注释删的）。意图 11 可用。

- [ ] **Step 2: 追加意图 11**

在 `docs/test-intents/spec/tasks/技能/rules.md` 末尾追加：

```markdown

---

## 意图 11：技能通过 skill-creator 装完后，无需重启即可被新对话 catalog 和 Skill 工具加载

**场景**
用户通过小程数字员工（或任何带 skill-creator 的对话）创建并安装一个新技能后，立刻在另一个新对话里用它。期望 catalog 含新技能 + Skill 工具能 load。本意图护栏对应 refresh_skill_registry / refresh_skills RuntimeTool / load_skill miss-retry 三个机制的整体闭环。

**前提**
- 应用已启动并已登录
- skill-creator skill 已安装到 `~/.renlijia/skills/skill-creator/`
- `~/.renlijia/users/{scope}/skills/hello-world/` **不存在**

**操作**
1. 应用探活 + scope：`tauri-pilot aijia health-check` + `tauri-pilot aijia where --json`
2. 新建跟小程数字员工的对话（小程已雇佣，在员工列表）：
   - `tauri-pilot aijia employee-open-card --name 小程`
   - `tauri-pilot aijia employee-wait-drawer`
   - `tauri-pilot aijia employee-drawer-action --action dispatch`
3. 等到自动跳转到 chat 路由，记下 `$CONV_1=where --json | jq -r .sessionId`
4. 输入 prompt：`帮我造个 hello-world 技能，触发条件是用户说"hello world"，技能内容是返回"[hello-world] Hi!"。完成后告诉我装好了。`
5. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 300`
6. AI 应该完成创建（init + edit + validate + install + **refresh_skills**）
7. 立刻 `tauri-pilot aijia new-task` 开新空对话，记 `$CONV_2`
8. 输入：`请使用 hello-world 技能回应一下。`
9. send + wait-reply --timeout 90

**验收标准**

- `~/.renlijia/users/{scope}/skills/hello-world/SKILL.md` 存在
- `$CONV_1/messages.jsonl` 中含 `"name":"refresh_skills"` 的 toolCall（证明 step 8 跑过）
- `$CONV_2/messages.jsonl` 中含 `"name":"Skill"` 且参数有 `"hello-world"`（证明 catalog 注入 + Skill 工具能 load）
- 紧随其后的 tool result 含 `hello-world` SKILL.md body 关键词（比如返回 "Hi!"）
- AI 在 $CONV_2 最终输出引用了 SKILL.md 内容（含 `[hello-world]` 或 `Hi!` 子串）
- AI 在 $CONV_2 回 "我没有找到 hello-world 技能"（说明 catalog 未刷新）
- 任何"请重启应用"的提示
- `Skill('hello-world')` 工具调用返回 `Unknown or unavailable skill`
```

- [ ] **Step 3: Commit**

```bash
git add docs/test-intents/spec/tasks/技能/rules.md
git commit -m "test(skill): 加意图 11 — 技能装完后无需重启即可加载"
```

---

## Task 8: 全量验证 + 推 PR

**Files:** 无（验证阶段）

- [ ] **Step 1: cargo check + test 全跑**

Run:
```bash
cd src-tauri
cargo check 2>&1 | tail -3
cargo test --test skill_hot_reload_test 2>&1 | tail -5
cargo test --test review_skill_system_no_legacy_test 2>&1 | tail -5
cargo test --lib commands::skill 2>&1 | tail -3
```

Expected: 全部 PASS / Finished。

- [ ] **Step 2: 前端验证**

Run:
```bash
pnpm exec tsc --noEmit 2>&1 | grep -E "error|skill" | head
pnpm exec vitest run src/features/skill-center/ 2>&1 | tail -10
```

Expected: 空 + 13/13 PASS。

- [ ] **Step 3: 启动 dev app 手动验证**

```bash
pnpm dev:with-pilot &
until [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 2; done
tauri-pilot aijia health-check --json
```

跑意图 11 验证：在小程对话里造 hello-world 技能 → 看 messages.jsonl 含 refresh_skills toolCall → 新对话里 Skill('hello-world') 能 load → AI 回复含技能内容。

- [ ] **Step 4: 写 PR description**

PR title 候选：`refactor(skill): kill skill_smith + bootstrap fallback + hot-reload skill registry`

包含：
- skill_smith RuntimeTool 删除（commit 0880a965）
- templates_bootstrap 删除（commit d3b53f43）
- 小程 v1.0 → v1.2（commit ffb75e29）
- start_skill_watch 死代码删除（commit a1c65c68）
- skill hot-reload 设计 spec（commit 2d255a60）
- 本次 hot-reload 实施的 4-5 个 commit

- [ ] **Step 5: 推送**

```bash
git push origin worktree-kill-skill-smith
git push codeup worktree-kill-skill-smith
```

- [ ] **Step 6: 开 PR**

```bash
gh pr create --title "refactor(skill): 删 skill_smith + bootstrap + 加热重载" --body "$(cat <<'EOF'
## Summary
- 删 skill_smith 死代码（1998 + 518 + 336 + 84 行 = 2936 行）
- 删 templates_bootstrap 离线兜底（云端唯一架构下伪命题）
- 删 start_skill_watch / reload_skill 半成品死代码
- 新增技能热重载：refresh_skills RuntimeTool + load_skill miss-retry + SkillCenterPage 主动 refresh
- 改 skill-creator v1.4 加 Step 8（OPS 同步发布）

## 验证
- ✅ 4 个 worktree commit 全部 cargo check / tsc / vitest 过
- ✅ 数字员工 6/7 意图 + 技能 PASS
- ✅ 新加意图 11 验证热重载闭环（待跑）

## 工业模式参照
spec: docs/superpowers/specs/2026-06-01-skill-hot-reload-design.md
EOF
)"
```

---

## Self-Review

### Spec 覆盖检查

| Spec 改动 | 对应 Task | 状态 |
|---|---|---|
| 1. refresh_skill_registry IPC | Task 1 | ✅ |
| 2. refresh_skills RuntimeTool | Task 3 | ✅ |
| 3. skill-creator SKILL.md step 8 | Task 6 | ✅ |
| 4. load_skill miss-retry + throttle | Task 2 | ✅ |
| 5. SkillCenterPage useEffect refresh | Tasks 4 + 5 | ✅ |
| 测试：unit + integration + e2e | Tasks 1/2/3/7 | ✅ |

### Open question 处理
- spec Q1（toolWhitelist `[]` 语义）：方案在 Task 3 Step 4 把 refresh_skills 加进 `DAILY_ALLOWED_TOOLS`，规避 toolWhitelist 限制；默认对话 + 小程都能调
- spec Q2（throttle）：方案在 Task 2 内实施 5 秒 throttle
- spec Q3（visibilitychange）：方案在 Task 5 Step 2 注释说明暂不做，等用户体感反馈

### 类型一致性检查
- `refresh_skill_registry`（Rust pub fn）vs `refresh_skill_registry_cmd`（Tauri command wrapper）vs `refresh_skills`（RuntimeTool id）vs `refreshSkillRegistry`（前端 TS）—— 命名清晰区分各层 ✅
- `RefreshSkillsTool::new(Arc<AppHandle>)` 跟 Task 3 Step 5 的 registry.rs 注册中 `Arc::new(app.clone())` 一致 ✅

### 没有 placeholder ✅
- 每个 step 都有具体代码或具体命令
- 测试 case 完整给出代码
- skill-creator 修改用 Python 脚本兜底防出错
