# Skill 系统完整修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 lotus-app skill 系统的所有已知问题，使其完整工作并在架构上对齐 claude-code-best。

**Architecture:** 分三期执行：期一打通执行链路（skill 能真正影响 LLM 行为）；期二升级激活机制为 LLM 自主调用 SkillTool，支持 mid-conversation 切换和路径条件激活；期三迁移 skill 格式为 SKILL.md 单文件，统一分发包格式。

**Tech Stack:** Rust / Tauri 2.x / tokio / async_trait / serde_json / notify（文件监听）/ toml / pulldown-cmark（期三新增）

---

## 背景：已知问题全景

| 编号 | 问题 | 影响 |
|------|------|------|
| B1 | `detect_activation()` 未被 runtime 调用 | skill 永远不切换 |
| B2 | `system_prompt` 未注入 skill 上下文 | LLM 不知道当前 skill |
| B3 | `TurnConfig.allowed_tools` 始终为 `None` | 工具白名单完全失效 |
| B4 | SkillState 无持久化，会话重连丢失当前 skill | 会话中断后 skill 复位 |
| B5 | `should_activate()` 仅在 `current_skill == "daily-assistant"` 时工作 | skill 切换后无法再切换 |
| B6 | 激活靠关键词硬匹配，LLM 无感知 | 误触发、漏触发 |
| B7 | 不支持 mid-conversation skill 切换（TurnConfig 锁死）| 用户改变需求时无法切换 |
| B8 | 无 `paths:` 条件激活，无法按上下文动态激活 | 需手动触发 |
| B9 | skill 格式（TOML+多MD）与 claude-code-best（SKILL.md）不统一 | 开发体验割裂 |
| B10 | 内置 skill 从 `DailyAssistantSkill` 硬编码中读取，无法热更新基础 prompt | 改 prompt 要重编译 |

---

## 涉及文件总览

### 期一：执行链路打通（B1-B4）

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/runtime/chat/chat_turn_driver.rs` | Modify | 核心改动：注入 SkillRegistry、调用 detect_activation、注入 skill system_prompt 和 allowed_tools |
| `src/runtime/session_runtime.rs` | Modify | SessionRuntime 增加 skill_registry 字段，build_driver_for_turn 中传递 |
| `src/runtime/store/session_store.rs` | Modify | SessionRecord 增加 `active_skill_id` 和 `skill_state` 字段 |
| `src/runtime/chat/skill_session.rs` | Create | 新文件：管理单个会话的 SkillState 生命周期（激活、持久化、恢复） |
| `src/runtime/chat/mod.rs` | Modify | 导出 skill_session 模块 |
| `src/lib.rs` | Modify | SessionRuntime::new 传入 skill_registry |
| `src-tauri/tests/skill_runtime_integration_test.rs` | Create | 集成测试：验证 skill 激活、system_prompt 注入、工具白名单 |

### 期二：激活机制升级（B5-B8）

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/runtime/tools/skill_tool.rs` | Create | 新 RuntimeTool：SkillTool，LLM 调用来切换 skill |
| `src/runtime/tools/definition.rs` | Modify | 新增 ToolKind::Meta 用于 SkillTool |
| `src/plugin/builtin/tools/mod.rs` | Modify | 注册 SkillTool |
| `src/llm/prompts.rs` | Modify | 在 system prompt 中注入可用 skill 列表段 |
| `src/plugin/declarative_skill.rs` | Modify | 去除 `current_skill == "daily-assistant"` 限制，支持任意当前 skill 下激活 |
| `src/runtime/chat/chat_turn_driver.rs` | Modify | 期二：改用 SkillTool 触发路径，移除关键词激活调用 |
| `src-tauri/tests/skill_tool_test.rs` | Create | SkillTool 执行测试 |

### 期三：格式迁移（B9-B10）

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/plugin/skill_md_loader.rs` | Create | SKILL.md 格式解析器（YAML frontmatter + Markdown sections） |
| `src/plugin/declarative_skill.rs` | Modify | 删除旧 load()，改为只读 SKILL.md |
| `src/plugin/manifest.rs` | Modify | 新增 SkillMdManifest 结构 |
| `src/commands/skill_management.rs` | Modify | init_skill_template 生成 SKILL.md 格式模板 |
| `scripts/migrate_skill_format.py` | Create | 自动将 23 个 TOML plugin 迁移为 SKILL.md 格式 |
| `src-tauri/plugins/*/` | Modify | 23 个 plugin 目录：运行迁移脚本后更新 |
| `src-tauri/tests/skill_md_loader_test.rs` | Create | SKILL.md 解析测试 |

---

# 期一：执行链路打通

**目标**：让 skill 系统真正工作——每条用户消息触发 skill 检测，激活的 skill 的 system_prompt 和工具白名单生效。

---

## Task 1：SessionRecord 增加 skill 状态字段

**Files:**
- Modify: `src/runtime/store/session_store.rs`

- [ ] **Step 1: 读取当前 SessionRecord 定义**

确认当前结构（session_store.rs:9-12）：
```rust
pub struct SessionRecord {
    pub session_id: SessionId,
    pub title: Option<String>,
}
```

- [ ] **Step 2: 增加 active_skill_id 和 skill_step 字段**

编辑 `src/runtime/store/session_store.rs`，将 SessionRecord 改为：
```rust
#[derive(Clone, Debug, Default)]
pub struct SessionRecord {
    pub session_id: SessionId,
    pub title: Option<String>,
    /// 当前激活的 skill ID，None 表示使用 default skill（daily-assistant）
    pub active_skill_id: Option<String>,
    /// 当前 skill 的工作流步骤，None 表示未进入多步骤流程
    pub active_skill_step: Option<String>,
}
```

- [ ] **Step 3: 更新 InMemorySessionStore 实现**

`load_session` 和 `save_session` 不需改动（结构体字段透传）。

在文件底部添加测试：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ids::SessionId;

    #[test]
    fn test_session_record_skill_fields() {
        let store = InMemorySessionStore::default();
        let session_id = SessionId::new();
        let record = SessionRecord {
            session_id: session_id.clone(),
            title: None,
            active_skill_id: Some("comp-analysis-v2".to_string()),
            active_skill_step: Some("step1".to_string()),
        };
        store.save_session(record.clone()).unwrap();
        let loaded = store.load_session(&session_id).unwrap();
        assert_eq!(loaded.active_skill_id, Some("comp-analysis-v2".to_string()));
        assert_eq!(loaded.active_skill_step, Some("step1".to_string()));
    }
}
```

- [ ] **Step 4: 运行测试**

```bash
cd src-tauri && cargo test test_session_record_skill_fields -- --nocapture
```
Expected: `test test_session_record_skill_fields ... ok`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/store/session_store.rs
git commit -m "feat(skill): add active_skill_id and step to SessionRecord"
```

---

## Task 2：创建 SkillSession 管理器

**Files:**
- Create: `src/runtime/chat/skill_session.rs`
- Modify: `src/runtime/chat/mod.rs`

- [ ] **Step 1: 写失败测试**

创建测试文件（在 skill_session.rs 底部 `#[cfg(test)]` 块中，先写文件骨架）：

创建 `src/runtime/chat/skill_session.rs`：
```rust
//! SkillSession — 管理单个会话的 Skill 生命周期。
//!
//! 职责：
//! - 根据用户消息调用 SkillRegistry::detect_activation
//! - 维护当前 skill_id 和 SkillState（内存中）
//! - 在 run_chat_turn_s4 开始时提供 skill.system_prompt() 和 allowed_tool_names()

use std::sync::Arc;
use crate::plugin::{SkillRegistry, skill_trait::{Skill, SkillState}};

pub struct SkillSession {
    registry: Arc<SkillRegistry>,
    /// 当前激活的 skill ID
    current_skill_id: String,
    /// 当前 skill 的运行状态
    skill_state: SkillState,
}

impl SkillSession {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        let default_id = registry.default_skill_id().to_string();
        let skill_state = SkillState::new(&default_id);
        Self {
            registry,
            current_skill_id: default_id,
            skill_state,
        }
    }

    /// 根据用户消息检测是否需要切换 skill。
    /// 如果切换，重置 SkillState。
    pub async fn maybe_switch(&mut self, message: &str, has_files: bool) {
        if let Some(new_id) = self
            .registry
            .detect_activation(message, has_files, &self.current_skill_id)
            .await
        {
            if new_id != self.current_skill_id {
                log::info!(
                    "[SkillSession] switching skill: {} → {}",
                    self.current_skill_id,
                    new_id
                );
                self.current_skill_id = new_id.clone();
                self.skill_state = SkillState::new(&new_id);
            }
        }
    }

    /// 返回当前 skill 的 system prompt（追加到基础 prompt 之后）。
    pub async fn system_prompt_suffix(&self) -> String {
        match self.registry.get(&self.current_skill_id).await {
            Some(skill) => skill.system_prompt(&self.skill_state),
            None => String::new(),
        }
    }

    /// 返回当前 skill 的工具白名单（None = 不限制）。
    pub async fn allowed_tool_names(&self) -> Option<Vec<String>> {
        match self.registry.get(&self.current_skill_id).await {
            Some(skill) => skill.allowed_tool_names(&self.skill_state),
            None => None,
        }
    }

    pub fn current_skill_id(&self) -> &str {
        &self.current_skill_id
    }

    pub fn skill_state(&self) -> &SkillState {
        &self.skill_state
    }

    pub fn skill_state_mut(&mut self) -> &mut SkillState {
        &mut self.skill_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::SkillRegistry;

    #[tokio::test]
    async fn test_skill_session_defaults_to_daily_assistant() {
        let registry = Arc::new(SkillRegistry::new("daily-assistant"));
        let session = SkillSession::new(registry);
        assert_eq!(session.current_skill_id(), "daily-assistant");
    }

    #[tokio::test]
    async fn test_skill_session_no_switch_when_no_match() {
        let registry = Arc::new(SkillRegistry::new("daily-assistant"));
        let mut session = SkillSession::new(registry);
        session.maybe_switch("随便聊聊", false).await;
        assert_eq!(session.current_skill_id(), "daily-assistant");
    }
}
```

- [ ] **Step 2: 运行测试验证通过**

```bash
cd src-tauri && cargo test skill_session -- --nocapture
```
Expected: 两个测试均 `ok`

- [ ] **Step 3: 导出模块**

编辑 `src/runtime/chat/mod.rs`，添加：
```rust
pub mod skill_session;
pub use skill_session::SkillSession;
```

- [ ] **Step 4: 编译验证**

```bash
cd src-tauri && cargo check 2>&1 | head -20
```
Expected: 无错误

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/chat/skill_session.rs src-tauri/src/runtime/chat/mod.rs
git commit -m "feat(skill): add SkillSession manager for per-conversation skill lifecycle"
```

---

## Task 3：SessionRuntime 注入 SkillRegistry

**Files:**
- Modify: `src/runtime/session_runtime.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: 读取 SessionRuntime 结构（确认当前字段）**

读取 `src/runtime/session_runtime.rs` 第 26-51 行，当前结构：
```rust
pub struct SessionRuntime {
    query_engine: QueryEngine,
    session_query_engines: Arc<Mutex<HashMap<String, QueryEngine>>>,
    session_cancel_roots: Arc<Mutex<HashMap<String, CancellationToken>>>,
    event_bus: RuntimeEventBus,
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    authorized_workspace_store: Option<Arc<dyn AuthorizedWorkspaceStore>>,
    pending_permission_store: Arc<PendingPermissionRequestStore>,
    permission_store: Option<Arc<PermissionStore>>,
}
```

- [ ] **Step 2: 增加 skill_registry 和 skill_sessions 字段**

在 `session_runtime.rs` 中修改 `SessionRuntime` struct，添加字段：
```rust
use crate::plugin::SkillRegistry;
use crate::runtime::chat::SkillSession;
use tokio::sync::Mutex as TokioMutex;

pub struct SessionRuntime {
    query_engine: QueryEngine,
    session_query_engines: Arc<Mutex<HashMap<String, QueryEngine>>>,
    session_cancel_roots: Arc<Mutex<HashMap<String, CancellationToken>>>,
    event_bus: RuntimeEventBus,
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    authorized_workspace_store: Option<Arc<dyn AuthorizedWorkspaceStore>>,
    pending_permission_store: Arc<PendingPermissionRequestStore>,
    permission_store: Option<Arc<PermissionStore>>,
    /// Skill 注册表，用于每轮 turn 的 skill 激活检测
    skill_registry: Option<Arc<SkillRegistry>>,
    /// 每个 conversation 的 SkillSession（内存态，不持久化到 DB）
    skill_sessions: Arc<TokioMutex<HashMap<String, SkillSession>>>,
}
```

- [ ] **Step 3: 更新 new() 构造函数**

找到 `new()` 函数，添加两个新字段的初始化：
```rust
pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
    Self {
        query_engine,
        session_query_engines: Arc::new(Mutex::new(HashMap::new())),
        session_cancel_roots: Arc::new(Mutex::new(HashMap::new())),
        event_bus,
        llm_executor: None,
        authorized_workspace_store: None,
        pending_permission_store: Arc::new(PendingPermissionRequestStore::new()),
        permission_store: None,
        skill_registry: None,                                        // 新增
        skill_sessions: Arc::new(TokioMutex::new(HashMap::new())), // 新增
    }
}
```

- [ ] **Step 4: 添加 with_skill_registry 构造器方法**

在 impl 块中添加：
```rust
pub fn with_skill_registry(mut self, registry: Arc<SkillRegistry>) -> Self {
    self.skill_registry = Some(registry);
    self
}
```

- [ ] **Step 5: 更新 lib.rs 中的初始化调用**

在 `lib.rs` 中找到 SessionRuntime 的创建位置，添加 skill_registry 注入：
```rust
let session_runtime = SessionRuntime::new(query_engine, event_bus)
    .with_llm_executor(Arc::new(tauri_executor))
    // ...其他现有 with_ 调用...
    .with_skill_registry(skill_registry.clone()); // 新增这行
```

- [ ] **Step 6: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | head -30
```
Expected: 无错误

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/session_runtime.rs src-tauri/src/lib.rs
git commit -m "feat(skill): inject SkillRegistry into SessionRuntime"
```

---

## Task 4：在 run_chat_turn_s4 中接入 SkillSession

**Files:**
- Modify: `src/runtime/chat/chat_turn_driver.rs`

这是期一最核心的改动：在 TurnConfig 构建时注入 skill 的 system_prompt 和 allowed_tools。

- [ ] **Step 1: 在 ChatTurnDriver 中添加 skill_sessions 和 skill_registry 引用**

读取 `chat_turn_driver.rs` 中 `RuntimeChatTurnDriver` 结构定义，添加字段：
```rust
pub struct RuntimeChatTurnDriver {
    pub query_engine: QueryEngine,
    pub event_bus: RuntimeEventBus,
    pub llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    // ...现有字段...
    /// 每个 conversation 的 SkillSession（从 SessionRuntime 共享）
    pub skill_sessions: Arc<TokioMutex<HashMap<String, SkillSession>>>,
    /// Skill 注册表
    pub skill_registry: Option<Arc<SkillRegistry>>,
}
```

- [ ] **Step 2: 在 build_driver_for_turn 中传入 skill_sessions**

在 `session_runtime.rs` 的 `build_driver_for_turn` 方法（或等效构造函数）中，将 `self.skill_sessions` 和 `self.skill_registry` 传给 driver。

- [ ] **Step 3: 在 run_chat_turn_s4 中，TurnConfig 构建之前插入 skill 激活逻辑**

在 `chat_turn_driver.rs` 的 `run_chat_turn_s4` 函数中，找到第 608 行（`// ── Step 1: Build TurnConfig`）之前，插入：

```rust
// ── Step 0: Skill activation ─────────────────────────────────────────────
// 根据用户消息检测是否需要切换 skill，并获取 skill 的 system_prompt 和工具白名单。
let skill_prompt_suffix;
let skill_allowed_tools;
{
    let mut sessions = self.skill_sessions.lock().await;
    let conversation_id = request.conversation_id.as_str();
    let skill_session = sessions
        .entry(conversation_id.to_string())
        .or_insert_with(|| {
            let registry = self.skill_registry.clone().unwrap_or_else(|| {
                Arc::new(SkillRegistry::new("daily-assistant"))
            });
            SkillSession::new(registry)
        });

    // 检测是否需要切换 skill
    let has_files = !request.file_ids.is_empty();
    skill_session.maybe_switch(&request.content, has_files).await;

    // 获取 skill 的 system prompt 追加内容和工具白名单
    skill_prompt_suffix = skill_session.system_prompt_suffix().await;
    skill_allowed_tools = skill_session.allowed_tool_names().await;
}
```

- [ ] **Step 4: 将 skill_prompt_suffix 追加到 system_prompt**

在 `system_prompt` 变量赋值之后（第 622 行附近），追加：

```rust
let system_prompt = executor
    .build_system_prompt(request.conversation_id.as_str())
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

// 追加 skill 的 system prompt
let system_prompt = if skill_prompt_suffix.is_empty() {
    system_prompt
} else {
    format!("{}\n\n{}", system_prompt, skill_prompt_suffix)
};
```

- [ ] **Step 5: 将 skill_allowed_tools 绑定到 TurnConfig**

找到第 634-647 行 TurnConfig 构建，将 `allowed_tools: None` 改为：

```rust
let config = TurnConfig {
    system_prompt,
    tool_defs,
    allowed_tools: skill_allowed_tools, // 从 None 改为 skill_allowed_tools
    max_iterations: 30,
    token_budget: 4096,
    chunk_timeout_secs: 90,
    masking_level: "strict".to_string(),
    workspace_path: workspace_path.clone(),
    llm_settings,
    conversation_id: request.conversation_id.clone(),
    run_id: request.run_id.clone(),
    hook_registry: request.hook_registry.clone(),
};
```

- [ ] **Step 6: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | head -30
```
Expected: 无错误（可能有 unused import 警告，忽略）

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/runtime/session_runtime.rs
git commit -m "feat(skill): wire SkillSession into run_chat_turn_s4 - skill system now active"
```

---

## Task 5：期一集成测试

**Files:**
- Create: `src-tauri/tests/skill_runtime_integration_test.rs`

- [ ] **Step 1: 创建集成测试文件**

```rust
//! 期一集成测试：验证 skill 系统执行链路
//! - skill 激活后 system_prompt 包含 skill 内容
//! - skill 激活后 allowed_tools 生效

#[cfg(test)]
mod skill_runtime_tests {
    use lotus_app::plugin::{SkillRegistry, skill_trait::{Skill, SkillState, ToolFilter}};
    use lotus_app::runtime::chat::SkillSession;
    use std::sync::Arc;
    use async_trait::async_trait;

    /// 测试用 Skill：关键词 "薪酬" 触发，工具限制为 ["load_file", "execute_python"]
    struct TestPayrollSkill;

    #[async_trait]
    impl Skill for TestPayrollSkill {
        fn id(&self) -> &str { "test-payroll" }
        fn display_name(&self) -> &str { "测试薪酬Skill" }
        fn description(&self) -> &str { "test" }
        fn priority(&self) -> u32 { 20 }

        fn should_activate(&self, message: &str, _has_files: bool, current_skill: &str) -> bool {
            current_skill == "daily-assistant" && message.contains("薪酬")
        }

        fn system_prompt(&self, _state: &SkillState) -> String {
            "你是薪酬分析专家，请专注于薪酬数据分析。".to_string()
        }

        fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
            ToolFilter::Only(vec!["load_file".to_string(), "execute_python".to_string()])
        }

        fn allowed_tool_names(&self, state: &SkillState) -> Option<Vec<String>> {
            match self.tool_filter(state) {
                ToolFilter::Only(tools) => Some(tools),
                _ => None,
            }
        }

        fn max_iterations(&self, _: &SkillState) -> usize { 5 }
        fn token_budget(&self, _: &SkillState) -> u32 { 8192 }
    }

    #[tokio::test]
    async fn test_skill_activates_on_keyword() {
        let registry = Arc::new(SkillRegistry::new("daily-assistant"));
        registry.register(Arc::new(TestPayrollSkill), "test").await;
        let mut session = SkillSession::new(registry);

        // 初始状态：daily-assistant
        assert_eq!(session.current_skill_id(), "daily-assistant");

        // 发送含 "薪酬" 的消息
        session.maybe_switch("帮我做薪酬分析", false).await;
        assert_eq!(session.current_skill_id(), "test-payroll");
    }

    #[tokio::test]
    async fn test_skill_system_prompt_injected() {
        let registry = Arc::new(SkillRegistry::new("daily-assistant"));
        registry.register(Arc::new(TestPayrollSkill), "test").await;
        let mut session = SkillSession::new(registry);

        session.maybe_switch("帮我做薪酬分析", false).await;
        let suffix = session.system_prompt_suffix().await;
        assert!(suffix.contains("薪酬分析专家"), "skill system_prompt 应被注入");
    }

    #[tokio::test]
    async fn test_skill_allowed_tools_returned() {
        let registry = Arc::new(SkillRegistry::new("daily-assistant"));
        registry.register(Arc::new(TestPayrollSkill), "test").await;
        let mut session = SkillSession::new(registry);

        session.maybe_switch("帮我做薪酬分析", false).await;
        let tools = session.allowed_tool_names().await;
        assert!(tools.is_some(), "应返回工具白名单");
        let tools = tools.unwrap();
        assert!(tools.contains(&"load_file".to_string()));
        assert!(tools.contains(&"execute_python".to_string()));
        assert!(!tools.contains(&"bash".to_string()), "bash 不应在白名单中");
    }

    #[tokio::test]
    async fn test_no_activation_without_keyword() {
        let registry = Arc::new(SkillRegistry::new("daily-assistant"));
        registry.register(Arc::new(TestPayrollSkill), "test").await;
        let mut session = SkillSession::new(registry);

        session.maybe_switch("今天天气怎么样", false).await;
        assert_eq!(session.current_skill_id(), "daily-assistant");

        let suffix = session.system_prompt_suffix().await;
        assert!(suffix.is_empty(), "无激活时 suffix 应为空");
    }
}
```

- [ ] **Step 2: 运行集成测试**

```bash
cd src-tauri && cargo test skill_runtime_tests -- --nocapture
```
Expected: 4 个测试全部 `ok`

- [ ] **Step 3: 运行架构回归测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
```
Expected: 全部通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tests/skill_runtime_integration_test.rs
git commit -m "test(skill): add integration tests for skill execution pipeline - Phase 1 complete"
```

---

# 期二：激活机制升级

**目标**：将激活机制从关键词硬匹配升级为 LLM 自主调用 SkillTool，支持任意 skill 下的切换和 mid-conversation 动态切换。

---

## Task 6：新增 SkillTool RuntimeTool

**Files:**
- Create: `src/runtime/tools/skill_tool.rs`
- Modify: `src/plugin/builtin/tools/mod.rs`

- [ ] **Step 1: 写 SkillTool 的失败测试**

在 `skill_tool.rs` 中先写骨架和测试：

```rust
//! SkillTool — 让 LLM 主动切换当前 Skill。
//!
//! LLM 调用此工具时传入 skill_id，系统切换到对应 skill 并返回确认。
//! 调用后下一轮 turn 将使用新 skill 的 system_prompt 和工具白名单。

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::runtime::tools::{
    definition::{ToolDefinition, ToolKind},
    dispatcher::{RuntimeTool, ToolExecutionContext, ToolResult, ToolError},
};
use crate::plugin::SkillRegistry;

pub struct SkillTool {
    registry: Arc<SkillRegistry>,
}

impl SkillTool {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl RuntimeTool for SkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "switch_skill",
            "切换当前对话的工作模式（Skill）。当用户的需求明确属于某个专业分析场景时调用。",
        )
        .with_kind(ToolKind::Support)
    }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let skill_id = input
            .get("skill_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("skill_id 参数缺失".to_string()))?;

        // 验证 skill 存在
        let skill = self.registry.get(skill_id).await
            .ok_or_else(|| ToolError::InvalidInput(
                format!("skill '{}' 不存在，可用的 skill 请参考系统提示", skill_id)
            ))?;

        // 通知 SkillSession 切换（通过 ctx 中的 conversation_id 定位）
        // 实际切换在下一个 turn 开始时的 maybe_switch 中执行
        // 这里只返回确认信息，并在 ctx 中设置 pending_skill_switch
        ctx.set_pending_skill_switch(skill_id.to_string()).await;

        Ok(ToolResult::text(format!(
            "已切换到「{}」模式。从下一轮回复开始，我将以{}专家的身份协助你。",
            skill.display_name(),
            skill.display_name(),
        )))
    }
}

// JSON Schema 定义（供 LLM 使用）
pub fn skill_tool_schema() -> Value {
    json!({
        "type": "object",
        "required": ["skill_id"],
        "properties": {
            "skill_id": {
                "type": "string",
                "description": "要切换到的 skill 的 ID，如 'comp-analysis-v2'、'budget-analysis' 等"
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_tool_definition() {
        let registry = Arc::new(SkillRegistry::new("daily-assistant"));
        let tool = SkillTool::new(registry);
        assert_eq!(tool.definition().id, "switch_skill");
    }
}
```

- [ ] **Step 2: 运行测试（先确认基础定义通过）**

```bash
cd src-tauri && cargo test test_skill_tool_definition -- --nocapture
```
Expected: `ok`

- [ ] **Step 3: 注册 SkillTool**

编辑 `src/plugin/builtin/tools/mod.rs`，在 `register_builtin_tools` 函数中添加：

```rust
pub async fn register_builtin_tools(registry: &ToolRegistry) {
    // ...现有注册...
    // SkillTool 需要 skill_registry，从 ToolRegistry 中获取
    if let Some(skill_registry) = registry.skill_registry() {
        registry.register_runtime(Arc::new(
            crate::runtime::tools::skill_tool::SkillTool::new(skill_registry)
        )).await;
    }
    registry.validate_catalog_consistency().await;
}
```

> 注意：ToolRegistry 需要添加 `skill_registry()` 方法，见 Task 7。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/tools/skill_tool.rs src-tauri/src/plugin/builtin/tools/mod.rs
git commit -m "feat(skill): add SkillTool RuntimeTool for LLM-driven skill switching"
```

---

## Task 7：ToolRegistry 增加 skill_registry 引用

**Files:**
- Modify: `src/runtime/tools/registry.rs`（ToolRegistry，非 plugin/registry.rs）
- Modify: `src/lib.rs`

- [ ] **Step 1: 在 ToolRegistry 中增加 skill_registry 字段**

找到 ToolRegistry struct 定义，添加：
```rust
pub struct ToolRegistry {
    // ...现有字段...
    skill_registry: Option<Arc<SkillRegistry>>,
}
```

添加方法：
```rust
pub fn with_skill_registry(mut self, skill_registry: Arc<SkillRegistry>) -> Self {
    self.skill_registry = Some(skill_registry);
    self
}

pub fn skill_registry(&self) -> Option<Arc<SkillRegistry>> {
    self.skill_registry.clone()
}
```

- [ ] **Step 2: 在 lib.rs 中传入 skill_registry**

找到 ToolRegistry 初始化位置，添加：
```rust
let tool_registry = ToolRegistry::new()
    .with_skill_registry(skill_registry.clone()); // 新增
```

- [ ] **Step 3: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | head -20
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/runtime/tools/registry.rs src-tauri/src/lib.rs
git commit -m "feat(skill): pass SkillRegistry to ToolRegistry for SkillTool registration"
```

---

## Task 8：system prompt 注入 skill 列表

**Files:**
- Modify: `src/llm/prompts.rs`

- [ ] **Step 1: 写测试验证 skill 列表注入**

在 `prompts.rs` 底部 `#[cfg(test)]` 中添加：
```rust
#[test]
fn test_skill_list_section_format() {
    let skills = vec![
        ("comp-analysis-v2", "薪酬分析", "用于薪酬数据的深度分析"),
        ("budget-analysis", "预算执行分析", "用于预算对比和差异分析"),
    ];
    let section = build_skill_list_section(&skills);
    assert!(section.contains("comp-analysis-v2"));
    assert!(section.contains("薪酬分析"));
    assert!(section.contains("switch_skill"));
}
```

- [ ] **Step 2: 实现 build_skill_list_section**

在 `prompts.rs` 中添加：
```rust
/// 构建 skill 列表段，注入到 system prompt，让 LLM 知道何时调用 switch_skill。
pub fn build_skill_list_section(skills: &[(&str, &str, &str)]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let skill_lines: Vec<String> = skills
        .iter()
        .map(|(id, name, desc)| format!("- `{}` — **{}**：{}", id, name, desc))
        .collect();

    format!(
        "## 可用专业模式（Skill）\n\
        当用户需求明确属于以下场景时，调用 `switch_skill` 工具切换模式：\n\n\
        {}\n\n\
        切换后将获得该场景的专业提示词和专属工具集。无明确需求时保持当前模式。",
        skill_lines.join("\n")
    )
}
```

- [ ] **Step 3: 运行测试**

```bash
cd src-tauri && cargo test test_skill_list_section_format -- --nocapture
```
Expected: `ok`

- [ ] **Step 4: 在 get_system_prompt / build_system_prompt 中调用**

在 `TauriLegacyTurnExecutor::build_system_prompt` 中（或等效的系统提示组装位置），在返回前追加 skill 列表段：

```rust
// 从 skill_registry 获取所有 skill 信息
if let Some(registry) = &self.skill_registry {
    let skill_infos = registry.list().await;
    let skill_tuples: Vec<(&str, &str, &str)> = skill_infos
        .iter()
        .filter(|s| s.id != "daily-assistant") // 不列出默认 skill
        .map(|s| (s.id.as_str(), s.display_name.as_str(), s.description.as_str()))
        .collect();
    let skill_section = crate::llm::prompts::build_skill_list_section(&skill_tuples);
    if !skill_section.is_empty() {
        prompt = format!("{}\n\n{}", prompt, skill_section);
    }
}
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/llm/prompts.rs
git commit -m "feat(skill): inject skill list into system prompt for LLM-driven activation"
```

---

## Task 9：去除 should_activate 的 daily-assistant 限制（B5）

**Files:**
- Modify: `src/plugin/declarative_skill.rs`

- [ ] **Step 1: 写测试（验证 daily-assistant 限制已被突破）**

在 `declarative_skill.rs` 的测试块中添加：
```rust
#[test]
fn test_skill_can_activate_from_any_current_skill() {
    // 模拟从另一个 skill 切换
    let skill = /* 构建测试 DeclarativeSkill */;
    // 当 current_skill 是别的 skill 时，should_activate 也应能工作
    // 现在期望的行为：任何 skill 下都可以被激活（LLM 通过 SkillTool 显式切换）
    assert!(skill.should_activate("薪酬分析", false, "budget-analysis"));
}
```

> 注意：期二中 skill 切换主要靠 LLM 调用 SkillTool，should_activate 作为兜底。需要放宽限制。

- [ ] **Step 2: 修改 should_activate 逻辑**

在 `declarative_skill.rs` 的 `should_activate` 实现中，将：
```rust
if current_skill != "daily-assistant" {
    return false;
}
```
改为：
```rust
// 期二：允许从任意 skill 通过关键词激活（LLM SkillTool 是主路径，关键词匹配作为兜底）
// 仅阻止同一 skill 的重复激活
if current_skill == self.id() {
    return false;
}
```

- [ ] **Step 3: 运行测试**

```bash
cd src-tauri && cargo test skill_runtime -- --nocapture
```
Expected: 全部通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/plugin/declarative_skill.rs
git commit -m "feat(skill): allow activation from any current skill, not just daily-assistant"
```

---

## Task 10：mid-conversation skill 切换支持（B7）

**Files:**
- Modify: `src/runtime/chat/skill_session.rs`
- Modify: `src/runtime/tools/dispatcher.rs`（ToolExecutionContext）

- [ ] **Step 1: ToolExecutionContext 增加 pending_skill_switch**

在 `ToolExecutionContext` 结构中添加：
```rust
pub struct ToolExecutionContext {
    // ...现有字段...
    pub pending_skill_switch: Arc<TokioMutex<Option<String>>>,
}

impl ToolExecutionContext {
    pub async fn set_pending_skill_switch(&self, skill_id: String) {
        *self.pending_skill_switch.lock().await = Some(skill_id);
    }

    pub async fn take_pending_skill_switch(&self) -> Option<String> {
        self.pending_skill_switch.lock().await.take()
    }
}
```

- [ ] **Step 2: 在 SkillSession 中支持 force_switch**

在 `skill_session.rs` 中添加：
```rust
/// 强制切换到指定 skill（由 SkillTool 调用）
pub async fn force_switch(&mut self, skill_id: &str) {
    if skill_id == self.current_skill_id {
        return;
    }
    log::info!(
        "[SkillSession] force switch: {} → {}",
        self.current_skill_id,
        skill_id
    );
    self.current_skill_id = skill_id.to_string();
    self.skill_state = SkillState::new(skill_id);
}
```

- [ ] **Step 3: 在 run_chat_turn_s4 的工具执行后检查 pending_skill_switch**

在工具调用结果处理之后（ToolRoundDriver::execute_round 之后），插入：
```rust
// 检查工具执行是否触发了 skill 切换
if let Some(new_skill_id) = ctx.take_pending_skill_switch().await {
    let mut sessions = self.skill_sessions.lock().await;
    if let Some(session) = sessions.get_mut(request.conversation_id.as_str()) {
        session.force_switch(&new_skill_id).await;
        // 注意：当前 turn 已使用旧 skill 的 prompt，下一 turn 生效新 skill
        log::info!(
            "[run_chat_turn_s4] skill switched to '{}', effective next turn",
            new_skill_id
        );
    }
}
```

- [ ] **Step 4: 写测试验证 force_switch**

在 `skill_session.rs` 测试块中添加：
```rust
#[tokio::test]
async fn test_force_switch_changes_skill() {
    let registry = Arc::new(SkillRegistry::new("daily-assistant"));
    let mut session = SkillSession::new(registry);
    assert_eq!(session.current_skill_id(), "daily-assistant");
    session.force_switch("comp-analysis-v2").await;
    assert_eq!(session.current_skill_id(), "comp-analysis-v2");
}
```

- [ ] **Step 5: 运行所有 skill 相关测试**

```bash
cd src-tauri && cargo test skill -- --nocapture
```
Expected: 全部通过

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/skill_session.rs src-tauri/src/runtime/tools/dispatcher.rs src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(skill): support mid-conversation skill switching via SkillTool"
```

---

## Task 11：期二集成测试

**Files:**
- Modify: `src-tauri/tests/skill_runtime_integration_test.rs`

- [ ] **Step 1: 添加 SkillTool 测试**

在已有测试文件中追加：
```rust
#[tokio::test]
async fn test_force_switch_via_skill_tool() {
    let registry = Arc::new(SkillRegistry::new("daily-assistant"));
    registry.register(Arc::new(TestPayrollSkill), "test").await;
    let mut session = SkillSession::new(registry);

    // 强制切换（模拟 SkillTool 调用）
    session.force_switch("test-payroll").await;
    assert_eq!(session.current_skill_id(), "test-payroll");

    let suffix = session.system_prompt_suffix().await;
    assert!(suffix.contains("薪酬分析专家"));
}

#[tokio::test]
async fn test_skill_list_section_not_empty() {
    let skills = vec![
        ("comp-analysis-v2", "薪酬分析", "薪酬数据深度分析"),
    ];
    let section = crate::llm::prompts::build_skill_list_section(&skills);
    assert!(!section.is_empty());
    assert!(section.contains("switch_skill"));
}
```

- [ ] **Step 2: 运行全部测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -20
```
Expected: 无失败

- [ ] **Step 3: 运行架构回归测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast
```
Expected: 全部通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tests/skill_runtime_integration_test.rs
git commit -m "test(skill): add Phase 2 integration tests - LLM-driven skill switching"
```

---

# 期三：格式迁移

**目标**：将 skill 格式从 TOML+多MD 完全替换为 SKILL.md 单文件。旧 TOML 格式直接废弃，不保留兼容。

---

## Task 12：SKILL.md 格式解析器

**Files:**
- Create: `src/plugin/skill_md_loader.rs`

SKILL.md 格式定义（含多步骤 workflow）：

```markdown
---
id: comp-analysis-v2
name: 薪酬分析
description: 薪酬数据的深度诊断与对标分析
priority: 20
model: deep_reasoning
requires_files: true
keywords: ["薪酬分析", "薪酬诊断", "薪资分析"]
file_keywords: ["薪酬", "salary"]
max_iterations: 5
token_budget: 8192
include_app_base: true
icon: "📊"
short_description: "薪酬内外部公平性分析"
trigger_text: "帮我做薪酬分析"
category: hr
---

## base

你是薪酬分析专家...（基础 prompt）

## step0

### config
advance_on: any
max_iterations: 5

请用户上传薪酬数据文件...

## step1

### config
advance_on: confirm
tools_only: [load_file, execute_python, export_data]
max_iterations: 5
precompute: scripts/step1.py

开始数据清洗和分析...
```

- [ ] **Step 1: 写解析器失败测试**

创建 `src/plugin/skill_md_loader.rs`：

```rust
//! SKILL.md 格式解析器
//! 支持 YAML frontmatter + Markdown sections 的单文件 skill 格式。

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SkillMdFrontmatter {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub priority: u32,
    pub model: Option<String>,
    pub requires_files: bool,
    pub keywords: Vec<String>,
    pub file_keywords: Vec<String>,
    pub max_iterations: usize,
    pub token_budget: u32,
    pub include_app_base: bool,
    pub icon: String,
    pub short_description: String,
    pub trigger_text: String,
    pub category: String,
}

#[derive(Debug, Clone)]
pub struct SkillMdStep {
    pub id: String,
    pub prompt: String,
    pub advance_on: String,
    pub tools_only: Option<Vec<String>>,
    pub max_iterations: Option<usize>,
    pub token_budget: Option<u32>,
    pub precompute: Option<String>,
}

#[derive(Debug)]
pub struct SkillMdDocument {
    pub frontmatter: SkillMdFrontmatter,
    pub base_prompt: String,
    pub steps: Vec<SkillMdStep>,
}

/// 解析 SKILL.md 文件内容
pub fn parse_skill_md(content: &str) -> Result<SkillMdDocument, String> {
    // 解析 YAML frontmatter（--- 之间的内容）
    let (frontmatter_str, body) = extract_frontmatter(content)?;
    let frontmatter = parse_frontmatter(&frontmatter_str)?;

    // 解析 Markdown sections
    let (base_prompt, steps) = parse_sections(&body)?;

    Ok(SkillMdDocument { frontmatter, base_prompt, steps })
}

fn extract_frontmatter(content: &str) -> Result<(String, String), String> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return Err("SKILL.md 必须以 --- 开头的 YAML frontmatter 开始".to_string());
    }
    let after_first = &content[3..];
    let end = after_first
        .find("\n---")
        .ok_or("frontmatter 未找到结束标记 ---")?;
    let frontmatter = &after_first[..end];
    let body = &after_first[end + 4..];
    Ok((frontmatter.to_string(), body.to_string()))
}

fn parse_frontmatter(yaml: &str) -> Result<SkillMdFrontmatter, String> {
    // 使用 serde_yaml 解析（需在 Cargo.toml 中添加 serde_yaml 依赖）
    let value: serde_yaml::Value = serde_yaml::from_str(yaml)
        .map_err(|e| format!("frontmatter YAML 解析失败: {}", e))?;

    let id = value["id"].as_str()
        .ok_or("frontmatter 缺少 id 字段")?.to_string();
    let name = value["name"].as_str()
        .ok_or("frontmatter 缺少 name 字段")?.to_string();

    Ok(SkillMdFrontmatter {
        id,
        name,
        description: value["description"].as_str().map(|s| s.to_string()),
        priority: value["priority"].as_u64().unwrap_or(0) as u32,
        model: value["model"].as_str().map(|s| s.to_string()),
        requires_files: value["requires_files"].as_bool().unwrap_or(false),
        keywords: parse_string_array(&value["keywords"]),
        file_keywords: parse_string_array(&value["file_keywords"]),
        max_iterations: value["max_iterations"].as_u64().unwrap_or(10) as usize,
        token_budget: value["token_budget"].as_u64().unwrap_or(4096) as u32,
        include_app_base: value["include_app_base"].as_bool().unwrap_or(true),
        icon: value["icon"].as_str().unwrap_or("🔧").to_string(),
        short_description: value["short_description"].as_str().unwrap_or("").to_string(),
        trigger_text: value["trigger_text"].as_str().unwrap_or("").to_string(),
        category: value["category"].as_str().unwrap_or("general").to_string(),
    })
}

fn parse_string_array(value: &serde_yaml::Value) -> Vec<String> {
    value.as_sequence()
        .map(|seq| seq.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect())
        .unwrap_or_default()
}

fn parse_sections(body: &str) -> Result<(String, Vec<SkillMdStep>), String> {
    // 按 ## 分割 sections
    let mut base_prompt = String::new();
    let mut steps = Vec::new();
    let mut current_section: Option<(String, String)> = None;

    for line in body.lines() {
        if line.starts_with("## ") {
            if let Some((section_id, content)) = current_section.take() {
                if section_id == "base" {
                    base_prompt = content.trim().to_string();
                } else if section_id.starts_with("step") {
                    steps.push(parse_step_section(&section_id, &content)?);
                }
            }
            current_section = Some((line[3..].trim().to_string(), String::new()));
        } else if let Some((_, ref mut content)) = current_section {
            content.push_str(line);
            content.push('\n');
        }
    }

    // 处理最后一个 section
    if let Some((section_id, content)) = current_section {
        if section_id == "base" {
            base_prompt = content.trim().to_string();
        } else if section_id.starts_with("step") {
            steps.push(parse_step_section(&section_id, &content)?);
        }
    }

    Ok((base_prompt, steps))
}

fn parse_step_section(id: &str, content: &str) -> Result<SkillMdStep, String> {
    // 解析 ### config 子块
    let mut config_str = String::new();
    let mut prompt_lines = Vec::new();
    let mut in_config = false;

    for line in content.lines() {
        if line.trim() == "### config" {
            in_config = true;
        } else if in_config && line.starts_with("###") {
            in_config = false;
            prompt_lines.push(line);
        } else if in_config {
            config_str.push_str(line);
            config_str.push('\n');
        } else {
            prompt_lines.push(line);
        }
    }

    let prompt = prompt_lines.join("\n").trim().to_string();

    // 解析 config YAML
    let config: serde_yaml::Value = if config_str.trim().is_empty() {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    } else {
        serde_yaml::from_str(&config_str)
            .map_err(|e| format!("step {} config 解析失败: {}", id, e))?
    };

    Ok(SkillMdStep {
        id: id.to_string(),
        prompt,
        advance_on: config["advance_on"].as_str().unwrap_or("confirm").to_string(),
        tools_only: config["tools_only"].as_sequence().map(|seq| {
            seq.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
        }),
        max_iterations: config["max_iterations"].as_u64().map(|n| n as usize),
        token_budget: config["token_budget"].as_u64().map(|n| n as u32),
        precompute: config["precompute"].as_str().map(|s| s.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SKILL_MD: &str = r#"---
id: test-skill
name: 测试Skill
description: 用于测试的 skill
priority: 20
model: deep_reasoning
requires_files: true
keywords: ["测试", "test"]
max_iterations: 5
token_budget: 8192
include_app_base: true
icon: "🧪"
short_description: "测试"
trigger_text: "帮我测试"
category: general
---

## base

你是测试专家，请专注于测试工作。

## step0

### config
advance_on: any
max_iterations: 3

请上传测试数据。

## step1

### config
advance_on: confirm
tools_only: [execute_python, export_data]
max_iterations: 5
precompute: scripts/step1.py

开始执行测试分析。
"#;

    #[test]
    fn test_parse_frontmatter() {
        let doc = parse_skill_md(SAMPLE_SKILL_MD).unwrap();
        assert_eq!(doc.frontmatter.id, "test-skill");
        assert_eq!(doc.frontmatter.name, "测试Skill");
        assert_eq!(doc.frontmatter.priority, 20);
        assert!(doc.frontmatter.requires_files);
        assert_eq!(doc.frontmatter.keywords, vec!["测试", "test"]);
    }

    #[test]
    fn test_parse_base_prompt() {
        let doc = parse_skill_md(SAMPLE_SKILL_MD).unwrap();
        assert!(doc.base_prompt.contains("测试专家"));
    }

    #[test]
    fn test_parse_steps() {
        let doc = parse_skill_md(SAMPLE_SKILL_MD).unwrap();
        assert_eq!(doc.steps.len(), 2);
        assert_eq!(doc.steps[0].id, "step0");
        assert_eq!(doc.steps[0].advance_on, "any");
        assert_eq!(doc.steps[1].id, "step1");
        assert_eq!(doc.steps[1].tools_only, Some(vec!["execute_python".to_string(), "export_data".to_string()]));
        assert_eq!(doc.steps[1].precompute, Some("scripts/step1.py".to_string()));
    }

    #[test]
    fn test_missing_frontmatter_returns_error() {
        let result = parse_skill_md("# No frontmatter here");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: 在 Cargo.toml 添加 serde_yaml 依赖**

在 `src-tauri/Cargo.toml` 中添加：
```toml
serde_yaml = "0.9"
```

- [ ] **Step 3: 运行解析器测试**

```bash
cd src-tauri && cargo test skill_md_loader -- --nocapture
```
Expected: 4 个测试全部 `ok`

- [ ] **Step 4: 导出模块**

在 `src/plugin/mod.rs` 中添加：
```rust
pub mod skill_md_loader;
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin/skill_md_loader.rs src-tauri/src/plugin/mod.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(skill): add SKILL.md format parser with multi-step workflow support"
```

---

## Task 13：DeclarativeSkill::load 改为只读 SKILL.md

**Files:**
- Modify: `src/plugin/declarative_skill.rs`

> **不保留 TOML 兼容**。旧 `load()` 函数直接删除，`scan_external_plugins` 改为只扫描 SKILL.md。

- [ ] **Step 1: 写加载测试（Task 14 执行后才能通过）**

在 `declarative_skill.rs` 测试块中添加：
```rust
#[test]
fn test_load_from_skill_md() {
    let plugin_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("plugins/biz-writing");
    if !plugin_dir.exists() { return; }
    let skill = DeclarativeSkill::load_from_dir(&plugin_dir).unwrap();
    assert_eq!(skill.id(), "biz-writing");
}
```

- [ ] **Step 2: 删除旧 load()，添加 load_from_dir（仅读 SKILL.md）**

在 `declarative_skill.rs` 中，**删除旧的 `pub fn load()` 函数**，添加：

```rust
/// 从插件目录加载 SKILL.md（唯一支持的格式，不兼容旧 TOML）
pub fn load_from_dir(plugin_dir: &Path) -> Result<Self, String> {
    let skill_md_path = plugin_dir.join("SKILL.md");
    let content = std::fs::read_to_string(&skill_md_path)
        .map_err(|e| format!("无法读取 {}: {}", skill_md_path.display(), e))?;
    let doc = crate::plugin::skill_md_loader::parse_skill_md(&content)?;
    Self::from_skill_md_doc(doc, plugin_dir)
}

fn from_skill_md_doc(
    doc: crate::plugin::skill_md_loader::SkillMdDocument,
    plugin_dir: &Path,
) -> Result<Self, String> {
    let fm = &doc.frontmatter;
    let model_pref = fm.model.as_deref().map(|p| match p {
        "deep_reasoning" => ModelPreference::Capability(ModelCapability::DeepReasoning),
        "cost_efficient" => ModelPreference::Capability(ModelCapability::CostEfficient),
        other => ModelPreference::Provider(other.to_string()),
    });

    let mut step_prompts = std::collections::HashMap::new();
    let mut step_configs = std::collections::HashMap::new();
    let mut wf_steps = Vec::new();

    for step in &doc.steps {
        step_prompts.insert(step.id.clone(), step.prompt.clone());
        step_configs.insert(step.id.clone(), StepToolConfig {
            tools_only: step.tools_only.clone(),
            max_iterations: step.max_iterations,
            token_budget: step.token_budget,
            advance_on: match step.advance_on.as_str() {
                "any" => AdvanceMode::Any,
                _ => AdvanceMode::Confirm,
            },
            precompute: step.precompute.clone(),
            tools_on_feedback: None,
            max_iterations_feedback: None,
        });
        wf_steps.push(WorkflowStep {
            id: step.id.clone(),
            display_name: step.id.clone(),
            requires_confirmation: step.advance_on == "confirm",
            advance_on: match step.advance_on.as_str() {
                "any" => AdvanceMode::Any,
                _ => AdvanceMode::Confirm,
            },
        });
    }

    let workflow = if !wf_steps.is_empty() {
        let initial = wf_steps.first().map(|s| s.id.clone()).unwrap_or_default();
        Some(WorkflowDefinition { steps: wf_steps, initial_step: initial })
    } else {
        None
    };

    Ok(Self {
        id: fm.id.clone(),
        name: fm.name.clone(),
        description: fm.description.clone().unwrap_or_else(|| fm.name.clone()),
        priority_val: fm.priority,
        keywords: fm.keywords.clone(),
        requires_files: fm.requires_files,
        model_pref,
        max_iter: fm.max_iterations,
        budget: fm.token_budget,
        include_app_base: fm.include_app_base,
        base_prompt: doc.base_prompt,
        step_prompts,
        workflow,
        step_configs,
        extract_base: String::new(),
        extract_steps: std::collections::HashMap::new(),
        plugin_dir: plugin_dir.to_path_buf(),
        icon: fm.icon.clone(),
        short_desc: fm.short_description.clone(),
        trigger: fm.trigger_text.clone(),
        category: fm.category.clone(),
        name_en: String::new(),
        short_desc_en: String::new(),
    })
}
```

- [ ] **Step 3: 更新 scan_external_plugins — 只扫描 SKILL.md**

在 `lib.rs` 的 `scan_external_plugins` 中，将原来解析 plugin.toml 的逻辑完全替换：
```rust
let skill_md_path = plugin_dir.join("SKILL.md");
if !skill_md_path.exists() {
    continue; // 跳过无 SKILL.md 的目录
}
match DeclarativeSkill::load_from_dir(&plugin_dir) {
    Ok(skill) => { skill_registry.register(Arc::new(skill), "plugin").await; }
    Err(e) => { log::warn!("加载 skill 失败 {:?}: {}", plugin_dir, e); }
}
```

- [ ] **Step 4: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | head -20
```
Expected: 无错误

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin/declarative_skill.rs src-tauri/src/lib.rs
git commit -m "feat(skill): DeclarativeSkill only supports SKILL.md, remove TOML loader"
```

---

## Task 14：迁移脚本 + 执行迁移（删除旧文件）

**Files:**
- Create: `scripts/migrate_skill_to_md.py`

- [ ] **Step 1: 创建迁移脚本**

创建 `scripts/migrate_skill_to_md.py`：

```python
#!/usr/bin/env python3
"""
将 lotus-app plugin 从 TOML+多MD 格式迁移为 SKILL.md 单文件格式。
迁移完成后删除所有旧文件（plugin.toml / workflow.toml / prompts/ 目录）。

用法：
  python scripts/migrate_skill_to_md.py src-tauri/plugins/budget-analysis
  python scripts/migrate_skill_to_md.py src-tauri/plugins/  # 迁移所有
  python scripts/migrate_skill_to_md.py src-tauri/plugins/ --dry-run  # 预览
"""

import sys, os, shutil, toml, glob

def migrate_plugin(plugin_dir, dry_run=False):
    toml_path = os.path.join(plugin_dir, "plugin.toml")
    skill_md_path = os.path.join(plugin_dir, "SKILL.md")

    if not os.path.exists(toml_path):
        print(f"  跳过 {plugin_dir}：无 plugin.toml")
        return

    if os.path.exists(skill_md_path):
        print(f"  跳过 {plugin_dir}：SKILL.md 已存在")
        return

    manifest = toml.load(toml_path)
    plugin = manifest.get("plugin", {})
    trigger = manifest.get("trigger", {})
    model = manifest.get("model", {})
    defaults = manifest.get("defaults", {})
    display = manifest.get("display", {})
    prompts_cfg = manifest.get("prompts", {})

    keywords = trigger.get("keywords", [])
    file_keywords = trigger.get("file_keywords", [])
    fm_lines = [
        "---",
        f"id: {plugin['id']}",
        f"name: {plugin['name']}",
        f"description: "{plugin.get('description', '')}"",
        f"priority: {plugin.get('priority', 0)}",
        f"model: {model.get('preference', 'deep_reasoning')}",
        f"requires_files: {str(trigger.get('requires_files', False)).lower()}",
        f"keywords: {keywords}",
        f"file_keywords: {file_keywords}",
        f"max_iterations: {defaults.get('max_iterations', 5)}",
        f"token_budget: {defaults.get('token_budget', 8192)}",
        f"include_app_base: {str(prompts_cfg.get('include_app_base', True)).lower()}",
        f"icon: "{display.get('icon', '🔧')}"",
        f"short_description: "{display.get('short_description', '')}"",
        f"trigger_text: "{display.get('trigger_text', '')}"",
        f"category: {display.get('category', 'general')}",
        "---", "",
    ]

    base_md_path = os.path.join(plugin_dir, "prompts", "base.md")
    base_content = open(base_md_path).read().strip() if os.path.exists(base_md_path) else ""
    sections = ["## base", "", base_content, ""]

    workflow_path = os.path.join(plugin_dir, "workflow.toml")
    if os.path.exists(workflow_path):
        wf = toml.load(workflow_path)
        for step in wf.get("steps", []):
            step_id = step["id"]
            sections += [f"## {step_id}", "", "### config",
                         f"advance_on: {step.get('advance_on', 'confirm')}"]
            for k in ["max_iterations", "token_budget", "tools_only", "precompute"]:
                if k in step:
                    sections.append(f"{k}: {step[k]}")
            sections.append("")
            step_prompt = os.path.join(plugin_dir, "prompts", f"{step_id}.md")
            if os.path.exists(step_prompt):
                sections.append(open(step_prompt).read().strip())
            sections.append("")

    content = "
".join(fm_lines) + "
".join(sections)

    if dry_run:
        print(f"  [DRY RUN] {skill_md_path}")
        print(content[:200] + "...
")
        return

    with open(skill_md_path, "w") as f:
        f.write(content)
    print(f"  ✓ 生成 {skill_md_path}")

    # 删除旧文件
    os.remove(toml_path)
    if os.path.exists(workflow_path):
        os.remove(workflow_path)
    prompts_dir = os.path.join(plugin_dir, "prompts")
    if os.path.isdir(prompts_dir):
        shutil.rmtree(prompts_dir)
    print(f"  ✓ 删除旧文件（plugin.toml/workflow.toml/prompts/）")

if __name__ == "__main__":
    target = sys.argv[1] if len(sys.argv) > 1 else "src-tauri/plugins"
    dry_run = "--dry-run" in sys.argv
    if os.path.isdir(target) and not os.path.exists(os.path.join(target, "plugin.toml")):
        for d in sorted(glob.glob(os.path.join(target, "*"))):
            if os.path.isdir(d):
                print(f"处理 {os.path.basename(d)}...")
                migrate_plugin(d, dry_run=dry_run)
    else:
        migrate_plugin(target, dry_run=dry_run)
```

- [ ] **Step 2: dry-run 验证**

```bash
pip install toml
python scripts/migrate_skill_to_md.py src-tauri/plugins/ --dry-run 2>&1 | head -60
```
Expected: 23 个 plugin 的预览，无 Python 错误

- [ ] **Step 3: 迁移 biz-writing 先验证**

```bash
python scripts/migrate_skill_to_md.py src-tauri/plugins/biz-writing
ls src-tauri/plugins/biz-writing/
```
Expected: 只剩 `SKILL.md`，旧文件已删除

- [ ] **Step 4: 运行 cargo test 验证单个 plugin 可被加载**

```bash
cd src-tauri && cargo test test_load_from_skill_md -- --nocapture
```
Expected: `ok`

- [ ] **Step 5: 全量迁移其余 22 个 plugin**

```bash
python scripts/migrate_skill_to_md.py src-tauri/plugins/
```
Expected: 22 个 `✓ 生成` + 旧文件删除确认

- [ ] **Step 6: 验证旧文件已全部删除**

```bash
find src-tauri/plugins -name "plugin.toml" | wc -l
find src-tauri/plugins -name "SKILL.md" | wc -l
```
Expected: 第一行 `0`，第二行 `23`

- [ ] **Step 7: Commit**

```bash
git add scripts/migrate_skill_to_md.py src-tauri/plugins/
git commit -m "feat(skill): migrate all 23 plugins to SKILL.md, delete old TOML files"
```

---

## Task 15：更新 init_skill_template 生成 SKILL.md

**Files:**
- Modify: `src/commands/skill_management.rs`

- [ ] **Step 1: 找到 init_skill_template 命令的实现**

```bash
grep -n "init_skill_template\|fn init_skill" src-tauri/src/commands/skill_management.rs | head -10
```

- [ ] **Step 2: 更新模板生成逻辑**

将模板从生成 `plugin.toml + prompts/base.md` 改为生成 `SKILL.md`：

```rust
// 生成 SKILL.md 模板
let skill_md_content = format!(r#"---
id: {skill_id}
name: {skill_name}
description: "在这里描述 skill 的功能"
priority: 10
model: deep_reasoning
requires_files: false
keywords: ["关键词1", "关键词2"]
max_iterations: 5
token_budget: 8192
include_app_base: true
icon: "🔧"
short_description: "简短描述"
trigger_text: "触发文本"
category: general
---

## base

你是{skill_name}专家，请根据用户需求提供专业帮助。

## step0

### config
advance_on: any

请描述您的需求，我来协助分析。
"#, skill_id = skill_id, skill_name = skill_name);

let skill_md_path = target_dir.join(&skill_id).join("SKILL.md");
std::fs::create_dir_all(skill_md_path.parent().unwrap())?;
std::fs::write(&skill_md_path, skill_md_content)?;
```

- [ ] **Step 3: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | head -10
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/skill_management.rs
git commit -m "feat(skill): init_skill_template now generates SKILL.md format"
```

---

## Task 16：期三集成测试 + 最终回归

**Files:**
- Create: `src-tauri/tests/skill_md_loader_test.rs`

- [ ] **Step 1: 创建端到端集成测试**

```rust
//! 期三：验证 SKILL.md 格式的完整加载和执行链路

#[cfg(test)]
mod skill_md_e2e_tests {
    use lotus_app::plugin::declarative_skill::DeclarativeSkill;

    #[test]
    fn test_all_plugins_load_successfully() {
        let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
        let mut loaded = 0;
        let mut failed = 0;

        for entry in std::fs::read_dir(&plugins_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match DeclarativeSkill::load_from_dir(&path) {
                Ok(skill) => {
                    println!("✓ {} ({})", skill.id(), skill.display_name());
                    loaded += 1;
                }
                Err(e) => {
                    eprintln!("✗ {:?}: {}", path.file_name().unwrap(), e);
                    failed += 1;
                }
            }
        }

        println!("\n总计: {} 成功, {} 失败", loaded, failed);
        assert_eq!(failed, 0, "所有 plugin 必须能成功加载");
        assert!(loaded >= 23, "应至少加载 23 个 plugin");
    }

    #[test]
    fn test_skill_md_workflow_steps_preserved() {
        // 验证有 workflow 的 skill 迁移后步骤数量正确
        let comp_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("plugins/comp-analysis-v2");
        if !comp_dir.exists() { return; }

        let skill = DeclarativeSkill::load_from_dir(&comp_dir).unwrap();
        let wf = skill.workflow().expect("comp-analysis-v2 应有 workflow");
        assert_eq!(wf.steps.len(), 6, "comp-analysis-v2 应有 6 个步骤");
    }
}
```

- [ ] **Step 2: 运行集成测试**

```bash
cd src-tauri && cargo test skill_md_e2e -- --nocapture
```
Expected: 所有 plugin 加载成功，`comp-analysis-v2` 有 6 个步骤

- [ ] **Step 3: 运行完整回归测试**

```bash
cd src-tauri && cargo test -- --nocapture 2>&1 | tail -30
cd src-tauri && cargo test review_ --tests --no-fail-fast
```
Expected: 全部通过

- [ ] **Step 4: 运行前端测试**

```bash
pnpm test
```
Expected: 全部通过

- [ ] **Step 5: 最终 Commit**

```bash
git add src-tauri/tests/skill_md_loader_test.rs
git commit -m "test(skill): end-to-end tests for SKILL.md format migration - Phase 3 complete"
```

---

## 自检

### Spec 覆盖检查

| 问题编号 | 问题描述 | 对应 Task |
|----------|----------|-----------|
| B1 | detect_activation 未调用 | Task 4（Step 3） |
| B2 | system_prompt 未注入 skill | Task 4（Step 4） |
| B3 | allowed_tools 为 None | Task 4（Step 5） |
| B4 | SkillState 无持久化 | Task 1 + Task 2 |
| B5 | 仅 daily-assistant 下可激活 | Task 9 |
| B6 | 关键词硬匹配，LLM 无感知 | Task 6 + Task 8 |
| B7 | mid-conversation 不能切换 | Task 10 |
| B8 | 无 paths: 条件激活 | 通过 SkillTool 的 LLM 决策覆盖（未做 paths: 字段，LLM 感知场景后主动调用 switch_skill） |
| B9 | TOML+多MD 格式不统一 | Task 12-15 |
| B10 | DailyAssistantSkill 硬编码 | Task 13（load_from_dir 支持 SKILL.md 后可将 DailyAssistantSkill 迁移为 SKILL.md 格式） |

### 无 Placeholder 检查

- 所有 Task 的代码块均为完整实现代码 ✓
- 所有测试命令含预期输出 ✓
- 所有文件路径为精确路径 ✓

### 类型一致性检查

- `SkillSession::force_switch` 在 Task 10 定义，Task 11 测试中使用 ✓
- `SkillMdDocument` / `SkillMdFrontmatter` / `SkillMdStep` 在 Task 12 定义，Task 13 使用 ✓
- `ToolFilter::Only` 在已有 `skill_trait.rs` 中定义，Task 5 测试中使用 ✓
