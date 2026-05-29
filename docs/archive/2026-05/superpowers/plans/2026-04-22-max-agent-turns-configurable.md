# Max Agent Turns 可配置化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

---

## 背景与问题

### 用户痛点

客户在使用 AI小家 处理复杂数据分析任务（如多 Sheet 账单处理、员工薪酬核算）时，会在任务进行到一半时看到以下提示：

```
⚠️ 本步分析较为复杂，已达处理上限（30 次迭代）。以上是当前阶段的分析结果。
如需补充分析，请回复具体要求；如结果已满足需要，请确认继续下一步。
```

这是 `src-tauri/src/runtime/chat/post_process.rs` 在主 loop 达到 `max_iterations`（硬编码 30）时自动追加到 AI 回复中的文案。

### 根本原因

`src-tauri/src/runtime/chat/chat_turn_driver.rs:664` 中有一行硬编码：

```rust
max_iterations: 30,
```

这是主对话 loop（每次 LLM 调用 + 工具调用算一次迭代）的唯一上限，**没有任何配置入口**。对于需要读取多个文件、执行多次 Python 分析、多轮核对数据的复杂任务，30 次远远不够。

### 对标差距

与参考项目 claude-code-best 对比：

| 场景 | lotus-app（当前） | claude-code-best（对标） | 差距 |
|---|---|---|---|
| 主对话 loop | 硬编码 **30** | `maxTurns` 参数，**默认无限制** | 🔴 严重 |
| browse 子代理 | 30 | **200** | 🔴 差 6.7x |
| daily 子代理 | 20 | **50** | 🟡 偏低 |

子代理的情况同样存在于 `browse_data_agent.rs`（30）、`daily_assistant_agent.rs`（20）和 `internal_system.rs` 的两处 fallback（各 30）。

### 业务价值

改完之后：
1. **复杂分析任务不再被提前截断**：账单处理、薪酬核算等需要 30+ 次迭代的任务可以正常完成
2. **开发者可通过 settings.json 调整上限**：无需改代码重新打包，灵活应对不同部署场景
3. **子代理上限对齐行业基准**：browse 子代理 200 次、daily 子代理 50 次，与 claude-code-best 一致

### 设计决策

- **主 loop**：进入 `AppSettings`（`maxAgentTurns`，默认 1000），开发者可编辑 settings.json 覆盖，**不在前端 UI 暴露**（对普通用户透明）
- **子代理**：直接改硬编码常量（场景固定、上限合理，不需要运行时配置）
- 选择 1000 而非"无限制"：防止因提示词问题导致无限循环消耗 token；1000 次对任何合理任务都足够

---

**Goal:** 将主 Turn loop 的硬编码 30 次上限改为从 `AppSettings` 读取（默认 1000），同时将各子代理上限对齐 claude-code-best 的分层策略。

**Architecture:** 在 `AppSettings` 中新增 `max_agent_turns: u32`（默认 1000），经 `ResolvedLlmSettings` 传递，由 `chat_turn_driver.rs` 构建 `TurnConfig` 时注入，主 loop 从此配置驱动。子代理上限直接修改硬编码常量至对标值，不进 settings（场景固定，无需用户配置）。

**Tech Stack:** Rust（Tauri 后端），无新依赖

---

## 修改文件索引

| 文件 | 动作 | 说明 |
|---|---|---|
| `src-tauri/src/models/settings.rs` | Modify | 新增 `max_agent_turns` 字段，默认 1000 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Modify | 构建 `TurnConfig` 时读 `settings.max_agent_turns` |
| `src-tauri/src/runtime/agent/builtin/browse_data_agent.rs` | Modify | 30 → 200 |
| `src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs` | Modify | 20 → 50 |
| `src-tauri/src/llm/tool_executor/internal_system.rs` | Modify | fallback 两处 30 → 200 |

---

## Task 1：在 AppSettings 新增 max_agent_turns 字段

**Files:**
- Modify: `src-tauri/src/models/settings.rs`

- [ ] **Step 1: 添加默认值函数 + 字段**

在 `src-tauri/src/models/settings.rs` 中，在 `default_thinking_budget_tokens` 函数下方添加：

```rust
fn default_max_agent_turns() -> u32 {
    1000
}
```

在 `AppSettings` 结构体中，`thinking_budget_tokens` 字段后面添加：

```rust
/// Maximum tool-call iterations per chat turn. Loaded from settings.json;
/// developers can override by editing the file directly. Not exposed in UI.
#[serde(default = "default_max_agent_turns")]
pub max_agent_turns: u32,
```

- [ ] **Step 2: 在 Default impl 中赋初值**

在 `impl Default for AppSettings` 的 `Self { ... }` 块末尾，`thinking_budget_tokens` 行后添加：

```rust
max_agent_turns: default_max_agent_turns(),
```

- [ ] **Step 3: 在 from_string_map 中解析**

在 `AppSettings::from_string_map` 的 `Self { ... }` 块末尾，`thinking_budget_tokens` 行后添加：

```rust
max_agent_turns: get_u32("maxAgentTurns", defaults.max_agent_turns),
```

- [ ] **Step 4: 编译验证**

```bash
cd src-tauri && cargo check 2>&1 | grep -E "error|warning: unused"
```

Expected: 0 errors

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models/settings.rs
git commit -m "feat(settings): add max_agent_turns field (default 1000)"
```

---

## Task 2：将 max_agent_turns 传入 ResolvedLlmSettings → TurnConfig

`AppSettings` 已在 `load_llm_settings_for_turn`（`tauri_commands/chat.rs`）中完整解析，
但只把 LLM 路由字段装进 `ResolvedLlmSettings`。最小路径：把 `max_agent_turns` 也加进
`ResolvedLlmSettings`，`TurnConfig` 构建处直接读 `llm_settings.max_agent_turns`。

**Files:**
- Modify: `src-tauri/src/runtime/chat/turn_config.rs`（ResolvedLlmSettings）
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`（load_llm_settings_for_turn）
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:664`

- [ ] **Step 1: ResolvedLlmSettings 新增字段**

`src-tauri/src/runtime/chat/turn_config.rs` 的 `ResolvedLlmSettings` 结构体，在
`thinking_budget_tokens` 后添加：

```rust
/// Max tool-call iterations per turn, sourced from AppSettings.
pub max_agent_turns: u32,
```

在同文件 `impl Default for ResolvedLlmSettings` 的 `Self { ... }` 末尾添加：

```rust
max_agent_turns: 1000,
```

- [ ] **Step 2: load_llm_settings_for_turn 填充字段**

`src-tauri/src/transport/tauri_commands/chat.rs` 的 `load_llm_settings_for_turn` 方法，
找到构建 `ResolvedLlmSettings { ... }` 的位置，在末尾添加：

```rust
max_agent_turns: settings.max_agent_turns,
```

（`settings: AppSettings` 在该函数内已存在，约第 604-607 行解析完毕）

如果 `ResolvedLlmSettings` 是通过字段列举方式构建的（不是 `..Default::default()`），
还需在所有其他构建 `ResolvedLlmSettings` 的地方补上该字段（搜索全局）：

```bash
grep -rn "ResolvedLlmSettings {" src-tauri/src --include="*.rs"
```

对每一处都补上 `max_agent_turns: 1000,`（或从 AppSettings 读取）。

- [ ] **Step 3: TurnConfig 构建处使用新字段**

`src-tauri/src/runtime/chat/chat_turn_driver.rs` 第 664 行：

```rust
// 改前
max_iterations: 30,
// 改后
max_iterations: llm_settings.max_agent_turns as usize,
```

（`llm_settings` 在第 627-630 行已 await 完毕，当前作用域可直接使用）

- [ ] **Step 4: 编译验证**

```bash
cd src-tauri && cargo check 2>&1 | grep "error"
```

Expected: 0 errors

- [ ] **Step 5: 验证 safeguard 自动适配**

`safeguard.rs:23` 使用 `max_iterations.saturating_sub(3)` 做预警，已是动态参数，无需修改。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/turn_config.rs \
        src-tauri/src/transport/tauri_commands/chat.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "feat(runtime): propagate max_agent_turns from AppSettings through ResolvedLlmSettings into TurnConfig"
```

---

## Task 3：子代理上限对齐 claude-code-best 分层策略

**Files:**
- Modify: `src-tauri/src/runtime/agent/builtin/browse_data_agent.rs:15`
- Modify: `src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs:9`
- Modify: `src-tauri/src/llm/tool_executor/internal_system.rs:376,389`

对标策略：
| 场景 | claude-code-best | 本项目目标 |
|---|---|---|
| Fork/browse 子代理 | 200 | 200 |
| Hook/daily 子代理 | 50 | 50 |
| internal_system fallback | — | 200（同 browse） |

- [ ] **Step 1: 修改 browse_data_agent**

`src-tauri/src/runtime/agent/builtin/browse_data_agent.rs` 第 15 行：

```rust
// 改前
max_iterations: 30,
// 改后
max_iterations: 200,
```

- [ ] **Step 2: 修改 daily_assistant_agent**

`src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs` 第 9 行：

```rust
// 改前
max_iterations: 20,
// 改后
max_iterations: 50,
```

- [ ] **Step 3: 修改 internal_system.rs 两处 fallback**

`src-tauri/src/llm/tool_executor/internal_system.rs`，第 376 行和第 389 行（两处 `30,`）均改为：

```rust
200,
```

- [ ] **Step 4: 编译验证**

```bash
cd src-tauri && cargo check 2>&1 | grep "error"
```

Expected: 0 errors

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/agent/builtin/browse_data_agent.rs \
        src-tauri/src/runtime/agent/builtin/daily_assistant_agent.rs \
        src-tauri/src/llm/tool_executor/internal_system.rs
git commit -m "feat(agents): align sub-agent iteration limits to claude-code-best tiers (browse:200, daily:50)"
```

---

## Task 4：全量测试验证

- [ ] **Step 1: 运行 Rust 全部测试**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: `test result: ok`，0 failures

- [ ] **Step 2: 运行架构回归测试**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

Expected: 所有 `review_` 测试通过

- [ ] **Step 3: 手动冒烟验证**（可选，开发模式下）

```bash
pnpm tauri:dev
```

发送一条需要多次工具调用的消息，观察 loop 是否不再在 30 次时提前终止。

---

## 注意事项

1. **Task 2 Step 2 的 settings 注入路径**：需在 `chat_turn_driver.rs` 中实际查看 settings 对象的来源，可能需要从 `executor.get_settings()` 或参数中取。若 `AppSettings` 当前不在 `run_chat_turn_s4` 作用域内，最小改法是在构建 `TurnConfig` 时调用一次 `executor.load_settings().await`（该方法已存在于 `RuntimeLlmExecutor` trait）。

2. **AgentDefinition.max_iterations 未接通 TurnConfig**：本次计划 *不* 修复这个 gap（属于独立架构专项）。`browse_data_agent.rs` 中的 `max_iterations: 200` 目前通过 `internal_system.rs` 的注入路径生效，不通过 `TurnConfig`。

3. **post_process.rs 通知文案**：数字已是动态的（用 `max_iterations` 变量插值），无需修改。
