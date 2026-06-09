# 上下文压缩系统修正方案

> 设计时间：2026-06-01，修订：2026-06-01
> 基准分析：`context-compaction-gap-analysis.md`
> 对标参考：`/Users/a20250311/github/claude-code-best` 的 `src/services/compact/`
> 当前代码路径：`/Users/a20250311/.qoder/worktree/lotus-app/MyvROh`

---

## 一、Context

lotus-app 已在 Plan-K 中建设了四阶段预处理管线（budget → microcompact → collapse → auto-compact），但最关键的生产实现 `CompactSummaryClient` 从未接通，导致 auto-compact 在生产环境实际处于空转状态。同时缺少 PreservedSegment 元数据和 Compact Boundary 视图隔离等机制。Post-Compact Reinjection 本方案限定为 CLAUDE.md 一项（文件缓存/Skill 重注入等扩展等依赖子系统就位后追加）。

本方案设计一套完整的、可逐 Task 执行的修正计划。**执行方式**：每个 Task 严格按 TDD 步骤（先写失败测试 → 最小实现 → 确认通过 → review_ 约束固化），与 Plan-K 风格一致。

**执行顺序**（必须按序，下游依赖上游）：
```
Phase 0（基础设施预热）
  ↓ 编译通过
Phase R1（核心修复 — 让 compact 工作起来）
  ↓ auto-compact 真正触发
Phase R2（Post-Compact Reinjection — 让 compact 后能力不降）
  ↓ compact 后上下文恢复
Phase R3（Boundary 强化 — 让 compact 状态可追溯）
  ↓ 可观测性提升
Phase R4（Token 精度 — 让 compact 触发更准确）
  ↓ 精度提升
Phase R5（前端闭环 — 让用户感知 compact）
  ↓ 完整闭环
Phase R6（Session Memory — 零 API 成本 compact）
  ↓ 独立立项
尾：已编码系统验证
```

---

## 二、Phase 0：基础设施预热

### Task 0.1：统一 token 估算基础设施

**对标**：claude-code-best `src/services/compact/autoCompact.ts` 的 `getEffectiveContextWindowSize()` + `estimateMessageTokens()`

**目标**：在一个地方统一 token 估算逻辑和**模型级别**的 context window 查询函数，让所有后续阶段能引用相同的基础设施。

> **重要设计决策：Context Window 如何决定？**
>
> ###### CoT 全链路排查——模型名断链
>
> `chat_turn_driver.rs:1953` 当前用 `config.llm_settings.primary_model` 做 context window 判定。但 `primary_model` 是**客户端本地设置**（默认 `"deepseek-v3"`），而 Lotus 网关实际路由时使用的是 `cloud_model`（用户登录后在 `/v1/models` 中选择的真实模型）。**两条线用的是不同模型名**——压缩管线基于 `primary_model` 算阈值，网关基于 `cloud_model` 选模型。
>
> ```
> 压缩管线: primary_model = "deepseek-v3" → context_window = 64K → threshold ≈ 31K tokens
> 网关路由: cloud_model  = "claude-sonnet-4-6" → 实际窗口 = 200K
> ```
>
> 修正：使用 `cloud_model`（网关返回的真实模型名）而非 `primary_model`。`context_window_for_provider()` 已存在，改名为 `context_window_for_model()` 并将匹配粒度提升到 model 名级别（如 `deepseek-v4`→1M, `deepseek`→64K），保留作为默认回退。同时保留 `AppSettings.context_window` 作为手工覆写出口。
>
> ###### 设计原则
>
> `resolve_context_window()` 的优先级：
>
> | 优先 | 来源 | 说明 |
> |---|---|---|
> | 1 | `AppSettings.context_window` | 手动覆写 |
> | 2 | `cloud_model` → `context_window_for_model(cloud_model)` | 网关返回的真实模型名；函数已存在（当前 `context_window_for_provider`），改名为 model 级匹配 |
> | 3 | 保守值 64K | cloud_model 为空时（未登录）的兜底 |
>
> ```
> resolve_context_window(settings) → usize
>   ├── settings.context_window     // 1: 手工指定
>   ├── context_window_for_model(cloud_model)  // 2: 模型名匹配（现有函数改名升级）
>   └── 64_000                      // 3: 兜底
> ```
>

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/llm/context_decay.rs` | 新增 `estimate_tokens()`, `estimate_tokens_from_json()`, `estimate_context_tokens()`, `resolve_context_window()`, `effective_auto_compact_threshold()` |
| `src-tauri/src/llm/mod.rs` | 导出新函数 |
| `src-tauri/src/models/settings.rs` | `AppSettings` 增加 `context_window: Option<usize>`（用户/管理员可覆写） |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | import 更新；调用处改用 `resolve_context_window()` |
| `src-tauri/tests/plan_ad_token_thinking_test.rs` | 新增估算单测 + context window 匹配测试 |

**调用处修正**（`chat_turn_driver.rs`）：

```rust
let context_window = resolve_context_window(
    config.llm_settings.custom_context_window,
    Some(&config.llm_settings.cloud_model),
);

#### 实现细节

**核心函数 `resolve_context_window()`**：

```rust
/// 当前会话的 context window（tokens）。
/// 优先级：settings 覆写 > cloud_model 匹配 > 保守 64K。
pub const CONSERVATIVE_CONTEXT_WINDOW: usize = 64_000;

pub fn resolve_context_window(
    settings_override: Option<usize>,
    cloud_model: Option<&str>,
) -> usize {
    settings_override
        .or_else(|| cloud_model.map(|m| context_window_for_model(m)))
        .unwrap_or(CONSERVATIVE_CONTEXT_WINDOW)
}
```

**Token 估算函数**（带 tool_calls 字符累计，ChatMessage + JSON Value 双重重载）：

```rust
pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter()
        .map(|m| {
            let mut chars = m.content.len();
            if let Some(tool_calls) = &m.tool_calls {
                chars += serde_json::to_string(tool_calls)
                    .map(|s| s.len())
                    .unwrap_or(0);
            }
            chars
        })
        .sum::<usize>()
        / 4
}

pub fn estimate_tokens_from_json(messages: &[serde_json::Value]) -> usize {
    messages.iter()
        .map(|v| v.to_string().len() / 4)
        .sum()
}

pub fn estimate_context_tokens(system_prompt: &str, messages: &[serde_json::Value]) -> usize {
    let system = system_prompt.len() / 4;
    let msg = estimate_tokens_from_json(messages);
    system + msg
}

// ChatMessage 重载（测试和现有调用方兼容）
pub fn estimate_context_tokens_chat(system_prompt: &str, messages: &[ChatMessage]) -> usize {
    (system_prompt.len() + estimate_tokens(messages) * 4) / 4
}
```

**动态 Auto-Compact 阈值计算**：

```rust
pub const AUTOCOMPACT_BUFFER_TOKENS: usize = 13_000;
pub const MAX_OUTPUT_TOKENS_FOR_SUMMARY: usize = 20_000;
pub const CONTEXT_OVERFLOW_THRESHOLD: f64 = 0.8;

pub fn effective_auto_compact_threshold(custom_window: Option<usize>) -> usize {
    let raw_window = resolve_context_window(custom_window);
    let effective = raw_window.saturating_sub(MAX_OUTPUT_TOKENS_FOR_SUMMARY);
    let threshold_tokens = effective.saturating_sub(AUTOCOMPACT_BUFFER_TOKENS);
    threshold_tokens * 4
}
```

#### 测试

```rust
#[test]
fn ad1_empty_messages() { assert_eq!(estimate_tokens(&[]), 0); }

#[test]
fn ad1_resolve_context_window_uses_custom() {
    assert_eq!(resolve_context_window(Some(200_000)), 200_000);
}

#[test]
fn ad1_resolve_context_window_fallback_to_conservative() {
    assert_eq!(resolve_context_window(None), 64_000);
}

#[test]
fn ad1_effective_threshold_with_custom() {
    assert_eq!(
        effective_auto_compact_threshold(Some(200_000)),
        (200_000 - 20_000 - 13_000) * 4
    );
}

#[test]
fn ad1_effective_threshold_conservative_fallback() {
    assert_eq!(
        effective_auto_compact_threshold(None),
        (64_000 - 20_000 - 13_000) * 4
    );
}
```

#### 验证命令

```bash
cd src-tauri && cargo test --test plan_ad_token_thinking_test -- ad1_ --nocapture
cd src-tauri && cargo test --lib 2>&1 | tail -10
```

---

### Task 0.2：`LlmStepInput` 携带 `estimated_tokens`

**对标**：claude-code-best 每次 LLM step 都会记录 token 用量用于触发判定

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/runtime/chat/turn_config.rs` | `LlmStepInput` 增加 `estimated_tokens: usize` |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 构建 `LlmStepInput` 时调用 `estimate_context_tokens()` 填入 |

#### 验证命令

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -10
```

---

## 三、Phase R1：核心修复 — 让 Auto-Compact 真正工作

### Task R1.1：实现 `CompactSummaryClient` 生产版本

**对标**：claude-code-best `compact.ts::compactConversation()` 调用独立 LLM 生成摘要

**这是最关键的 Task**：auto-compact 管线在 `compact_client.rs` 定义了 trait，但 `chat_turn_driver.rs` 中 `compact_client.as_ref()` 实际为 `None`，永远走 `warn_no_compact_client()` → `Ok(String::new())` 分支。

#### 设计

在 `src-tauri/src/llm/` 下新建 `compact_summary_client.rs`，实现通过 Lotus 网关发送独立摘要请求的 `CompactSummaryClient`。

**关键约束**（来自 CLAUDE.md 决策 5 & 6）：
- 生产路径走 Anthropic 协议（`lotus.rs` → `claude.rs` → `/anthropic/v1/messages`）
- System prompt 通过 `SystemPromptSegment` + `cache_control: ephemeral` 多段缓存（上限 3 块，预留 1 给 tools）
- Token 统计需含 `cache_creation_input_tokens` / `cache_read_input_tokens`，按 1.25× / 0.1× 加权
- `runtime/` 下的模块禁止 `use tauri::*`
- 摘要请求使用独立的、无工具的 LLM 调用
- `compact_summary()` 的 `conversation_id` 参数仅用于日志/trace 上下文，client 内部不通过它查询任何会话状态（保持无状态）

#### 实现细节

```rust
// src-tauri/src/llm/compact_summary_client.rs

use async_trait::async_trait;
use crate::runtime::chat::compact_client::CompactSummaryClient;
use crate::runtime::chat::turn_config::TurnError;
use crate::llm::gateway::LlmGateway;

pub struct LlmCompactSummaryClient {
    gateway: Arc<dyn LlmGateway>,
}

impl LlmCompactSummaryClient {
    pub fn new(gateway: Arc<dyn LlmGateway>) -> Self {
        Self { gateway }
    }
}

#[async_trait]
impl CompactSummaryClient for LlmCompactSummaryClient {
    async fn compact_summary(
        &self,
        conversation_id: &str,
        messages: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        // 1. 提取纯文本消息（跳过 tool result），保留 user/assistant
        let text_messages = extract_text_only_messages(messages);
        
        // 2. 构造 system prompt——不使用 cache_control。
        //    理由：compact 是一次性独立 LLM 调用，量小不值得占 system 侧 3 块 ephemeral 配额；
        //    与主对话 cache 状态完全解耦，避免互相影响。
        let system_segments = vec![
            SystemPromptSegment::text(COMPACT_SYSTEM_PROMPT),
        ];
        
        // 3. 通过 gateway 发送非流式请求
        //    Anthropic messages 端点本身是参数化 stream（可以在请求中传 stream: false），
        //    不需要新增 send_non_streaming() trait 方法。
        let summary = self.gateway
            .stream_message_with_segments(
                &text_messages,
                Vec::new(),            // 无 tools
                &system_segments,
                8_000,                // max_tokens: compact 摘要输出上限
                None,                  // no thinking
            )
            .await
            .map_err(|e| TurnError::LlmError(e.to_string()))?;
        
        Ok(summary)
    }
}

const COMPACT_SYSTEM_PROMPT: &str = r#"你是一个对话摘要助手。请用中文生成对话历史的结构化摘要。
要求：
1. 保留关键问题和需求
2. 保留重要操作结果、文件路径、代码片段
3. 输出不超过 8000 字符
4. 不遗漏未完成的操作
5. 以"以下是对话历史摘要："开头"#;
```

#### 待修改文件

| 动作 | 文件 |
|---|---|
| **新建** | `src-tauri/src/llm/compact_summary_client.rs` |
| **修改** | `src-tauri/src/llm/mod.rs`（导出新模块） |
| **核实** | `src-tauri/src/llm/streaming.rs`（确认 `stream_message_with_segments` 已支持非流式收集；若已支持则不改 `LlmRequest`） |
| **修改** | `src-tauri/src/runtime/chat/chat_turn_driver.rs`（生产注入点） |
| **修改** | `src-tauri/src/lib.rs`（Tauri 启动时注入 `CompactSummaryClient`） |
| **新建** | `src-tauri/tests/plan_r1_compact_summary_client_test.rs` |
| **修改** | `src-tauri/tests/review_autocompact_constraints_test.rs`（补回路测试） |

#### 测试

```rust
#[test]
fn r1_compact_summary_strips_tool_messages() {
    let messages = vec![
        json!({"role": "user", "content": "hello"}),
        json!({"role": "assistant", "content": "thinking", "toolCalls": [...]}),
        json!({"role": "tool", "toolCallId": "tc1", "content": "result"}),
        json!({"role": "assistant", "content": "final response"}),
    ];
    let stripped = strip_tool_messages(&messages);
    assert_eq!(stripped.len(), 3); // tool 被删，但前后 assistant 还在
    assert!(!stripped.iter().any(|m| m["role"] == "tool"));
}

#[test]
fn r1_compact_summary_verifies_non_streaming() {
    // 验证 gateway 调用参数：无 tools、非流式收集结果
}

#[test]
fn r1_compact_summary_client_integration() {
    // 用 mock gateway 验证端到端: messages → summary text
}
```

#### 验证命令

```bash
cd src-tauri && cargo test --test plan_r1_compact_summary_client_test -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast
```

---

### Task R1.2：Dynamic Auto-Compact Threshold

**对标**：claude-code-best `autoCompact.ts` 的 `getAutoCompactThreshold(model)` 基于 `effectiveContextWindow - BUFFER` 动态计算

**当前**：`AutoCompactConfig::default().threshold_chars = 480_000`（约 120K tokens，对所有模型一样）

**改为**：`AutoCompactConfig` 新增 `custom_context_window: Option<usize>`，传递 settings 中的覆写值。阈值计算使用 Task 0.1 的 `effective_auto_compact_threshold(custom_window)`。不再依赖模型名。

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/runtime/chat/compaction.rs` | `AutoCompactConfig` 新增 `custom_context_window: Option<usize>` |
| `src-tauri/src/runtime/chat/preprocess.rs` | `PreprocessConfig` 接受 `custom_window: Option<usize>`，构造 `AutoCompactConfig` 时传入 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 传递 `custom_window` 给 preprocess（间接来自 settings，不传模型名） |

#### 测试

```rust
#[test]
fn r1_effective_threshold_with_custom() {
    // custom_window=200_000 → (200-20-13)K * 4 = 668K chars
    assert_eq!(
        effective_auto_compact_threshold(Some(200_000)),
        (200_000 - 20_000 - 13_000) * 4
    );
}

#[test]
fn r1_effective_threshold_conservative_fallback() {
    assert_eq!(
        effective_auto_compact_threshold(None),
        (64_000 - 20_000 - 13_000) * 4
    );
}
```

---

### Task R1.3：Image Pre-Cleaning（图片预清理）

**对标**：claude-code-best `compact.ts::stripImagesFromMessages()`

**问题**：图片内容在 compaction 请求中占用大量 token，可能使 compact 请求本身触发 PromptTooLong。

**实现**：在 `prepare_messages_for_llm` 的早期阶段（在预算之前），将 user 消息中的图片内容替换为 `[image]` 占位符。这仅在 auto-compact 将执行时才需要，但可以设计为无条件执行（因为 microcompact 阶段也会受益）。

```rust
pub fn strip_images_from_messages(messages: &[Value]) -> Vec<Value> {
    messages.iter().map(|m| {
        if m["role"] == "user" {
            // 检查 content 是否为数组（OpenAI multi-part）
            // 将 type=image_url 或 type=image 的块替换为 [image]
        }
        m.clone()
    }).collect()
}
```

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/runtime/chat/preprocess.rs` | 在 Stage 1 前插入 `strip_images_from_messages()` |

#### 测试

```rust
#[test]
fn r1_strip_images_replaces_image_blocks() { /* ... */ }
#[test]
fn r1_strip_images_preserves_text_blocks() { /* ... */ }
```

---

## 四、Phase R2：Post-Compact Reinjection

**对标**：claude-code-best `compact.ts::buildPostCompactMessages()` 在 compact 后按 token budget 重注入关键上下文。

**lotus-app 实际上下文**（与 claude-code-best 不同，直接对标会导致错位）：

| claude-code-best 重注入项 | lotus-app 对应态 | 结论 |
|---|---|---|
| 最近读取的文件 | FileStateCache **不存在**（GAP-③），依赖未立项的子系统 | **本方案不做**，待 FileStateCache 落地后追加 |
| 激活的 Skill | Skill 已改为 stateless `load_skill` 工具按需加载（MEMORY `project_skill_first_architecture`），不存在"激活 skill"概念 | **本方案不做** |
| MCP 工具发现结果 | MCP 工具动态注册到 `ToolRegistry` + `TOOL_CATALOG`（CLAUDE.md MCP 章节），不是 system message 注入 | **本方案不做** |
| CLAUDE.md | 可通过 `skill_content` 注入 | **本方案做** |

**本 Phase 实际范围**：只做 CLAUDE.md 等当前确实存在的上下文的重注入，其余等依赖子系统就位后追加。重注入走 system message（独立 segment），不占用 Anthropic user role 的 cache 配额。

### Task R2.1：CompactBoundaryRecord 扩展 + PreservedSegment

在 `CompactBoundaryRecord` 中增加 PreservedSegment 结构，记录 compact 保留了哪些消息。

**注意**：tail 消息端点由 `CompactBoundaryRecord.tail_message_id` 单独记录（与 `history.rs::apply_boundary()` 现有逻辑兼容）。`PreservedSegment` 只保留 head / anchor 和 token 计数，避免与 `tail_message_id` 字段重叠（早期设计曾包含 `last_preserved_message_id`，已删除）。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreservedSegment {
    pub first_preserved_message_id: String,  // headUuid
    pub anchor_message_id: String,           // compact boundary 前最后一条消息
    pub preserved_token_count: u64,          // 保留段的估算 token 数
}
```

**注意**：`CompactBoundaryRecord` 已持久化到 `compact_boundaries.jsonl`，新增字段必须 `#[serde(default)]` 兼容旧记录。

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/runtime/chat/compaction.rs` | `CompactBoundaryRecord` 增加 `preserved_segment: Option<PreservedSegment>` |
| `src-tauri/src/runtime/chat/preprocess.rs` | `compact_messages_via_llm` 调用端填充 PreservedSegment |

#### 测试

```rust
#[test]
fn r2_preserved_segment_serializes_correctly() { /* ... */ }
#[test]
fn r2_old_boundary_deserializes_without_preserved_segment() { /* 兼容旧数据 */ }
#[test]
fn r2_tail_message_id_and_preserved_segment_consistent() { /* tail = last preserved */ }
```

---

### Task R2.2：CLAUDE.md 重注入

**对标**：claude-code-best `createPostCompactFileAttachments()` 中 CLAUDE.md 的恢复。

compact 后，将 CLAUDE.md 内容以独立 system message segment 形式重新注入到消息列表中。

```rust
pub fn reinject_claude_md_content(
    messages: &mut Vec<Value>,
    claude_md: &str,
) {
    // CLAUDE.md 作为 system 消息注入，避免占用 user role 的 cache_control 配额
    // 走 system segment，模型可直接读取
}
```

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/runtime/chat/compaction.rs` | `compact_messages_via_llm` 后调用 CLAUDE.md 重注入 |
| `src-tauri/src/runtime/chat/preprocess.rs` | 传递 CLAUDE.md 内容给 compaction 阶段 |

#### 测试

```rust
#[test]
fn r2_reinject_claude_md_appended_after_compact() { /* CLAUDE.md 在 compact boundary 之后 */ }
#[test]
fn r2_reinject_skipped_when_no_claude_md() { /* 无 CLAUDE.md 时不注入 */ }
```

---

## 五、Phase R3：Boundary 强化 — 让 Compact 状态可追溯

### Task R3.1：PreservedSegment 精确记录

在 `compact_messages_via_llm()` 中，确定 compact 后保留的消息范围并记录到 `CompactBoundaryRecord.preserved_segment`。

```rust
// compact_messages_via_llm 中增强
let (tail_start, tail_end) = find_tail_round_boundary(&messages);
let preserved_segment = PreservedSegment {
    first_preserved_message_id: messages[tail_start]["id"].as_str()...,
    anchor_message_id: messages[tail_start - 1]["id"].as_str()...,
    preserved_token_count: estimate_tokens_from_json(&messages[tail_start..]),
};
// 注：tail_message_id 在 boundary record 中独立记录，与 preserved_segment 不重复
```

### Task R3.2：Boundary 视图隔离强化

**对标**：claude-code-best `getMessagesAfterCompactBoundary()` — 所有操作（包括 microcompact、auto-compact 检查）只对 boundary 之后的消息执行

**当前**：`history.rs::apply_boundary()` 只在历史加载时做截断，compact 阶段的检查仍然基于全量消息。

**改进**：在 `prepare_messages_for_llm` 中，增加一个强制性的 "只处理 boundary 之后的消息" 步骤。思路：
1. 在 `PreprocessConfig` 中携带 `compact_boundary: Option<CompactBoundaryRecord>`
2. 在预处理管线开始时，先调用 `apply_boundary` 截断
3. 各阶段只处理截断后的消息
4. 最终输出时再拼接 compact boundary 摘要

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/runtime/chat/preprocess.rs` | `PreprocessConfig` 增加 `compact_boundary` 字段；管线开始处调用 `apply_boundary` |
| `src-tauri/src/runtime/chat/history.rs` | 导出现有 `apply_boundary` 为公开函数 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 在调用 `prepare_messages_for_llm` 前，传入最新的 compact boundary |

---

### Task R3.3：Compact 请求自身 PTL 时的清理重试

**对标**：claude-code-best 的 `truncateHeadForPTLRetry()` — compact 请求本身触发 PromptTooLong 时，按 round 分组丢弃旧组后重试。

**注意**：`AutoCompactState` 的熔断是 **per-turn** 的——`record_success()` 清零，且 `TurnIterationState::new()` 每 turn 重建。不存在"永久跳过"。

**状态**：当前 `preprocess.rs:373` 已有同签名防重入 guard（`last_prompt_too_long_signature`）。`prepare_messages_for_llm` 在 `PromptTooLongRecovery` 模式下已有三处例外：熔断器触发跳过、`stop_hook_active` 跳过、同签名跳过。

**改进**（拆两层，避免混淆 client 层和 main 管线层的职责）：

- **L1 — client 内部（`compact_summary_client.rs`）**：在 `compact_summary()` 实现中捕获 compact LLM 调用自身的 `TurnError::PromptTooLong` → 调用 `truncate_messages_for_ptl_retry()` 截断 20% 最旧 round → 用截断后的消息再次调用 gateway。最多重试 3 次（`MAX_PTL_RETRIES`）。这是新增的 **"compact 请求级"重试**，对调用方无感知。
- **L2 — main 管线���`preprocess.rs`）**：本层 **无需改动**。L1 截断成功后，新一轮 PTL 时 prompt 签名自然与上一轮不同（`last_prompt_too_long_signature` 变化），same-signature guard 自然放行；若 L1 内部 3 次重试后仍 PTL，`compact_summary()` 返回 `Err`，`AutoCompactState::record_failure()` 推进熔断计数，按 per-turn 熔断处理。

`protocol_path_anthropic` 等独立上下文在 L1 截断时需保留（不在被丢弃的 round 范围内）。

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/runtime/chat/compaction.rs` | 新增 `truncate_messages_for_ptl_retry()` |
| `src-tauri/src/llm/compact_summary_client.rs` | 在 PTL 回退路径中调用截断 + 重试 |
| `src-tauri/tests/plan_r1_compact_summary_client_test.rs` | 新增 PTL 恢复测试 |

---

## 六、Phase R4：Token 精度提升

### Task R4.1：Token 观测接入（context_decay 保留不动）

**对标**：claude-code-best `autoCompact.ts::calculateTokenWarningState()`

**现状**：`context_decay.rs` 中的 `apply_decay()` 当前仅被 `storage/file_store/cognitive.rs:840` 调用（文件维度衰减，不是消息管线），与 compaction 管线无冲突。保留该函数不变，仅在此文件中新增 compaction 用的估算函数。

**改进**：
1. 在 `context_decay.rs` 中新增 `estimate_tokens()` 等函数（已在 Task 0.1 中完成）
2. 在 `chat_turn_driver.rs` 的 LLM step 前插入 token 观测日志
3. `LlmStepInput` 已携带 `estimated_tokens` 字段，在 executor 中记录 `debug!` 日志

```rust
// chat_turn_driver.rs 中 AD2 风险观测
let estimated = estimate_context_tokens(&config.system_prompt, &state.messages);
let window = resolve_context_window(
    config.llm_settings.custom_context_window,
    Some(&config.llm_settings.cloud_model),
);
if estimated as f64 > window as f64 * CONTEXT_OVERFLOW_THRESHOLD {
    warn!(
        "[compact] Context overflow risk: ~{} tokens / {} window ({}%)",
        estimated, window, (estimated as f64 / window as f64 * 100.0) as u32,
    );
}
```

#### 待修改文件

| 文件 | 修改内容 |
|---|---|
| `src-tauri/src/llm/context_decay.rs` | 保留 `apply_decay()` 不动（仅 `cognitive.rs:840` 调用，与 chat 管线无关），新增常量 `CONTEXT_OVERFLOW_THRESHOLD` |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | 插入 token 观测日志（build LlmStepInput 前） |

---

## 七、Phase R5：前端闭环

### Task R5.1：CompactCompleted 事件 + boundary 消息渲染

**对标**：claude-code-best 前端有 compaction 结果提示。

**后端**：新增 `RuntimeEventKind::CompactCompleted`，携带 `CompactCompletedPayload { pre_tokens: u64, post_tokens: u64, messages_summarized: usize }`，在 compact 成功后发送，由 `TauriEventAdapter` 映射为 Tauri event。

**前端**：`src/lib/tauri.ts:297` 已有 `TurnStageKind`，`useStreaming.ts:789` 已订阅 Compacting，`StreamingBubble.tsx:100` 已有 spinner + label。需要新增：
- 订阅 `CompactCompleted` 事件
- compact_boundary system 消息渲染为折叠式"对话已压缩"提示条，显示 token 节省量

---

## 八、Phase R6：Session Memory Compact（独立立项）

**对标**：claude-code-best `sessionMemoryCompact.ts`

**当前**：lotus-app 没有 Session Memory 概念。每次 compact 都依赖独立的 LLM 摘要调用，消耗 tokens 和 API 调用。

**思路**：引入 Session Memory 作为对话信息的持久化摘要缓存：
1. 每次 chat turn 完成后，异步提取 Session Memory（结构化摘要）
2. Session Memory 持久化到 `~/.renlijia/users/{scope}/conversations/{conv_id}/session_memory.json`
3. compact 时先检查 Session Memory 是否可用 → 零 API 成本
4. Session Memory 也可用于对话恢复、新设备同步等场景

这是一个独立立项，不在本方案中展开具体 Task。

---

## 九、文件变更总览

### 新建文件

| 文件 | 用途 | Phase |
|---|---|---|
| `src-tauri/src/llm/compact_summary_client.rs` | `CompactSummaryClient` 生产实现 | R1 |
| `src-tauri/tests/plan_r1_compact_summary_client_test.rs` | CompactSummaryClient 集成测试 | R1 |
| `src-tauri/tests/plan_r2_reinjection_test.rs` | CLAUDE.md 重注入测试 | R2 |
| `src-tauri/tests/plan_r3_boundary_test.rs` | Boundary 强化测试 | R3 |
| `src-tauri/tests/review_compact_anthropic_protocol_test.rs` | Anthropic 协议正确性回归 | R1-R3 |

### 修改文件

| 文件 | 修改要点 | Phase |
|---|---|---|
| `src-tauri/src/llm/context_decay.rs` | 新增 `estimate_tokens()`, `resolve_context_window()` | 0, R4 |
| `src-tauri/src/llm/streaming.rs` | **核实**：确认 `stream_message_with_segments` 已支持非流式收集；若已支持则无需改 `LlmRequest` | R1 |
| `src-tauri/src/llm/mod.rs` | 导出 `compact_summary_client` | R1 |
| `src-tauri/src/runtime/chat/compaction.rs` | PreservedSegment + PTL 截断 + CLAUDE.md 重注入 | R1-R3 |
| `src-tauri/src/runtime/chat/preprocess.rs` | 图片预清理 + boundary 视图 + compaction 管线 | R1-R3 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | CompactSummaryClient 注入 + token 观测 | 0, R1, R4 |
| `src-tauri/src/runtime/chat/compact_client.rs` | 无修改（trait 已定义） | - |
| `src-tauri/src/runtime/chat/history.rs` | 导出 `apply_boundary` 为公开函数 | R3 |
| `src-tauri/src/runtime/chat/turn_config.rs` | `LlmStepInput.estimated_tokens` | 0 |
| `src-tauri/src/runtime/chat/mod.rs` | 导出 compact 相关模块 | R2 |
| `src-tauri/src/lib.rs` | Tauri 启动时注入 CompactSummaryClient | R1 |
| `src-tauri/src/events.rs` | 增加 `RuntimeEventKind::CompactCompleted` | R5 |
| `src/...`（前端文件）| compact_boundary 消息渲染 | R5 |
| `src-tauri/tests/review_autocompact_constraints_test.rs` | 补回路测试（新约束） | R1-R5 |
| `src-tauri/tests/review_compact_anthropic_protocol_test.rs` | Anthropic 协议正确性回归（tool_use/tool_result 配对、cache_control 保持） | R1-R3 |

---

## 十、执行顺序与依赖关系

```
Phase 0 （无外部依赖）
  Task 0.1: 统一 token 估算基础设施
  Task 0.2: LlmStepInput.estimated_tokens
  
Phase R1 （依赖 Phase 0）
  Task R1.1: CompactSummaryClient 生产实现  ← 最关键！
  Task R1.2: Dynamic auto-compact threshold
  Task R1.3: Image pre-cleaning

Phase R2 （依赖 R1.1，因为 reinjection 在 compact 之后）
  Task R2.1: CompactBoundaryRecord + PreservedSegment
  Task R2.2: Post-Compact Reinjection 引擎

Phase R3 （依赖 R1, R2）
  Task R3.1: PreservedSegment 精确记录
  Task R3.2: Boundary 视图隔离强化
  Task R3.3: CompactSummaryClient PTL 清理重试

Phase R4 （依赖 R1）
  Task R4.1: context_decay 新增函数 + token 观测

Phase R5 （依赖 R1，后端事件可先独立完成）
  Task R5.1: CompactCompleted 事件 + boundary 消息渲染（Compacting spinner 已存在）

Phase R6 （独立立项，无前置依赖）
  Session Memory 系统
```

### 推荐起点

```
Task 0.1 → Task 0.2   （顺序执行）
  ↓
Task R1.1              （阻塞项：auto-compact 现在在生产中是空转）
  ↓
Task R1.2 + R1.3       （无冲突，代码可同时修改，测试顺序执行）
  ↓
Task R2.1 → R2.2      （顺序：类型定义 → 实现）
  ↓
Task R3.1 → R3.2      （顺序：记录 → 视图隔离）
  ↓
Task R3.3              （单独完成 PTL 清理重试）
  ↓
Task R4.1              （token 观测接入）
  ↓
Phase R5               （前端闭环）
```

---

## 十一、验证方案

### 阶段验证命令

```bash
# Phase 0 验证
cd src-tauri && cargo test --test plan_ad_token_thinking_test -- ad1_ --nocapture
cd src-tauri && cargo test --lib 2>&1 | tail -10

# Phase R1 验证
cd src-tauri && cargo test --test plan_r1_compact_summary_client_test -- --nocapture
cd src-tauri && cargo test review_ --tests --no-fail-fast

# Phase R2 验证
cd src-tauri && cargo test --test plan_r2_reinjection_test -- --nocapture
cd src-tauri && cargo test plan_u3_ --tests --no-fail-fast

# Phase R3 验证
cd src-tauri && cargo test --test plan_r3_boundary_test -- --nocapture

# 全量回归
cd src-tauri && cargo test review_ --tests --no-fail-fast
cd src-tauri && cargo test plan_k_ --tests --no-fail-fast
pnpm test  # 前端 Vitest
```

### 意图测试（产品级验收）

| 意图 | 步骤 | 验收标准 |
|---|---|---|
| 长对话超阈值触发 compact | 连续 50+ 轮工具调用 → 超过阈值 | Compacting spinner 出现 → CompactCompleted 事件收到 |
| compact 后 token 展示 | 同上，完成后检查 UI | boundary 提示条显示 token 节省量（pre/post） |
| compact 后对话可继续 | compact 后发一条新消息 | LLM 正常响应，不报 400 |

### 关键验证场景

| 场景 | 输入 | 预期输出 | 验证文件 |
|---|---|---|---|
| Auto-compact 触发 | >480K chars + wired client | compact_messages_via_llm 被调用 | `plan_r1_compact_summary_client_test.rs` |
| Post-Compact CLAUDE.md 重注入 | compact 后 | CLAUDE.md 出现在 boundary 之后 | `plan_r2_reinjection_test.rs` |
| PreservedSegment 正确 | compact 保留 5 条消息 | segment 记录精确 UUID，tail_message_id 一致 | `plan_r2_reinjection_test.rs` |
| PTL 清理重试 | compact_summary 返回 PTL，签名不同 | 截断 20% 后重试成功 | `plan_r1_compact_summary_client_test.rs` |
| Tool pair 完整性（Anthropic） | compact 后含 tool_use→result | 无孤立 tool_use，无孤立 tool_result | `review_compact_anthropic_protocol_test.rs` |
| Cache control 保持 | compact 前后 | system segment cache_control 未被意外清除 | `review_compact_anthropic_protocol_test.rs` |
| 多轮连续 compact | 3 次 compact | 每次 boundary 正确叠加，无消息丢失 | `review_autocompact_constraints_test.rs` |
| 旧数据兼容 | 旧 compact_boundaries.jsonl | 反序列化成功，PreservedSegment 为 None | `review_autocompact_constraints_test.rs` |
| 前端 boundary 渲染 | 历史含 compact_boundary | 折叠提示条显示 token 节省量 | 前端 vitest |

---

## 十二、风险与注意事项

1. **CompactSummaryClient 的 Anthropic 协议对接**：使用现有的 `stream_message_with_segments()` 方法（Anthropic 原生已参数化 stream）发送非流式请求。System prompt 通过 `SystemPromptSegment::text()` 注入但 **不传 `cache_control`**——compact 是一次性独立调用，量小不值得占 system 侧 3 块 ephemeral 配额，且与主对话 cache 状态完全解耦避免互相影响。Token 统计需含 `cache_creation_input_tokens` / `cache_read_input_tokens` 并按 CLAUDE.md 决策 6 的 1.25× / 0.1× 加权，但当前 `estimate_tokens` 仅为 chars/4 粗估，真正的 cache-aware 阈值计算需单独升级。

2. **PreservedSegment 的向后兼容**：`CompactBoundaryRecord` 已持久化到磁盘，新增字段必须有 `#[serde(default)]`，读取旧文件时新字段为 `None`。

3. **Reinjection 的扩展性**：当前 R2.2 只做 CLAUDE.md 重注入。文件读缓存（FileStateCache）和 Skill 重注入等扩展等依赖子系统（GAP-③）就位后再追加，接口已预留（`compact_messages_via_llm` 后调用的 hook 点）。

4. **前端 compact 展示**：Compacting UI 已存在（`useStreaming.ts:789` + `StreamingBubble.tsx:100`）。`CompactCompleted` 事件及 compact_boundary 消息渲染见 Phase R5。

5. **Session Memory（Phase R6）**：这是一个独立的大型项目，需要设计 Session Memory 的提取 prompt、存储格式、生命周期、GC 策略。不应与前面 Phase 混在一起实现。

6. **Context Window 的精度边界**：使用 `cloud_model` + `context_window_for_model()` 模型名匹配作为默认值，`AppSettings.context_window` 作为手工覆写。后续可演进为网关返回 `max_input_tokens` 后优先使用。

7. **Compact 数据的存储归属**（CLAUDE.md 存储规范强制要求）：
   - `compact_boundaries.jsonl` 写入路径为 `~/.renlijia/users/{scope}/conversations/{conv_id}/compact_boundaries.jsonl`（L1 用户私有域），已在当前实现中就位。
   - Boundary 记录 vs `messages.jsonl` 里的 `compact_boundary` system 消息：**`messages.jsonl` 是单文件 LWW + UUID dedup 的唯一真相源**（Phase A 已落地）。`compact_boundaries.jsonl` 是衍生的索引/日志，用于快速查询最后一个 boundary，不参与消息重建。
   - `PreservedSegment` 和 CLAUDE.md 重注入内容 **不单独持久化**——它们从 boundary 记录中可计算/可恢复。PreservedSegment 的 `first_preserved_message_id` 指向 `messages.jsonl` 中的具体消息，重注入的 CLAUDE.md 从 `skill_content` 或 settings 中实时读取。

8. **cargo test 不能并行跑**：同项目 `cargo test` 共享 artifact lock。方案执行图中"可并行"标注仅指代码修改无冲突、可同时写，不是指测试命令可以同时跑。实际执行时按 Task 顺序逐个提交。已从执行图中移除所有"可并行"误导标注。

---


## 十三、ContentBudgetManager 专项与向前兼容设计

### 13.1 为什么需要独立专项而非内嵌在当前 Phase？

ContentBudgetManager 不是"等前面的做完再想"——它是**前面各 Phase 的自然聚合层**。把它放在独立 Phase 的原因：

| 原因 | 说明 |
|---|---|
| **依赖链** | ContentBudgetManager 需要知道模型窗口大小（Phase 0）、能触发 compact（Phase R1）、compact 后能恢复上下文（Phase R2）。这些都没到位时，写了 ContentBudgetManager 也只是空壳 |
| **状态持有** | ContentBudgetManager 需要持有一个**跨 turn 存在的全局状态**（FileStateCache、ContentReplacementState），这需要先确定 session 级 state 的 owner——这个决策在架构蓝图的 `Session state 无 owner` 问题（GAP-④）中还没解决 |
| **影响面** | 它管控每个工具结果的 token 占用，意味着要修改 `ToolResult` 结构和工具执行出口，影响面横跨 tool runtime、preprocess、compaction 三个子系统。拆成专项可以独立 review、独立回滚 |

### 13.2 当前方案中已预留的接口承接点

每个 Phase 在设计时就考虑了 ContentBudgetManager 的接入点：

```
ContentBudgetManager 未来需要注入的位置 vs 当前方案预留的扩展点：

① 上下文窗口和阈值
   ├── 当前 → resolve_context_window() + effective_auto_compact_threshold()
   └── 未来 → ContentBudgetManager::context_window() 封装这两个，加网关集成

② 触发判定（何时 compact）
   ├── 当前 → should_auto_compact() 在 preprocess.rs 中直接检查 chars 阈值
   └── 未来 → ContentBudgetManager::should_compact() 
             内部计算 effective_window - used_budget - buffer，替代 chars 粗估

③ 各阶段预算配置
   ├── 当前 → PreprocessConfig { budget, microcompact, collapse, auto_compact }
   └── 未来 → PreprocessConfig 增加 content_budget: Option<ContentBudgetConfig>
             各阶段从 ContentBudgetConfig 读取分桶上限

④ 工具结果追踪
   ├── 当前 → ToolResultBudgetConfig.aggregate_char_budget = 64K（固定）
   └── 未来 → ContentBudgetManager::track_tool_result(tool_name, estimated_tokens)
             累加到 ContentReplacementState，超限时触发 per-tool eviction

⑤ 文件去重
   ├── 当前 → 无（每次工具读文件都全量注入 context）
   └── 未来 → ContentBudgetManager::register_file_read(file_path, content_hash)
             Host 注入 FileStateCache，已注入过的文件跳过重复注入

⑥ Compact 后上下文恢复
   ├── 当前 → 仅 CLAUDE.md 重注入（R2.2 范围；从 skill_content/settings 实时读取）
   │         文件/Skill/MCP 重注入未做（FileStateCache 子系统未立项）
   └── 未来 → ContentBudgetManager::request_reinjection_budget(bucket)
             统一管理 reinjection 和初始上下文两个阶段的 budget 分配
             对标 claude-code-best：50K total / 5K per file / 25K per skills
```

### 13.3 关键结构上的 #[serde(default)] 保证序列化兼容

所有持久化结构在新增字段时都用 `#[serde(default)]`：

```rust
// CompactBoundaryRecord — 已有 + Phase R2 新增 PreservedSegment，全部 default 兼容
pub struct CompactBoundaryRecord {
    pub id: String,
    pub conversation_id: String,
    pub trigger: CompactTrigger,
    pub pre_tokens: u64,
    pub post_tokens: u64,
    pub messages_summarized: usize,
    pub created_at: String,
    #[serde(default)]              // ← 旧数据无此字段 → ""
    pub summary_text: String,
    #[serde(default)]              // ← 旧数据无此字段 → None
    pub tail_message_id: Option<String>,
    #[serde(default)]              // ← Phase R2 新增，旧数据 → None
    pub preserved_segment: Option<PreservedSegment>,
    // reinjection 不持久化到 boundary record；
    // CLAUDE.md 内容从 skill_content/settings 实时读取
}

// PreprocessConfig — 新增分桶字段时使用 default
pub struct PreprocessConfig {
    pub budget: ToolResultBudgetConfig,
    pub microcompact: MicrocompactConfig,
    pub collapse: CollapseConfig,
    pub auto_compact: AutoCompactConfig,
    // ContentBudgetManager 接入时新增：
    // #[serde(default)]
    // pub content_budget: Option<ContentBudgetConfig>,
}
```

### 13.4 Trait 隔离保证实现可替换

`CompactSummaryClient` trait（`compact_client.rs`）已经展示了这种模式——接口和实现完全解耦：

```rust
// trait 定义（永不改变语义）
pub trait CompactSummaryClient: Send + Sync { ... }

// 当前：None → 空转（warn log）
// Phase R1：LlmCompactSummaryClient → LLM 摘要
// 未来：SessionMemoryCompactClient → 零 API 成本摘要
```

ContentBudgetManager 会遵循同样的 trait 隔离模式：

```rust
// 未来接口（示意）
pub trait ContentBudgetTracker: Send + Sync {
    fn context_window(&self) -> usize;
    fn current_usage(&self) -> usize;
    fn should_compact(&self) -> bool;
    fn track_tool_result(&mut self, tool: &str, tokens: usize) -> BudgetOutcome;
    fn request_reinjection_budget(&self, bucket: BudgetBucket) -> usize;
}

// Phase 0-R2 期间：NoOpBudgetTracker → 所有方法返回默认值
// 专项完成后：FileBasedBudgetTracker → 真正的追踪
```

### 13.5 验收：从当前到 ContentBudgetManager 的升级路径

```
Phase 0-R2 完成后（当前方案的目标态）：
  prepare_messages_for_llm(messages, config, ...)
    ├── apply_tool_result_budget(64K chars)     ──┐
    ├── microcompact(动态阈值)                    ──┤ 这四个是 ContentBudgetManager
    ├── collapse_tool_results(8K chars)          ──┤ 未来要收编的独立阶段
    ├── auto_compact(LLM 摘要)                    ──┘
    └── reinject_claude_md(R2.2 范围：仅 CLAUDE.md，文件/Skill 重注入留待 FileStateCache 子系统)

ContentBudgetManager 专项完成后（升级路径）：
  ContentBudgetManager::prepare(messages, &budget_config)
    ├── check: 总消耗 < window - 13K？ → 否 → 触发 compact
    ├── check: ToolResultBucket 超限？  → 是 → microcompact + collapse
    ├── compact → LLM 摘要
    ├── reinject: CLAUDE.md 等现有上下文（批量注入，不占独立 budget）
    └── 返回 (processed_messages, BudgetReport)

关键：prepare_messages_for_llm() 的函数签名不需要变，
只是内部从"手动调用四个独立阶段"变成"委托给 ContentBudgetManager::prepare()"。
调用的 chat_turn_driver.rs 完全不受影响。
```


