# Chat History Storage Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用 OpenAI camelCase 格式贯通聊天记录的内存传递、磁盘存储和历史恢复，消除工具调用入参（`toolCalls`）丢失和 `tool` 消息无法还原两个缺口。

**Architecture:** 存储层改为完整保存 OpenAI camelCase 格式消息（`{role, content, toolCalls?, toolCallId?, name?}`）；`load_history` 直接将磁盘记录还原为 LLM 可消费格式，不再只取 `content.text`；旧数据兼容通过字段映射处理。

**Tech Stack:** Rust, serde_json, AppStorage (JSONL shard), `chat_turn_driver.rs`, `tauri_commands/chat.rs`

---

## 文件变更索引

| 文件 | 操作 | 职责变更 |
|------|------|---------|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Modify | trait `persist_assistant_message` 新增 `tool_calls` 参数；Step 7 调用时传入；新增 trait 方法 `persist_tool_messages` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | Modify | 实现新签名的 `persist_assistant_message`（content_json 加入 `toolCalls`）；实现 `persist_tool_messages`；修复 `build_history_message_content` 支持 `tool` role 和 `toolCalls` 还原 |
| `src-tauri/tests/review_chat_history_persistence_test.rs` | Create | 新增架构约束测试：assistant toolCalls 持久化、tool 消息持久化、load_history 还原 |

---

## Task 1：trait 扩展 — `persist_assistant_message` 加 `tool_calls` 参数

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

### 背景

`TurnIterationState.messages` 中 assistant 调用工具时的消息格式：
```json
{"role":"assistant","content":"...", "toolCalls":[{"id":"tc1","name":"browse","arguments":{}}]}
```
这份 `toolCalls` 从 `LlmStepResult::ToolCalls { tool_calls, .. }` 取得，目前只用于在内存追加历史，没有传给 `persist_assistant_message`。

- [ ] **Step 1.1：修改 trait 定义，新增 `tool_calls` 参数**

在 `src-tauri/src/runtime/chat/chat_turn_driver.rs` 第 122 行附近，将 trait 方法签名改为：

```rust
async fn persist_assistant_message(
    &self,
    conversation_id: &str,
    content: &str,
    tool_calls: &[serde_json::Value],   // ← 新增，每个元素是 {id, name, arguments}
    generated_file_ids: &[String],
    file_metas: &[serde_json::Value],
) -> Result<String, TurnError>;
```

- [ ] **Step 1.2：修改 Step 7 的调用，传入 `normalized_tool_calls` 聚合**

`TurnIterationState` 里没有累积 tool_calls 列表，需要在 `state` 上新增字段收集每轮的 tool calls，或在 Step 7 从 `state.messages` 里提取。最简单方案：在 `TurnIterationState` 加字段。

在 `turn_config.rs` 的 `TurnIterationState` 结构体里新增：

```rust
pub all_tool_calls: Vec<serde_json::Value>,
```

在 `TurnIterationState::new` 的初始化里加：

```rust
all_tool_calls: Vec::new(),
```

在 Step 5e `normalized_tool_calls` 构建之后，追加到 state：

```rust
// 已有代码附近（第 1043 行）
let normalized_tool_calls: Vec<serde_json::Value> = tool_calls
    .iter()
    .map(|call| {
        serde_json::json!({
            "id": call.tool_call_id,
            "name": call.tool_name,
            "arguments": call.args,
        })
    })
    .collect();

// 新增这行
state.all_tool_calls.extend(normalized_tool_calls.clone());
```

在 Step 7 的调用处（第 1154 行附近）：

```rust
let message_id = executor
    .persist_assistant_message(
        config.conversation_id.as_str(),
        &state.full_content,
        &state.all_tool_calls,   // ← 新增
        &state.generated_file_ids,
        &state.all_file_metas,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;
```

- [ ] **Step 1.3：运行编译确认签名错误**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -20
```

预期：`persist_assistant_message` 的实现方（`TauriLegacyTurnExecutor`）报参数数量不匹配错误。

---

## Task 2：实现 — `persist_assistant_message` 写入 `toolCalls`

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 2.1：更新 `TauriLegacyTurnExecutor` 的实现签名**

在 `chat.rs` 第 697 行附近，将签名改为：

```rust
async fn persist_assistant_message(
    &self,
    conversation_id: &str,
    content: &str,
    tool_calls: &[serde_json::Value],   // ← 新增
    generated_file_ids: &[String],
    file_metas: &[serde_json::Value],
) -> Result<String, TurnError> {
```

- [ ] **Step 2.2：在 `content_value` 构建时附加 `toolCalls`**

找到构建 `content_value` 的位置（目前是 `serde_json::json!({ "text": filtered_content })`），在所有三个分支（有文件/无文件/查询失败）都加入 `toolCalls` 字段：

```rust
// 工具函数：构建带 toolCalls 的 content JSON
fn build_assistant_content_json(
    text: &str,
    tool_calls: &[serde_json::Value],
    generated_files: Option<Vec<serde_json::Value>>,
) -> serde_json::Value {
    let mut obj = serde_json::json!({ "text": text });
    if !tool_calls.is_empty() {
        obj["toolCalls"] = serde_json::json!(tool_calls);
    }
    if let Some(files) = generated_files {
        if !files.is_empty() {
            obj["generatedFiles"] = serde_json::json!(files);
        }
    }
    obj
}
```

在 `persist_assistant_message` 内，替换所有 `serde_json::json!({ "text": filtered_content })` 为：

```rust
build_assistant_content_json(&filtered_content, tool_calls, None)
```

有 generatedFiles 的分支：

```rust
build_assistant_content_json(&filtered_content, tool_calls, Some(gen_files))
```

- [ ] **Step 2.3：编译通过**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -20
```

预期：0 error。

- [ ] **Step 2.4：Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/runtime/chat/turn_config.rs \
        src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(storage): persist assistant toolCalls in content JSON"
```

---

## Task 3：持久化 tool 消息到磁盘

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

### 背景

工具执行结果消息（`tool_result_messages`）的格式（来自 `tool_result_collector.rs`）：
```json
{"role":"tool","toolCallId":"tc1","name":"browse","content":"结果文本"}
```
这是完整的 OpenAI camelCase 格式，直接存磁盘即可。

- [ ] **Step 3.1：在 trait 新增 `persist_tool_messages`**

在 `chat_turn_driver.rs` trait 定义里新增（默认 no-op，生产实现 override）：

```rust
/// 持久化本轮 tool result 消息到存储。纯 I/O，不含事件发射。
/// 默认 no-op；生产 executor 必须 override。
async fn persist_tool_messages(
    &self,
    _conversation_id: &str,
    _tool_messages: &[serde_json::Value],
) -> Result<(), TurnError> {
    Ok(())
}
```

- [ ] **Step 3.2：在 Step 5e 的 `state.append_messages_batch` 之后调用**

在 `history_batch` 追加完成、`state.append_messages_batch(history_batch)` 之后，立即调用持久化（注意 CP-3 取消检查之后）：

```rust
state.append_messages_batch(history_batch);
state.all_file_metas.extend(results.new_file_metas);
state.generated_file_ids.extend(results.new_generated_file_ids);

// 新增：持久化本轮 tool 消息（忽略错误，不阻断流程）
{
    let tool_msgs: Vec<serde_json::Value> = results
        .tool_result_messages
        .iter()
        .cloned()
        .collect();
    if !tool_msgs.is_empty() {
        if let Err(e) = executor
            .persist_tool_messages(config.conversation_id.as_str(), &tool_msgs)
            .await
        {
            log::warn!("[chat_turn_driver] Failed to persist tool messages: {}", e);
        }
    }
}
```

> 注意：`results.tool_result_messages` 已被 move 进 `history_batch`，需在 move 前保留副本。修改 `collect_results` 返回结构，或在构建 `history_batch` 前先 clone。最简单：在 `history_batch.push(msg)` 循环前克隆一份备用：
>
> ```rust
> let tool_msgs_for_persist: Vec<serde_json::Value> =
>     results.tool_result_messages.clone();
> ```
>
> 然后在 `state.append_messages_batch` 之后用 `tool_msgs_for_persist`。

- [ ] **Step 3.3：在 `TauriLegacyTurnExecutor` 实现 `persist_tool_messages`**

在 `chat.rs` 中添加（紧跟 `persist_user_message` 之后）：

```rust
async fn persist_tool_messages(
    &self,
    conversation_id: &str,
    tool_messages: &[serde_json::Value],
) -> Result<(), TurnError> {
    for msg in tool_messages {
        let msg_id = format!("tool-{}", uuid::Uuid::new_v4());
        // 存储格式：直接存 OpenAI camelCase tool message 的 content 部分
        // {toolCallId, name, content} — 和 ChatMessage 对齐
        let content_json = serde_json::json!({
            "toolCallId": msg.get("toolCallId").cloned().unwrap_or(serde_json::Value::Null),
            "name": msg.get("name").cloned().unwrap_or(serde_json::Value::Null),
            "content": msg.get("content").cloned().unwrap_or(serde_json::Value::Null),
        })
        .to_string();
        if let Err(e) = self.services.db.insert_message(
            &msg_id,
            conversation_id,
            "tool",
            &content_json,
        ) {
            log::warn!(
                "[persist_tool_messages] Failed to save tool message id={} conv={}: {}",
                msg_id,
                conversation_id,
                e
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 3.4：编译通过**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -20
```

预期：0 error。

- [ ] **Step 3.5：Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(storage): persist tool result messages to disk"
```

---

## Task 4：修复 `load_history` — 还原 `tool` 消息和 `assistant` 的 `toolCalls`

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

### 背景

`build_history_from_compact_boundary` 当前：
1. 对每条消息调用 `build_history_message_content(role, content, ...)` 取到一个 `String`
2. 输出 `{"role", "content": String}`

这对 `tool` 和带 `toolCalls` 的 assistant 都不够，需要输出完整结构。

- [ ] **Step 4.1：将 `build_history_from_compact_boundary` 改为输出完整 JSON**

将现有的 `filter_map` 闭包改为：

```rust
chat_messages.extend(filtered_messages.into_iter().filter_map(|msg| {
    let role = msg["role"].as_str()?.to_string();

    match role.as_str() {
        "tool" => {
            // 磁盘格式（新）: {toolCallId, name, content}
            // 磁盘格式（旧）: {toolCallId, toolName, isError, result}
            let content_obj = msg.get("content")?;
            let tool_call_id = content_obj
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let name = content_obj
                .get("name")
                // 旧格式兼容
                .or_else(|| content_obj.get("toolName"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let content_str = content_obj
                .get("content")
                // 旧格式兼容
                .or_else(|| content_obj.get("result"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if content_str.is_empty() && tool_call_id.is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "role": "tool",
                "toolCallId": tool_call_id,
                "name": name,
                "content": content_str,
            }))
        }
        "assistant" => {
            let content_obj = msg.get("content")?;
            let text = content_obj
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if text.trim().is_empty() {
                // tool-call-only assistant，也需要还原（toolCalls 存在但 text 为空）
                let tool_calls = content_obj.get("toolCalls");
                if tool_calls.is_none() {
                    return None;
                }
            }
            let mut out = serde_json::json!({
                "role": "assistant",
                "content": text,
            });
            if let Some(tcs) = content_obj.get("toolCalls") {
                if tcs.as_array().map_or(false, |a| !a.is_empty()) {
                    out["toolCalls"] = tcs.clone();
                }
            }
            Some(out)
        }
        "user" => {
            let content_obj = msg.get("content")?;
            let content_str = build_history_message_content(
                "user",
                content_obj,
                has_authorized_workspace,
            )?;
            if content_str.trim().is_empty() {
                return None;
            }
            // isCompactSummary 的 user 消息也走此处（来自 boundary summary 注入，不在 filtered_messages 里）
            Some(serde_json::json!({
                "role": "user",
                "content": content_str,
            }))
        }
        _ => None,
    }
}));
```

- [ ] **Step 4.2：编译通过**

```bash
cd src-tauri && cargo build 2>&1 | grep "error\[" | head -20
```

预期：0 error。

- [ ] **Step 4.3：Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "fix(history): restore tool messages and assistant toolCalls from disk"
```

---

## Task 5：架构约束回归测试

**Files:**
- Create: `src-tauri/tests/review_chat_history_persistence_test.rs`

- [ ] **Step 5.1：写测试——assistant toolCalls 写入磁盘后可读回**

```rust
// review_chat_history_persistence_test.rs

use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

fn make_storage() -> (AppStorage, TempDir) {
    let dir = TempDir::new().unwrap();
    let storage = AppStorage::new(dir.path()).unwrap();
    (storage, dir)
}

/// assistant 消息带 toolCalls 时，写入再读回应保留 toolCalls 字段。
#[test]
fn review_assistant_tool_calls_round_trip() {
    let (storage, _dir) = make_storage();
    let conv_id = "conv-tc-rt";
    storage.create_conversation(conv_id, "test").unwrap();

    let content_json = serde_json::json!({
        "text": "我来帮你打开页面",
        "toolCalls": [
            {"id": "tc-001", "name": "browse_navigate", "arguments": {"url": "https://example.com"}}
        ]
    })
    .to_string();

    storage
        .insert_message("msg-001", conv_id, "assistant", &content_json)
        .unwrap();

    let msgs = storage.get_recent_messages(conv_id, 10).unwrap();
    assert_eq!(msgs.len(), 1);
    let content = &msgs[0]["content"];
    assert_eq!(
        content["toolCalls"][0]["name"].as_str().unwrap(),
        "browse_navigate",
        "toolCalls must survive storage round-trip"
    );
    assert_eq!(
        content["toolCalls"][0]["arguments"]["url"].as_str().unwrap(),
        "https://example.com"
    );
}
```

- [ ] **Step 5.2：写测试——tool 消息写入磁盘后可读回**

```rust
/// tool result 消息写入再读回应保留 toolCallId、name、content 字段。
#[test]
fn review_tool_message_round_trip() {
    let (storage, _dir) = make_storage();
    let conv_id = "conv-tool-rt";
    storage.create_conversation(conv_id, "test").unwrap();

    let content_json = serde_json::json!({
        "toolCallId": "tc-001",
        "name": "browse_navigate",
        "content": "Page ready: https://example.com"
    })
    .to_string();

    storage
        .insert_message("msg-tool-001", conv_id, "tool", &content_json)
        .unwrap();

    let msgs = storage.get_recent_messages(conv_id, 10).unwrap();
    let tool_msg = msgs
        .iter()
        .find(|m| m["role"].as_str() == Some("tool"))
        .expect("tool message must be stored");
    assert_eq!(
        tool_msg["content"]["toolCallId"].as_str().unwrap(),
        "tc-001"
    );
    assert_eq!(
        tool_msg["content"]["name"].as_str().unwrap(),
        "browse_navigate"
    );
    assert_eq!(
        tool_msg["content"]["content"].as_str().unwrap(),
        "Page ready: https://example.com"
    );
}
```

- [ ] **Step 5.3：写测试——load_history 还原 tool 消息为 LLM 格式**

```rust
/// build_history_from_compact_boundary 必须将磁盘 tool 消息还原为
/// {role:"tool", toolCallId, name, content} 格式供 LLM 消费。
#[test]
fn review_load_history_restores_tool_messages() {
    use app_lib::transport::tauri_commands::chat::build_history_from_compact_boundary;

    let raw = vec![
        serde_json::json!({
            "id": "m1",
            "role": "user",
            "content": {"text": "帮我打开百度"},
        }),
        serde_json::json!({
            "id": "m2",
            "role": "assistant",
            "content": {
                "text": "",
                "toolCalls": [{"id": "tc-1", "name": "browse_navigate", "arguments": {"url": "https://baidu.com"}}]
            },
        }),
        serde_json::json!({
            "id": "m3",
            "role": "tool",
            "content": {
                "toolCallId": "tc-1",
                "name": "browse_navigate",
                "content": "Page ready: https://baidu.com"
            },
        }),
    ];

    let history = build_history_from_compact_boundary(raw, None, false);

    // user
    assert_eq!(history[0]["role"], "user");
    assert_eq!(history[0]["content"], "帮我打开百度");

    // assistant with toolCalls
    assert_eq!(history[1]["role"], "assistant");
    assert_eq!(history[1]["toolCalls"][0]["name"], "browse_navigate");

    // tool
    assert_eq!(history[2]["role"], "tool");
    assert_eq!(history[2]["toolCallId"], "tc-1");
    assert_eq!(history[2]["name"], "browse_navigate");
    assert_eq!(history[2]["content"], "Page ready: https://baidu.com");
}
```

- [ ] **Step 5.4：写测试——旧格式 tool 消息兼容性**

```rust
/// 旧磁盘格式（toolName/result 字段）必须被兼容还原。
#[test]
fn review_load_history_restores_legacy_tool_messages() {
    use app_lib::transport::tauri_commands::chat::build_history_from_compact_boundary;

    let raw = vec![serde_json::json!({
        "id": "m1",
        "role": "tool",
        "content": {
            "toolCallId": "tc-legacy",
            "toolName": "browse_navigate",   // 旧字段
            "isError": false,
            "result": "Page ready: https://baidu.com",  // 旧字段
        },
    })];

    let history = build_history_from_compact_boundary(raw, None, false);

    assert_eq!(history[0]["role"], "tool");
    assert_eq!(history[0]["toolCallId"], "tc-legacy");
    assert_eq!(history[0]["name"], "browse_navigate");
    assert_eq!(history[0]["content"], "Page ready: https://baidu.com");
}
```

- [ ] **Step 5.5：确认 `build_history_from_compact_boundary` 已 `pub`**

```bash
grep "pub fn build_history_from_compact_boundary" src-tauri/src/transport/tauri_commands/chat.rs
```

若不是 pub，改为 pub。

- [ ] **Step 5.6：运行测试确认全通过**

```bash
cd src-tauri && cargo test review_assistant_tool_calls_round_trip \
  review_tool_message_round_trip \
  review_load_history_restores_tool_messages \
  review_load_history_restores_legacy_tool_messages \
  -- --nocapture 2>&1 | tail -20
```

预期：4 tests passed。

- [ ] **Step 5.7：运行现有回归测试确认无退化**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -30
```

预期：所有 review_ 测试通过。

- [ ] **Step 5.8：Commit**

```bash
git add src-tauri/tests/review_chat_history_persistence_test.rs
git commit -m "test(review): add chat history persistence round-trip constraints"
```

---

## Task 6：端到端冒烟验证

- [ ] **Step 6.1：启动应用，发送一条触发工具调用的消息**

```bash
pnpm tauri:dev
```

发送："帮我打开 https://www.baidu.com 看看首页"（会触发 `browse_navigate` 工具）。

- [ ] **Step 6.2：检查磁盘上新写入的消息**

```bash
CONV=$(ls -t ~/.renlijia/conversations/ | head -1)
python3 -c "
import json, os
path = os.path.expanduser(f'~/.renlijia/conversations/$CONV/messages.1.jsonl')
raw = open(path, 'rb').read()
for line in raw.split(b'\n'):
    line = line.strip().rstrip(b'\xe2\x9c\x93').rstrip(b'\t')
    if not line: continue
    try:
        obj = json.loads(line)
        role = obj.get('role')
        content = obj.get('content', {})
        print(f'role={role} content_keys={list(content.keys()) if isinstance(content,dict) else type(content).__name__}')
    except: pass
"
```

预期输出包含：
```
role=user content_keys=['sender', 'text']
role=tool content_keys=['toolCallId', 'name', 'content']
role=assistant content_keys=['text', 'toolCalls']
```

- [ ] **Step 6.3：切换对话再切回，确认工具调用历史保留**

切换到另一个对话，再切回来，继续发送一条消息，观察 LLM 是否能引用上一轮的工具结果（如"页面标题是什么"不需要重新打开浏览器）。

- [ ] **Step 6.4：最终 commit**

```bash
git add -A
git commit -m "chore: verify chat history persistence e2e"
```

---

## 自检

- [x] Task 1-3 覆盖改动 A/B（持久化 toolCalls + tool 消息）
- [x] Task 4 覆盖改动 C（load_history 还原）
- [x] Task 5 tests 覆盖改动 D（旧格式兼容）
- [x] 所有签名变更集中在 trait + 唯一实现，无遗漏 override
- [x] `tool_result_messages` 在被 move 进 `history_batch` 前需 clone，已在 Task 3.2 说明
- [x] 空 toolCalls 不写入 content_json（`if !tool_calls.is_empty()`），不污染无工具调用的 assistant 消息
