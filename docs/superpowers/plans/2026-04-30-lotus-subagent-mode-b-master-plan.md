# Lotus Subagent (Mode B) 综合实施大计划

> **Status**：草案 v1
> **Date**：2026-04-30
> **Scope**：仅模式 B（普通 Subagent，sync + async 全套）。模式 A（主会话 agent profile）和模式 C（Teams）不在本计划。
> **依赖文档**：对标报告 `2026-04-30-subagent-benchmark-vs-claude-code-best.md` v2
> **For agentic workers**：REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 按阶段实施。Steps use checkbox (`- [ ]`) syntax for tracking.

---

## 0. Goal / Architecture / Tech Stack

**Goal**：让父 LLM 能通过通用 `spawn_subagent` 工具主动派子 agent 执行任务，子 agent 拥有独立 LLM loop / 独立工具白名单 / 可指定模型 / 可同步等结果或后台运行；用户可通过 `~/.renlijia/users/{scope}/agents/*.md` 自定义 agent，结果通过 `<task-notification>` 注入父上下文。

**Architecture**：
```
父 LLM tool_use("spawn_subagent", { subagent_type, prompt, model?, run_in_background? })
  → SpawnSubagentRuntimeTool (新增, runtime/tools/builtin/spawn_subagent.rs)
    → AgentRegistry::resolve(subagent_type)              ← 三层合并: builtin + user .md + project .md
    → resolve_subagent_tools(definition, registry)        ← 三层白名单
    ├─ sync 路径: SubagentWorkerRuntime.run() await       → SubAgentResultEnvelope 直接 tool_result 回父
    └─ async 路径:
       ├─ AsyncAgentTaskStore.register(name?, agentId)    ← agentNameRegistry
       ├─ tokio::spawn(run_async_agent_lifecycle)
       ├─ 立即返回 { status: "async_launched", agent_id, output_file }
       ├─ 子 agent 完成 → enqueue_task_notification(<task-notification> XML)
       └─ 父下一 tool round 的 attachment 阶段 drain → 注入 user message
```

**Tech Stack**：
- Rust (tokio async, anyhow, serde_yaml + frontmatter, dashmap)
- 现有 `runtime/agent/` + `runtime/tools/` + `storage/user_scoped_paths.rs`
- 前端不需要新组件（task-notification 由后端自动注入 LLM 上下文，前端只需要展示父 LLM 输出即可）

**核心设计原则（红线）**：
1. 不做模式 A，不做模式 C，不做 worktree 隔离。
2. `~/.renlijia/users/{scope}/agents/` 是 user 级路径（每用户独立），与 `skills_dir()` 同级。
3. 不破坏现有 `browse_data` 流程：`browse_data_agent` / `daily_assistant_agent` 两个内置 agent 保留，仅迁移到新通用入口（旧 `browse_data` 工具改为 `spawn_subagent("browse_data_agent")` 的语法糖包装，向后兼容）。
4. 模型三级优先：调用入参 `model` > definition.model > 继承父。
5. async agent 默认 auto-deny 需要用户确认的工具（避免阻塞）；sync agent 沿用主链路 AskRequired 冒泡。
6. Frequent commits（每个 step 完成可 commit）。TDD：先写失败测试，再实现。

---

## 1. File Structure

### 新建文件

| 路径 | 责任 |
|---|---|
| `src-tauri/src/runtime/agent/markdown_loader.rs` | 解析 `<dir>/*.md` (YAML frontmatter + body) → `AgentDefinition` |
| `src-tauri/src/runtime/agent/registry_loader.rs` | 三层合并：builtin + user dir + project dir → `AgentRegistry` |
| `src-tauri/src/runtime/agent/async_task_store.rs` | `AsyncAgentTaskStore`：name → agentId 注册 + LocalAgentTaskState |
| `src-tauri/src/runtime/agent/task_notification.rs` | `<task-notification>` XML 构造 + 内存队列 |
| `src-tauri/src/runtime/agent/output_writer.rs` | 后台 agent transcript 写盘 + output file 路径解析 |
| `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs` | 通用 `spawn_subagent` RuntimeTool |
| `src-tauri/src/runtime/tools/builtin/task_output.rs` | `task_output` RuntimeTool（父读取后台 agent 增量输出） |
| `src-tauri/src/runtime/agent/builtin/general_purpose.rs` | 内置 `general-purpose` agent definition |
| `src-tauri/src/runtime/agent/builtin/explore.rs` | 内置 `explore` agent definition（只读探索） |
| `src-tauri/tests/spawn_subagent_sync_test.rs` | 集成测试：sync 路径 |
| `src-tauri/tests/spawn_subagent_async_test.rs` | 集成测试：async 路径 + notification |
| `src-tauri/tests/agent_markdown_loader_test.rs` | 集成测试：.md 文件加载 |
| `src-tauri/tests/review_agent_b_constraints.rs` | 架构约束回归 |

### 修改文件

| 路径 | 改动 |
|---|---|
| `src-tauri/src/runtime/agent/definition.rs` | 扩展字段：`disallowed_tools`, `permission_mode`, `background_default` |
| `src-tauri/src/runtime/agent/registry.rs` | 拆出 `with_builtins()` → `register_builtins(&mut self)`；增加 `merge_from_loader` |
| `src-tauri/src/runtime/agent/mod.rs` | 暴露新模块 |
| `src-tauri/src/llm/sub_agent.rs` | `SubAgentConfig` 增 `model_override: Option<String>`, `agent_name: Option<String>`, `definition: Arc<AgentDefinition>` |
| `src-tauri/src/runtime/agent/worker_runtime.rs` | 按 `model_override` 构造子 `LlmGateway`；async 路径不阻塞调用方 |
| `src-tauri/src/runtime/tools/dispatcher.rs` | 同回合并行 dispatch（spawn_subagent 是 concurrency-safe） |
| `src-tauri/src/storage/user_scoped_paths.rs` | 增 `agents_dir()` 方法 |
| `src-tauri/src/runtime/tools/catalog.rs` | 注册 `spawn_subagent` + `task_output` 到 TOOL_CATALOG；从 `DAILY_ALLOWED_TOOLS` 移除 `browse_data`，加入 `spawn_subagent` |
| `src-tauri/src/runtime/event_bus.rs` | 增 `RuntimeEvent::TaskNotificationEnqueued { agent_id }` (调试观察用) |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | tool round 之间 drain task-notification 注入下一 user message |

**不动文件**：
- `runtime/agent/team.rs`（模式 C 不做）
- `runtime/agent/worktree.rs`（worktree 不做）
- `plugin/builtin/tools/browse_data.rs`（保留旧入口，内部转调 spawn_subagent，向后兼容）

---

## 2. Phase 总览

| Phase | 主题 | 必要前置 | 完成后软件状态 |
|---|---|---|---|
| **P0** | AgentDefinition 扩展 + 共享基础类型 | — | 测试新增字段无编译错误，旧逻辑继续工作 |
| **P1** | Markdown loader + 三层 registry 合并 | P0 | 启动时能从 user/project dir 读 .md，注册到 registry |
| **P2** | 通用 `spawn_subagent` tool（sync only） | P1 | 父 LLM 能主动派 sync 子 agent，等结果回传 |
| **P3** | 模型 override 透传 | P2 | spawn_subagent 调用入参 `model` 真正切换子 agent 模型 |
| **P4** | 三层工具白名单 + 递归保护 | P2 | 子 agent 工具集受限；默认禁递归 spawn |
| **P5** | 同回合并行 dispatch | P2 | 父一回合多个 spawn_subagent 并发执行 |
| **P6** | Async 子 agent + AsyncAgentTaskStore | P2 | 父调用返回 agentId 立即继续，子后台跑 |
| **P7** | Task notification 注入 | P6 | 后台子完成后，父下一轮自动看到 `<task-notification>` |
| **P8** | `task_output` tool（父增量观察后台） | P6 | 父可主动读后台 agent progress |
| **P9** | 内置通用 agent + browse_data 兼容包装 | P2-P8 | 内置 general-purpose / explore；browse_data 保持兼容 |
| **P10** | 端到端集成测试 + review_ 约束 | 全部 | 所有路径有测试覆盖 |

每个 Phase 完成都形成可工作软件（增量发布）。中途暂停不留半成品。

---

## 3. Phase P0 — AgentDefinition 扩展 + 共享基础

### Task P0.1 扩展 AgentDefinition 字段

**Files**：
- Modify: `src-tauri/src/runtime/agent/definition.rs`
- Test: `src-tauri/src/runtime/agent/definition.rs` (内联 #[cfg(test)] 模块)

- [ ] **Step 1: 写失败测试**

在 `definition.rs` 末尾添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_definition_supports_disallowed_tools() {
        let def = AgentDefinition {
            name: "x".to_string(),
            description: "y".to_string(),
            allowed_tools: vec!["a".to_string()],
            disallowed_tools: vec!["b".to_string()],
            max_iterations: 10,
            model: AgentModel::Inherit,
            system_prompt: AgentPrompt::Inline("p".to_string()),
            source: AgentSource::Builtin,
            permission_mode: AgentPermissionMode::AutoDeny,
            background_default: false,
        };
        assert_eq!(def.disallowed_tools, vec!["b".to_string()]);
        assert!(matches!(def.permission_mode, AgentPermissionMode::AutoDeny));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cd src-tauri && cargo test --lib runtime::agent::definition::tests::agent_definition_supports_disallowed_tools 2>&1 | tail -10
```

Expected: FAIL（编译错误：缺字段）

- [ ] **Step 3: 添加字段实现**

在 `definition.rs` 中：

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentPermissionMode {
    /// async 子 agent 默认：所有需用户确认的工具自动拒绝
    AutoDeny,
    /// sync 子 agent 默认：AskRequired 冒泡到父
    Bubble,
}

#[derive(Clone, Debug)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,        // 新增
    pub max_iterations: usize,
    pub model: AgentModel,
    pub system_prompt: AgentPrompt,
    pub source: AgentSource,
    pub permission_mode: AgentPermissionMode, // 新增
    pub background_default: bool,             // 新增
}
```

- [ ] **Step 4: 修复所有 AgentDefinition 构造点**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -20
```

逐个修复（builtin/browse_data_agent.rs、builtin/daily_assistant_agent.rs 等），添加默认值：
```rust
disallowed_tools: vec![],
permission_mode: AgentPermissionMode::Bubble,
background_default: false,
```

- [ ] **Step 5: 测试通过**

```bash
cd src-tauri && cargo test --lib runtime::agent::definition 2>&1 | tail -10
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/agent/
git commit -m "feat(agent): extend AgentDefinition with disallowed_tools/permission_mode/background_default"
```

### Task P0.2 增加 user-scoped agents_dir 路径

**Files**：
- Modify: `src-tauri/src/storage/user_scoped_paths.rs`

- [ ] **Step 1: 写失败测试**

在 `user_scoped_paths.rs` 的 `paths_snapshot_consistent` 测试中追加：

```rust
assert_eq!(paths.agents_dir(), root.join("users/t_1__u_2/agents"));
```

- [ ] **Step 2: 运行确认失败**

```bash
cd src-tauri && cargo test --lib storage::user_scoped_paths 2>&1 | tail -10
```

Expected: FAIL（无 `agents_dir` 方法）

- [ ] **Step 3: 实现**

```rust
pub fn agents_dir(&self) -> PathBuf {
    self.base.join("agents")
}
```

- [ ] **Step 4: 测试通过 + commit**

```bash
cd src-tauri && cargo test --lib storage::user_scoped_paths
git add src-tauri/src/storage/user_scoped_paths.rs
git commit -m "feat(storage): add agents_dir to UserScopedPaths"
```

---

## 4. Phase P1 — Markdown Loader + 三层 Registry 合并

### Task P1.1 实现 Markdown frontmatter 解析

**Files**：
- Create: `src-tauri/src/runtime/agent/markdown_loader.rs`
- Test: `src-tauri/tests/agent_markdown_loader_test.rs`

- [ ] **Step 1: 添加 dependency**

`src-tauri/Cargo.toml`：
```toml
serde_yaml = "0.9"
gray_matter = "0.2"
```

- [ ] **Step 2: 写测试 fixture + 失败测试**

`src-tauri/tests/agent_markdown_loader_test.rs`:

```rust
use std::fs;
use tempfile::TempDir;
use aijia::runtime::agent::markdown_loader::load_agent_from_markdown;
use aijia::runtime::agent::definition::{AgentModel, AgentPermissionMode};

#[test]
fn parses_frontmatter_with_all_fields() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("explore.md");
    fs::write(&path, r#"---
name: explore
description: 只读探索代码库
allowed_tools: ["read_file", "grep"]
disallowed_tools: ["write_file"]
max_iterations: 15
model: haiku
permission_mode: auto_deny
background_default: false
---
You are a read-only explorer. Search and report findings."#).unwrap();

    let def = load_agent_from_markdown(&path).expect("must parse");
    assert_eq!(def.name, "explore");
    assert_eq!(def.allowed_tools, vec!["read_file", "grep"]);
    assert_eq!(def.disallowed_tools, vec!["write_file"]);
    assert_eq!(def.max_iterations, 15);
    assert!(matches!(def.model, AgentModel::Fixed(ref m) if m == "haiku"));
    assert!(matches!(def.permission_mode, AgentPermissionMode::AutoDeny));
    match &def.system_prompt {
        aijia::runtime::agent::definition::AgentPrompt::Inline(s) =>
            assert!(s.contains("read-only explorer")),
        _ => panic!("expected inline"),
    }
}

#[test]
fn rejects_missing_required_fields() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.md");
    fs::write(&path, "---\ndescription: no name\n---\nbody").unwrap();
    assert!(load_agent_from_markdown(&path).is_err());
}
```

- [ ] **Step 3: 运行确认失败**

```bash
cd src-tauri && cargo test --test agent_markdown_loader_test 2>&1 | tail -15
```

Expected: FAIL（模块不存在）

- [ ] **Step 4: 实现 markdown_loader.rs**

```rust
use std::path::Path;
use anyhow::{Context, Result};
use serde::Deserialize;

use crate::runtime::agent::definition::{
    AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};

#[derive(Deserialize)]
struct AgentFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    disallowed_tools: Vec<String>,
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "default_permission_mode")]
    permission_mode: String,
    #[serde(default)]
    background_default: bool,
}

fn default_max_iterations() -> usize { 20 }
fn default_permission_mode() -> String { "bubble".into() }

pub fn load_agent_from_markdown(path: &Path) -> Result<AgentDefinition> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
    let parsed = matter.parse(&raw);
    let fm: AgentFrontmatter = parsed
        .data
        .ok_or_else(|| anyhow::anyhow!("missing frontmatter in {}", path.display()))?
        .deserialize()
        .with_context(|| format!("parse frontmatter {}", path.display()))?;

    let permission_mode = match fm.permission_mode.as_str() {
        "auto_deny" => AgentPermissionMode::AutoDeny,
        "bubble" => AgentPermissionMode::Bubble,
        other => anyhow::bail!("unknown permission_mode: {other}"),
    };

    let model = match fm.model {
        Some(m) => AgentModel::Fixed(m),
        None => AgentModel::Inherit,
    };

    Ok(AgentDefinition {
        name: fm.name,
        description: fm.description,
        allowed_tools: fm.allowed_tools,
        disallowed_tools: fm.disallowed_tools,
        max_iterations: fm.max_iterations,
        model,
        system_prompt: AgentPrompt::Inline(parsed.content),
        source: AgentSource::User,
        permission_mode,
        background_default: fm.background_default,
    })
}
```

- [ ] **Step 5: 测试通过 + commit**

```bash
cd src-tauri && cargo test --test agent_markdown_loader_test
git add .
git commit -m "feat(agent): add markdown frontmatter loader for AgentDefinition"
```

### Task P1.2 三层 Registry 合并

**Files**：
- Create: `src-tauri/src/runtime/agent/registry_loader.rs`
- Modify: `src-tauri/src/runtime/agent/registry.rs`
- Test: `src-tauri/tests/agent_registry_merge_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
use std::fs;
use tempfile::TempDir;
use aijia::runtime::agent::registry::AgentRegistry;
use aijia::runtime::agent::registry_loader::load_registry_with_user_dir;

#[test]
fn user_md_overrides_builtin_same_name() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("browse_data_agent.md"), r#"---
name: browse_data_agent
description: User custom override
allowed_tools: ["custom_tool"]
---
custom system prompt"#).unwrap();

    let reg = load_registry_with_user_dir(Some(dir.path()), None).unwrap();
    let def = reg.get("browse_data_agent").expect("must exist");
    assert_eq!(def.allowed_tools, vec!["custom_tool"]);
    assert_eq!(def.description, "User custom override");
}

#[test]
fn builtin_loaded_when_no_user_files() {
    let reg = load_registry_with_user_dir(None, None).unwrap();
    assert!(reg.get("browse_data_agent").is_some());
    assert!(reg.get("daily_assistant_agent").is_some());
}

#[test]
fn user_dir_missing_files_silently_ignored() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("not-md.txt"), "garbage").unwrap();
    fs::write(dir.path().join("bad.md"), "no frontmatter").unwrap();
    let reg = load_registry_with_user_dir(Some(dir.path()), None).unwrap();
    // builtin 仍在
    assert!(reg.get("browse_data_agent").is_some());
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cd src-tauri && cargo test --test agent_registry_merge_test 2>&1 | tail -15
```

Expected: FAIL

- [ ] **Step 3: 实现 registry_loader.rs**

```rust
use std::path::Path;
use anyhow::Result;
use tracing::warn;

use crate::runtime::agent::markdown_loader::load_agent_from_markdown;
use crate::runtime::agent::registry::AgentRegistry;

/// 优先级（后者覆盖前者）：builtin < user_dir < project_dir
pub fn load_registry_with_user_dir(
    user_dir: Option<&Path>,
    project_dir: Option<&Path>,
) -> Result<AgentRegistry> {
    let mut reg = AgentRegistry::with_builtins();
    if let Some(dir) = user_dir {
        merge_dir(&mut reg, dir, "user");
    }
    if let Some(dir) = project_dir {
        merge_dir(&mut reg, dir, "project");
    }
    Ok(reg)
}

fn merge_dir(reg: &mut AgentRegistry, dir: &Path, source_label: &str) {
    if !dir.is_dir() { return; }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => { warn!("agent dir read failed [{source_label}]: {err}"); return; }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") { continue; }
        match load_agent_from_markdown(&path) {
            Ok(def) => reg.register(def),
            Err(err) => warn!("agent md parse failed [{source_label}] {}: {err}", path.display()),
        }
    }
}
```

- [ ] **Step 4: mod.rs 暴露**

`src-tauri/src/runtime/agent/mod.rs` 增 `pub mod markdown_loader;` `pub mod registry_loader;`

- [ ] **Step 5: 测试通过 + commit**

```bash
cd src-tauri && cargo test --test agent_registry_merge_test
git add .
git commit -m "feat(agent): three-tier registry merge (builtin < user < project)"
```

### Task P1.3 启动时按 UserScope 加载 user agents 目录

**Files**：
- Modify: `src-tauri/src/lib.rs`（在 `app.manage` 阶段挂载 registry）

- [ ] **Step 1: 找到现有 AgentRuntime 注入点**

```bash
grep -n "AgentRegistry::with_builtins\|AgentRuntime::from_storage" /Users/a20250311/.codex/worktrees/4dc8/lotus-app/src-tauri/src/lib.rs | head
```

记录行号。

- [ ] **Step 2: 改为按 user scope 加载**

```rust
// 原: let registry = AgentRegistry::with_builtins();
// 改:
let user_dir = paths.agents_dir();
let registry = crate::runtime::agent::registry_loader::load_registry_with_user_dir(
    Some(&user_dir),
    None, // project dir 暂不做
)?;
```

- [ ] **Step 3: 编译 + 手测**

```bash
cd src-tauri && cargo build
```

启动后在 `~/.renlijia/users/<scope>/agents/` 放一个 .md，重启验证 registry list。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(agent): load user agents dir at startup"
```

---

## 5. Phase P2 — 通用 spawn_subagent Tool（sync only）

### Task P2.1 SubAgentConfig 增 model_override / definition

**Files**：
- Modify: `src-tauri/src/llm/sub_agent.rs`
- Test: `src-tauri/src/llm/sub_agent.rs` 内联

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn sub_agent_config_carries_model_override() {
    let cfg = SubAgentConfig {
        task: "do".into(),
        system_prompt: "you are".into(),
        allowed_tools: vec![],
        max_iterations: 5,
        dynamic_context: String::new(),
        conversation_id: "c1".into(),
        parent_run_id: None,
        background: false,
        app_handle: None,
        cancel_token: None,
        permission_mode: PermissionMode::Default,
        model_override: Some("haiku".into()),
        agent_name: Some("inst1".into()),
    };
    assert_eq!(cfg.model_override.as_deref(), Some("haiku"));
    assert_eq!(cfg.agent_name.as_deref(), Some("inst1"));
}
```

- [ ] **Step 2: 运行失败**

```bash
cd src-tauri && cargo test --lib llm::sub_agent::tests::sub_agent_config_carries_model_override 2>&1 | tail -5
```

- [ ] **Step 3: 增加字段**

```rust
pub struct SubAgentConfig {
    // ... existing
    pub model_override: Option<String>,
    pub agent_name: Option<String>,
}
```

- [ ] **Step 4: 修所有构造点（execute_browse_data 等），加默认值**

```bash
cd src-tauri && cargo build 2>&1 | grep error | head
```

逐个加 `model_override: None, agent_name: None,`。

- [ ] **Step 5: 测试通过 + commit**

```bash
cd src-tauri && cargo test --lib llm::sub_agent
git add .
git commit -m "feat(agent): add model_override and agent_name to SubAgentConfig"
```

### Task P2.2 worker_runtime 按 model_override 切换 LlmGateway

**Files**：
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs`

- [ ] **Step 1: 找 gateway 使用点**

```bash
grep -n "self.gateway\|gateway: &" /Users/a20250311/.codex/worktrees/4dc8/lotus-app/src-tauri/src/runtime/agent/worker_runtime.rs | head -20
```

- [ ] **Step 2: 写测试（用 MockLlmExecutor）**

`src-tauri/tests/spawn_subagent_model_override_test.rs`（新建）：

```rust
// 用 docs/test-intents/context/capabilities.md 的 MockLlmExecutor pattern
// 断言：传入 model_override="haiku" 时, mock executor 收到的 model 字段是 "haiku"
// 不传时是父 gateway 默认 model
```

具体内容参考 `docs/test-intents/context/capabilities.md`（用 ProbeExecutor 拦截 model 参数）。

- [ ] **Step 3: 在 worker_runtime 中切换**

在 `SubagentWorkerRuntime::run` 开头加：

```rust
let effective_gateway = match config.model_override.as_deref() {
    Some(model) => {
        // 浅克隆 gateway，覆盖 model 字段
        Arc::new(self.gateway.clone_with_model(model.to_string()))
    }
    None => Arc::new(self.gateway.clone()),
};
// 后续所有 self.gateway.* 改用 effective_gateway
```

`LlmGateway::clone_with_model` 需要新增：

```rust
// llm/gateway.rs
impl LlmGateway {
    pub fn clone_with_model(&self, model: String) -> Self {
        let mut cloned = self.clone();
        cloned.model = model;
        cloned
    }
}
```

- [ ] **Step 4: 测试通过 + commit**

```bash
cd src-tauri && cargo test --test spawn_subagent_model_override_test
git add .
git commit -m "feat(agent): worker_runtime applies model_override per call"
```

### Task P2.3 实现 spawn_subagent RuntimeTool（sync only）

**Files**：
- Create: `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`
- Modify: `src-tauri/src/runtime/tools/catalog.rs`
- Test: `src-tauri/tests/spawn_subagent_sync_test.rs`

- [ ] **Step 1: 在 catalog.rs 注册 ToolDefinition**

```rust
TOOL_CATALOG.register(ToolDefinition {
    id: "spawn_subagent".into(),
    description: "Spawn a sub-agent to execute a focused task. Returns the sub-agent's final output.".into(),
    parameters: serde_json::json!({
        "type": "object",
        "required": ["subagent_type", "prompt", "description"],
        "properties": {
            "subagent_type": {"type": "string", "description": "Agent type name from registry"},
            "prompt": {"type": "string", "description": "Full task instruction for the sub-agent"},
            "description": {"type": "string", "description": "3-5 word task description"},
            "model": {"type": "string", "enum": ["haiku", "sonnet", "opus"], "description": "Override model for this call"},
            "run_in_background": {"type": "boolean", "default": false},
            "name": {"type": "string", "description": "Optional name for SendMessage routing"}
        }
    }),
    default_read_only: false,
    default_destructive: false,
});
```

- [ ] **Step 2: 写失败的集成测试**

```rust
// tests/spawn_subagent_sync_test.rs
// 注册 mock agent_registry，让 spawn_subagent("dummy_agent", prompt="hello")
// 跑通 worker_runtime，断言：
// - 返回 ToolResult content 包含子 agent 的最终消息
// - AgentRuntime spawn_child_run 被调用
// - 父 cancel_token 取消时子也取消
```

- [ ] **Step 3: 实现 spawn_subagent.rs**

```rust
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

#[async_trait]
pub trait SubagentLauncher: Send + Sync {
    async fn launch_sync(
        &self,
        subagent_type: &str,
        prompt: &str,
        model_override: Option<String>,
        ctx: &ToolExecutionContext,
    ) -> Result<String, ToolError>;

    async fn launch_async(
        &self,
        subagent_type: &str,
        prompt: &str,
        model_override: Option<String>,
        name: Option<String>,
        ctx: &ToolExecutionContext,
    ) -> Result<String, ToolError>; // returns agent_id
}

pub struct SpawnSubagentRuntimeTool {
    launcher: Arc<dyn SubagentLauncher>,
    registry: Arc<AgentRegistry>,
}

impl SpawnSubagentRuntimeTool {
    pub fn new(launcher: Arc<dyn SubagentLauncher>, registry: Arc<AgentRegistry>) -> Self {
        Self { launcher, registry }
    }
}

#[async_trait]
impl RuntimeTool for SpawnSubagentRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("spawn_subagent")
            .unwrap_or_else(|| ToolDefinition::new("spawn_subagent", "Spawn sub-agent"))
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool { true } // 允许同回合并行

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let subagent_type = input.get("subagent_type")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing subagent_type".into()))?;
        let prompt = input.get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing prompt".into()))?;
        let model_override = input.get("model")
            .and_then(Value::as_str).map(str::to_string);
        let run_in_background = input.get("run_in_background")
            .and_then(Value::as_bool).unwrap_or(false);
        let name = input.get("name").and_then(Value::as_str).map(str::to_string);

        // verify subagent_type exists
        let def = self.registry.get(subagent_type)
            .ok_or_else(|| ToolError::ExecutionFailed(
                format!("unknown subagent_type: {subagent_type}")))?;

        // 三级 model 优先级
        let effective_model = model_override
            .or_else(|| match &def.model {
                crate::runtime::agent::definition::AgentModel::Fixed(m) => Some(m.clone()),
                crate::runtime::agent::definition::AgentModel::Inherit => None,
            });

        if run_in_background {
            let agent_id = self.launcher.launch_async(
                subagent_type, prompt, effective_model, name, &ctx,
            ).await?;
            Ok(ToolResult::text(format!(
                r#"{{"status":"async_launched","agent_id":"{agent_id}"}}"#
            )))
        } else {
            let output = self.launcher.launch_sync(
                subagent_type, prompt, effective_model, &ctx,
            ).await?;
            Ok(ToolResult::text(output))
        }
    }
}
```

- [ ] **Step 4: 实现 SubagentLauncherImpl（位于 lib.rs 注入处）**

在 lib.rs 的 app.manage 阶段：

```rust
let launcher: Arc<dyn SubagentLauncher> = Arc::new(DefaultSubagentLauncher {
    gateway: llm_gateway.clone(),
    tool_registry: tool_registry.clone(),
    runtime_deps: sub_agent_deps.clone(),
    settings: app_settings.clone(),
    registry: agent_registry.clone(),
});

tool_dispatcher.register(Arc::new(SpawnSubagentRuntimeTool::new(
    launcher,
    agent_registry.clone(),
)));
```

`DefaultSubagentLauncher::launch_sync` 内部构造 `SubAgentConfig` → 调 `run_sub_agent` → 取 envelope.output 返回。

- [ ] **Step 5: 测试通过 + commit**

```bash
cd src-tauri && cargo test --test spawn_subagent_sync_test
git add .
git commit -m "feat(tools): add spawn_subagent RuntimeTool (sync path)"
```

### Task P2.4 把 spawn_subagent 加入 daily_assistant_agent 白名单

**Files**：
- Modify: `src-tauri/src/runtime/tools/catalog.rs`（DAILY_ALLOWED_TOOLS）

- [ ] **Step 1**

把 `"spawn_subagent"` 加入 `DAILY_ALLOWED_TOOLS`。

- [ ] **Step 2: 编译 + commit**

```bash
cd src-tauri && cargo build
git add src-tauri/src/runtime/tools/catalog.rs
git commit -m "feat(catalog): allow spawn_subagent in daily_assistant_agent"
```

---

## 6. Phase P3 — 模型 Override 已在 P2 完成

P3 实质并入 P2.2 + P2.3。无独立任务。Phase 标记为 ✅ Done after P2。

---

## 7. Phase P4 — 三层工具白名单 + 递归保护

### Task P4.1 定义三层白名单常量

**Files**：
- Create: `src-tauri/src/runtime/agent/tool_whitelist.rs`
- Test: 内联 #[cfg(test)]

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_disallowed_blocks_recursive_spawn_for_async_agent() {
        let allowed = resolve_agent_tools(
            &["read_file".to_string(), "spawn_subagent".to_string()],
            &[],
            &["read_file".to_string(), "spawn_subagent".to_string(), "write_file".to_string()],
            /* is_async */ true,
            /* allow_recursive_spawn */ false,
        );
        assert!(!allowed.contains(&"spawn_subagent".to_string()));
        assert!(allowed.contains(&"read_file".to_string()));
    }

    #[test]
    fn definition_disallowed_overrides_allowed() {
        let allowed = resolve_agent_tools(
            &["read_file".to_string(), "write_file".to_string()],
            &["write_file".to_string()],
            &["read_file".to_string(), "write_file".to_string()],
            false,
            false,
        );
        assert_eq!(allowed, vec!["read_file".to_string()]);
    }

    #[test]
    fn async_only_keeps_safe_subset() {
        let allowed = resolve_agent_tools(
            &["read_file".to_string(), "ask_user_question".to_string()],
            &[],
            &["read_file".to_string(), "ask_user_question".to_string()],
            true,
            false,
        );
        // ask_user_question 不在 ASYNC_AGENT_ALLOWED → 被过滤
        assert!(!allowed.contains(&"ask_user_question".to_string()));
    }
}
```

- [ ] **Step 2: 实现 tool_whitelist.rs**

```rust
/// 任何 subagent 都不能用的工具（系统级 disallowed）
pub const ALL_AGENT_DISALLOWED: &[&str] = &[
    "ask_user_question",  // 子不能反向问父之外
    "exit_plan_mode",
    "enter_plan_mode",
];

/// async（后台）subagent 额外限制：仅以下工具允许
pub const ASYNC_AGENT_ALLOWED: &[&str] = &[
    "read_file", "write_file", "edit_file",
    "bash", "grep", "glob",
    "web_search", "web_fetch",
    "spawn_subagent",  // 后面被 allow_recursive_spawn 二次过滤
    "browse_and_extract", "browse_navigate", "read_page_content",
    "page_execute_js", "extract_table_data", "extract_with_pagination",
    "task_output",
];

pub fn resolve_agent_tools(
    def_allowed: &[String],
    def_disallowed: &[String],
    available: &[String],
    is_async: bool,
    allow_recursive_spawn: bool,
) -> Vec<String> {
    let mut out: Vec<String> = available.iter()
        .filter(|t| def_allowed.is_empty() || def_allowed.contains(t))
        .filter(|t| !def_disallowed.contains(t))
        .filter(|t| !ALL_AGENT_DISALLOWED.contains(&t.as_str()))
        .cloned()
        .collect();

    if is_async {
        out.retain(|t| ASYNC_AGENT_ALLOWED.contains(&t.as_str()));
    }

    if !allow_recursive_spawn {
        out.retain(|t| t != "spawn_subagent");
    }

    out
}
```

- [ ] **Step 3: 测试通过 + commit**

```bash
cd src-tauri && cargo test --lib runtime::agent::tool_whitelist
git add .
git commit -m "feat(agent): three-tier tool whitelist + recursive spawn guard"
```

### Task P4.2 worker_runtime 改为调用 resolve_agent_tools

**Files**：
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs::build_turn_request`

- [ ] **Step 1: 找现有 filter 位置**

`worker_runtime.rs:99-103`（`config.allowed_tools.contains(&schema.name)` filter）

- [ ] **Step 2: 改为**

```rust
let all_schemas = self.tool_registry.get_all_schemas().await;
let available_names: Vec<String> = all_schemas.iter().map(|s| s.name.clone()).collect();
let final_allowed = crate::runtime::agent::tool_whitelist::resolve_agent_tools(
    &config.allowed_tools,
    &config.disallowed_tools, // 新字段，需要在 SubAgentConfig 加
    &available_names,
    config.background,
    /* allow_recursive_spawn */ false, // 默认禁，未来 def 可开
);
let tool_defs: Vec<ToolDefinition> = all_schemas.into_iter()
    .filter(|s| final_allowed.contains(&s.name))
    .collect();
```

- [ ] **Step 3: SubAgentConfig 加 disallowed_tools 字段**

`llm/sub_agent.rs`:
```rust
pub disallowed_tools: Vec<String>,
```

修所有构造点。

- [ ] **Step 4: 删 sub_agent.rs:114-119 的 browse_data 硬编码守卫**

（已被通用机制取代）

- [ ] **Step 5: 测试 + commit**

```bash
cd src-tauri && cargo test
git add .
git commit -m "feat(agent): worker_runtime applies three-tier whitelist"
```

---

## 8. Phase P5 — 同回合并行 Dispatch

### Task P5.1 dispatcher 并行执行 concurrency-safe tool

**Files**：
- Modify: `src-tauri/src/runtime/tools/dispatcher.rs`

- [ ] **Step 1: 找现有 round 执行位置**

dispatcher.rs:346 已经有 `futures::future::join_all(futures).await`，确认是否对所有 concurrency_safe tool 都生效。Read 该函数完整逻辑。

- [ ] **Step 2: 写测试**

`src-tauri/tests/spawn_subagent_parallel_test.rs`：

```rust
// 模拟父一回合返回 2 个 spawn_subagent tool_use
// 注入 launcher 内部 tokio::sleep(500ms) 模拟工作
// 总耗时应 < 700ms (并行) 而非 > 1000ms (串行)
```

- [ ] **Step 3: 确认 spawn_subagent.is_concurrency_safe() 返回 true**

已在 P2.3 实现。

- [ ] **Step 4: 测试通过**

如果失败：调整 dispatcher 把 concurrency_safe 的 tool 提取出来 `try_join_all`。

- [ ] **Step 5: Commit**

```bash
git add .
git commit -m "feat(dispatcher): parallel dispatch of concurrency-safe tools (incl spawn_subagent)"
```

---

## 9. Phase P6 — Async 子 Agent + AsyncAgentTaskStore

### Task P6.1 AsyncAgentTaskStore（内存注册表）

**Files**：
- Create: `src-tauri/src/runtime/agent/async_task_store.rs`

- [ ] **Step 1: 写测试**

```rust
use std::sync::Arc;
use crate::runtime::ids::AgentId;

#[test]
fn registers_and_finds_by_name() {
    let store = AsyncAgentTaskStore::new();
    let agent_id = AgentId::new("a1".into());
    store.register("worker1", agent_id.clone(), AsyncTaskState::Running);
    let found = store.find_by_name("worker1").expect("must find");
    assert_eq!(found.agent_id, agent_id);
}

#[test]
fn pending_messages_queue_and_drain() {
    let store = AsyncAgentTaskStore::new();
    let agent_id = AgentId::new("a1".into());
    store.register("worker1", agent_id.clone(), AsyncTaskState::Running);
    store.queue_pending_message(&agent_id, "msg1".into()).unwrap();
    store.queue_pending_message(&agent_id, "msg2".into()).unwrap();
    let drained = store.drain_pending_messages(&agent_id);
    assert_eq!(drained, vec!["msg1", "msg2"]);
    assert!(store.drain_pending_messages(&agent_id).is_empty());
}
```

- [ ] **Step 2: 实现**

```rust
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use crate::runtime::ids::AgentId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsyncTaskState {
    Running, Backgrounded, Completed, Failed, Killed,
}

#[derive(Clone, Debug)]
pub struct AsyncTaskHandle {
    pub agent_id: AgentId,
    pub state: AsyncTaskState,
    pub output_file: std::path::PathBuf,
    pub description: String,
}

#[derive(Default)]
struct Inner {
    by_name: HashMap<String, AgentId>,
    by_id: HashMap<AgentId, AsyncTaskHandle>,
    pending: HashMap<AgentId, Vec<String>>,
}

#[derive(Clone, Default)]
pub struct AsyncAgentTaskStore {
    inner: Arc<Mutex<Inner>>,
}

impl AsyncAgentTaskStore {
    pub fn new() -> Self { Self::default() }

    pub fn register(&self, name: &str, agent_id: AgentId, state: AsyncTaskState) { /* ... */ }
    pub fn find_by_name(&self, name: &str) -> Option<AsyncTaskHandle> { /* ... */ }
    pub fn find_by_id(&self, id: &AgentId) -> Option<AsyncTaskHandle> { /* ... */ }
    pub fn update_state(&self, id: &AgentId, state: AsyncTaskState) { /* ... */ }
    pub fn queue_pending_message(&self, id: &AgentId, msg: String) -> anyhow::Result<()> { /* ... */ }
    pub fn drain_pending_messages(&self, id: &AgentId) -> Vec<String> { /* ... */ }
}
```

- [ ] **Step 3: 测试通过 + commit**

```bash
cd src-tauri && cargo test --lib runtime::agent::async_task_store
git add .
git commit -m "feat(agent): AsyncAgentTaskStore for named async agents + pendingMessages"
```

### Task P6.2 launch_async 实现 + tokio::spawn 后台 lifecycle

**Files**：
- Modify: `src-tauri/src/runtime/tools/builtin/spawn_subagent.rs`（DefaultSubagentLauncher）

- [ ] **Step 1: 写集成测试**

`src-tauri/tests/spawn_subagent_async_test.rs`:

```rust
// 调 spawn_subagent({run_in_background: true, prompt: "echo hi"})
// 立即返回 status: async_launched + agent_id
// 等待 1s
// 检查 AsyncAgentTaskStore.find_by_id 状态变成 Completed
// 检查 task_notification 队列里有该 agentId 的 notification
```

- [ ] **Step 2: 实现 launch_async**

```rust
async fn launch_async(...) -> Result<String, ToolError> {
    let agent_id = AgentId::new(uuid::Uuid::new_v4().to_string());

    let task_store = self.task_store.clone();
    let notif_queue = self.notif_queue.clone();

    if let Some(name) = &name {
        task_store.register(name, agent_id.clone(), AsyncTaskState::Running);
    }

    // detached spawn
    let cfg_clone = build_subagent_config(...); // background=true, model_override
    let runtime_deps = self.runtime_deps.clone();
    let gateway = self.gateway.clone();
    let tool_registry = self.tool_registry.clone();
    let settings = self.settings.clone();
    let id = agent_id.clone();
    let parent_tool_use_id = ctx.tool_call_id.clone();

    tokio::spawn(async move {
        let result = crate::llm::sub_agent::run_sub_agent(
            &gateway, &tool_registry, &runtime_deps, cfg_clone, &settings,
        ).await;
        let (state, notif) = match result {
            Ok(r) => (AsyncTaskState::Completed, build_notification(
                &id, &parent_tool_use_id, "completed", Some(&r.envelope.output), None)),
            Err(e) => (AsyncTaskState::Failed, build_notification(
                &id, &parent_tool_use_id, "failed", None, Some(&e.to_string()))),
        };
        task_store.update_state(&id, state);
        notif_queue.enqueue(id.clone(), notif);
    });

    Ok(agent_id.to_string())
}
```

- [ ] **Step 3: 测试 + commit**

```bash
cd src-tauri && cargo test --test spawn_subagent_async_test
git add .
git commit -m "feat(agent): async spawn_subagent with detached lifecycle"
```

---

## 10. Phase P7 — Task Notification 注入

### Task P7.1 TaskNotificationQueue + XML 构造

**Files**：
- Create: `src-tauri/src/runtime/agent/task_notification.rs`

- [ ] **Step 1: 写测试**

```rust
#[test]
fn xml_format_matches_spec() {
    let notif = build_task_notification_xml(
        "agent-123", Some("toolu_abc"), "/tmp/agent-123.output",
        "completed", "Test agent done", Some("hello result"), Some(1234),
    );
    assert!(notif.contains("<task-id>agent-123</task-id>"));
    assert!(notif.contains("<tool-use-id>toolu_abc</tool-use-id>"));
    assert!(notif.contains("<output-file>/tmp/agent-123.output</output-file>"));
    assert!(notif.contains("<status>completed</status>"));
    assert!(notif.contains("<result>hello result</result>"));
    assert!(notif.contains("<total_tokens>1234</total_tokens>"));
}

#[test]
fn queue_enqueue_drain() {
    let q = TaskNotificationQueue::new();
    q.enqueue("agent-1".into(), "<task-notification>n1</task-notification>".into());
    q.enqueue("agent-2".into(), "<task-notification>n2</task-notification>".into());
    let drained = q.drain_all();
    assert_eq!(drained.len(), 2);
    assert!(q.drain_all().is_empty());
}
```

- [ ] **Step 2: 实现**

```rust
use std::sync::{Arc, Mutex};

pub fn build_task_notification_xml(
    agent_id: &str,
    parent_tool_use_id: Option<&str>,
    output_file: &str,
    status: &str,
    summary: &str,
    result: Option<&str>,
    total_tokens: Option<u64>,
) -> String {
    let mut s = String::from("<task-notification>\n");
    s.push_str(&format!("  <task-id>{}</task-id>\n", agent_id));
    if let Some(t) = parent_tool_use_id {
        s.push_str(&format!("  <tool-use-id>{}</tool-use-id>\n", t));
    }
    s.push_str(&format!("  <output-file>{}</output-file>\n", output_file));
    s.push_str(&format!("  <status>{}</status>\n", status));
    s.push_str(&format!("  <summary>{}</summary>\n", summary));
    if let Some(r) = result {
        s.push_str(&format!("  <result>{}</result>\n", xml_escape(r)));
    }
    if let Some(t) = total_tokens {
        s.push_str(&format!("  <usage><total_tokens>{}</total_tokens></usage>\n", t));
    }
    s.push_str("</task-notification>");
    s
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[derive(Clone, Default)]
pub struct TaskNotificationQueue {
    inner: Arc<Mutex<Vec<(String, String)>>>, // (agent_id, xml)
}

impl TaskNotificationQueue {
    pub fn new() -> Self { Self::default() }
    pub fn enqueue(&self, agent_id: String, xml: String) {
        self.inner.lock().unwrap().push((agent_id, xml));
    }
    pub fn drain_all(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.inner.lock().unwrap())
    }
}
```

- [ ] **Step 3: 测试 + commit**

```bash
cd src-tauri && cargo test --lib runtime::agent::task_notification
git add .
git commit -m "feat(agent): TaskNotificationQueue + XML builder"
```

### Task P7.2 chat_turn_driver 在每个 tool round 之间注入 notification

**Files**：
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

- [ ] **Step 1: 找 tool round 循环位置**

```bash
grep -n "tool_round\|next_user_message\|attachment" /Users/a20250311/.codex/worktrees/4dc8/lotus-app/src-tauri/src/runtime/chat/chat_turn_driver.rs | head
```

- [ ] **Step 2: 写集成测试**

```rust
// 父跑 spawn_subagent({run_in_background: true})
// → 立即返回 agent_id
// 父继续下一 tool round
// 子完成（mock 立即完成）
// 检查父的下一 user message attachment 包含 <task-notification>
```

- [ ] **Step 3: 在 tool round 之后追加 attachment 阶段**

```rust
let pending = self.task_notification_queue.drain_all();
for (_agent_id, xml) in pending {
    next_user_attachments.push(MessageAttachment::TaskNotification(xml));
}
```

`MessageAttachment::TaskNotification(String)` 在 ChatMessage 序列化时作为 user message content 的一部分注入。

- [ ] **Step 4: 测试 + commit**

```bash
cd src-tauri && cargo test --test spawn_subagent_async_test
git add .
git commit -m "feat(chat): inject pending task notifications into next user turn"
```

---

## 11. Phase P8 — task_output Tool

### Task P8.1 实现 task_output（读后台 agent transcript 增量）

**Files**：
- Create: `src-tauri/src/runtime/tools/builtin/task_output.rs`
- Create: `src-tauri/src/runtime/agent/output_writer.rs`

- [ ] **Step 1: output_writer.rs — 后台 agent transcript 落盘**

`launch_async` 路径中，把每条子 agent 消息追加到：
```
~/.renlijia/users/<scope>/subagent_transcripts/<agentId>.jsonl
```

每行一个 JSON：`{"role": "assistant", "content": "..."}` 或 `{"role": "tool", ...}`。

- [ ] **Step 2: 写 task_output 测试**

```rust
// 启动 mock async agent，写 3 行到 transcript
// 调 task_output(taskId=agentId, offset=0) → 拿到 3 行
// 调 task_output(taskId, offset=3) → 拿到空
// 子写第 4 行后再调 → 拿到第 4 行
```

- [ ] **Step 3: 实现 task_output.rs**

```rust
pub struct TaskOutputRuntimeTool {
    paths: Arc<dyn UserScopedPathResolver>,
}

#[async_trait]
impl RuntimeTool for TaskOutputRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("task_output").unwrap()
    }
    fn is_read_only(&self, _: &Value) -> bool { true }

    async fn execute(&self, input: Value, _ctx: ToolExecutionContext)
        -> Result<ToolResult, ToolError>
    {
        let task_id = input.get("task_id").and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("missing task_id".into()))?;
        let offset = input.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let paths = self.paths.require_paths()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let path = paths.subagent_transcripts_dir().join(format!("{task_id}.jsonl"));
        if !path.exists() {
            return Ok(ToolResult::text(r#"{"lines":[],"new_offset":0}"#));
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let lines: Vec<&str> = content.lines().skip(offset).collect();
        let new_offset = offset + lines.len();
        Ok(ToolResult::text(serde_json::json!({
            "lines": lines, "new_offset": new_offset,
        }).to_string()))
    }
}
```

- [ ] **Step 4: 注册到 catalog + register 到 dispatcher（lib.rs）**

- [ ] **Step 5: 测试 + commit**

```bash
cd src-tauri && cargo test --test task_output_test
git add .
git commit -m "feat(tools): task_output for reading async subagent transcript incrementally"
```

---

## 12. Phase P9 — 内置通用 Agent + browse_data 兼容包装

### Task P9.1 内置 general-purpose / explore agent

> **2026-04-30 实施修正（重要）**：本节原 spec 把 `explore.model` 写成 `AgentModel::Fixed("haiku")`，
> 来自 claude-code-best 的 Anthropic 原生 alias 设计，**与 lotus 的 LLM 接入架构不兼容**。
>
> **lotus 接入架构事实**：
> 1. 所有 LLM provider（lotus / openai / deepseek / qwen / volcano / custom）都使用 **OpenAI Chat Completions 协议**；唯一异类是 claude provider（Anthropic Messages API）。
> 2. `AppSettings.primary_model` 字段名误导——它实际是"endpoint 选择 key"（决定 base URL + auth 格式），**不是请求 body 的 `"model"` 字段值**。
> 3. 真正的 model id 在请求 body 的 `"model"` 字段里。当前**只有 lotus / custom 两个 provider 把 model id 透传到了请求 body**（`cloud_model` / `custom_model_name`），openai / deepseek / qwen / volcano / claude 全部把 model 写死成各自的 `DEFAULT_MODEL`。
> 4. 字符串 "haiku" / "sonnet" / "opus" 这种 alias 不是任何下游服务认识的 model id，请求会被拒绝。
>
> **修正策略**：
> - `explore.model` 改为 `AgentModel::Inherit`（跟 parent 走，零硬编码 id）。用户想给 explore 配轻量 model 时，自己写 `~/.renlijia/users/<scope>/agents/explore.md` frontmatter `model: <实际下游 id>`，三层 merge 让用户值赢。
> - `AgentModel::Fixed(String)` 的 String 语义文档化为"下游服务认识的 model id"（如 lotus 远端 `/v1/models` 下发的 id、openai 的 "gpt-4o-mini"、custom endpoint 接受的 id），不是 alias。
> - `effective_settings_for_subagent` 的语义校正见 P9.1.5。
>
> **遗留 gap**（独立专项 `P-router-model-passthrough`，不在 Mode B 范围）：让所有 OpenAI-兼容 provider 都接受外部传入的 model id，而非写死 `DEFAULT_MODEL`。完成后 sub-agent model_override 才能在所有路径生效。

**Files**：
- Create: `src-tauri/src/runtime/agent/builtin/general_purpose.rs`
- Create: `src-tauri/src/runtime/agent/builtin/explore.rs`
- Modify: `src-tauri/src/runtime/agent/registry.rs`

- [ ] **Step 1: general_purpose.rs**

```rust
use crate::runtime::agent::definition::*;

pub fn general_purpose_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "general-purpose".into(),
        description: "通用 subagent，可调用绝大多数工具完成任务".into(),
        allowed_tools: vec![], // 空 = 全集（受 ALL_AGENT_DISALLOWED 过滤）
        disallowed_tools: vec![],
        max_iterations: 30,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(
            "You are a general-purpose sub-agent. Complete the assigned task and return a concise final answer.".into()
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::Bubble,
        background_default: false,
    }
}
```

- [ ] **Step 2: explore.rs**

```rust
pub fn explore_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "explore".into(),
        description: "只读探索：搜索/读取文件，不修改".into(),
        allowed_tools: vec![
            "read_file".into(), "grep".into(), "glob".into(),
            "list_directory".into(), "web_search".into(),
        ],
        disallowed_tools: vec![],
        max_iterations: 25,
        model: AgentModel::Inherit,  // 修正：原 spec 写 Fixed("haiku") 与 lotus 不兼容；见本节顶部说明
        system_prompt: AgentPrompt::Inline(
            "You are a read-only explorer. Search and read files to answer questions. Never modify anything.".into()
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::AutoDeny,
        background_default: false,
    }
}
```

- [ ] **Step 3: registry.rs::with_builtins() 注册**

```rust
registry.register(general_purpose_agent_definition());
registry.register(explore_agent_definition());
```

- [ ] **Step 4: 测试 + commit**

```bash
cd src-tauri && cargo test --test agent_registry_merge_test
git add .
git commit -m "feat(agent): builtin general-purpose and explore agents"
```

### Task P9.2 browse_data 兼容包装

**Files**：
- Modify: `src-tauri/src/plugin/builtin/tools/browse_data.rs`

- [ ] **Step 1: 内部转调 spawn_subagent**

把 `execute_browse_data` 改为：
```rust
// 旧：直接调 run_sub_agent
// 新：构造 spawn_subagent 调用，subagent_type="browse_data_agent"，prompt=task
//     这样所有逻辑（model/whitelist/notification）走通用通道
```

或者保留旧路径但新增 deprecation warning，等下版本删除。

- [ ] **Step 2: 跑现有 browse_data 集成测试，确保不破坏**

```bash
cd src-tauri && cargo test browse_data
```

- [ ] **Step 3: Commit**

```bash
git commit -am "refactor(browse_data): route through spawn_subagent for consistency"
```

---

## 13. Phase P10 — 端到端集成测试 + 架构约束回归

### Task P10.1 端到端：spawn_subagent("explore", prompt="find Cargo.toml")

**Files**：
- Create: `src-tauri/tests/e2e_spawn_subagent_explore.rs`

- [ ] **Step 1**

```rust
// 用 ProbeExecutor 模拟父 LLM 输出 spawn_subagent("explore", prompt="find files")
// 子 LLM (mock) 调 grep tool 返回 "Cargo.toml"
// 断言父收到的 tool_result content 包含 "Cargo.toml"
```

### Task P10.2 端到端 async：spawn_subagent + task_output + notification

**Files**：
- Create: `src-tauri/tests/e2e_spawn_subagent_async.rs`

- [ ] **Step 1**

```rust
// 父调 spawn_subagent({run_in_background: true, name: "w1", prompt: "long task"})
// 立即返回 async_launched
// 父调 task_output("w1", offset=0) 多次观察
// 子完成
// 父下一 turn 收到 <task-notification>
```

### Task P10.3 review_ 架构约束

**Files**：
- Create: `src-tauri/tests/review_agent_b_constraints.rs`

- [ ] **Step 1**

```rust
#[test]
fn agent_module_does_not_use_tauri_directly() {
    let src = include_str!("../src/runtime/agent/markdown_loader.rs");
    assert!(!src.contains("use tauri::"));
    // 同样检查 registry_loader.rs / async_task_store.rs / task_notification.rs
}

#[test]
fn spawn_subagent_tool_is_concurrency_safe() {
    use aijia::runtime::tools::builtin::spawn_subagent::SpawnSubagentRuntimeTool;
    // 构造 mock instance, 断言 is_concurrency_safe(&Value::Null) == true
}

#[test]
fn async_agent_default_disallows_ask_user_question() {
    use aijia::runtime::agent::tool_whitelist::resolve_agent_tools;
    let allowed = resolve_agent_tools(
        &[], &[],
        &["ask_user_question".to_string(), "read_file".to_string()],
        true, false,
    );
    assert!(!allowed.contains(&"ask_user_question".to_string()));
}
```

- [ ] **Step 2: 全跑 + commit**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
git add .
git commit -m "test(agent): e2e + architecture constraint regression"
```

---

## 14. Self-Review Checklist

写完执行前自检：

- [ ] **Spec 覆盖**：
  - 模式 B sync ✅（P2-P5, P9）
  - 模式 B async ✅（P6-P8）
  - 用户 .md 自定义 ✅（P1）
  - 模型三级优先 ✅（P2）
  - 不做 worktree ✅（无相关任务）
  - 不做模式 A/C ✅（无相关任务）

- [ ] **Placeholder 扫描**：所有 step 都有具体代码或命令，无 "TODO/TBD/适当处理"。

- [ ] **类型一致**：
  - `AgentDefinition` 字段在 P0/P1/P9 一致
  - `SubAgentConfig` 字段在 P2.1 加入后所有 phase 用相同名字
  - `AsyncTaskState` 枚举值固定
  - tool name 全文统一为 `spawn_subagent` / `task_output`

- [ ] **每阶段独立可工作**：
  - P0-P5 完成 = sync subagent 全功能可用
  - P6-P8 完成 = async + notification + task_output 全功能可用
  - 中途任何阶段停下不破坏现有 browse_data 流程

---

## 15. 实施顺序与可退化保证

```
P0 → P1 → P2 → P4 → P5 → [里程碑 M1: sync 全功能]
                          ↓
                          P6 → P7 → P8 → [里程碑 M2: async 全功能]
                                          ↓
                                          P9 → P10 → [里程碑 M3: 兼容 + 测试齐全]
```

**可退化保证**：
- M1 之前：所有改动只新增字段（默认值兼容旧逻辑）；旧 `browse_data` 路径不动
- M1 之后：spawn_subagent 可用，但旧 browse_data 仍并存
- M2 之后：async 能力上线，但 sync 仍可单独使用
- M3：browse_data 改为薄包装（可回退到 M2 状态）

任何里程碑结束都能稳定发布。

---

## 16. Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-30-lotus-subagent-mode-b-master-plan.md`**.

两种执行方式：

**1. Subagent-Driven（推荐）**
- 用 `superpowers:subagent-driven-development` skill
- 每个 task 派 fresh subagent 执行；阶段间 review
- 适合本计划：P0→P10 顺序明确，每 task 独立

**2. Inline Execution**
- 用 `superpowers:executing-plans` skill
- 当前 session 批量执行 + checkpoint
- 适合连续推进多个 task

**请选哪种？**
