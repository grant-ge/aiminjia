# Prompt 架构一次性修复 — 实施 Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 lotus-app prompt 装配链路上所有已知的"已实现但被废 / 已设计未接通 / 已声称未做对"的问题一次性修完，未实现功能加 TODO 留口子。

**Architecture:** PromptAssembler 输出真正进入 LLM 请求；DAILY_ALLOWED_TOOLS 双轨拆开（schema 过滤 vs 运行时权限）；PromptCachePolicy 驱动 wire format（Claude 多块化 + OpenAI content 数组）；4 个内置 subagent 各自独立人格 prompt；coordinator 等未实现项加 TODO。

**Tech Stack:** Rust 2021 / Tokio / Tauri 2.x / serde / async_trait

**关联文档：**
- Spec: `docs/superpowers/specs/2026-05-08-prompt-architecture-fixes-design.md`
- 调研: `docs/2026-05-08-claude-code-system-prompt-comparison.md`
- 项目约定: `CLAUDE.md`

**关键路径更正（与 spec 不同）：**
- `chat_runtime_impl.rs` 实际路径是 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`（spec §2.2 写错了，本 plan 用真实路径）
- `build_visible_tool_defs` 签名是 `pub(crate) async fn`，不是 `pub fn`

**总工时估算：** 6 天。分 4 波。每波结束有手动验证关卡。

---

## 背景：为什么要做这件事

**症状**：lotus-app 的 system prompt 设计文档声称有完整分层（base / tool_preference / memory_mechanics / persona / daily），但调研发现真实发给 LLM 的 system prompt 只有 100 字符的 `DAILY_BASE_PROMPT`——5 块 prompt 中**只有 4 条简短规则真正生效**。

**根因**：`chat.rs:1394` 一行代码 `system_prompt: Some(DAILY_BASE_PROMPT.into())` 让 driver 在 `chat_turn_driver.rs:1082-1093` 用 `unwrap_or_else` 永远拿到这个极简版，PromptAssembler 装配出的完整产物被直接抛弃。

**附带问题**（5 个并行 subagent 调研发现）：
1. 工具白名单 `DAILY_ALLOWED_TOOLS` 17 个，但生产路径是死代码——主对话实际看到 30+ 工具
2. `PromptCachePolicy::{StaticPrefix, SessionDynamic, Volatile}` 标注了 cache 语义，但 provider wire format 完全忽略这个标注
3. 4 个内置 subagent prompt 全是 1 行英文或空字符串
4. `EmployeeRecord.systemPromptExtra` 字段名误导（实际进入用户消息，不是 system prompt）
5. `runtime/agent/team.rs` 是 5 行 stub，coordinator 概念从未落地

**完整分析**见 `docs/2026-05-08-claude-code-system-prompt-comparison.md`。

---

## 架构总览：改前 vs 改后

### 当前（问题状态）

```
PromptAssembler.build_system_prompt()
  → 装配出 [base, tool_preference, memory_mechanics, persona, daily] 5 块
  ↓
load_turn_config_overrides()
  → 永远返回 Some(DAILY_BASE_PROMPT)  ← P0 病灶
  ↓
driver: effective = override.unwrap_or(snapshot)
  → 用极简版覆盖完整版，PromptAssembler 产物被抛弃
  ↓
gateway 把单字符串扔给 provider
  → Claude 整体打一个 cache_control（粒度太粗）
  → OpenAI 兼容端点 flatten 成单字符串（cache_policy 标注完全失效）
```

### 改后（目标状态）

```
PromptAssembler.build_system_prompt()
  → 5 块 PromptBlock，每块带 cache_policy 标注
  ↓
load_turn_config_overrides()
  → system_prompt: None（不再覆盖）
  → schema_filter: Daily/Employee/None（明确语义）
  → runtime_allowed_tools: 独立计算（双轨拆开）
  ↓
driver: effective = snapshot（PromptAssembler 真正生效）
  ↓
gateway 接收 PromptSystemView（多块视图）
  ↓
provider 按 cache_policy 输出多块 wire body
  → Claude: system 是数组，static 块带 cache_control
  → OpenAI: content 是数组（capability 不支持就降级）
```

### 4 个 Subagent 改前 vs 改后

| Agent | 改前 system_prompt | 改后 |
|---|---|---|
| `general-purpose` | 1 行英文 | 200 字独立中文人格（搜索/分析/输出格式） |
| `explore` | 1 行英文 | 300 字严格只读人格（明确禁止列表） |
| `browse_data_agent` | 空字符串 | 200 字数据提取专家（数据真实性约束） |
| `daily_assistant_agent` | 空字符串 | 200 字日常助手（专业资质边界） |

> 子代理**不**继承主对话 "AI小家" 身份，避免人格混淆。

---

## Claude Code 对照分析（我们抄什么、不抄什么）

### 借鉴

| Claude Code 概念 | 借鉴方式 | 对应到 lotus 哪里 |
|---|---|---|
| `SYSTEM_PROMPT_DYNAMIC_BOUNDARY`（静态/动态分界） | 借鉴思路，**用 `PromptCachePolicy` 枚举实现**而不是 marker 字符串 | `prompt/types.rs:19` PromptBlock.cache_policy |
| `buildSystemPromptBlocks()`（多块输出 + cache_control） | 直接对标 | `claude.rs::build_request_body_from_view`（Wave 2 新增）|
| `splitSysPromptPrefix()`（按 boundary 分 cache scope） | 简化借鉴：只用 ephemeral 一种 scope | OpenAI renderer + Claude provider |
| 子代理独立人格（`generalPurposeAgent.ts` 等几百行 prompt） | **翻译版骨架 + 本地原创补丁**（spec §6 / §6.A） | Wave 4 替换 4 个 builtin agent prompt |
| `enhanceSystemPromptWithEnvDetails`（追加 env） | lotus 已有（`spawn_subagent.rs::build_env_info`），保留 | 不修改 |

### 不抄

| Claude Code 做法 | 不抄的原因 | lotus 决策 |
|---|---|---|
| CLAUDE.md / `.claude/` / AGENTS.md 加载 | 用户决策"不引入兼容层"，减少多源指令冲突 | Wave 4 加注释明确"维持 AGENT.md 单一命名" |
| Coordinator 多 worker 调度 | 当前没有真实业务需要 | Wave 4 仅 TODO 留口子，`team.rs` 保持 stub |
| 多种 cache scope（global / org / 默认） | 用量没到那个级别，简化为单 ephemeral 即可 | 不引入 |
| 子代理英文长 prompt（claude-code-best 是英文） | 我们是中文产品 | 翻译为中文 + 适配本地工具名 |

---

## 4 波改造一目了然

| Wave | 解决什么问题 | 改的核心文件 | 工时 | 验证方式 |
|---|---|---|---|---|
| **Wave 1** | P0 一行废掉的 prompt 恢复 + 工具白名单双轨拆开 + token 估算 | `chat.rs`, `chat_runtime_impl.rs`, `chat_turn_driver.rs` | 1 天 | 端到端测试 + employee 派活手动验证 |
| **Wave 2** | `PromptCachePolicy` 真正驱动 provider wire format | `renderer_openai.rs`, `providers/claude.rs` | 1.5 天 | wire body snapshot 测试 |
| **Wave 3** | gateway 接口升级到 `PromptSystemView`（3 个调用点） | `gateway.rs`, `chat.rs:538`, `worker_runtime.rs:303` | 1.5 天 | Claude subagent 调用不再 400 |
| **Wave 4** | 4 个 subagent 独立人格 + 3 处 TODO 注释 | `agent/builtin/*.rs`, `team.rs`, `dispatch_prompt.rs`, `renlijia_md.rs` | 2 天 | persona 测试 + 4 场景手动验证 |

**为什么这个顺序**：
- Wave 1 最先：P0 是其他 wave 的前提（不修这个 wave 2/3 就算改好了也看不到效果）
- Wave 2 在 Wave 3 之前：先让 provider 准备好接收多块视图，再升级 gateway 接口送进去
- Wave 4 最后：subagent 人格依赖 Wave 3 的 view 接口（`worker_runtime.rs:303` 升级到 view 后才能让 cache 生效）

---

## 怎么读这个 Plan

下面 4 波每一波都是一组 Task，每个 Task 又拆成 5-7 个 Step。**Step 粒度故意做得很细**（每步 2-5 分钟一个动作），是为了让 AI subagent 执行时能逐步推进，每步都能验证。

**人类读者可以跳过 Step 细节**，只看：
- 每个 Task 开头的「Files」清单（这次改哪个文件）
- 每个 Task 结尾的「Expected」（验证标准）
- 每个 Wave 末尾的 commit message（这一波做了什么）

**AI executor 必须按 Step 顺序执行**，包括"跑测试看失败"这种步骤——这是 TDD 节奏的一部分，不是冗余。

---

## Wave 1: P0 + 工具白名单双职责拆分（1 天）

### Task 1.1: 新增 `ToolSchemaFilter` 枚举与 schema 过滤逻辑

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:30-55`

- [ ] **Step 1: 写失败测试**

新增文件 `src-tauri/tests/tool_schema_filter_test.rs`：

```rust
use app_lib::transport::tauri_commands::chat::chat_runtime_impl::{
    build_visible_tool_defs, ToolSchemaFilter,
};
use std::collections::HashSet;

#[tokio::test]
async fn daily_filter_excludes_tools_not_in_whitelist() {
    let registry = make_test_registry_with_tools(&[
        "search_memory", "read_workspace_file", "obscure_tool_not_in_daily",
    ]);
    let defs = build_visible_tool_defs(
        &registry,
        true,
        ToolSchemaFilter::DailyWhitelist,
    ).await;
    let names: HashSet<_> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains("search_memory"));
    assert!(!names.contains("obscure_tool_not_in_daily"));
}

#[tokio::test]
async fn employee_filter_uses_employee_whitelist_only() {
    let registry = make_test_registry_with_tools(&[
        "search_memory", "browse_navigate", "extract_table_data",
    ]);
    let mut employee_set = HashSet::new();
    employee_set.insert("browse_navigate".to_string());
    employee_set.insert("extract_table_data".to_string());
    let defs = build_visible_tool_defs(
        &registry,
        true,
        ToolSchemaFilter::EmployeeWhitelist(employee_set),
    ).await;
    let names: HashSet<_> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(!names.contains("search_memory"),
        "employee path must NOT leak daily-only tools");
    assert!(names.contains("browse_navigate"));
    assert!(names.contains("extract_table_data"));
}

#[tokio::test]
async fn no_filter_returns_full_set() {
    let registry = make_test_registry_with_tools(&["a", "b", "c"]);
    let defs = build_visible_tool_defs(&registry, true, ToolSchemaFilter::None).await;
    assert_eq!(defs.len(), 3);
}

fn make_test_registry_with_tools(names: &[&str]) -> app_lib::plugin::registry::ToolRegistry {
    // 用现有 ToolRegistry::new 加上 mock 工具注册的最小 helper
    // 具体实现参考 src-tauri/tests/ 里现有的 registry test fixture
    todo!("see existing tests/*.rs for ToolRegistry test fixtures")
}
```

> **注意 todo!()**：测试 fixture helper 实现需要参考已有测试，不要把 `todo!()` 留到 commit。Step 2 跑前必须填好。

- [ ] **Step 2: 跑测试验证失败**

```bash
cd src-tauri && cargo test --test tool_schema_filter_test 2>&1 | tail -20
```
Expected: 编译失败，提示 `ToolSchemaFilter` 不存在。

- [ ] **Step 3: 实现 `ToolSchemaFilter` 枚举 + 改 `build_visible_tool_defs` 签名**

修改 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs:30-55`：

```rust
/// 决定 schema 过滤策略。和"运行时权限白名单"是两回事——
/// 后者由 TurnConfigOverrides.allowed_tools 控制，进入 tool_round_driver。
#[derive(Debug, Clone)]
pub enum ToolSchemaFilter {
    /// 普通对话：用 DAILY_ALLOWED_TOOLS 白名单过滤
    DailyWhitelist,
    /// Employee 派活：用员工自定义白名单过滤
    EmployeeWhitelist(std::collections::HashSet<String>),
    /// 无过滤（subagent 路径或显式全量）
    None,
}

pub(crate) async fn build_visible_tool_defs(
    registry: &ToolRegistry,
    has_authorized_workspace: bool,
    schema_filter: ToolSchemaFilter,
) -> Vec<crate::llm::streaming::ToolDefinition> {
    let defs = if has_authorized_workspace {
        registry.get_schemas_filtered(&ToolFilter::All).await
    } else {
        registry
            .get_schemas_filtered(&ToolFilter::Exclude(
                WORKSPACE_TOOL_NAMES.iter().map(|s| s.to_string()).collect(),
            ))
            .await
    };

    match schema_filter {
        ToolSchemaFilter::DailyWhitelist => {
            let allowed: std::collections::HashSet<&str> =
                crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS
                    .iter()
                    .copied()
                    .collect();
            defs.into_iter()
                .filter(|d| allowed.contains(d.name.as_str()))
                .collect()
        }
        ToolSchemaFilter::EmployeeWhitelist(allowed) => defs
            .into_iter()
            .filter(|d| allowed.contains(d.name.as_str()))
            .collect(),
        ToolSchemaFilter::None => defs,
    }
}
```

- [ ] **Step 4: 跑测试验证通过**

```bash
cd src-tauri && cargo test --test tool_schema_filter_test 2>&1 | tail -20
```
Expected: 3 个测试 PASS。

- [ ] **Step 5: 编译检查全仓库**

```bash
cd src-tauri && cargo check 2>&1 | tail -20
```
Expected: 编译失败，提示 `chat.rs:1383` 调用 `build_visible_tool_defs` 参数类型不匹配（这是预期的，下一个 Task 会修）。

**不 commit**——Wave 1 收尾时统一 commit。

---

### Task 1.2: 修 `chat.rs::load_turn_config_overrides` 调用点 + 取消 DAILY_BASE_PROMPT 覆盖

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:1343-1402`

- [ ] **Step 1: 读现状代码确认行号**

```bash
sed -n '1343,1402p' src-tauri/src/transport/tauri_commands/chat.rs
```
Expected: 确认 `load_turn_config_overrides` 函数在 1343-1402 区间，包含 1394 行 `system_prompt: Some(...)`、1383 行 `build_visible_tool_defs(..., Some(&allowed_tools))`、1397 行 `allowed_tools: Some(allowed_tools)`。

- [ ] **Step 2: 替换函数体**

`src-tauri/src/transport/tauri_commands/chat.rs:1343-1402`，把整个 `load_turn_config_overrides` 函数体替换为：

```rust
async fn load_turn_config_overrides(
    &self,
    request: &ChatTurnRequest,
) -> Result<TurnConfigOverrides, TurnError> {
    let employee_overrides = self
        .services
        .employee_run_overrides
        .lock()
        .ok()
        .and_then(|map| map.get(request.conversation_id.as_str()).cloned());

    // 第一步：决定 schema 过滤策略
    let schema_filter = match &employee_overrides {
        Some(ov) if !ov.tool_whitelist.is_empty() => {
            chat_runtime_impl::ToolSchemaFilter::EmployeeWhitelist(
                ov.tool_whitelist.iter().cloned().collect(),
            )
        }
        _ => chat_runtime_impl::ToolSchemaFilter::DailyWhitelist,
    };

    // 第二步：独立计算运行时权限白名单（与 schema 过滤是两回事）
    let runtime_allowed_tools: std::collections::HashSet<String> =
        match &employee_overrides {
            Some(ov) if !ov.tool_whitelist.is_empty() => {
                ov.tool_whitelist.iter().cloned().collect()
            }
            _ => crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };

    let max_iterations = employee_overrides
        .as_ref()
        .map(|ov| ov.max_iterations)
        .unwrap_or(30);

    let authorized_workspace = chat_runtime_impl::load_authorized_workspace(
        &self.services.app,
        request.conversation_id.as_str(),
    );
    let visible_tool_defs = chat_runtime_impl::build_visible_tool_defs(
        self.services.tool_registry.as_ref(),
        authorized_workspace.is_some(),
        schema_filter,
    )
    .await;
    let json_defs = visible_tool_defs
        .into_iter()
        .filter_map(|td| serde_json::to_value(&td).ok())
        .collect();

    Ok(TurnConfigOverrides {
        system_prompt: None, // P0 修复：让 PromptAssembler 产物真正进入 LLM
        tool_defs: Some(json_defs),
        allowed_tools: Some(runtime_allowed_tools),
        max_iterations: Some(max_iterations),
        token_budget: None,
    })
}
```

- [ ] **Step 3: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```
Expected: 编译通过（或仅剩 dead_code / unused warning）。

- [ ] **Step 4: 删除已无引���的 `chat.rs::get_tool_defs` impl**

```bash
grep -n "fn get_tool_defs" src-tauri/src/transport/tauri_commands/chat.rs
```
若确认 1308 区域有 `async fn get_tool_defs` impl 块（约 30 行），删除整个 impl 块。

- [ ] **Step 5: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```
Expected: 编译通过。

---

### Task 1.3: 删除 `RuntimeLlmExecutor::get_tool_defs` trait 默认实现

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:251-257`

- [ ] **Step 1: 确认 trait 默认 impl 位置**

```bash
sed -n '248,260p' src-tauri/src/runtime/chat/chat_turn_driver.rs
```
Expected: 看到 `async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> { Ok(vec![]) }`。

- [ ] **Step 2: 删除默认实现，改为必须 override**

把 `chat_turn_driver.rs:251-257` 区域改成：

```rust
/// 返回本次 Turn 使用的 tool definitions（JSON schema）。
///
/// 不再提供默认实现——所有 mock executor 必须显式 override，
/// 否则会因为返回空 vec 让测试静默通过。
async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError>;
```

- [ ] **Step 3: 编译检查，找出所有需要补 override 的 mock**

```bash
cd src-tauri && cargo check --tests 2>&1 | grep -E "not all trait items|missing.*get_tool_defs" | head -20
```
Expected: 列出所有未实现 `get_tool_defs` 的测试 mock executor。

- [ ] **Step 4: 为每个 mock 补 override**

对 Step 3 列出的每个文件，找到 `impl RuntimeLlmExecutor for ...` 块，补：

```rust
async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
    Ok(vec![])  // 显式声明此 mock 不关心 tool_defs
}
```

> 重要：每个 mock 都要写这一行，**不能**留默认实现。`s4_driver_loop_test.rs:885` 的 `ToolDefsCapturingExecutor` 例外——它会在 Task 1.5 重做。

- [ ] **Step 5: 跑全部测试编译，不跑（先收尾本 task）**

```bash
cd src-tauri && cargo test --no-run 2>&1 | tail -10
```
Expected: 编译通过。

---

### Task 1.4: 修复 token 估算（主对话）

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:1394` 附近

- [ ] **Step 1: 找到当前估算逻辑**

```bash
grep -n "estimate_tokens_from_json\|estimated_input_tokens" src-tauri/src/runtime/chat/chat_turn_driver.rs | head -10
```

- [ ] **Step 2: 替换为分项估算**

把单一的 `estimate_tokens_from_json(&state.messages)` 调用替换为：

```rust
let system_chars = config.system_prompt.len();
let dynamic_chars = state
    .dynamic_context
    .as_deref()
    .map(|s| s.len())
    .unwrap_or(0);
let messages_chars = serde_json::to_string(&state.messages)
    .map(|s| s.len())
    .unwrap_or(0);
let tools_chars = serde_json::to_string(&config.tool_defs)
    .map(|s| s.len())
    .unwrap_or(0);
let estimated_input_tokens =
    (system_chars + dynamic_chars + messages_chars + tools_chars) / 4;

// 在诊断 emit 处分项记录
record_diagnostic(DiagnosticEvent {
    name: "turn.tokens.estimated".into(),
    source: DiagnosticSource::ChatRuntime,
    fields: serde_json::json!({
        "system_chars": system_chars,
        "dynamic_chars": dynamic_chars,
        "messages_chars": messages_chars,
        "tools_chars": tools_chars,
        "estimated_input_tokens": estimated_input_tokens,
    }),
});
```

> 实施时需要查找当前 `estimated_input_tokens` 的使用点，确保替换后所有读取者拿到的是新值。

- [ ] **Step 3: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```
Expected: 编译通过。

---

### Task 1.5: 端到端测试 — 验证 PromptAssembler 产物真正进入 LLM 请求

**Files:**
- Create: `src-tauri/tests/effective_system_prompt_test.rs`

- [ ] **Step 1: 写完整测试**

```rust
//! 验证 P0 修复：PromptAssembler 产出的完整 system prompt 真正进入 LLM 请求，
//! 不再被 DAILY_BASE_PROMPT 覆盖。

use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    TurnError,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;

struct CapturingExecutor {
    captured_system_prompt: Mutex<Option<String>>,
}

#[async_trait]
impl RuntimeLlmExecutor for CapturingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        *self.captured_system_prompt.lock().unwrap() = Some(input.system_prompt.to_string());
        Ok(LlmStepResult::ContentComplete {
            content: "ok".into(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".into()),
        })
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(PathBuf::from("/tmp/test-workspace"))
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg".into())
    }

    // build_prompt_snapshot 默认实现走 PromptAssembler；
    // 不 override，让真实分层逻辑生效。

    // load_turn_config_overrides 默认返回 TurnConfigOverrides::default()
    // → system_prompt: None → 使用 prompt_snapshot
}

#[tokio::test]
async fn p0_fix_effective_system_prompt_includes_base_md_content() {
    let executor = Arc::new(CapturingExecutor {
        captured_system_prompt: Mutex::new(None),
    });

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-p0");
    let mut turn = TurnState::new(mapping, RunId::new("test-run"), "hello".into());
    let request = ChatTurnRequest::new("conv-p0", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor
        .captured_system_prompt
        .lock()
        .unwrap()
        .clone()
        .expect("system_prompt must be captured");

    // 关键断言：base.md 的标志性内容必须在
    assert!(
        captured.contains("AI小家"),
        "system prompt must include base.md identity, got len={}",
        captured.len()
    );
    // tool_preference 标志
    assert!(
        captured.contains("工具") || captured.contains("tool"),
        "system prompt must include tool preference section"
    );
    // memory mechanics 标志
    assert!(
        captured.contains("记忆") || captured.contains("memory"),
        "system prompt must include memory mechanics section"
    );
    // 长度断言：极简 DAILY_BASE_PROMPT 约 100 字符；完整 prompt 应 ≥ 1000
    assert!(
        captured.len() >= 1000,
        "expected full PromptAssembler output (≥1000 chars), got {}",
        captured.len()
    );
}
```

- [ ] **Step 2: 跑测试**

```bash
cd src-tauri && cargo test --test effective_system_prompt_test -- --nocapture 2>&1 | tail -30
```
Expected: PASS。如果 FAIL 表示 P0 修复没生效，回查 Task 1.2。

---

### Task 1.6: 修订 `s4_driver_loop_test.rs` 工具白名单测试

**Files:**
- Modify: `src-tauri/tests/s4_driver_loop_test.rs:885-940`（具体行号实施时确认）

- [ ] **Step 1: 找到原测试**

```bash
grep -n "driver_s4_daily_tool_defs_match_whitelist\|ToolDefsCapturingExecutor" src-tauri/tests/s4_driver_loop_test.rs
```

- [ ] **Step 2: 修订测试，让它走真实路径**

把 `ToolDefsCapturingExecutor::get_tool_defs` mock 替换为：通过 mock executor 的 `load_turn_config_overrides` 返回 `Some(json_defs)`，json_defs 由实际调用 `build_visible_tool_defs(registry, true, ToolSchemaFilter::DailyWhitelist)` 生成。新增断言：`runtime_allowed_tools` 与 `DAILY_ALLOWED_TOOLS` 一致。

具体改动：
```rust
async fn load_turn_config_overrides(
    &self,
    _request: &ChatTurnRequest,
) -> Result<TurnConfigOverrides, TurnError> {
    let registry = self.test_registry.clone();
    let visible = build_visible_tool_defs(
        &registry, true, ToolSchemaFilter::DailyWhitelist,
    ).await;
    let json_defs: Vec<serde_json::Value> = visible
        .into_iter()
        .filter_map(|td| serde_json::to_value(&td).ok())
        .collect();
    let runtime_allowed: HashSet<String> = DAILY_ALLOWED_TOOLS
        .iter().map(|s| s.to_string()).collect();
    Ok(TurnConfigOverrides {
        system_prompt: None,
        tool_defs: Some(json_defs),
        allowed_tools: Some(runtime_allowed),
        max_iterations: Some(30),
        token_budget: None,
    })
}
```

- [ ] **Step 3: 跑测试**

```bash
cd src-tauri && cargo test --test s4_driver_loop_test driver_s4_daily_tool_defs_match_whitelist -- --nocapture 2>&1 | tail -20
```
Expected: PASS。

---

### Task 1.7: Wave 1 全量回归 + commit

- [ ] **Step 1: 跑 architecture review 测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```
Expected: 全部 PASS。

- [ ] **Step 2: 跑前端事件联调关键回归**

```bash
cd .. && pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -20
```
Expected: 全部 PASS（这些测试不依赖 prompt 内容，应该完全不受影响）。

- [ ] **Step 3: 全量 cargo test**

```bash
cd src-tauri && cargo test --all 2>&1 | tail -20
```
Expected: 全部 PASS。

- [ ] **Step 4: 手动验证 employee 派活仍受白名单限制**

启动 `pnpm tauri:dev`，触发任意一个员工（小招/小周/销售/市场分析），抓后端日志或加临时 `tracing::info!` 打印 `runtime_allowed_tools.len()`，确认 = employee whitelist size，不是 17（DAILY size）也不是全部工具数。

- [ ] **Step 5: commit Wave 1**

```bash
git add -A
git commit -m "feat(prompt): wave 1 - P0 fix + tool whitelist dual-track separation

- 取消 chat.rs:1394 对 DAILY_BASE_PROMPT 的强制覆盖，让 PromptAssembler 产物真正进入 LLM
- chat_runtime_impl.rs 新增 ToolSchemaFilter 枚举，明确 schema 过滤 vs 运行时权限白名单
- chat.rs::load_turn_config_overrides 重构：schema_filter 与 runtime_allowed_tools 独立计算
- 删除 chat.rs::get_tool_defs impl 与 trait 默认 impl（���免 mock 测试静默通过）
- chat_turn_driver.rs 修复 token 估算，分项记录 system/dynamic/messages/tools chars
- 新增 effective_system_prompt_test 端到端验证 PromptAssembler 产物真正生效
- 修订 s4_driver_loop_test 测试 mock 走真实 build_visible_tool_defs 路径

关联 spec: docs/superpowers/specs/2026-05-08-prompt-architecture-fixes-design.md (Wave 1)"
```

**Wave 1 验收点**：
- ✅ `effective_system_prompt_test` PASS
- ✅ `tool_schema_filter_test` PASS
- ✅ employee 派活手动验证工具白名单仍限制
- ✅ 所有 review_* 回归测试 PASS

---

## Wave 2: Provider wire format 多块化（1.5 天）

### Task 2.1: 改 `OpenAiChatPromptRenderer` 输出 content 数组

**Files:**
- Modify: `src-tauri/src/runtime/chat/prompt/renderer_openai.rs`

- [ ] **Step 1: 写失败测试**

新增 `src-tauri/tests/prompt_renderer_openai_test.rs`：

```rust
use app_lib::runtime::chat::prompt::{
    OpenAiChatPromptRenderer, PromptAssembly, PromptBlock, PromptCachePolicy, PromptSectionId,
};

#[test]
fn render_emits_content_array_with_static_block_cache_control() {
    let assembly = PromptAssembly::new(vec![
        PromptBlock::static_block(PromptSectionId::new("base"), "static content"),
        PromptBlock::dynamic_block(PromptSectionId::new("persona"), "dynamic content"),
        PromptBlock::volatile_block(
            PromptSectionId::new("env"),
            "volatile content",
            "test",
        ),
    ]);

    let msg = OpenAiChatPromptRenderer::render_system_message(&assembly)
        .expect("render must produce something");

    assert_eq!(msg["role"], "system");
    let content = msg["content"].as_array().expect("content should be array");
    assert!(content.len() >= 2, "should have at least 2 content blocks");
    // 第一个块应当是 static，带 cache_control
    let first = &content[0];
    assert_eq!(first["type"], "text");
    assert!(first["text"].as_str().unwrap().contains("static content"));
    assert_eq!(first["cache_control"]["type"], "ephemeral");
    // 找到包含 volatile 内容的块，应不带 cache_control
    let volatile_block = content
        .iter()
        .find(|b| b["text"].as_str().unwrap_or("").contains("volatile"))
        .expect("should find volatile block");
    assert!(volatile_block.get("cache_control").is_none(),
        "volatile block must NOT have cache_control");
}

#[test]
fn render_returns_none_for_empty_assembly() {
    let assembly = PromptAssembly::new(vec![]);
    let msg = OpenAiChatPromptRenderer::render_system_message(&assembly);
    assert!(msg.is_none());
}
```

- [ ] **Step 2: 跑测试验证失败**

```bash
cd src-tauri && cargo test --test prompt_renderer_openai_test 2>&1 | tail -20
```
Expected: FAIL（当前 renderer 返回单字符串）。

- [ ] **Step 3: 改 renderer 实现**

`src-tauri/src/runtime/chat/prompt/renderer_openai.rs` 整个文件替换为：

```rust
use super::{PromptAssembly, PromptCachePolicy};

pub struct OpenAiChatPromptRenderer;

impl OpenAiChatPromptRenderer {
    /// 输出 OpenAI 兼容的 system message：
    /// content 是一个数组，每个元素一个 PromptBlock，
    /// StaticPrefix / SessionDynamic 段带 cache_control，Volatile 不带。
    pub fn render_system_message(assembly: &PromptAssembly) -> Option<serde_json::Value> {
        let blocks: Vec<serde_json::Value> = assembly
            .blocks()
            .iter()
            .filter(|b| !b.text.trim().is_empty())
            .map(|b| {
                let mut item = serde_json::json!({
                    "type": "text",
                    "text": b.text,
                });
                match b.cache_policy {
                    PromptCachePolicy::StaticPrefix | PromptCachePolicy::SessionDynamic => {
                        item["cache_control"] =
                            serde_json::json!({ "type": "ephemeral" });
                    }
                    PromptCachePolicy::Volatile => {}
                }
                item
            })
            .collect();

        if blocks.is_empty() {
            return None;
        }
        Some(serde_json::json!({
            "role": "system",
            "content": blocks,
        }))
    }

    /// 兼容降级：某些 OpenAI 兼容端点不支持 content 数组形式。
    /// 调用方判断 provider capability 决定走哪个。
    pub fn render_system_message_flat(assembly: &PromptAssembly) -> Option<serde_json::Value> {
        let content = assembly.flatten();
        if content.trim().is_empty() {
            return None;
        }
        Some(serde_json::json!({ "role": "system", "content": content }))
    }
}
```

- [ ] **Step 4: 跑测试验证通过**

```bash
cd src-tauri && cargo test --test prompt_renderer_openai_test 2>&1 | tail -20
```
Expected: PASS。

- [ ] **Step 5: 全仓库编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```
Expected: 编译通过。

---

### Task 2.2: Provider capability 判断 + 降级 fallback

**Files:**
- Modify: `src-tauri/src/runtime/chat/prompt/types.rs:133-137`

- [ ] **Step 1: 找到 `openai_system_message` 调用方**

```bash
grep -rn "openai_system_message\|render_system_message" src-tauri/src --include="*.rs" | head -10
```

- [ ] **Step 2: 引入 capability 判断**

`src-tauri/src/runtime/chat/prompt/types.rs:133-137` 替换：

```rust
pub fn openai_system_message(&self) -> Option<serde_json::Value> {
    crate::runtime::chat::prompt::OpenAiChatPromptRenderer::render_system_message(
        &self.assembly,
    )
}

/// 降级版本：用于不支持 content 数组的 OpenAI 兼容端点。
pub fn openai_system_message_flat(&self) -> Option<serde_json::Value> {
    crate::runtime::chat::prompt::OpenAiChatPromptRenderer::render_system_message_flat(
        &self.assembly,
    )
}
```

> 调用方判断 `provider.supports_cache_control_in_content_array()` 决定走哪个。本 Task 暂不接调用方——Wave 3 gateway 升级时统一接入。

- [ ] **Step 3: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

---

### Task 2.3: Claude provider 多块化（核心）

**Files:**
- Modify: `src-tauri/src/llm/providers/claude.rs:194-260`

- [ ] **Step 1: 读现有 system 抽取与 cache_control 注入逻辑**

```bash
sed -n '180,260p' src-tauri/src/llm/providers/claude.rs
```
确认当前是把整个 system 字符串当成一个整体加 `cache_control: ephemeral`。

- [ ] **Step 2: 写失败测试**

新增 `src-tauri/tests/claude_provider_multi_block_test.rs`：

```rust
use app_lib::llm::providers::claude::ClaudeProvider;
use app_lib::runtime::chat::prompt::{
    PromptAssembly, PromptBlock, PromptCachePolicy, PromptSectionId, PromptSystemView,
};

#[test]
fn claude_emits_multi_block_system_with_cache_control_on_static() {
    let view = PromptSystemView {
        blocks: vec![
            PromptBlock::static_block(PromptSectionId::new("base"), "BASE_TEXT"),
            PromptBlock::static_block(PromptSectionId::new("tool_pref"), "TOOL_PREF_TEXT"),
            PromptBlock::dynamic_block(PromptSectionId::new("persona"), "PERSONA_TEXT"),
        ],
    };
    let body = ClaudeProvider::build_request_body_from_view(
        "claude-sonnet-4-6",
        &view,
        &[],   // tools
        &[],   // messages
        4096,
    );

    let system = body["system"].as_array().expect("system must be array");
    assert!(system.len() >= 2, "expected multi-block, got {}", system.len());
    // 第一个 block 含 cache_control
    assert_eq!(system[0]["type"], "text");
    assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
    // 各 block 的文本片段都在
    let all_text = serde_json::to_string(&system).unwrap();
    assert!(all_text.contains("BASE_TEXT"));
    assert!(all_text.contains("PERSONA_TEXT"));
}

#[test]
fn claude_falls_back_to_string_system_when_view_is_empty() {
    let view = PromptSystemView { blocks: vec![] };
    let body = ClaudeProvider::build_request_body_from_view(
        "claude-sonnet-4-6", &view, &[], &[], 4096,
    );
    // 空 view 应当不输出 system 字段，或输出空数组 — 由实现决定
    if let Some(system) = body.get("system") {
        if let Some(arr) = system.as_array() {
            assert!(arr.is_empty());
        }
    }
}
```

- [ ] **Step 3: 跑测试验证失败**

```bash
cd src-tauri && cargo test --test claude_provider_multi_block_test 2>&1 | tail -20
```
Expected: FAIL（`build_request_body_from_view` 不存在）。

- [ ] **Step 4: 实现 `build_request_body_from_view`**

`src-tauri/src/llm/providers/claude.rs` 新增 pub 方法（保留原有 `build_request_body` 不动作为兼容）：

```rust
pub fn build_request_body_from_view(
    model: &str,
    system_view: &PromptSystemView,
    tools: &[serde_json::Value],
    messages: &[serde_json::Value],
    max_tokens: usize,
) -> serde_json::Value {
    let system_blocks: Vec<serde_json::Value> = system_view
        .blocks
        .iter()
        .filter(|b| !b.text.trim().is_empty())
        .map(|b| {
            let mut item = serde_json::json!({
                "type": "text",
                "text": b.text,
            });
            match b.cache_policy {
                PromptCachePolicy::StaticPrefix | PromptCachePolicy::SessionDynamic => {
                    item["cache_control"] = serde_json::json!({ "type": "ephemeral" });
                }
                PromptCachePolicy::Volatile => {}
            }
            item
        })
        .collect();

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": messages,
    });

    if !system_blocks.is_empty() {
        body["system"] = serde_json::Value::Array(system_blocks);
    }

    if !tools.is_empty() {
        // 保留现有 tools cache_control 逻辑（最后一个 tool 加 cache_control）
        let mut tools_with_cache: Vec<serde_json::Value> = tools.to_vec();
        if let Some(last) = tools_with_cache.last_mut() {
            last["cache_control"] = serde_json::json!({ "type": "ephemeral" });
        }
        body["tools"] = serde_json::Value::Array(tools_with_cache);
    }

    body
}
```

- [ ] **Step 5: 跑测试验证通过**

```bash
cd src-tauri && cargo test --test claude_provider_multi_block_test 2>&1 | tail -20
```
Expected: PASS。

---

### Task 2.4: Wave 2 commit

- [ ] **Step 1: 跑 review_ + 全量测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -10
cd src-tauri && cargo test --all 2>&1 | tail -10
```
Expected: 全部 PASS。

- [ ] **Step 2: commit**

```bash
git add -A
git commit -m "feat(prompt): wave 2 - PromptCachePolicy drives provider wire format

- OpenAiChatPromptRenderer 输出 content 数组，static/session_dynamic 块带 cache_control
- 保留 render_system_message_flat 作为降级版本（capability 不支持时用）
- ClaudeProvider 新增 build_request_body_from_view，按 PromptCachePolicy 输出多块 system
- 测试覆盖：cache_wire_format snapshot

关联 spec: Wave 2"
```

---

## Wave 3: Gateway 接口升级（1.5 天）

### Task 3.1: gateway 新增 `_with_view` 重载

**Files:**
- Modify: `src-tauri/src/llm/gateway.rs:243-260, 361-380`

- [ ] **Step 1: 看现有签名**

```bash
sed -n '243,260p' src-tauri/src/llm/gateway.rs
sed -n '361,380p' src-tauri/src/llm/gateway.rs
```

- [ ] **Step 2: 新增重载方法**

`gateway.rs` 添加（`stream_message` 和 `send_message` 各加一个 `_with_view`）：

```rust
pub async fn stream_message_with_view(
    &self,
    settings: &ResolvedLlmSettings,
    system_view: &PromptSystemView,
    /* 其他参数与 stream_message 一致 */
) -> Result<...> {
    // 内部调用 build_request 的 _with_view 变体
    // Claude 路径走 ClaudeProvider::build_request_body_from_view
    // OpenAI 兼容路径根据 capability 决定走 view 还是 flat
}

pub async fn send_message_with_view(
    /* 同上 */
) -> Result<...>
```

并保留原 `stream_message(system_prompt: Option<&str>, ...)` 实现，内部转：

```rust
pub async fn stream_message(
    &self,
    settings: &ResolvedLlmSettings,
    system_prompt: Option<&str>,
    /* 其他参数 */
) -> Result<...> {
    let view = match system_prompt {
        Some(s) if !s.trim().is_empty() => PromptSystemView {
            blocks: vec![PromptBlock::static_block(
                PromptSectionId::new("legacy_flat"),
                s,
            )],
        },
        _ => PromptSystemView { blocks: vec![] },
    };
    self.stream_message_with_view(settings, &view, ...).await
}
```

- [ ] **Step 3: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

- [ ] **Step 4: 加单元测试覆盖 view → request body 链路**

新增 `src-tauri/tests/gateway_with_view_test.rs`：测试 `_with_view` 路径的 wire body 包含正确的多块 system 结构。

```rust
// 用 mock provider 捕获 build_request 输出，断言：
// 1. system 字段是数组
// 2. 第一个 element 有 cache_control
```

---

### Task 3.2: 升级 `chat.rs:538` 调用点（主对话）

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs:538` 区域

- [ ] **Step 1: 找到调用点**

```bash
grep -n "stream_message\|send_message" src-tauri/src/transport/tauri_commands/chat.rs | head -10
```

- [ ] **Step 2: 改为传 view**

把 `gateway.stream_message(settings, system_prompt.as_deref(), ...)` 改为：

```rust
let system_view = config
    .prompt_snapshot
    .as_ref()
    .map(|s| s.system_view())
    .unwrap_or_else(|| PromptSystemView { blocks: vec![] });

gateway.stream_message_with_view(settings, &system_view, ...)
```

- [ ] **Step 3: 编译 + 测试**

```bash
cd src-tauri && cargo check && cargo test --test effective_system_prompt_test
```

---

### Task 3.3: 升级 `worker_runtime.rs:303` 调用点（subagent）+ 修 max_tokens 硬编码

**Files:**
- Modify: `src-tauri/src/runtime/agent/worker_runtime.rs:295-315`

- [ ] **Step 1: 找当前调用**

```bash
sed -n '295,315p' src-tauri/src/runtime/agent/worker_runtime.rs
```

- [ ] **Step 2: 替换为 view 调用 + 修 max_tokens**

```rust
// 旧：max_tokens: Some(4096)
let max_tokens = crate::llm::max_tokens::default_max_tokens_for_model(&model_name);

// 旧：gateway.stream_message(..., system_prompt.as_deref(), ...)
let system_view = subagent_prompt_assembly.to_system_view();
gateway.stream_message_with_view(
    settings,
    &system_view,
    /* 其他参数 */
).await
```

> 注意：`subagent_prompt_assembly` 来源是 Wave 4 子代理人格升级后的产物。Wave 3 实施时若 Wave 4 还没改，可暂时把 prompt 字符串包成 `PromptSystemView::single_static_block(s)`。

- [ ] **Step 3: 编译 + 测试**

```bash
cd src-tauri && cargo check && cargo test --test claude_provider_multi_block_test
```

---

### Task 3.4: `conversation_service.rs:398` 保留旧接口（标题生成不升级）

**Files:**
- 不修改任何文件

- [ ] **Step 1: 确认决策**

`conversation_service.rs:398` 是标题生成路径，一次性极简调用，无 cache 价值。保留 `stream_message(system_prompt: Option<&str>, ...)` 旧接口即可，无需改动。

- [ ] **Step 2: 加文档注释明确决策**

在 `src-tauri/src/runtime/conversation_service.rs:398` 上方加注释：

```rust
// NOTE: 标题生成是一次性极简调用，不需要 prompt cache。
// 故保留 stream_message 旧接口，不升级到 stream_message_with_view。
```

---

### Task 3.5: Wave 3 验证 + commit

- [ ] **Step 1: 跑全量测试**

```bash
cd src-tauri && cargo test --all 2>&1 | tail -20
```
Expected: 全部 PASS。

- [ ] **Step 2: 手动验证 Claude subagent 不再 400**

启动 `pnpm tauri:dev`，触发一个 subagent 任务（如 explore agent），抓 Claude provider 日志，确认 wire body 的 system 是数组格式且请求成功。

- [ ] **Step 3: commit**

```bash
git add -A
git commit -m "feat(prompt): wave 3 - gateway interface upgrade for PromptSystemView

- gateway.rs 新增 stream_message_with_view / send_message_with_view 重载
- 旧接口 stream_message(Option<&str>) 保留，内部转 view（向后兼容）
- chat.rs:538 主对话升级到 _with_view
- worker_runtime.rs:303 subagent 升级到 _with_view，并修复 max_tokens 硬编码（用 default_max_tokens_for_model）
- conversation_service.rs:398 标题生成保留旧接口（无 cache 价值，加注释说明）

关联 spec: Wave 3"
```

---

## Wave 4: Subagent 独立人格 + TODO 留口子（2 天）

### Task 4.1: 替换 `general-purpose` subagent prompt

**Files:**
- Modify: `src-tauri/src/runtime/agent/builtin/general_purpose.rs:13-15`

- [ ] **Step 1: 替换 prompt 内容**

把 `general_purpose.rs:13-15` 的 `system_prompt: AgentPrompt::Inline("...")` 替换为：

```rust
system_prompt: AgentPrompt::Inline(
    "你是一个子代理，由调用方派出来完成一项任务。利用可用工具把任务完整做完——不要镀金，但也别留半截。\n\
\n\
任务完成后，用一段简短的报告回复，说明做了什么、有哪些关键发现——调用方会把这段报告转交给用户，所以只写要点。\n\
\n\
你擅长：\n\
- 在大型代码库中搜索代码、配置、模式\n\
- 分析多个文件以理解系统架构\n\
- 调研需要探索许多文件的复杂问题\n\
- 执行多步研究任务\n\
\n\
工作准则：\n\
- 文件搜索：不知道东西在哪时广撒网；知道具体路径就直接读\n\
- 分析：从宽到窄，第一次没结果就换搜索策略\n\
- 彻底：检查多个位置，考虑不同命名习惯，留意相关文件\n\
- 绝不创建文件，除非完成目标绝对必要。永远优先编辑现有文件\n\
- 绝不主动创建文档文件（*.md）或 README。仅在用户显式要求时才创建文档\n\
\n\
输出：\n\
- 用纯 Markdown\n\
- 引用具体文件用 `path:line` 格式\n\
- 数据/文件/搜索结果必须如实汇报，不能编造\n\
- 末尾用一段不超过 5 行���话总结结果"
        .into()
),
```

> spec §6 + §6.A 合并版本：取翻译版的"擅长 / 工作准则"骨架 + 原创版的"输出格式 / 数据真实性"补丁。

- [ ] **Step 2: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

---

### Task 4.2: 替换 `explore` subagent prompt

**Files:**
- Modify: `src-tauri/src/runtime/agent/builtin/explore.rs:19-21`

- [ ] **Step 1: 替换 prompt 内容**

`explore.rs:19-21` 替换为：

```rust
system_prompt: AgentPrompt::Inline(
    "你是文件搜索专家，擅长彻底地浏览和探索代码库。\n\
\n\
=== 严格只读模式 — 不允许任何文件修改 ===\n\
这是只读探索任务。严格禁止：\n\
- 创建新文件（不允许 Write、touch 或任何形式的创建）\n\
- 修改现有文件（不允许 Edit 操作）\n\
- 删除文件（不允许 rm 或删除）\n\
- 移动或复制文件（不允许 mv 或 cp）\n\
- 在任何位置创建临时文件，包括 /tmp\n\
- 使用重定向（>、>>、|）或 heredoc 写文件\n\
- 运行任何会改变系统状态的命令\n\
\n\
你的角色仅限于搜索和分析现有代码。你没有文件编辑工具——任何编辑尝试都会失败。\n\
\n\
你的能力：\n\
- 用 search_files 做宽泛文件名匹配\n\
- 用 grep_content 做正则内容搜索\n\
- 用 read_workspace_file 读具体文件\n\
- 用 list_directory 看目录结构\n\
- 必要时用 web_search 补充背景\n\
\n\
工作准则：\n\
- 多次小搜索 > 一次大搜索\n\
- 知道路径就直接读，不知道再搜\n\
- 不要捏造：搜索没结果，如实说\"未找到\"，不要编造路径或代码\n\
- 不要预设结论：先搜索，再下判断\n\
\n\
输出：\n\
- 用 Markdown 列出发现的事实\n\
- 引用代码必须 `path:line` 标注\n\
- 末尾给一段 ≤ 5 行的\"结论\"总结\n\
- 信息不足以回答时，明确说\"信息不足\"，列出还需要查的方向"
        .into()
),
```

- [ ] **Step 2: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

---

### Task 4.3: 替换 `browse_data_agent` subagent prompt

**Files:**
- Modify: `src-tauri/src/runtime/agent/builtin/browse_data_agent.rs:18`

- [ ] **Step 1: 替换 prompt 内容**

`browse_data_agent.rs:18` 把 `AgentPrompt::Inline(String::new())` 替换为：

```rust
system_prompt: AgentPrompt::Inline(
    "你是浏览器数据提取专家，从企业内部业务系统的网页中抽取结构化数据。\n\
\n\
你的能力：\n\
- browse_navigate / read_page_content：浏览页面\n\
- extract_table_data：从 HTML 表格里抽数据\n\
- extract_with_pagination：跨分页抽取\n\
- page_execute_js：必要时跑 JS 拿数据\n\
- browse_and_extract：综合操作\n\
\n\
工作方式：\n\
1. 先用 read_page_content 看页面结构，判断数据放在哪\n\
2. 优先用 extract_table_data / extract_with_pagination 这种结构化工具\n\
3. 只在结构化工具不够用时才退到 page_execute_js\n\
4. 抽完数据立刻返回结构化 JSON 结果，不做业务解读\n\
\n\
数据真实性：\n\
- 抽到什么写什么，不要补全字段\n\
- 字段缺失时用 null 标识，不用空字符串\n\
- 注明每条数据的来源 URL 与抽取时间\n\
- 翻页失败 / 网页报错时如实说，不要假装抽到了\n\
\n\
输出：\n\
- 顶层用 Markdown 简短描述抽取概况\n\
- 主体用代码块包 JSON 数据\n\
- 末尾标注\"已抽取 N 条 / 失败 M 条\""
        .into()
),
```

- [ ] **Step 2: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

---

### Task 4.4: 替换 `daily_assistant_agent` subagent prompt

**Files:**
- Modify: `src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs:12`

- [ ] **Step 1: 替换 prompt 内容**

`daily_assistant_agent.rs:12` 替换：

```rust
system_prompt: AgentPrompt::Inline(
    "你是日常工作助手代理，处理办公场景里的常规任务。\n\
\n\
服务范围：\n\
- 写：起草邮件 / 周报 / 通知 / 简单文档\n\
- 查：在用户连接的资源里搜索特定信息\n\
- 整理：把零散信息归类成结构化清单\n\
- 初步分析：从数据里看出明显趋势\n\
\n\
边界（重要）：\n\
- 不做需要专业资质的判断：医疗诊断、法律意见、金融投资建议、税务规划\n\
- 不下\"应该 / 必须 / 一定\"的强建议；用\"建议 / 可以考虑 / 通常做法是\"\n\
- 不替用户做决定，提供选项让用户选\n\
\n\
工作方式：\n\
1. 任务模糊时先回复一两个澄清问题，不要瞎写一通\n\
2. 写文档时先列大纲，确认后再展开\n\
3. 找信息时优先用 search_memory / read_workspace_file，不要凭空生成\n\
\n\
输出：\n\
- 用 Markdown\n\
- 写作类任务直接给成品，不要\"以下是初稿\"这种废话\n\
- 整理类任务用清单或表格"
        .into()
),
```

- [ ] **Step 2: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

---

### Task 4.5: 4 个 subagent persona 测试

**Files:**
- Create: `src-tauri/tests/subagent_persona_test.rs`

- [ ] **Step 1: 写测试**

```rust
use app_lib::runtime::agent::builtin::{
    browse_data_agent::browse_data_agent_definition,
    daily_assistant_agent::daily_assistant_agent_definition,
    explore::explore_agent_definition,
    general_purpose::general_purpose_agent_definition,
};
use app_lib::runtime::agent::definition::AgentPrompt;

fn extract_prompt(p: &AgentPrompt) -> String {
    match p {
        AgentPrompt::Inline(s) => s.clone(),
        _ => panic!("expected Inline prompt"),
    }
}

#[test]
fn general_purpose_persona_has_safety_and_output_clauses() {
    let def = general_purpose_agent_definition();
    let prompt = extract_prompt(&def.system_prompt);
    assert!(prompt.contains("绝不创建文件"), "missing safety clause");
    assert!(prompt.contains("path:line"), "missing output format clause");
    assert!(prompt.contains("不能编造"), "missing data truthfulness clause");
    // 不应包含主对话身份（子代理是独立人格）
    assert!(!prompt.contains("AI小家"),
        "subagent must NOT inherit main identity");
    assert!(prompt.len() >= 200, "expected detailed persona");
}

#[test]
fn explore_persona_has_strict_readonly_block() {
    let def = explore_agent_definition();
    let prompt = extract_prompt(&def.system_prompt);
    assert!(prompt.contains("严格只读"), "missing strict readonly declaration");
    assert!(prompt.contains("严格禁止"), "missing prohibition list");
    assert!(prompt.contains("不要捏造"), "missing anti-fabrication clause");
    assert!(!prompt.contains("AI小家"));
    assert!(prompt.len() >= 300);
}

#[test]
fn browse_data_persona_has_data_truthfulness_clause() {
    let def = browse_data_agent_definition();
    let prompt = extract_prompt(&def.system_prompt);
    assert!(prompt.contains("数据真实性"));
    assert!(prompt.contains("不要补全字段"));
    assert!(prompt.contains("null"));
    assert!(prompt.len() >= 200);
}

#[test]
fn daily_assistant_persona_has_professional_boundary() {
    let def = daily_assistant_agent_definition();
    let prompt = extract_prompt(&def.system_prompt);
    assert!(prompt.contains("专业资质"));
    assert!(prompt.contains("不替用户做决定"));
    assert!(prompt.len() >= 200);
}
```

- [ ] **Step 2: 跑测试**

```bash
cd src-tauri && cargo test --test subagent_persona_test 2>&1 | tail -20
```
Expected: 4 个 PASS。

---

### Task 4.6: TODO 注释 — `team.rs`

**Files:**
- Modify: `src-tauri/src/runtime/agent/team.rs`

- [ ] **Step 1: 加文件头 TODO 块**

`team.rs` 最上方加：

```rust
// TODO(coordinator-not-implemented): 这是一个占位 stub，未实现。
//
// Claude Code 的 coordinatorMode 需要：
//   1. 多 worker 并发调度入口（dispatch_workers）
//   2. 综合层 prompt（汇总 worker 输出 → 给主对话一个最终答案）
//   3. worker prompt 自包含上下文（worker 不能依赖 coordinator 的对话历史）
//
// 落地路径见:
//   - docs/superpowers/specs/2026-04-20-subagent-alignment-design.md
//   - claude-code-best/src/coordinator/coordinatorMode.ts
//
// 暂不实施原因：当前没有真实业务在等多 worker 调度。
// 何时实施：当出现"一个任务需要多个独立子代理并行 + 综合"的真实需求时。
```

- [ ] **Step 2: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

---

### Task 4.7: TODO 注释 — `dispatch_prompt.rs`

**Files:**
- Modify: `src-tauri/src/runtime/employee/dispatch_prompt.rs:35,71` 区域

- [ ] **Step 1: 在 system_prompt_extra 拼装行附近加注释**

找到 `system_prompt_extra` 的引用点，在最近的拼装位置上方加：

```rust
// NOTE: 字段名 system_prompt_extra 是历史遗留。
// 它实际拼入用户派活消息（user_message 参数），安全层级 = 用户输入，
// 而不是 system prompt。模型可以选择性遵守，不像真正的 system prompt 是硬约束。
//
// TODO(rename): 将来重命名为 dispatch_prompt_extra 或 identity_extra。
// 重命名时需要：
//   - EmployeeRecord 字段名修改
//   - 加 #[serde(alias = "systemPromptExtra")] 向前兼容旧文件
//   - 前端 src/features/employees 同步
//   - 涉及多文件变更，建议单独成 PR
```

- [ ] **Step 2: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

---

### Task 4.8: TODO 注释 — `renlijia_md.rs`

**Files:**
- Modify: `src-tauri/src/runtime/renlijia_md.rs`

- [ ] **Step 1: 加文件头注释**

`renlijia_md.rs` 最上方加：

```rust
//! 项目指令文件加载器。
//!
//! 支持的文件名（仅这些）：
//! - `~/.renlijia/AGENT.md`（用户全局）
//! - 工作目录及其父目录的 `AGENT.md` / `.aijia/AGENT.md` / `AGENT.local.md`
//!
//! 不兼容：CLAUDE.md / AGENTS.md（复数）/ .claude/CLAUDE.md / .claude/rules/*.md。
//! 这是 2026-05-08 的有意决定（用户决策），减少多源指令的冲突面与维护成本。
//! 用户从 Claude Code 迁移过来时由用户自己重命名 AGENT.md 即可。
//!
//! 多文件按追加方式合并（不是覆盖）。
//! 当前消费侧（chat_turn_driver::build_renlijia_md_context_message）
//! 没有为每段加来源标记，未来如有混淆问题再考虑加 `<from path="...">` 标注。
```

- [ ] **Step 2: 编译检查**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

---

### Task 4.9: Wave 4 全量回归 + 手动验证 + commit

- [ ] **Step 1: 跑 review_ + 全量测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -10
cd src-tauri && cargo test --all 2>&1 | tail -20
```
Expected: 全部 PASS。

- [ ] **Step 2: 跑前端关键回归**

```bash
cd .. && pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts 2>&1 | tail -10
```
Expected: 全部 PASS。

- [ ] **Step 3: TODO 注释 grep 验证**

```bash
grep -rn "TODO(coordinator-not-implemented)\|TODO(rename)" src-tauri/src 2>&1 | head -5
```
Expected: 至少各命中 1 处。

- [ ] **Step 4: 手动 4 场景验证**

启动 `pnpm tauri:dev`：

1. **普通对话**：发一条 "你是谁？"，验证回复包含 AI小家身份（base.md 生效证据）
2. **员工派活**：触发任意一个员工，验证日志里 `runtime_allowed_tools` ≠ 全工具集
3. **子代理调用**：让主对话调用 explore 子代理，验证子代理回复风格是只读探索员（不是 AI小家）
4. **恢复对话**：触发一次后台 task notification，验证流程不被破坏

- [ ] **Step 5: commit Wave 4**

```bash
git add -A
git commit -m "feat(prompt): wave 4 - subagent independent personas + TODO placeholders

- 4 个内置 subagent (general_purpose / explore / browse_data / daily_assistant)
  各自有 200-400 字独立中文人格 prompt（不继承主对话 AI小家身份）
- general/explore 合并自 claude-code-best 翻译版 + 本地原创补丁
- browse_data / daily_assistant 沿用本地原创版（claude-code-best 无对应物）
- team.rs / dispatch_prompt.rs / renlijia_md.rs 三处加 TODO 块注释，
  明确未实现功能的范围、为什么不实施、何时再考虑
- 测试覆盖：subagent_persona_test 验证每个 agent 都有安全/输出/数据真实性条款

关联 spec: Wave 4

至此 spec 全部完成。"
```

---

## 全局验收（实施完成后）

- [ ] **跑全部测试**

```bash
cd src-tauri && cargo test --all 2>&1 | tail -20
cd .. && pnpm test 2>&1 | tail -20
```

- [ ] **lint**

```bash
cd .. && pnpm lint 2>&1 | tail -10
```

- [ ] **检查 git log**

```bash
git log --oneline | head -10
```
Expected: 看到 4 个 wave commit + 之前的 spec/research commit。

- [ ] **回放本 plan §5 spec 验收清单**

逐条跑 spec 里 §5 的 8 条验收：
1. 诊断 sections 列出 5 块 ✓
2. Claude wire body 多块 + cache_control ✓（cache_wire_format_test）
3. 工具可见性（普通对话）✓
4. 工具可见性（employee）✓
5. subagent persona ✓（不含 AI小家）
6. token 估算分项 ✓
7. TODO grep ✓
8. 4 场景手动验证 ✓

- [ ] **更新 CLAUDE.md 的"关键设计决策"段（可选）**

如果用户希望，把"PromptCachePolicy 真正驱动 wire format / 工具白名单 dual-track / subagent 独立人格"加到 CLAUDE.md 的关键设计决策列表，避免后续读者重新发现。

---

## Self-Review 修订记录

写完后自查发现并修订的问题：

1. ✅ Task 1.1 测试 fixture 用了 `todo!()` —— 加了警告说明 commit 前必须填好
2. ✅ Task 3.1 重载方法签名 `_with_view`（保持向后兼容），不删旧接口
3. ✅ Task 3.3 与 Wave 4 之间的依赖：worker_runtime 升级 view 早于 subagent 人格升级，加了"暂时包成 single_static_block"的过渡说明
4. ✅ Task 4.5 测试验证 subagent **不**继承 AI小家身份（spec §5.5 验收点）
5. ✅ Wave 1 / 2 / 3 / 4 各自的 commit message 格式统一
6. ✅ chat_runtime_impl.rs 实际路径（`src-tauri/src/transport/tauri_commands/chat/`）已在文档头部纠正

---

## 执行选项

**Plan 完成，已保存到 `docs/superpowers/plans/2026-05-08-prompt-architecture-fixes-plan.md`。两个执行选项：**

1. **Subagent-Driven（推荐）** - 每个 Task 派一个��� subagent 独立完成，主 session 在 task 间做 review，反馈快、上下文保护好
2. **Inline Execution** - 在当前 session 顺序执行，批量配合 checkpoint 复盘

**你选哪个？**
