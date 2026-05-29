# Prompt Caching（Plan-AA）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Claude provider 的 system prompt 和 tools list 末尾添加 `cache_control` breakpoint，降低 90% 输入 token 成本
**Architecture:** 先做 lotus 的最小正确对齐：修改 `ClaudeProvider::build_request_body()`，system prompt 从顶层字符串改为 content block 数组（末尾 block 带 `cache_control`），tools list 最后一项加 `cache_control`；通过 `supports_prompt_caching()` trait method 隔离，其他 provider 返回 false，不受影响。**注意：这不是完整复刻 `claude-code-best` 的 global cache scope / 1h TTL / message breakpoint 策略，只是当前 lotus Claude provider 的最小安全接入。**
**Tech Stack:** Rust, serde_json
**Worktree branch:** pzc

---

## 背景与约束

### 当前状态（需修改）

`src-tauri/src/llm/providers/claude.rs` `build_request_body()` 第 141–145 行：

```rust
if let Some(system) = system_content {
    if !system.is_empty() {
        body["system"] = json!(system);   // ← 字符串，无 cache_control
    }
}
```

第 153–165 行：

```rust
if !request.tools.is_empty() {
    let tools: Vec<Value> = request.tools.iter().map(|t| {
        json!({
            "name": t.name,
            "description": t.description,
            "input_schema": t.parameters,
        })
    }).collect();
    body["tools"] = json!(tools);  // ← 无 cache_control
}
```

### Anthropic Prompt Caching 规则

- `cache_control: { "type": "ephemeral" }` 必须放在 content block **自身**的字段中
- 每次请求最多 **4 个** cache breakpoint；本计划只用 2 个（system + tools 末项），安全范围内
- system prompt 使用 block 数组格式：`"system": [{"type": "text", "text": "...", "cache_control": {"type": "ephemeral"}}]`
- tools list 仅在最后一个工具对象上插入 `"cache_control": {"type": "ephemeral"}`，前面的工具不加
- 仅限 Anthropic Messages API；其他 provider（OpenAI、DeepSeek 等）无此字段，不能污染

### 对标修订（2026-04-19）

- `claude-code-best` 的完整实现位于 `src/services/api/claude.ts`，其真实策略比 lotus 当前计划更复杂：包含 system/tool/message 多层 breakpoint、global cache scope、1h TTL、以及基于动态工具集的 cache strategy 选择。
- lotus 当前 `ClaudeProvider` 仅负责构造基础 Anthropic Messages API body，没有 `querySource`、TTL、global scope、message breakpoint 等上层状态，因此本计划只接入 **system + tools** 两个静态 breakpoint，不伪实现 `claude-code-best` 的高级缓存策略。
- Anthropic 官方文档确认 prompt cache 前缀顺序是 **`tools -> system -> messages`**；因此把静态内容（system + tools）标记为缓存前缀是正确方向。
- `src-tauri/tests/plan_aa_prompt_caching_test.rs` 是 integration test，不能依赖 `#[cfg(test)]` 才存在的 crate API；若测试需要读取 request body，helper 必须在库导出中可见（可用 `#[doc(hidden)] pub fn` 形式），不能写成 `#[cfg(test)] pub fn`。
- 现有 crate 名为 `app_lib`，不是 `lotus_app`；测试 import 需要按实际 crate 名修正。

### 接口约束

`LlmProviderTrait`（`src-tauri/src/llm/providers/mod.rs`）需新增带默认实现的方法 `supports_prompt_caching()`，其他 provider 无需改动即自动返回 `false`。

---

## Task AA1 — 新增 `supports_prompt_caching()` trait method

### 文件

| 操作 | 文件 |
|------|------|
| Modify | `src-tauri/src/llm/providers/mod.rs` |
| Modify | `src-tauri/src/llm/providers/claude.rs` |
| Test   | `src-tauri/tests/plan_aa_prompt_caching_test.rs` |

### TDD 步骤

- [ ] 新建 `src-tauri/tests/plan_aa_prompt_caching_test.rs`，写入 AA1 失败测试
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test review_only_claude_supports_prompt_caching -- --nocapture` 确认编译失败（方法不存在）
- [ ] 在 `LlmProviderTrait` 新增默认方法 `supports_prompt_caching() -> bool { false }`
- [ ] 在 `impl LlmProviderTrait for ClaudeProvider` 覆写为 `true`
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test review_only_claude_supports_prompt_caching -- --nocapture` 确认通过
- [ ] commit

### 测试代码

新建 `src-tauri/tests/plan_aa_prompt_caching_test.rs`：

```rust
//! Plan-AA: Prompt Caching — architecture regression tests
//!
//! AA1: supports_prompt_caching() trait contract
//! AA2: system prompt serialized as cache_control content block
//! AA3: last tool in tools list carries cache_control breakpoint
//! AA4: total cache breakpoints in request body never exceed 4

use app_lib::llm::providers::claude::ClaudeProvider;
use app_lib::llm::providers::LlmProviderTrait;
use app_lib::llm::streaming::{ChatMessage, LlmRequest, ToolDefinition};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_provider() -> ClaudeProvider {
    ClaudeProvider::new("test-key".to_string(), None)
}

fn build_body(provider: &ClaudeProvider, request: &LlmRequest) -> Value {
    provider.build_request_body_for_test(request)
}

fn make_tool(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: format!("Tool {}", name),
        parameters: json!({"type": "object", "properties": {}}),
    }
}

// ---------------------------------------------------------------------------
// AA1: Only ClaudeProvider returns true for supports_prompt_caching
// ---------------------------------------------------------------------------

#[test]
fn review_only_claude_supports_prompt_caching() {
    let claude = test_provider();
    assert!(
        claude.supports_prompt_caching(),
        "ClaudeProvider must return true for supports_prompt_caching()"
    );
}
```

### 实现代码

**`src-tauri/src/llm/providers/mod.rs`** — 在 `LlmProviderTrait` 中，紧跟 `supports_streaming` 之后新增：

```rust
/// Whether this provider supports Anthropic-style prompt caching
/// via cache_control content blocks. Defaults to false.
/// Only ClaudeProvider overrides to true.
fn supports_prompt_caching(&self) -> bool {
    false
}
```

**`src-tauri/src/llm/providers/claude.rs`** — 在 `impl LlmProviderTrait for ClaudeProvider` 中，紧跟 `supports_streaming` 覆写之后新增：

```rust
fn supports_prompt_caching(&self) -> bool {
    true
}
```

同时，在 `impl ClaudeProvider` 块中新增测试专用公开包装（AA2 测试需要）。**不要加 `#[cfg(test)]`，否则 integration test 看不到该方法。**

```rust
/// Expose build_request_body for integration tests.
#[doc(hidden)]
pub fn build_request_body_for_test(&self, request: &LlmRequest) -> serde_json::Value {
    self.build_request_body(request)
}
```

### cargo test 命令与预期输出

```bash
cd src-tauri && cargo test --test plan_aa_prompt_caching_test review_only_claude_supports_prompt_caching -- --nocapture
```

预期输出：

```
running 1 test
test review_only_claude_supports_prompt_caching ... ok

test result: ok. 1 passed; 0 failed
```

### git commit

```bash
git add src-tauri/src/llm/providers/mod.rs \
        src-tauri/src/llm/providers/claude.rs \
        src-tauri/tests/plan_aa_prompt_caching_test.rs
git commit -m "$(cat <<'EOF'
feat(llm): add supports_prompt_caching() to LlmProviderTrait - AA1

ClaudeProvider overrides to true; all other providers use default false.
Also exposes build_request_body_for_test for integration tests.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task AA2 — system prompt 改为带 `cache_control` 的 content block 数组

### 文件

| 操作 | 文件 |
|------|------|
| Modify | `src-tauri/src/llm/providers/claude.rs` |
| Test   | `src-tauri/tests/plan_aa_prompt_caching_test.rs` |

### TDD 步骤

- [ ] 在测试文件追加 AA2 测试（见下方）
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test system_prompt_serialized_as_cache_control_block -- --nocapture` 确认失败（当前 system 是字符串）
- [ ] 修改 `build_request_body()` 的 system prompt 赋值逻辑
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test -- --nocapture` 确认全部通过
- [ ] commit

### 测试代码

追加到 `src-tauri/tests/plan_aa_prompt_caching_test.rs`：

```rust
// ---------------------------------------------------------------------------
// AA2: system prompt is a cache_control content block array
// ---------------------------------------------------------------------------

#[test]
fn system_prompt_serialized_as_cache_control_block() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "You are a helpful assistant."),
            ChatMessage::text("user", "Hello"),
        ],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);

    let system = &body["system"];
    assert!(
        system.is_array(),
        "system must be a content block array, got: {}",
        system
    );

    let blocks = system.as_array().unwrap();
    assert_eq!(blocks.len(), 1, "Expected exactly one system block");

    let block = &blocks[0];
    assert_eq!(block["type"], "text", "Block type must be 'text'");
    assert_eq!(
        block["text"], "You are a helpful assistant.",
        "Block text must match original system prompt"
    );
    assert_eq!(
        block["cache_control"]["type"], "ephemeral",
        "cache_control.type must be 'ephemeral'"
    );
}

#[test]
fn system_prompt_absent_when_no_system_message() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "Hello")],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    assert!(
        body.get("system").is_none(),
        "system key must be absent when no system message is provided"
    );
}
```

### 实现代码

**`src-tauri/src/llm/providers/claude.rs`** — 修改 system prompt 赋值段（约第 141–145 行）：

```rust
if let Some(system) = system_content {
    if !system.is_empty() {
        if self.supports_prompt_caching() {
            // Anthropic Prompt Caching: system must be a content block array
            // to support cache_control on the last block.
            body["system"] = json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" },
            }]);
        } else {
            body["system"] = json!(system);
        }
    }
}
```

### cargo test 命令与预期输出

```bash
cd src-tauri && cargo test --test plan_aa_prompt_caching_test -- --nocapture
```

预期输出：

```
running 3 tests
test review_only_claude_supports_prompt_caching ... ok
test system_prompt_serialized_as_cache_control_block ... ok
test system_prompt_absent_when_no_system_message ... ok

test result: ok. 3 passed; 0 failed
```

既有 claude 单测不退化：

```bash
cd src-tauri && cargo test -p lotus-app -- llm::providers::claude 2>&1 | tail -3
```

预期：`test result: ok. N passed; 0 failed`

### git commit

```bash
git add src-tauri/src/llm/providers/claude.rs \
        src-tauri/tests/plan_aa_prompt_caching_test.rs
git commit -m "$(cat <<'EOF'
feat(llm): convert system prompt to content block array with cache_control - AA2

Anthropic caching requires system field to be a block array;
last block gets cache_control=ephemeral to mark cache breakpoint.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task AA3 — tools list 最后一项加 `cache_control`

### 文件

| 操作 | 文件 |
|------|------|
| Modify | `src-tauri/src/llm/providers/claude.rs` |
| Test   | `src-tauri/tests/plan_aa_prompt_caching_test.rs` |

### TDD 步骤

- [ ] 追加 AA3 失败测试（见下方）
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test last_tool_has_cache_control -- --nocapture` 确认失败
- [ ] 修改 tools list 构建逻辑，仅对最后一项插入 `cache_control`
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test -- --nocapture` 确认全部通过
- [ ] commit

### 测试代码

追加到 `src-tauri/tests/plan_aa_prompt_caching_test.rs`：

```rust
// ---------------------------------------------------------------------------
// AA3: Last tool in tools list carries cache_control breakpoint
// ---------------------------------------------------------------------------

#[test]
fn last_tool_has_cache_control() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "go")],
        tools: vec![
            make_tool("tool_alpha"),
            make_tool("tool_beta"),
            make_tool("tool_gamma"),
        ],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let tools = body["tools"].as_array().expect("tools must be an array");
    assert_eq!(tools.len(), 3);

    // Last tool must have cache_control
    let last = &tools[2];
    assert_eq!(last["name"], "tool_gamma");
    assert_eq!(
        last["cache_control"]["type"], "ephemeral",
        "Last tool must have cache_control.type=ephemeral"
    );

    // Non-last tools must NOT have cache_control
    for tool in &tools[..2] {
        assert!(
            tool.get("cache_control").is_none(),
            "Non-last tool '{}' must not have cache_control",
            tool["name"]
        );
    }
}

#[test]
fn single_tool_has_cache_control() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "go")],
        tools: vec![make_tool("only_tool")],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(
        tools[0]["cache_control"]["type"], "ephemeral",
        "Single tool must have cache_control"
    );
}

#[test]
fn no_tools_does_not_add_tools_key() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "go")],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    assert!(
        body.get("tools").is_none(),
        "tools key must be absent when request.tools is empty"
    );
}
```

### 实现代码

**`src-tauri/src/llm/providers/claude.rs`** — 修改 tools list 构建段（约第 153–165 行）：

```rust
if !request.tools.is_empty() {
    let mut tools: Vec<Value> = request
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect();

    // Add cache_control breakpoint to the last tool (Anthropic Prompt Caching).
    // Only the last tool gets the breakpoint to avoid exceeding the 4-breakpoint limit.
    if self.supports_prompt_caching() {
        if let Some(last) = tools.last_mut() {
            if let Some(obj) = last.as_object_mut() {
                obj.insert(
                    "cache_control".to_string(),
                    json!({ "type": "ephemeral" }),
                );
            }
        }
    }

    body["tools"] = json!(tools);
}
```

### cargo test 命令与预期输出

```bash
cd src-tauri && cargo test --test plan_aa_prompt_caching_test -- --nocapture
```

预期输出：

```
running 6 tests
test review_only_claude_supports_prompt_caching ... ok
test system_prompt_serialized_as_cache_control_block ... ok
test system_prompt_absent_when_no_system_message ... ok
test last_tool_has_cache_control ... ok
test single_tool_has_cache_control ... ok
test no_tools_does_not_add_tools_key ... ok

test result: ok. 6 passed; 0 failed
```

### git commit

```bash
git add src-tauri/src/llm/providers/claude.rs \
        src-tauri/tests/plan_aa_prompt_caching_test.rs
git commit -m "$(cat <<'EOF'
feat(llm): add cache_control breakpoint to last tool in tools list - AA3

Only the final tool definition gets the ephemeral cache breakpoint;
preceding tools are unchanged to avoid exceeding the 4-breakpoint limit.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task AA4 — 集成验证：cache breakpoint 总数不超过 4 个

### 文件

| 操作 | 文件 |
|------|------|
| Test   | `src-tauri/tests/plan_aa_prompt_caching_test.rs` |

### TDD 步骤

- [ ] 追加 AA4 测试（见下方）
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test cache_breakpoints_do_not_exceed_api_limit -- --nocapture` 确认通过（AA2+AA3 实现后 breakpoint 总数为 2，不超限）
- [ ] `cd src-tauri && cargo test --test plan_aa_prompt_caching_test -- --nocapture` 确认全套全部通过
- [ ] commit

### 测试代码

追加到 `src-tauri/tests/plan_aa_prompt_caching_test.rs`：

```rust
// ---------------------------------------------------------------------------
// AA4: Total cache breakpoints in request body never exceed 4 (Anthropic limit)
// ---------------------------------------------------------------------------

/// Recursively count JSON objects that have a "cache_control" key.
fn count_cache_breakpoints(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            let self_count = if map.contains_key("cache_control") { 1 } else { 0 };
            // Do NOT recurse into cache_control's own value to avoid double-counting
            let child_count: usize = map
                .iter()
                .filter(|(k, _)| k.as_str() != "cache_control")
                .map(|(_, v)| count_cache_breakpoints(v))
                .sum();
            self_count + child_count
        }
        Value::Array(arr) => arr.iter().map(count_cache_breakpoints).sum(),
        _ => 0,
    }
}

#[test]
fn cache_breakpoints_do_not_exceed_api_limit() {
    let provider = test_provider();

    // Worst-case: system prompt + 20 tools — breakpoints must still be ≤ 4
    let tools: Vec<ToolDefinition> = (0..20)
        .map(|i| make_tool(&format!("tool_{:02}", i)))
        .collect();

    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "You are an agent with many tools."),
            ChatMessage::text("user", "Use whatever tools you need."),
        ],
        tools,
        max_tokens: 4096,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let breakpoint_count = count_cache_breakpoints(&body);

    assert!(
        breakpoint_count <= 4,
        "Request body contains {} cache breakpoints; Anthropic API allows at most 4",
        breakpoint_count
    );
}

#[test]
fn cache_breakpoints_count_system_plus_last_tool_equals_two() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "System prompt here."),
            ChatMessage::text("user", "Hello"),
        ],
        tools: vec![make_tool("alpha"), make_tool("beta")],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let breakpoint_count = count_cache_breakpoints(&body);

    assert_eq!(
        breakpoint_count, 2,
        "Expected exactly 2 breakpoints (system + last tool), got {}",
        breakpoint_count
    );
}

#[test]
fn cache_breakpoints_count_system_only_when_no_tools() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![
            ChatMessage::text("system", "System prompt."),
            ChatMessage::text("user", "Hi"),
        ],
        tools: vec![],
        max_tokens: 1024,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let breakpoint_count = count_cache_breakpoints(&body);

    assert_eq!(
        breakpoint_count, 1,
        "Expected exactly 1 breakpoint (system only), got {}",
        breakpoint_count
    );
}

#[test]
fn cache_breakpoints_zero_when_no_system_no_tools() {
    let provider = test_provider();
    let request = LlmRequest {
        messages: vec![ChatMessage::text("user", "hello")],
        tools: vec![],
        max_tokens: 512,
        temperature: 1.0,
        stream: false,
    };

    let body = build_body(&provider, &request);
    let breakpoint_count = count_cache_breakpoints(&body);

    assert_eq!(
        breakpoint_count, 0,
        "No breakpoints expected when system and tools are both absent"
    );
}
```

### cargo test 命令与预期输出

```bash
cd src-tauri && cargo test --test plan_aa_prompt_caching_test -- --nocapture
```

预期输出：

```
running 10 tests
test review_only_claude_supports_prompt_caching ... ok
test system_prompt_serialized_as_cache_control_block ... ok
test system_prompt_absent_when_no_system_message ... ok
test last_tool_has_cache_control ... ok
test single_tool_has_cache_control ... ok
test no_tools_does_not_add_tools_key ... ok
test cache_breakpoints_do_not_exceed_api_limit ... ok
test cache_breakpoints_count_system_plus_last_tool_equals_two ... ok
test cache_breakpoints_count_system_only_when_no_tools ... ok
test cache_breakpoints_zero_when_no_system_no_tools ... ok

test result: ok. 10 passed; 0 failed
```

全量 llm 模块回归（确认内置单测无退化）：

```bash
cd src-tauri && cargo test llm -- --nocapture 2>&1 | tail -3
```

预期：`test result: ok. N passed; 0 failed`

### git commit

```bash
git add src-tauri/tests/plan_aa_prompt_caching_test.rs
git commit -m "$(cat <<'EOF'
test(llm): add cache breakpoint count guard (≤4) for prompt caching - AA4

Regression tests ensure system + tools cache breakpoints never exceed
the Anthropic API limit of 4, regardless of tools list size.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## 完整 cargo test 命令汇总

```bash
# 全部 Plan-AA 测试
cd src-tauri && cargo test --test plan_aa_prompt_caching_test -- --nocapture

# 单个 Task 验证
cd src-tauri && cargo test --test plan_aa_prompt_caching_test review_only_claude_supports_prompt_caching -- --nocapture
cd src-tauri && cargo test --test plan_aa_prompt_caching_test system_prompt_serialized_as_cache_control_block -- --nocapture
cd src-tauri && cargo test --test plan_aa_prompt_caching_test last_tool_has_cache_control -- --nocapture
cd src-tauri && cargo test --test plan_aa_prompt_caching_test cache_breakpoints_do_not_exceed_api_limit -- --nocapture

# 全量回归（确认 claude.rs 内置测试不退化）
cd src-tauri && cargo test llm -- --nocapture
```

---

## 完成验收清单

- [ ] AA1：`supports_prompt_caching()` 加入 trait，Claude=true，其他=false
- [ ] AA1：`build_request_body_for_test` 公开包装已添加（integration test 可见，不使用 `#[cfg(test)]`）
- [ ] AA2：system prompt 序列化为 `[{"type":"text","text":"...","cache_control":{"type":"ephemeral"}}]`
- [ ] AA2：无 system message 时 `body["system"]` 不存在
- [ ] AA3：tools list 最后一项含 `"cache_control":{"type":"ephemeral"}`，前项无此字段
- [ ] AA3：tools 为空时 `body["tools"]` 不存在
- [ ] AA4：任意请求的 cache breakpoint 总数 ≤ 4
- [ ] AA4：system+tools 均有时 breakpoint 数精确为 2
- [ ] 全套 `cargo test --test plan_aa_prompt_caching_test` 通过（10 个测试）
- [ ] 既有 llm 模块单测（`cargo test llm`）无退化
- [ ] 不修改任何其他 provider（OpenAI / DeepSeek / Qwen 等）
