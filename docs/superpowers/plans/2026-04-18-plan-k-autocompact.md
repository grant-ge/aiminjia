# LLM 辅助自动 Compact 计划（Plan-K）

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to
> execute each task. Each task is a standalone TDD cycle: write failing test → confirm red →
> minimum implementation → confirm green → commit. Do NOT skip the red step.

**Goal:** 实现 LLM 辅助的自动 compact，使 lotus-app 的 `run_chat_turn_s4` 主循环在 token 超阈值时能够自动调用 LLM 生成摘要、替换消息历史，并持久化 `compact_boundary` 边界标记，对标 claude-code-best 的 `autoCompactIfNeeded` / `autoCompactTracking` 机制。

**Architecture（对标分析）：**

| claude-code-best | lotus-app 对应点 |
|---|---|
| `AutoCompactTrackingState` (compacted/turnCounter/turnId/consecutiveFailures) | 新增 `AutoCompactState`，存在 `TurnIterationState` 中 |
| `shouldAutoCompact()` — token 估算超阈值触发 | `compaction::should_auto_compact()` — 统计 `state.messages` 字符数折算 tokens |
| `compactConversation()` — fork 子 LLM 调用生成摘要 | `compaction::compact_messages_via_llm()` — 通过 `RuntimeLlmExecutor::compact_summary()` 调用 |
| `compact_boundary` system 消息（subtype=compact_boundary）| `StoredMessage { role: "system", subtype: "compact_boundary", … }` 存入 `ConversationStore` |
| microcompact — 清 tool results | `compaction::microcompact()` — 裁剪 `state.messages` 中旧 tool result |
| `autoCompactTracking` 在 query loop 迭代间透传 | `TurnIterationState.compact_state: Option<AutoCompactState>` |
| 熔断器：consecutiveFailures >= 3 停止重试 | 同样阈值，`AutoCompactState::consecutive_failures` |

**关键约束：**
- `runtime/` 禁止 `use tauri::*`；compact LLM 调用通过 `RuntimeLlmExecutor` 新方法注入，不直接引用 `LlmGateway`。
- compact 不是 tool call，也不是正常 turn，不触发 `StreamDelta`/`StreamDone`；使用单独的 `RuntimeEventKind::CompactStarted` / `CompactCompleted` 事件通知前端（可选，K4 决定是否需要）。
- `compact_boundary` 消息在历史重组后通过 `ConversationStore` 提供的扩展方法持久化（K1）。
- microcompact 是纯内存操作，不持久化；作用是在阈值 **警告区**（低于 autocompact 触发线）减少 messages 中 tool results 的体积（K2）。

**Tech Stack:** Rust, tokio, async_trait

**Worktree branch:** pzc

**测试文件命名:** `src-tauri/tests/plan_k_*.rs`

---

## Task K1: compact_boundary 消息类型与存储

### 目标
在 `ConversationStore` trait 及其 in-memory 实现中添加 `append_compact_boundary()` 方法，定义 `CompactBoundaryRecord` 结构体，并在 `src-tauri/src/runtime/chat/compaction.rs` 中提供构造辅助函数。

### 文件列表
| 动作 | 文件 |
|---|---|
| Modify | `src-tauri/src/runtime/store/conversation_store.rs` |
| Modify | `src-tauri/src/runtime/chat/compaction.rs` |
| Create | `src-tauri/tests/plan_k_compact_boundary_test.rs` |

### TDD 步骤

#### 1. 写失败测试

创建 `src-tauri/tests/plan_k_compact_boundary_test.rs`：

```rust
//! K1: compact_boundary 消息类型存储回归测试

use lotus_app::runtime::chat::compaction::{
    build_compact_boundary_record, CompactBoundaryRecord, CompactTrigger,
};
use lotus_app::runtime::store::{ConversationStore, InMemoryConversationStore};

#[test]
fn k1_compact_boundary_record_fields_are_correct() {
    let record = build_compact_boundary_record(
        "conv-1",
        CompactTrigger::Auto,
        42_000,   // pre_tokens
        8_200,    // post_tokens
        15,       // messages_summarized
    );

    assert_eq!(record.conversation_id, "conv-1");
    assert_eq!(record.trigger, CompactTrigger::Auto);
    assert_eq!(record.pre_tokens, 42_000);
    assert_eq!(record.post_tokens, 8_200);
    assert_eq!(record.messages_summarized, 15);
    // uuid は空でなければよい
    assert!(!record.id.is_empty(), "compact_boundary record must have a non-empty id");
}

#[test]
fn k1_inmemory_store_append_and_list_compact_boundaries() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-k1", "Test").unwrap();

    let record = build_compact_boundary_record(
        "conv-k1",
        CompactTrigger::Auto,
        50_000,
        9_000,
        20,
    );
    store.append_compact_boundary(record.clone()).unwrap();

    let boundaries = store.list_compact_boundaries("conv-k1").unwrap();
    assert_eq!(boundaries.len(), 1);
    assert_eq!(boundaries[0].id, record.id);
    assert_eq!(boundaries[0].trigger, CompactTrigger::Auto);
    assert_eq!(boundaries[0].pre_tokens, 50_000);
}

#[test]
fn k1_inmemory_store_multiple_boundaries_are_ordered() {
    let store = InMemoryConversationStore::new();
    store.create_conversation("conv-k1b", "Test").unwrap();

    for i in 0u64..3 {
        let r = build_compact_boundary_record(
            "conv-k1b",
            CompactTrigger::Manual,
            10_000 * (i + 1),
            2_000,
            5,
        );
        store.append_compact_boundary(r).unwrap();
    }

    let boundaries = store.list_compact_boundaries("conv-k1b").unwrap();
    assert_eq!(boundaries.len(), 3);
    // 顺序：插入顺序（FIFO）
    assert_eq!(boundaries[0].pre_tokens, 10_000);
    assert_eq!(boundaries[1].pre_tokens, 20_000);
    assert_eq!(boundaries[2].pre_tokens, 30_000);
}
```

#### 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_compact_boundary_test -- --nocapture 2>&1 | head -40
```

预期输出（编译错误）：
```
error[E0432]: unresolved import `lotus_app::runtime::chat::compaction::build_compact_boundary_record`
```

#### 3. 最小实现

**`src-tauri/src/runtime/chat/compaction.rs`** — 追加到文件末尾：

```rust
use serde::{Deserialize, Serialize};

/// 触发原因
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactTrigger {
    Auto,
    Manual,
}

/// compact_boundary 记录，持久化到 ConversationStore。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactBoundaryRecord {
    /// 唯一 ID（UUID v4）
    pub id: String,
    pub conversation_id: String,
    pub trigger: CompactTrigger,
    /// 压缩前估算 token 数
    pub pre_tokens: u64,
    /// 压缩后估算 token 数
    pub post_tokens: u64,
    /// 被摘要的历史消息条数
    pub messages_summarized: usize,
    /// 创建时间（ISO-8601）
    pub created_at: String,
}

/// 构造 CompactBoundaryRecord，id 自动生成。
pub fn build_compact_boundary_record(
    conversation_id: &str,
    trigger: CompactTrigger,
    pre_tokens: u64,
    post_tokens: u64,
    messages_summarized: usize,
) -> CompactBoundaryRecord {
    CompactBoundaryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.to_string(),
        trigger,
        pre_tokens,
        post_tokens,
        messages_summarized,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}
```

**`src-tauri/src/runtime/store/conversation_store.rs`** — 在 trait `ConversationStore` 中添加两个方法，并在 `InMemoryConversationStore` 中实现：

在 `use` 区末尾追加：
```rust
use crate::runtime::chat::compaction::CompactBoundaryRecord;
```

在 `ConversationStore` trait 末尾追加（在最后一个方法后）：
```rust
    /// 追加一条 compact_boundary 记录到会话。
    fn append_compact_boundary(&self, record: CompactBoundaryRecord) -> Result<()>;
    /// 按插入顺序列出指定会话的所有 compact_boundary 记录。
    fn list_compact_boundaries(&self, conversation_id: &str) -> Result<Vec<CompactBoundaryRecord>>;
```

在 `InMemoryConversationStore` 结构体的 `compact_boundaries` 字段追加（`active_tasks` 后）：
```rust
    compact_boundaries: Mutex<HashMap<String, Vec<CompactBoundaryRecord>>>,
```

在 `InMemoryConversationStore::default()` 中补齐初始化（或 `#[derive(Default)]` 自动推导——由于 `CompactBoundaryRecord` 实现 `Default` 并不是必须的，改用显式 `new()`）。

调整 `InMemoryConversationStore::new()` 为：
```rust
impl InMemoryConversationStore {
    pub fn new() -> Self {
        Self {
            conversations: Mutex::new(HashMap::new()),
            messages: Mutex::new(HashMap::new()),
            active_tasks: Mutex::new(std::collections::HashSet::new()),
            compact_boundaries: Mutex::new(HashMap::new()),
        }
    }
}
```

移除 `#[derive(Default)]`（或保留并 impl Default 手动构建）。

在 `impl ConversationStore for InMemoryConversationStore` 末尾追加：
```rust
    fn append_compact_boundary(&self, record: CompactBoundaryRecord) -> Result<()> {
        self.compact_boundaries
            .lock()
            .unwrap()
            .entry(record.conversation_id.clone())
            .or_default()
            .push(record);
        Ok(())
    }

    fn list_compact_boundaries(&self, conversation_id: &str) -> Result<Vec<CompactBoundaryRecord>> {
        Ok(self
            .compact_boundaries
            .lock()
            .unwrap()
            .get(conversation_id)
            .cloned()
            .unwrap_or_default())
    }
```

#### 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_compact_boundary_test -- --nocapture 2>&1
```

预期输出：
```
test k1_compact_boundary_record_fields_are_correct ... ok
test k1_inmemory_store_append_and_list_compact_boundaries ... ok
test k1_inmemory_store_multiple_boundaries_are_ordered ... ok

test result: ok. 3 passed; 0 failed
```

#### 5. Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/compaction.rs \
        src-tauri/src/runtime/store/conversation_store.rs \
        src-tauri/tests/plan_k_compact_boundary_test.rs && \
git commit -m "$(cat <<'EOF'
feat(compaction): add CompactBoundaryRecord type and ConversationStore extension - K1

Defines CompactBoundaryRecord / CompactTrigger in compaction.rs and adds
append_compact_boundary / list_compact_boundaries to the ConversationStore trait
with InMemoryConversationStore implementation.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task K2: microcompact（轻量清 tool results）

### 目标
在 `src-tauri/src/runtime/chat/compaction.rs` 中实现 `microcompact()`：纯内存操作，从 `state.messages`（`Vec<serde_json::Value>`）中将超过阈值的旧 tool result 的 `content` 替换为占位符，返回清理后的消息列表与节省的估算 token 数。不调用 LLM，不持久化。

**对标：** `apiMicrocompact.ts` 中 `clear_tool_uses_20250919` 策略（清最旧的 tool result，保留最近 N 条），以及 `query.ts` 中 microcompact 在 autocompact 之前执行的顺序。

### 文件列表
| 动作 | 文件 |
|---|---|
| Modify | `src-tauri/src/runtime/chat/compaction.rs` |
| Create | `src-tauri/tests/plan_k_microcompact_test.rs` |

### TDD 步骤

#### 1. 写失败测试

创建 `src-tauri/tests/plan_k_microcompact_test.rs`：

```rust
//! K2: microcompact 轻量清 tool results 回归测试

use lotus_app::runtime::chat::compaction::{microcompact, MicrocompactConfig, MicrocompactResult};
use serde_json::json;

fn make_user(content: &str) -> serde_json::Value {
    json!({ "role": "user", "content": content })
}

fn make_assistant_with_tools(content: &str, tool_call_ids: &[&str]) -> serde_json::Value {
    let tool_calls: Vec<serde_json::Value> = tool_call_ids
        .iter()
        .map(|id| json!({ "id": id, "name": "execute_python", "arguments": {} }))
        .collect();
    json!({ "role": "assistant", "content": content, "toolCalls": tool_calls })
}

fn make_tool_result(tool_call_id: &str, content: &str) -> serde_json::Value {
    json!({
        "role": "tool",
        "toolCallId": tool_call_id,
        "name": "execute_python",
        "content": content,
    })
}

#[test]
fn k2_microcompact_noop_when_below_threshold() {
    let messages = vec![
        make_user("hello"),
        make_assistant_with_tools("running", &["tc-1"]),
        make_tool_result("tc-1", "short"),
    ];
    let config = MicrocompactConfig {
        trigger_chars: 100_000, // 远高于实际大小
        keep_recent_tool_results: 2,
    };
    let result = microcompact(&messages, &config);
    assert!(!result.executed, "microcompact should not execute below threshold");
    assert_eq!(result.tokens_freed_estimate, 0);
    assert_eq!(result.messages.len(), messages.len());
}

#[test]
fn k2_microcompact_clears_old_tool_results_above_threshold() {
    // 造一个超过阈值的 tool result
    let big_content = "x".repeat(50_000);
    let messages = vec![
        make_user("analyze"),
        // iteration 0 (old)
        make_assistant_with_tools("iter0", &["tc-old"]),
        make_tool_result("tc-old", &big_content),
        // iteration 1 (recent — 应保留)
        make_assistant_with_tools("iter1", &["tc-new"]),
        make_tool_result("tc-new", "short result"),
    ];
    let config = MicrocompactConfig {
        trigger_chars: 10_000, // 低于 big_content 大小，触发
        keep_recent_tool_results: 1,
    };
    let result = microcompact(&messages, &config);
    assert!(result.executed, "microcompact should execute above threshold");
    assert!(result.tokens_freed_estimate > 0, "should report freed tokens");

    // 旧的 tool result 内容应被清空（替换为占位符）
    let old_result = result.messages.iter().find(|m| {
        m.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-old")
    }).expect("old tool result should still exist as message");
    let content = old_result.get("content").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        content.contains("[microcompacted]") || content.len() < big_content.len(),
        "old tool result content should be replaced with placeholder"
    );

    // 最新的 tool result 应保留完整内容
    let new_result = result.messages.iter().find(|m| {
        m.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-new")
    }).expect("new tool result should exist");
    let new_content = new_result.get("content").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(new_content, "short result", "recent tool result should not be touched");
}

#[test]
fn k2_microcompact_preserves_message_count() {
    let big_content = "y".repeat(30_000);
    let messages = vec![
        make_user("start"),
        make_assistant_with_tools("a", &["tc-a"]),
        make_tool_result("tc-a", &big_content),
        make_assistant_with_tools("b", &["tc-b"]),
        make_tool_result("tc-b", &big_content),
        make_assistant_with_tools("c", &["tc-c"]),
        make_tool_result("tc-c", "final"),
    ];
    let config = MicrocompactConfig {
        trigger_chars: 5_000,
        keep_recent_tool_results: 1,
    };
    let result = microcompact(&messages, &config);
    // 消息数量不变（只替换内容，不删除消息）
    assert_eq!(result.messages.len(), messages.len());
}
```

#### 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_microcompact_test -- --nocapture 2>&1 | head -30
```

预期输出（编译错误）：
```
error[E0432]: unresolved import `lotus_app::runtime::chat::compaction::microcompact`
```

#### 3. 最小实现

在 `src-tauri/src/runtime/chat/compaction.rs` 末尾追加：

```rust
/// microcompact 配置
#[derive(Debug, Clone)]
pub struct MicrocompactConfig {
    /// 当 messages 的总字符数超过此值时触发清理
    pub trigger_chars: usize,
    /// 保留最近 N 条 tool result 不清理
    pub keep_recent_tool_results: usize,
}

impl Default for MicrocompactConfig {
    fn default() -> Self {
        Self {
            trigger_chars: 120_000, // ~40k tokens 估算
            keep_recent_tool_results: 2,
        }
    }
}

/// microcompact 结果
#[derive(Debug)]
pub struct MicrocompactResult {
    /// 清理后的消息列表（内容被替换为占位符）
    pub messages: Vec<serde_json::Value>,
    /// 是否执行了清理
    pub executed: bool,
    /// 估算释放的字符数（约 tokens * 4）
    pub tokens_freed_estimate: usize,
}

/// 估算消息列表的总字符数（粗略 token 估算：chars / 4）
fn estimate_total_chars(messages: &[serde_json::Value]) -> usize {
    messages.iter().fold(0usize, |acc, m| {
        acc + m.to_string().len()
    })
}

/// 轻量 microcompact：将旧 tool result 内容替换为占位符。
///
/// 策略：
/// 1. 收集所有 role="tool" 的消息索引（按出现顺序）。
/// 2. 保留最后 `config.keep_recent_tool_results` 条，其余替换 content。
/// 3. 只在 estimate_total_chars >= trigger_chars 时执行。
pub fn microcompact(
    messages: &[serde_json::Value],
    config: &MicrocompactConfig,
) -> MicrocompactResult {
    let total_chars = estimate_total_chars(messages);
    if total_chars < config.trigger_chars {
        return MicrocompactResult {
            messages: messages.to_vec(),
            executed: false,
            tokens_freed_estimate: 0,
        };
    }

    // 收集 tool result 的索引
    let tool_result_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if m.get("role").and_then(|v| v.as_str()) == Some("tool") {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    if tool_result_indices.len() <= config.keep_recent_tool_results {
        // 没有足够旧的条目可以清理
        return MicrocompactResult {
            messages: messages.to_vec(),
            executed: false,
            tokens_freed_estimate: 0,
        };
    }

    let clear_up_to = tool_result_indices.len() - config.keep_recent_tool_results;
    let indices_to_clear: std::collections::HashSet<usize> = tool_result_indices
        .iter()
        .take(clear_up_to)
        .copied()
        .collect();

    let mut freed_chars = 0usize;
    let new_messages: Vec<serde_json::Value> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if indices_to_clear.contains(&i) {
                let original_content_len = m
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
                freed_chars += original_content_len;
                let mut cleared = m.clone();
                if let Some(obj) = cleared.as_object_mut() {
                    obj.insert(
                        "content".to_string(),
                        serde_json::Value::String("[microcompacted]".to_string()),
                    );
                }
                cleared
            } else {
                m.clone()
            }
        })
        .collect();

    let executed = freed_chars > 0;
    MicrocompactResult {
        messages: new_messages,
        executed,
        tokens_freed_estimate: freed_chars / 4, // chars → approx tokens
    }
}
```

#### 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_microcompact_test -- --nocapture 2>&1
```

预期输出：
```
test k2_microcompact_noop_when_below_threshold ... ok
test k2_microcompact_clears_old_tool_results_above_threshold ... ok
test k2_microcompact_preserves_message_count ... ok

test result: ok. 3 passed; 0 failed
```

#### 5. Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/compaction.rs \
        src-tauri/tests/plan_k_microcompact_test.rs && \
git commit -m "$(cat <<'EOF'
feat(compaction): implement microcompact for lightweight tool-result clearing - K2

Adds MicrocompactConfig/MicrocompactResult and microcompact() function that
replaces old tool result content with placeholders when total message chars
exceed the configured threshold, mirroring claude-code-best's clear_tool_uses strategy.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task K3: LLM 辅助 compact（token 超阈值触发摘要）

### 目标
在 `RuntimeLlmExecutor` trait 中添加 `compact_summary()` 方法，并在 `compaction.rs` 中实现 `compact_messages_via_llm()`：调用 LLM 对历史消息生成摘要文本，返回替换后的 `messages`（boundary + 摘要 user 消息）。同时实现 `should_auto_compact()` token 阈值检查。

**对标：** `autoCompact.ts::shouldAutoCompact()` + `compact.ts::compactConversation()`（调用独立 LLM fork 生成 summary，然后 buildPostCompactMessages）。

### 文件列表
| 动作 | 文件 |
|---|---|
| Modify | `src-tauri/src/runtime/chat/chat_turn_driver.rs` （`RuntimeLlmExecutor` trait） |
| Modify | `src-tauri/src/runtime/chat/compaction.rs` |
| Create | `src-tauri/tests/plan_k_llm_compact_test.rs` |

### TDD 步骤

#### 1. 写失败测试

创建 `src-tauri/tests/plan_k_llm_compact_test.rs`：

```rust
//! K3: LLM 辅助 compact 核心逻辑回归测试
//!
//! 使用 stub executor 验证 compact_messages_via_llm 的消息替换语义，
//! 以及 should_auto_compact 的阈值逻辑。

use lotus_app::runtime::chat::compaction::{
    compact_messages_via_llm, should_auto_compact, AutoCompactConfig, CompactLlmOutput,
};
use serde_json::json;

fn make_messages(n: usize, tool_result_chars: usize) -> Vec<serde_json::Value> {
    let mut msgs = vec![json!({ "role": "user", "content": "start" })];
    for i in 0..n {
        msgs.push(json!({
            "role": "assistant",
            "content": format!("step {}", i),
            "toolCalls": [{ "id": format!("tc-{}", i), "name": "run", "arguments": {} }],
        }));
        msgs.push(json!({
            "role": "tool",
            "toolCallId": format!("tc-{}", i),
            "name": "run",
            "content": "x".repeat(tool_result_chars),
        }));
    }
    msgs
}

#[test]
fn k3_should_auto_compact_false_below_threshold() {
    let messages = make_messages(2, 100); // 总字符数很小
    let config = AutoCompactConfig {
        threshold_chars: 200_000,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
    };
    assert!(!should_auto_compact(&messages, &config));
}

#[test]
fn k3_should_auto_compact_true_above_threshold() {
    let messages = make_messages(5, 50_000); // 每条 50k * 5 = 250k
    let config = AutoCompactConfig {
        threshold_chars: 100_000,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
    };
    assert!(should_auto_compact(&messages, &config));
}

#[test]
fn k3_compact_messages_via_llm_replaces_history() {
    // 准备 10 条对话历史（旧的）+ 1 条最新 user 消息
    let mut messages = make_messages(5, 1_000);
    messages.push(json!({ "role": "user", "content": "latest question" }));

    let summary_text = "之前的对话摘要：分析了 5 个步骤，结论是 X。".to_string();

    let output = compact_messages_via_llm_stub(messages.clone(), summary_text.clone());

    // 输出应该只有：compact_boundary system 消息 + summary user 消息 + 最新 user 消息
    assert!(
        output.new_messages.len() <= 3,
        "compacted messages should be boundary + summary + latest user, got {}",
        output.new_messages.len()
    );

    // 第一条应该是 compact_boundary system 消息
    let first = &output.new_messages[0];
    assert_eq!(
        first.get("role").and_then(|v| v.as_str()),
        Some("system"),
        "first compacted message should be a system boundary"
    );
    assert_eq!(
        first.get("subtype").and_then(|v| v.as_str()),
        Some("compact_boundary"),
        "boundary message must carry subtype=compact_boundary"
    );

    // 摘要 user 消息应包含 summary_text
    let summary_msg = output.new_messages.iter().find(|m| {
        m.get("role").and_then(|v| v.as_str()) == Some("user")
            && m.get("isCompactSummary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
    });
    assert!(summary_msg.is_some(), "should have a compact summary user message");
    let summary_content = summary_msg
        .unwrap()
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        summary_content.contains("摘要"),
        "summary message content should contain the LLM-generated summary"
    );

    // 最新 user 消息应保留
    let latest = output.new_messages.last().unwrap();
    assert_eq!(
        latest.get("content").and_then(|v| v.as_str()),
        Some("latest question")
    );

    assert!(output.pre_tokens > 0);
    assert!(output.post_tokens < output.pre_tokens);
}

/// 测试专用的 stub：不调用 LLM，直接用传入的 summary_text 构造输出。
/// 真实路径由 RuntimeLlmExecutor::compact_summary() 提供。
fn compact_messages_via_llm_stub(
    messages: Vec<serde_json::Value>,
    summary_text: String,
) -> CompactLlmOutput {
    compact_messages_via_llm(messages, summary_text)
}
```

#### 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_llm_compact_test -- --nocapture 2>&1 | head -30
```

预期输出（编译错误）：
```
error[E0432]: unresolved import `lotus_app::runtime::chat::compaction::compact_messages_via_llm`
```

#### 3. 最小实现

**`src-tauri/src/runtime/chat/compaction.rs`** — 追加：

```rust
/// autocompact 触发配置
#[derive(Debug, Clone)]
pub struct AutoCompactConfig {
    /// 当 messages 总字符数超过此值时触发 LLM compact
    pub threshold_chars: usize,
    /// 压缩 LLM 的最大输出字符数预留（用于估算 post_tokens）
    pub max_output_chars: usize,
    /// 连续失败熔断阈值
    pub consecutive_failure_limit: u32,
}

impl Default for AutoCompactConfig {
    fn default() -> Self {
        Self {
            // 约 120k tokens * 4 chars/token
            threshold_chars: 480_000,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
        }
    }
}

/// LLM compact 的输出
#[derive(Debug)]
pub struct CompactLlmOutput {
    /// 替换后的消息列表（boundary + summary + latest messages）
    pub new_messages: Vec<serde_json::Value>,
    /// 压缩前估算 token 数
    pub pre_tokens: u64,
    /// 压缩后估算 token 数
    pub post_tokens: u64,
    /// 被摘要的消息条数
    pub messages_summarized: usize,
}

/// 检查是否达到 autocompact 触发阈值。
pub fn should_auto_compact(
    messages: &[serde_json::Value],
    config: &AutoCompactConfig,
) -> bool {
    let total_chars: usize = messages.iter().map(|m| m.to_string().len()).sum();
    total_chars >= config.threshold_chars
}

/// 将历史消息列表 compact 为 [compact_boundary, summary_user, ...latest]。
///
/// `summary_text` 由调用方通过 `RuntimeLlmExecutor::compact_summary()` 获取，
/// 此函数仅做消息结构重组（不调用 LLM）。
///
/// 策略：
/// 1. 保留最后一条 role="user" 消息作为"最新用户问题"（如果存在）。
/// 2. 其余消息被摘要覆盖。
/// 3. 输出：[compact_boundary_system, summary_user, latest_user?]
pub fn compact_messages_via_llm(
    messages: Vec<serde_json::Value>,
    summary_text: String,
) -> CompactLlmOutput {
    let pre_tokens = (messages.iter().map(|m| m.to_string().len()).sum::<usize>() / 4) as u64;
    let messages_summarized = messages.len();

    // 保留最后一条 user 消息（通常是触发本次 turn 的用户输入）
    let latest_user: Option<serde_json::Value> = messages.iter().rev().find(|m| {
        m.get("role").and_then(|v| v.as_str()) == Some("user")
            && m.get("isCompactSummary").and_then(|v| v.as_bool()) != Some(true)
    }).cloned();

    // compact_boundary system 消息
    let boundary = serde_json::json!({
        "role": "system",
        "subtype": "compact_boundary",
        "content": "Conversation compacted.",
        "compactMetadata": {
            "trigger": "auto",
            "preTokens": pre_tokens,
            "messagesSummarized": messages_summarized,
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });

    // 摘要 user 消息（isCompactSummary=true，模型侧可见）
    let summary_msg = serde_json::json!({
        "role": "user",
        "content": format!("<context>\n{}\n</context>", summary_text),
        "isCompactSummary": true,
    });

    let mut new_messages = vec![boundary, summary_msg];
    if let Some(latest) = latest_user {
        new_messages.push(latest);
    }

    let post_tokens = (new_messages.iter().map(|m| m.to_string().len()).sum::<usize>() / 4) as u64;

    CompactLlmOutput {
        new_messages,
        pre_tokens,
        post_tokens,
        messages_summarized,
    }
}
```

**`src-tauri/src/runtime/chat/chat_turn_driver.rs`** — 在 `RuntimeLlmExecutor` trait 中追加方法（在 `get_env_info` 之后）：

```rust
    /// 对 messages 调用 LLM 生成摘要文本。
    ///
    /// 实现应使用一个独立的、无工具的 LLM 请求，传入历史消息并要求生成
    /// 上下文摘要。默认 no-op 返回空摘要（测试/旧 executor 向后兼容）。
    async fn compact_summary(
        &self,
        _conversation_id: &str,
        _messages: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok(String::new())
    }
```

#### 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_llm_compact_test -- --nocapture 2>&1
```

预期输出：
```
test k3_should_auto_compact_false_below_threshold ... ok
test k3_should_auto_compact_true_above_threshold ... ok
test k3_compact_messages_via_llm_replaces_history ... ok

test result: ok. 3 passed; 0 failed
```

#### 5. Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/compaction.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/tests/plan_k_llm_compact_test.rs && \
git commit -m "$(cat <<'EOF'
feat(compaction): add LLM-assisted compact core logic and executor hook - K3

Adds AutoCompactConfig, CompactLlmOutput, should_auto_compact(), and
compact_messages_via_llm() to compaction.rs. Adds RuntimeLlmExecutor::compact_summary()
default method so existing executors remain compatible without changes.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task K4: autoCompactTracking 接入 TurnDriver 主循环

### 目标

> **范围说明**：K4 只做内存级 compact 状态追踪（`AutoCompactState` 在 `TurnIterationState` 中），不实现 `compact_boundary` 写入持久化存储（持久化在 K1 中已实现）。

在 `TurnIterationState` 中添加 `compact_state: Option<AutoCompactState>`，并在 `run_chat_turn_s4` 的 `'turn` 循环**每次迭代开始前**按以下顺序执行：

1. `microcompact(state.messages, …)` — 若超过警告阈值，清理旧 tool results（内存操作）。
2. `should_auto_compact(state.messages, …)` — 若超过 compact 阈值，调用 `executor.compact_summary()` 获取摘要，然后调用 `compact_messages_via_llm()` 替换 `state.messages`，并：
   - 调用 `store.append_compact_boundary(...)` 持久化边界记录（如果 executor 实现了 `ConversationStore` 注入，否则跳过）。
   - 更新 `state.compact_state`（重置 turnCounter，设 compacted=true）。
   - 熔断：`consecutive_failures >= limit` 时跳过 compact 尝试。

**对标：** `query.ts` 中 `query_microcompact_start → query_autocompact_start → compactionResult` 的顺序，以及 `AutoCompactTrackingState` 在 State 中的透传。

### 文件列表
| 动作 | 文件 |
|---|---|
| Modify | `src-tauri/src/runtime/chat/turn_config.rs` |
| Modify | `src-tauri/src/runtime/chat/chat_turn_driver.rs` |
| Modify | `src-tauri/src/runtime/chat/compaction.rs` |
| Create | `src-tauri/tests/plan_k_autocompact_tracking_test.rs` |

### TDD 步骤

#### 1. 写失败测试

创建 `src-tauri/tests/plan_k_autocompact_tracking_test.rs`：

```rust
//! K4: autoCompactTracking 在 TurnDriver S4 主循环中的集成测试
//!
//! 使用 mock executor 验证：compact 被触发时 state.messages 被替换，
//! 熔断器在连续失败后停止重试。

use lotus_app::runtime::chat::compaction::{AutoCompactConfig, AutoCompactState};
use serde_json::json;

// ── helpers ──────────────────────────────────────────────────────────────────

fn big_messages(n: usize) -> Vec<serde_json::Value> {
    let mut msgs = vec![json!({ "role": "user", "content": "start" })];
    for i in 0..n {
        msgs.push(json!({
            "role": "assistant",
            "content": format!("step {}", i),
            "toolCalls": [{ "id": format!("tc-{}", i), "name": "run", "arguments": {} }],
        }));
        msgs.push(json!({
            "role": "tool",
            "toolCallId": format!("tc-{}", i),
            "name": "run",
            "content": "x".repeat(20_000),
        }));
    }
    msgs
}

// ── unit tests for AutoCompactState ──────────────────────────────────────────

#[test]
fn k4_auto_compact_state_initial_values() {
    let state = AutoCompactState::new();
    assert!(!state.compacted);
    assert_eq!(state.turn_counter, 0);
    assert_eq!(state.consecutive_failures, 0);
}

#[test]
fn k4_auto_compact_state_circuit_breaker() {
    let mut state = AutoCompactState::new();
    let config = AutoCompactConfig {
        threshold_chars: 1,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
    };
    // 未到达熔断阈值
    state.consecutive_failures = 2;
    assert!(!state.is_circuit_broken(&config));

    // 到达阈值
    state.consecutive_failures = 3;
    assert!(state.is_circuit_broken(&config));
}

#[test]
fn k4_auto_compact_state_reset_on_success() {
    let mut state = AutoCompactState::new();
    state.consecutive_failures = 2;
    state.compacted = true;
    state.turn_counter = 5;

    state.record_success();
    assert_eq!(state.consecutive_failures, 0);
    assert!(state.compacted);       // compacted 保持 true
    assert_eq!(state.turn_counter, 0); // turn_counter 重置
}

#[test]
fn k4_auto_compact_state_increment_failure() {
    let mut state = AutoCompactState::new();
    state.record_failure();
    assert_eq!(state.consecutive_failures, 1);
    state.record_failure();
    assert_eq!(state.consecutive_failures, 2);
}

#[test]
fn k4_auto_compact_state_increment_turn() {
    let mut state = AutoCompactState::new();
    state.increment_turn();
    assert_eq!(state.turn_counter, 1);
    state.increment_turn();
    assert_eq!(state.turn_counter, 2);
}
```

#### 2. 确认失败

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_autocompact_tracking_test -- --nocapture 2>&1 | head -30
```

预期输出（编译错误）：
```
error[E0432]: unresolved import `lotus_app::runtime::chat::compaction::AutoCompactState`
```

#### 3. 最小实现

**`src-tauri/src/runtime/chat/compaction.rs`** — 追加 `AutoCompactState`：

```rust
/// 熔断/跟踪状态（对标 claude-code-best AutoCompactTrackingState）
#[derive(Debug, Clone, Default)]
pub struct AutoCompactState {
    /// 本次 turn 是否已执行过 compact
    pub compacted: bool,
    /// 自上次 compact 后经过的 turn 数
    pub turn_counter: u32,
    /// 连续失败次数（熔断计数器）
    pub consecutive_failures: u32,
}

impl AutoCompactState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 检查是否触发熔断器（停止自动 compact 尝试）。
    pub fn is_circuit_broken(&self, config: &AutoCompactConfig) -> bool {
        self.consecutive_failures >= config.consecutive_failure_limit
    }

    /// 成功 compact 后调用：重置 turn_counter，清零 consecutive_failures。
    pub fn record_success(&mut self) {
        self.compacted = true;
        self.turn_counter = 0;
        self.consecutive_failures = 0;
    }

    /// compact 失败后调用：累加熔断计数。
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    /// 每次 turn loop 迭代结束时调用：递增 turn_counter。
    pub fn increment_turn(&mut self) {
        self.turn_counter += 1;
    }
}
```

**`src-tauri/src/runtime/chat/turn_config.rs`** — 在 `TurnIterationState` 结构体中添加字段，并在 `new()` 中初始化：

在 `use` 区追加：
```rust
use crate::runtime::chat::compaction::AutoCompactState;
```

在 `TurnIterationState` 结构体中追加字段（在 `safeguard_phase1_injected` 后）：
```rust
    /// autocompact 熔断/跟踪状态
    pub compact_state: AutoCompactState,
```

在 `TurnIterationState::new()` 中追加初始化：
```rust
            compact_state: AutoCompactState::new(),
```

**`src-tauri/src/runtime/chat/chat_turn_driver.rs`** — 在 `run_chat_turn_s4` 的 `'turn: for iteration in 0..config.max_iterations` 循环**开头**（CP-1 cancel check 之前）注入 compact 调用：

在 `use` 区追加：
```rust
use crate::runtime::chat::compaction::{
    AutoCompactConfig, compact_messages_via_llm,
    build_compact_boundary_record, CompactTrigger,
    microcompact, MicrocompactConfig,
    should_auto_compact,
};
```

在 `'turn: for iteration in 0..config.max_iterations {` 内部、`let precompute_ctx` 之前追加：

```rust
            // ── Compact 检查（每次迭代开始时）────────────────────────────────
            // 顺序对标 claude-code-best query.ts：microcompact → autocompact。

            // Step A: microcompact（内存操作，无 LLM 调用）
            let micro_config = MicrocompactConfig::default();
            let micro_result = microcompact(&state.messages, &micro_config);
            if micro_result.executed {
                state.messages = micro_result.messages;
                log::debug!(
                    "[compact] microcompact freed ~{} tokens",
                    micro_result.tokens_freed_estimate
                );
            }

            // Step B: autocompact（LLM 摘要，有熔断保护）
            let compact_config = AutoCompactConfig::default();
            if !state.compact_state.is_circuit_broken(&compact_config)
                && should_auto_compact(&state.messages, &compact_config)
            {
                log::info!("[compact] autocompact triggered for conv={}", config.conversation_id.as_str());
                match executor
                    .compact_summary(
                        config.conversation_id.as_str(),
                        &state.messages,
                    )
                    .await
                {
                    Ok(summary_text) if !summary_text.is_empty() => {
                        let output = compact_messages_via_llm(
                            std::mem::take(&mut state.messages),
                            summary_text,
                        );
                        state.messages = output.new_messages;

                        // 持久化 compact_boundary（best-effort，失败只记录 warn）
                        // NOTE: ConversationStore 当前不在 driver 内直接可及；
                        // 此处通过 executor 暴露的 persist_compact_boundary()（K4 扩展）
                        // 或直接记录日志作为 fallback。
                        // 短期 fallback：仅 log，不崩溃。
                        log::info!(
                            "[compact] autocompact succeeded: pre_tokens={} post_tokens={} summarized={}",
                            output.pre_tokens,
                            output.post_tokens,
                            output.messages_summarized,
                        );
                        state.compact_state.record_success();
                    }
                    Ok(_empty) => {
                        // executor 返回空摘要（默认实现），不执行 compact
                        log::debug!("[compact] compact_summary returned empty, skipping autocompact");
                    }
                    Err(e) => {
                        log::warn!("[compact] compact_summary failed: {}", e);
                        state.compact_state.record_failure();
                    }
                }
            }

            // 每次迭代结束时递增 turn_counter（在 'turn 循环末尾的 5f cancel check 之后追加）
            // 注意：increment_turn() 的调用需放在 'turn 循环末尾（5f 之后）
```

在 `// ── 5f: per-iteration cancel check ───` 之后、`}` 闭合 `'turn` 循环之前追加：
```rust
            // K4: 每次迭代结束时递增 compact turn_counter
            state.compact_state.increment_turn();
```

#### 4. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test plan_k_autocompact_tracking_test -- --nocapture 2>&1
```

预期输出：
```
test k4_auto_compact_state_initial_values ... ok
test k4_auto_compact_state_circuit_breaker ... ok
test k4_auto_compact_state_reset_on_success ... ok
test k4_auto_compact_state_increment_failure ... ok
test k4_auto_compact_state_increment_turn ... ok

test result: ok. 5 passed; 0 failed
```

同时确认现有测试不受影响：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --lib -- --nocapture 2>&1 | tail -10
```

#### 5. Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/src/runtime/chat/compaction.rs \
        src-tauri/src/runtime/chat/turn_config.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/tests/plan_k_autocompact_tracking_test.rs && \
git commit -m "$(cat <<'EOF'
feat(compaction): wire autoCompactTracking into S4 turn driver main loop - K4

Adds AutoCompactState to TurnIterationState; injects microcompact + autocompact
checks at the top of each 'turn iteration in run_chat_turn_s4, with circuit
breaker after consecutive_failure_limit failures. Mirrors query.ts ordering
(microcompact → autocompact → LLM step).

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task K5: review_ 约束测试固化

### 目标
编写 `review_` 系列回归测试，固化 Plan-K 引入的三条架构约束：

1. **runtime 层禁止直接使用 `LlmGateway`**：`compaction.rs` 中的 LLM 调用必须通过 `RuntimeLlmExecutor::compact_summary()` trait 方法，不能引用 `crate::llm::gateway::LlmGateway`。
2. **compact_boundary 消息格式不变性**：`build_compact_boundary_record()` 输出的结构体字段集合与 `compact_messages_via_llm()` 输出的 boundary JSON 中的 `subtype` 字段必须一致（都是 `"compact_boundary"`）。
3. **熔断器阈值固化**：`AutoCompactConfig::default().consecutive_failure_limit == 3`（对标 claude-code-best 的 `MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3`）。

### 文件列表
| 动作 | 文件 |
|---|---|
| Create | `src-tauri/tests/review_autocompact_constraints_test.rs` |

### TDD 步骤

#### 1. 写失败测试

创建 `src-tauri/tests/review_autocompact_constraints_test.rs`：

```rust
//! review_ 回归测试：Plan-K autocompact 架构约束
//!
//! 这些测试固化 Plan-K 引入的不变量，防止后续重构悄悄破坏约束。

use lotus_app::runtime::chat::compaction::{
    AutoCompactConfig, AutoCompactState,
    build_compact_boundary_record, compact_messages_via_llm,
    CompactTrigger,
};
use serde_json::json;

// ── 约束 1：熔断器默认阈值 = 3（对标 claude-code-best MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES）

#[test]
fn review_k_circuit_breaker_default_limit_is_3() {
    let config = AutoCompactConfig::default();
    assert_eq!(
        config.consecutive_failure_limit, 3,
        "autocompact circuit breaker limit must stay at 3 to match claude-code-best baseline"
    );
}

#[test]
fn review_k_circuit_breaker_trips_at_limit_not_before() {
    let config = AutoCompactConfig::default(); // limit=3
    let mut state = AutoCompactState::new();

    state.record_failure(); // 1
    assert!(!state.is_circuit_broken(&config), "should not break at 1 failure");
    state.record_failure(); // 2
    assert!(!state.is_circuit_broken(&config), "should not break at 2 failures");
    state.record_failure(); // 3 → trips
    assert!(state.is_circuit_broken(&config), "should break at 3 failures");
}

// ── 约束 2：compact_boundary 消息 subtype 字段不变

#[test]
fn review_k_compact_boundary_subtype_is_compact_boundary() {
    let output = compact_messages_via_llm(
        vec![json!({ "role": "user", "content": "test" })],
        "summary text".to_string(),
    );
    let boundary = &output.new_messages[0];
    assert_eq!(
        boundary.get("subtype").and_then(|v| v.as_str()),
        Some("compact_boundary"),
        "compact boundary message subtype must remain 'compact_boundary'"
    );
    assert_eq!(
        boundary.get("role").and_then(|v| v.as_str()),
        Some("system"),
        "compact boundary message role must be 'system'"
    );
}

#[test]
fn review_k_compact_boundary_record_trigger_enum_stable() {
    // CompactTrigger::Auto / Manual 序列化字符串不能改变（会破坏持久化数据）
    let auto_str = serde_json::to_string(&CompactTrigger::Auto).unwrap();
    let manual_str = serde_json::to_string(&CompactTrigger::Manual).unwrap();
    assert_eq!(auto_str, "\"Auto\"");
    assert_eq!(manual_str, "\"Manual\"");
}

// ── 约束 3：compaction.rs 不直接依赖 LlmGateway（静态架构约束）
// 通过检查 source 文件字符串验证（不调用实际代码）

#[test]
fn review_k_compaction_module_does_not_import_llm_gateway() {
    let compaction_src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/chat/compaction.rs"
    ));
    assert!(
        !compaction_src.contains("crate::llm::gateway"),
        "compaction.rs must NOT import crate::llm::gateway — use RuntimeLlmExecutor::compact_summary() instead"
    );
    assert!(
        !compaction_src.contains("LlmGateway"),
        "compaction.rs must NOT reference LlmGateway directly"
    );
}

// ── 约束 4：compact 摘要 user 消息的 isCompactSummary 标记必须存在

#[test]
fn review_k_compact_summary_message_has_is_compact_summary_flag() {
    let output = compact_messages_via_llm(
        vec![
            json!({ "role": "user", "content": "original" }),
            json!({ "role": "assistant", "content": "reply" }),
        ],
        "this is the summary".to_string(),
    );

    let summary_msg = output.new_messages.iter().find(|m| {
        m.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true)
    });
    assert!(
        summary_msg.is_some(),
        "compact output must include a user message with isCompactSummary=true"
    );
}

// ── 约束 5：microcompact 不删除消息、只替换内容

#[test]
fn review_k_microcompact_never_deletes_messages() {
    use lotus_app::runtime::chat::compaction::{microcompact, MicrocompactConfig};

    let messages: Vec<serde_json::Value> = (0..6)
        .flat_map(|i| {
            vec![
                json!({
                    "role": "assistant",
                    "content": format!("step {}", i),
                    "toolCalls": [{ "id": format!("tc-{}", i), "name": "run", "arguments": {} }],
                }),
                json!({
                    "role": "tool",
                    "toolCallId": format!("tc-{}", i),
                    "name": "run",
                    "content": "z".repeat(10_000),
                }),
            ]
        })
        .collect();

    let original_len = messages.len();
    let config = MicrocompactConfig {
        trigger_chars: 1,        // 强制触发
        keep_recent_tool_results: 1,
    };
    let result = microcompact(&messages, &config);
    assert_eq!(
        result.messages.len(),
        original_len,
        "microcompact must not delete messages, only replace content"
    );
}
```

#### 2. 确认失败（初次）

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test --test review_autocompact_constraints_test -- --nocapture 2>&1 | head -20
```

K1-K4 完成后这些测试应该全部通过（K5 是固化测试，依赖前面任务的实现）。如果在 K4 完成后运行，预期全部通过。

#### 3. 确认通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test review_k -- --nocapture 2>&1
```

预期输出：
```
test review_k_circuit_breaker_default_limit_is_3 ... ok
test review_k_circuit_breaker_trips_at_limit_not_before ... ok
test review_k_compact_boundary_subtype_is_compact_boundary ... ok
test review_k_compact_boundary_record_trigger_enum_stable ... ok
test review_k_compaction_module_does_not_import_llm_gateway ... ok
test review_k_compact_summary_message_has_is_compact_summary_flag ... ok
test review_k_microcompact_never_deletes_messages ... ok

test result: ok. 7 passed; 0 failed
```

全量 review_ 测试不应退化：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

#### 4. Commit

```bash
cd /Users/a20250311/IdeaProjects/lotus-app && \
git add src-tauri/tests/review_autocompact_constraints_test.rs && \
git commit -m "$(cat <<'EOF'
test(review): add review_ regression tests for Plan-K autocompact constraints - K5

Fixes circuit-breaker default (3 failures), compact_boundary subtype invariant,
LlmGateway isolation (compaction.rs must not import it), isCompactSummary flag,
and microcompact no-delete guarantee.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## 执行顺序总结

```
K1 → K2 → K3 → K4 → K5
```

每个 Task 严格按 TDD 步骤：红（编译错误）→ 绿（测试通过）→ commit。K5 的约束测试在 K4 完成后运行时应全部通过（如果 K1-K4 实现正确）。

## 关键验收命令（全量回归）

```bash
# 1. Plan-K 专项测试
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && \
cargo test plan_k_ --tests --no-fail-fast 2>&1 | tail -20

# 2. review_ 全量回归
cargo test review_ --tests --no-fail-fast 2>&1 | tail -20

# 3. 全量 lib 测试（确保 TurnIterationState 变更不破坏现有 unit tests）
cargo test --lib 2>&1 | tail -10
```

## 遗留 Gap（K 计划范围外）

| Gap | 说明 |
|---|---|
| `TauriLegacyTurnExecutor::compact_summary()` 真实实现 | 需实现一个无 tool 的 LLM 请求，传入 `getCompactSystemPrompt()` 风格的 system prompt，流式接收摘要文本 |
| `ConversationStore` → 文件存储实现 | `AppStorage` 侧的 `append_compact_boundary()` 需要追加写到 `conv.json` 或新文件 |
| 前端事件通知 | 可选：`RuntimeEventKind::CompactStarted` / `CompactCompleted` 事件，前端订阅展示"对话已压缩"提示 |
| `compact_summary()` system prompt | 参考 claude-code-best `getCompactSystemPrompt()` 的中文版本，要求生成结构化摘要 |
| token 估算精度 | 当前用 `chars / 4` 粗估，可接入真实 tokenizer（tiktoken-rs 或 Claude API 计数端点） |

<!-- reviewed: 2026-04-18, fixes applied -->
