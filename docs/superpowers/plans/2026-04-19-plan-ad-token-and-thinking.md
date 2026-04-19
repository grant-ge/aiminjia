# Token 感知 + Extended Thinking（Plan-AD）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:test-driven-development — write tests first for each task before implementation.

**Goal:** 分两块推进：provider-aware Extended Thinking 接通 + token/上下文预算观测；不再把 `chars/4` 粗估算当作真正的 overflow 恢复方案。
**Tech Stack:** Rust, serde_json
**Worktree branch:** pzc

---

## 对标修订（2026-04-19）

- Thinking 配置需对齐 `claude-code-best/src/utils/thinking.ts`：`adaptive | enabled { budget_tokens } | disabled`，不能只做布尔开关。
- 模型/Provider gate 需放在请求组装层，不能只靠模型名字符串包含判断。
- `chars/4` 只作为本地近似观测或提前告警；真正的 overflow 恢复留在 `Plan-W`（如 `max_output_tokens`、prompt-too-long/compact 路径）。
- 若后续需要更精确 token 估算，应单独追加 `tokenEstimation` 对齐计划，而不是继续扩大本计划职责。

---

## 背景与现状

### Token 计算缺口（AD1-AD3）

当前唯一的 context 压缩机制是 `src-tauri/src/llm/context_decay.rs` 中的 `apply_decay()`，它基于字符数对旧 tool result 做截断（`RECENT_LIMIT=2000`, `OLD_LIMIT=500`）。这可以继续作为观测/退化策略，但不能等同于 `claude-code-best` 的 overflow/recovery 设计。当前缺口是：

- 没有全局 token 估算——不知道整个 messages + system prompt 总共占多少 context window
- `TurnConfig.token_budget` 字段含义是 `max_tokens`（LLM 输出预算），不是 context window 保护阈值
- 长会话遇到 context overflow 时直接报 Anthropic API 400 错误，无任何保护层

### Extended Thinking 缺口（AD4-AD5）

`src-tauri/src/llm/providers/claude.rs` 的 SSE 解析已能处理 `thinking_delta`（被动接收），但：

- `LlmRequest`（`src-tauri/src/llm/streaming.rs:131`）无 `thinking_config` 字段
- `ClaudeProvider::build_request_body()`（`claude.rs:72`）无法构造 `betas` + `thinking` 参数
- 无法主动开启 Claude extended thinking

---

## 文件一览

| 文件 | 说明 |
|---|---|
| `src-tauri/src/llm/streaming.rs` | `LlmRequest`、`ChatMessage` 定义 |
| `src-tauri/src/llm/providers/claude.rs` | `ClaudeProvider::build_request_body()` |
| `src-tauri/src/llm/context_decay.rs` | 现有 decay 逻辑（仅字符截断） |
| `src-tauri/src/runtime/chat/context_builder.rs` | `build_iteration_context()`，context 组装 |
| `src-tauri/src/runtime/chat/turn_config.rs` | `TurnConfig`、`LlmStepInput` 定义 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | `LlmStepInput` 构建、`run_llm_step` 调用点 |
| `src-tauri/tests/plan_ad_token_thinking_test.rs` | 本 plan 集成测试（新建） |

---

## 任务列表

### AD1 — `estimate_tokens(messages)` 工具函数

**文件：** `src-tauri/src/llm/context_decay.rs`（附加到 decay 模块，公开导出）

**实现：**

```rust
/// 粗糙 token 估算：chars / 4。不需要 API 调用，足够触发 overflow 保护。
/// 同时累计 system_prompt 字符数（若传入）。
pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    total_chars / 4
}

/// 含 system prompt 的完整 context 估算。
pub fn estimate_context_tokens(system_prompt: &str, messages: &[ChatMessage]) -> usize {
    let system_chars = system_prompt.len();
    let msg_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    (system_chars + msg_chars) / 4
}
```

**注意：**
- tool_calls JSON 也应计入——`ChatMessage` 有 `tool_calls: Option<Vec<ToolCall>>`，对应字段序列化后加入 chars 累计
- 函数保持纯函数、无 I/O，便于单测

---

### AD2 — context overflow 保护（超 80% 时触发 compact/decay）

**文件：** `src-tauri/src/runtime/chat/chat_turn_driver.rs`，在构建 `LlmStepInput` 之前插入检查点

**模型 context window 常量（新增到 `turn_config.rs` 或 `context_decay.rs`）：**

```rust
/// 各模型/场景的 context window（tokens）。
/// 用于 overflow 保护的阈值计算。
pub const CONTEXT_WINDOW_CLAUDE: usize = 200_000;
pub const CONTEXT_WINDOW_DEEPSEEK: usize = 128_000;
pub const CONTEXT_WINDOW_DEFAULT: usize = 100_000;

/// 触发 compact 的负载比例（80%）。
pub const CONTEXT_OVERFLOW_THRESHOLD: f64 = 0.8;
```

**实现位置：** `chat_turn_driver.rs` 中 `build LlmStepInput` 之前（约 713 行附近）

```rust
// AD2: overflow 保护 — 在组装 LlmStepInput 之前检查 token 估算
let estimated = estimate_context_tokens(&config.system_prompt, &state.messages);
let window = context_window_for_model(&config.llm_settings.primary_model);
if estimated as f64 > window as f64 * CONTEXT_OVERFLOW_THRESHOLD {
    warn!(
        "[AD2] Context overflow risk: estimated {} tokens > {}% of {} window. Triggering decay.",
        estimated,
        (CONTEXT_OVERFLOW_THRESHOLD * 100.0) as u32,
        window
    );
    // 已有 apply_decay 做 tool result 截断；必要时升级为 compaction
    // 当前 phase：直接调用 apply_decay（is_analysis=true 触发截断逻辑）
    // 后续可接 compaction.rs 中的 auto-compact
    let messages_json = state.messages.clone(); // JsonValue vec
    // NOTE: apply_decay 操作 ChatMessage，需先转换；见 AD2 实现细节
    state.compact_state.mark_overflow_triggered();
}
```

**实现细节：**
- `context_window_for_model(model: &str) -> usize`：简单字符串匹配（`contains("claude")` → 200k，`contains("deepseek")` → 128k，默认 100k）
- `chat_turn_driver.rs` 中 `state.messages` 类型是 `Vec<JsonValue>`（非 `Vec<ChatMessage>`），需在 `estimate_context_tokens` 中增加 `JsonValue` 重载，或在调用侧用 `serde_json::to_string` 累计字符数
- 建议新增 `estimate_tokens_from_json(messages: &[serde_json::Value]) -> usize`，避免反序列化开销：

```rust
pub fn estimate_tokens_from_json(messages: &[serde_json::Value]) -> usize {
    messages
        .iter()
        .map(|v| v.to_string().len())
        .sum::<usize>()
        / 4
}
```

- overflow 保护触发后，记录 `warn!` 日志即可（AD3 补充 estimated_tokens 到 input）；compact 调用可 no-op 先行，后续迭代升级为 `compaction.rs` 的 auto-compact

---

### AD3 — `LlmStepInput` 携带 `estimated_tokens` 供日志

**文件：** `src-tauri/src/runtime/chat/turn_config.rs`

**变更：** 在 `LlmStepInput` 结构体增加字段：

```rust
pub struct LlmStepInput<'a> {
    // ... 现有字段 ...
    /// 本次 step 发送的 context 估算 token 数（chars/4）。仅用于日志/调试，不影响业务逻辑。
    pub estimated_tokens: usize,
}
```

**调用点（`chat_turn_driver.rs`）：** 在构建 `LlmStepInput` 时填入 AD2 计算出的 `estimated` 值：

```rust
let input = LlmStepInput {
    // ... 现有字段 ...
    estimated_tokens: estimated,
};
```

**executor 使用：** `run_llm_step` 实现中加 `debug!` 日志：

```rust
debug!(
    "[AD3] LLM step: estimated_tokens={}, token_budget={}",
    input.estimated_tokens,
    input.token_budget
);
```

---

### AD4 — `ThinkingConfig` 类型 + `LlmRequest` 新增字段

**文件：** `src-tauri/src/llm/streaming.rs`

**新增类型：**

```rust
/// Extended thinking 配置（Claude API `thinking` 参数）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    /// 主动启用，指定最大思考 token 数。
    Enabled { budget_tokens: u32 },
    /// 显式禁用（用于覆盖模型默认行为）。
    Disabled,
}
```

**`LlmRequest` 新增字段：**

```rust
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub stream: bool,
    /// Extended thinking 配置。`None` 表示不发送 thinking 参数（沿用模型默认）。
    pub thinking_config: Option<ThinkingConfig>,
}
```

**`Default` 实现：** `thinking_config: None`（不改变现有默认行为）

**Settings 集成（前端 → 后端）：**

当前 Settings 持久化在前端 Zustand store 中，通过 Tauri IPC 传入 `ResolvedLlmSettings`（`turn_config.rs`）。`ResolvedLlmSettings` 新增：

```rust
pub struct ResolvedLlmSettings {
    // ... 现有字段 ...
    /// 是否启用 extended thinking，以及 budget_tokens。
    pub thinking_enabled: bool,
    pub thinking_budget_tokens: u32, // 默认 8000
}
```

`ResolvedLlmSettings::Default` → `thinking_enabled: false`, `thinking_budget_tokens: 8000`

executor 的 `run_llm_step` 据此构造 `LlmRequest.thinking_config`：

```rust
let thinking_config = if input.llm_settings.thinking_enabled {
    Some(ThinkingConfig::Enabled {
        budget_tokens: input.llm_settings.thinking_budget_tokens,
    })
} else {
    None
};
```

---

### AD5 — `ClaudeProvider::build_request_body()` 发送 thinking 参数

**文件：** `src-tauri/src/llm/providers/claude.rs`

**变更：** 在 `build_request_body` 末尾添加 thinking 处理：

```rust
// AD5: Extended Thinking
match &request.thinking_config {
    Some(ThinkingConfig::Enabled { budget_tokens }) => {
        // Anthropic extended thinking 需要 beta header + thinking 参数
        // budget_tokens 必须 < max_tokens
        let budget = (*budget_tokens).min(request.max_tokens.saturating_sub(1));
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget
        });
        // temperature 必须为 1.0（Anthropic API 约束）
        body.as_object_mut().unwrap().remove("temperature");
        // betas header 在 HTTP 请求层注入（见下）
    }
    Some(ThinkingConfig::Disabled) => {
        body["thinking"] = json!({ "type": "disabled" });
    }
    None => {}
}

body
```

**HTTP 请求层（`send` / `stream` 方法）：** 在发起请求前检查是否需要 beta header：

```rust
let mut req_builder = self
    .client
    .post(ANTHROPIC_API_URL)
    .header("x-api-key", &self.api_key)
    .header("anthropic-version", ANTHROPIC_VERSION)
    .header("content-type", "application/json");

if matches!(request.thinking_config, Some(ThinkingConfig::Enabled { .. })) {
    req_builder = req_builder.header("anthropic-beta", "interleaved-thinking-2025-05-14");
}

req_builder.json(&body).send().await?
```

**API 约束备注：**
- `budget_tokens` 必须 `< max_tokens`，代码已做 `min(budget, max_tokens - 1)` 限制
- 启用 thinking 时 temperature 必须为 1.0（Anthropic 硬性约束），`build_request_body` 必须移除 temperature 字段而非设置为 1.0（防止 f32 精度问题触发现有的非默认判断）
- beta header 字符串 `"interleaved-thinking-2025-05-14"` 定义为常量 `ANTHROPIC_BETA_THINKING`

---

## 测试文件

**路径：** `src-tauri/tests/plan_ad_token_thinking_test.rs`

```rust
// AD1 tests
#[test]
fn ad1_estimate_tokens_empty() { ... }
#[test]
fn ad1_estimate_tokens_chars_div_4() { ... }
#[test]
fn ad1_estimate_tokens_from_json() { ... }

// AD2 tests
#[test]
fn ad2_context_window_for_claude_model() { ... } // 200_000
#[test]
fn ad2_context_window_for_deepseek_model() { ... } // 128_000
#[test]
fn ad2_context_window_default() { ... } // 100_000
#[test]
fn ad2_overflow_threshold_constant() { ... } // 0.8

// AD3 tests
#[test]
fn ad3_llm_step_input_has_estimated_tokens_field() { ... }

// AD4 tests
#[test]
fn ad4_thinking_config_enabled_serializes() { ... }
#[test]
fn ad4_thinking_config_disabled_serializes() { ... }
#[test]
fn ad4_llm_request_default_no_thinking() { ... }

// AD5 tests
#[test]
fn ad5_build_request_body_with_thinking_enabled() {
    // 验证 body["thinking"]["type"] == "enabled"
    // 验证 body["thinking"]["budget_tokens"] < body["max_tokens"]
    // 验证 body.get("temperature") == None
}
#[test]
fn ad5_build_request_body_with_thinking_disabled() {
    // 验证 body["thinking"]["type"] == "disabled"
}
#[test]
fn ad5_build_request_body_no_thinking_config() {
    // 验证 body.get("thinking") == None（向后兼容）
}
#[test]
fn ad5_budget_tokens_clamped_below_max_tokens() {
    // budget_tokens = max_tokens + 1000 时，clamp 后 == max_tokens - 1
}
```

---

## 实施顺序

1. **AD1**：新增 `estimate_tokens` / `estimate_tokens_from_json` 到 `context_decay.rs`，写单测
2. **AD4**：新增 `ThinkingConfig` 枚举和 `LlmRequest.thinking_config` 字段，更新 `Default`，写单测
3. **AD3**：`LlmStepInput` 新增 `estimated_tokens` 字段（breaking change，需更新所有构建点和测试中的字段初始化）
4. **AD2**：在 `chat_turn_driver.rs` 插入 overflow 保护检查点，调用 AD1 工具函数填充 AD3 字段
5. **AD5**：`ClaudeProvider::build_request_body()` + HTTP 请求层注入 beta header，写单测

---

## 边界与约束

- **不改变现有 decay 行为**：AD2 保护层在现有 `apply_decay` 之上叠加，不删除 decay 逻辑
- **不引入 API 调用**：token 估算全部 chars/4 本地计算
- **backward compatible**：`LlmRequest.thinking_config = None` 时 `build_request_body` 行为与现在完全一致
- **extended thinking 仅限 Claude provider**：其他 provider（DeepSeek、Custom、Volcano）不受影响，`thinking_config` 字段在非 Claude provider 中被忽略
- **AD4 Settings 集成**：`ResolvedLlmSettings.thinking_enabled` 默认 `false`，不影响现有任何会话的 LLM 行为；前端 Settings UI 改动不在本 plan 范围内（可在 `TurnConfig` 构建处 hardcode false 直至 UI 就绪）
