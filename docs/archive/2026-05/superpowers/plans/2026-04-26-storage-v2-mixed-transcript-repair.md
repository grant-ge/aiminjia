# Storage V2 Mixed Transcript Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复同一会话同时写入 `messages.1.jsonl` 和 `messages.jsonl` 导致历史消息缺失的问题，并让后续所有聊天消息统一写入单文件 transcript。

**Architecture:** 先在启动迁移中把旧 shard `messages.N.jsonl` 一次性合并进新 single-file `messages.jsonl`，并用 `state.json` 的 `migrations.messageShardsToSingleFile=true` 打标；运行时读取和写入都只使用 `messages.jsonl`，避免长期双读兼容。对标 `claude-code-best` 的 transcript 思路：一个会话应有一个主 transcript 入口，恢复时做 read-side normalization，但写入不能按消息类型分裂到两个真实来源。

**Tech Stack:** Rust, Tauri, JSONL append-only storage, cargo tests

---

## 根因与对标结论

### Lotus 当前问题

当前 storage 层同时保留两套写入 API：

- `AppStorage::insert_message(...)` -> `messages.1.jsonl` 旧 shard
- `AppStorage::insert_chat_message_record(...)` -> `messages.jsonl` v2 单文件

当前 runtime 写入路径分裂：

- `persist_user_message` 仍调用 `db.insert_message(...)`，写入旧 shard。
- 普通 assistant 最终通过 `persist_assistant_content_json(...)`，需要检查/修正为 v2。
- `persist_iteration_assistant_message` 调用 `insert_chat_message_record(...)`，写入 v2。
- `persist_tool_messages` 调用 `insert_chat_message_record(...)`，写入 v2。

现场样本 `5a82d14f-76dc-4607-ae82-fd4979ae9516`：

- `messages.1.jsonl` 有 user + 普通 assistant 文本。
- `messages.jsonl` 有 assistant toolCalls + tool result。
- 读取如果只读 `messages.jsonl`，用户问题和助手文本消失。
- 读取如果只读 `messages.1.jsonl`，工具链消息消失。

### 什么时候引入

关键提交链：

- `314c551 feat(storage): land transcript v2 with single-file migration and compatibility` 引入 `messages.jsonl` 与 v2 API，但保留旧 `insert_message`。
- `5137264 fix(storage): persist tool and assistant-with-tool_calls messages via v2 path` 把 tool/toolCalls 消息切到 v2，但 user/部分普通 assistant 仍走旧 path。

从 `5137264` 后，运行时容易产生混合 transcript。

### claude-code-best 对标

`claude-code-best` 的 `src/utils/sessionStorage.ts` 使用单条 transcript/session chain 作为恢复来源；读侧有 `deserializeMessages`、`filterUnresolvedToolUses`、legacy migration/normalization，但不会让同一 session 的不同消息类型长期写入两个并行真实来源。Lotus 应采用同样原则：

1. 写入端统一一个主 transcript 文件。
2. 启动迁移把历史遗留格式转换到主 transcript。
3. 运行时读取保持单一来源，迁移/恢复逻辑幂等，不覆盖新数据。

---

## 文件变更索引

| 文件 | 操作 | 职责 |
| --- | --- | --- |
| `src-tauri/src/storage/file_store/messages.rs` | Modify | 保持 `get_messages` / `get_recent_messages` 读取 single-file；提供 shard 合并到 single-file 的迁移能力 |
| `src-tauri/src/storage/file_store/mod.rs` | Modify | 让 `AppStorage::insert_message` 内部写入 v2，保留方法签名兼容旧调用方 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | Modify | 视情况改 `persist_user_message` 直接构造 `StoredMessage` 写 v2；或依赖 `AppStorage::insert_message` facade 切到 v2 |
| `src-tauri/tests/message_storage_v2_test.rs` | Modify | 添加混合文件读取、旧 facade 写入 v2、recent 读取测试 |
| `docs/superpowers/plans/2026-04-26-storage-v2-mixed-transcript-repair.md` | Create | 本计划 |

---

## Task 1：读取层兼容混合 transcript

**Files:**
- Modify: `src-tauri/tests/message_storage_v2_test.rs`
- Modify: `src-tauri/src/storage/file_store/messages.rs`

- [ ] **Step 1.1：写失败测试：同时存在 `messages.1.jsonl` 和 `messages.jsonl` 时必须都读出来**

在 `src-tauri/tests/message_storage_v2_test.rs` 的 `insert_and_get_single_file` 附近添加：

```rust
#[test]
fn app_storage_get_messages_merges_legacy_shards_and_single_file() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");

    let legacy_user = serde_json::json!({
        "seq": 1,
        "_rev": 1,
        "id": "legacy-user",
        "conversationId": "c1",
        "role": "user",
        "content": {"text": "你有哪些技能可以使用呢？"},
        "createdAt": "2026-04-26T06:59:18.370259+00:00"
    });
    fs::write(
        conv_dir.join("messages.1.jsonl"),
        format!("{}\t✓\n", serde_json::to_string(&legacy_user).unwrap()),
    )
    .expect("write legacy shard");
    fs::write(conv_dir.join("_current"), "1:2").expect("write shard cursor");

    let mut v2_tool = common::make_tool_result("tool-1", "tc-1", "switch_skill", "Switched");
    v2_tool.created_at = "2026-04-26T06:59:18.970640+00:00".into();
    v2_tool.sequence = Some(2);
    insert_message_v2(storage.base_dir(), &v2_tool).expect("insert v2 tool message");

    let messages = storage.get_messages("c1").expect("read merged messages");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["id"], "legacy-user");
    assert_eq!(messages[0]["content"]["text"], "你有哪些技能可以使用呢？");
    assert_eq!(messages[1]["id"], "tool-1");
    assert_eq!(messages[1]["toolResult"]["name"], "switch_skill");
}
```

- [ ] **Step 1.2：运行失败测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test app_storage_get_messages_merges_legacy_shards_and_single_file -- --nocapture
```

Expected: FAIL，当前只返回 `messages.jsonl` 或只返回旧 shard，不能同时返回 2 条。

- [ ] **Step 1.3：实现混合读取 helper**

在 `src-tauri/src/storage/file_store/messages.rs` 中添加：

```rust
fn read_legacy_shard_messages(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<StoredMessage>> {
    let meta = read_shard_meta(base_dir, conversation_id);
    let mut all_msgs: Vec<StoredMessage> = Vec::new();

    for shard_num in 1..=meta.shard {
        let path = shard_path(base_dir, conversation_id, shard_num);
        match read_jsonl::<StoredMessage>(&path) {
            Ok(records) => all_msgs.extend(records),
            Err(e) => {
                warn!(
                    "Failed to read shard {} for {}: {}",
                    shard_num, conversation_id, e
                );
            }
        }
    }

    Ok(dedup_messages(all_msgs))
}

fn merge_stored_messages(mut messages: Vec<StoredMessage>) -> Vec<StoredMessage> {
    let mut by_id: HashMap<String, StoredMessage> = HashMap::new();
    for msg in messages.drain(..) {
        by_id.insert(msg.id.clone(), msg);
    }

    let mut result: Vec<StoredMessage> = by_id.into_values().collect();
    result.sort_by(|a, b| match (a.sequence, b.sequence) {
        (Some(a_seq), Some(b_seq)) => a_seq
            .cmp(&b_seq)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id)),
        _ => a
            .created_at
            .cmp(&b.created_at)
            .then_with(|| a.seq_or(u64::MAX).cmp(&b.seq_or(u64::MAX)))
            .then_with(|| a.id.cmp(&b.id)),
    });
    result
}

fn get_messages_all_formats(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<StoredMessage>> {
    let mut all = Vec::new();
    all.extend(read_legacy_shard_messages(base_dir, conversation_id)?);

    let single_file = messages_path(base_dir, conversation_id);
    if single_file.exists() {
        all.extend(get_messages_v2(base_dir, conversation_id)?);
    }

    Ok(merge_stored_messages(all))
}
```

然后把 `get_messages` 改成：

```rust
pub fn get_messages(
    base_dir: &Path,
    conversation_id: &str,
) -> StorageResult<Vec<serde_json::Value>> {
    let messages = get_messages_all_formats(base_dir, conversation_id)?;
    Ok(messages.into_iter().map(message_to_json).collect())
}
```

- [ ] **Step 1.4：运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test app_storage_get_messages_merges_legacy_shards_and_single_file -- --nocapture
```

Expected: PASS。

---

## Task 2：recent/history 读取也合并两种文件

**Files:**
- Modify: `src-tauri/tests/message_storage_v2_test.rs`
- Modify: `src-tauri/src/storage/file_store/messages.rs`

- [ ] **Step 2.1：写失败测试：`get_recent_messages` 必须从混合文件中取最近消息**

添加测试：

```rust
#[test]
fn app_storage_get_recent_messages_merges_legacy_shards_and_single_file() {
    let (storage, _dir) = setup_storage();
    let conv_dir = storage.base_dir().join("conversations").join("c1");

    let legacy_user = serde_json::json!({
        "seq": 1,
        "_rev": 1,
        "id": "legacy-user",
        "conversationId": "c1",
        "role": "user",
        "content": {"text": "old question"},
        "createdAt": "2026-04-26T06:59:18.000000+00:00"
    });
    fs::write(
        conv_dir.join("messages.1.jsonl"),
        format!("{}\t✓\n", serde_json::to_string(&legacy_user).unwrap()),
    )
    .expect("write legacy shard");
    fs::write(conv_dir.join("_current"), "1:2").expect("write shard cursor");

    let mut v2_tool = common::make_tool_result("tool-1", "tc-1", "switch_skill", "Switched");
    v2_tool.created_at = "2026-04-26T06:59:19.000000+00:00".into();
    insert_message_v2(storage.base_dir(), &v2_tool).expect("insert v2 tool message");

    let messages = storage.get_recent_messages("c1", 10).expect("read recent messages");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["id"], "legacy-user");
    assert_eq!(messages[1]["id"], "tool-1");
}
```

- [ ] **Step 2.2：运行失败测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test app_storage_get_recent_messages_merges_legacy_shards_and_single_file -- --nocapture
```

Expected: FAIL。

- [ ] **Step 2.3：改 `get_recent_messages` 复用 `get_messages_all_formats`**

把 `get_recent_messages` 改成：

```rust
pub fn get_recent_messages(
    base_dir: &Path,
    conversation_id: &str,
    limit: u32,
) -> StorageResult<Vec<serde_json::Value>> {
    let mut messages = get_messages_all_formats(base_dir, conversation_id)?;
    let start = messages.len().saturating_sub(limit as usize);
    Ok(messages.drain(start..).map(message_to_json).collect())
}
```

- [ ] **Step 2.4：运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test app_storage_get_recent_messages_merges_legacy_shards_and_single_file -- --nocapture
```

Expected: PASS。

---

## Task 3：写入层统一到 `messages.jsonl`

**Files:**
- Modify: `src-tauri/tests/message_storage_v2_test.rs`
- Modify: `src-tauri/src/storage/file_store/mod.rs`
- Modify: `src-tauri/src/storage/file_store/messages.rs`

- [ ] **Step 3.1：写失败测试：旧 facade `insert_message` 不再创建 shard**

添加测试：

```rust
#[test]
fn app_storage_insert_message_facade_writes_single_file_only() {
    let (storage, _dir) = setup_storage();

    storage
        .insert_message("m1", "c1", "user", r#"{"text":"hello"}"#)
        .expect("insert through legacy facade");

    let conv_dir = storage.base_dir().join("conversations").join("c1");
    assert!(conv_dir.join("messages.jsonl").exists());
    assert!(!conv_dir.join("messages.1.jsonl").exists());
    assert!(!conv_dir.join("_current").exists());

    let messages = storage.get_messages("c1").expect("read messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["content"]["text"], "hello");
}
```

- [ ] **Step 3.2：运行失败测试**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test app_storage_insert_message_facade_writes_single_file_only -- --nocapture
```

Expected: FAIL，因为当前 `insert_message` 会创建 `messages.1.jsonl`。

- [ ] **Step 3.3：修改 `AppStorage::insert_message` facade**

在 `src-tauri/src/storage/file_store/mod.rs` 中把：

```rust
messages::insert_message(&self.base_dir, id, conversation_id, role, content_json)?;
```

替换为构造 v2 record：

```rust
let content: serde_json::Value = serde_json::from_str(content_json)?;
let msg = types::StoredMessage {
    seq: None,
    rev: None,
    id: id.to_string(),
    conversation_id: conversation_id.to_string(),
    role: role.to_string(),
    content,
    created_at: chrono::Utc::now().to_rfc3339(),
    tool_calls: None,
    tool_call_id: None,
    name: None,
    run_id: None,
    schema_version: Some(2),
    sequence: None,
};
messages::insert_message_v2(&self.base_dir, &msg)?;
```

保留底层 `messages::insert_message` 函数用于测试旧 shard 兼容和历史迁移，不再让运行时 facade 走它。

- [ ] **Step 3.4：运行测试确认通过**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test app_storage_insert_message_facade_writes_single_file_only -- --nocapture
```

Expected: PASS。

---

## Task 4：确认 runtime 写入链路没有直接绕过 facade 写旧 shard

**Files:**
- Modify: `src-tauri/tests/message_storage_v2_test.rs`
- Inspect: `src-tauri/src/transport/tauri_commands/chat.rs`
- Inspect: `src-tauri/src/runtime/chat/chat_turn_driver.rs`

- [ ] **Step 4.1：搜索运行时代码中的旧 shard 写入调用**

Run:

```bash
rg -n "\.insert_message\(|messages::insert_message\(" src-tauri/src/runtime src-tauri/src/transport src-tauri/src/commands
```

Expected: 只有 `db.insert_message(...)` facade 调用；不应有 `messages::insert_message(...)` 直接调用。

- [ ] **Step 4.2：如果发现直接调用底层旧函数，改为 v2 facade**

直接调用旧函数时，改成 `AppStorage::insert_message` 或 `insert_chat_message_record`。不要让 runtime 知道 shard 文件。

- [ ] **Step 4.3：运行 storage test 全量**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test -- --nocapture
```

Expected: PASS。

---

## Task 5：清理/补强启动迁移语义

**Files:**
- Modify: `src-tauri/src/storage/file_store/mod.rs`
- Modify: `src-tauri/tests/message_storage_v2_test.rs`

- [ ] **Step 5.1：确认 `run_startup_migrations` 只在“没有 `messages.jsonl`”时做 shard->single**

现有逻辑：

```rust
if has_shards && !has_new {
    messages::migrate_shards_to_single_file(&self.base_dir, &conv_id)
}
```

这个逻辑保留。原因：如果两个文件都存在，不能迁移删除 shard，否则会丢掉混合历史里的旧 user/assistant；读取层合并后，后续新消息统一写 v2，历史 shard 可留存作为兼容源。

- [ ] **Step 5.2：添加注释说明为什么不能删除混合 shard**

在 `src-tauri/src/storage/file_store/mod.rs` 的 `run_startup_migrations` 判断前添加：

```rust
// If both messages.jsonl and legacy shards exist, keep both. Some builds wrote
// user/assistant text to shards while tool messages went to messages.jsonl.
// Read-side merge preserves those mixed transcripts; deleting shards here would
// lose visible conversation text.
```

- [ ] **Step 5.3：运行 storage tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test -- --nocapture
```

Expected: PASS。

---

## Task 6：端到端人工验证样本

**Files:**
- No code changes

- [ ] **Step 6.1：启动应用前确认样本文件状态**

Run:

```bash
ls -lh /Users/a20250311/.renlijia/conversations/5a82d14f-76dc-4607-ae82-fd4979ae9516/messages*.jsonl
```

Expected: 同时存在 `messages.1.jsonl` 和 `messages.jsonl`。

- [ ] **Step 6.2：启动应用打开会话**

打开会话：

```text
5a82d14f-76dc-4607-ae82-fd4979ae9516
```

Expected: 右侧同时展示：

```text
你有哪些技能可以使用呢？
您好！我是您的智能工作助手...
工具步骤 switch_skill / Switched to skill 'comp-analysis-v2'.
```

- [ ] **Step 6.3：发送一条新消息并检查只写 v2**

发送任意新消息后运行：

```bash
ls -lh /Users/a20250311/.renlijia/conversations/5a82d14f-76dc-4607-ae82-fd4979ae9516/messages*.jsonl
```

Expected: 新消息追加到 `messages.jsonl`；不再产生新的 shard 文件或增加旧 shard 行数。

---

## 验证命令总表

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test message_storage_v2_test -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib storage::migration::tests -- --nocapture
```

如果第二条因为无关 warning 输出较多，只看 exit code 和 test result；warning 不阻塞本计划。

---

## 风险与边界

- 不删除已有 `messages.1.jsonl`，避免丢历史文本。
- 不做大规模落盘重写，先用读侧合并保证历史可见。
- 写侧统一后，新的会话不应再混合产生两个真实来源。
- 旧底层 `messages::insert_message` 暂时保留，仅用于兼容测试/旧迁移，不作为 runtime 写入入口。

---

## 执行建议

优先顺序：

1. Task 1 + Task 2：先救历史显示。
2. Task 3 + Task 4：再阻止继续产生混合文件。
3. Task 5：补注释防止未来误删 shard。
4. Task 6：用真实会话人工验证。


---

## 计划调整记录（2026-04-26）

用户确认不希望 `get_messages` 长期同时读取 `messages.1.jsonl` 和 `messages.jsonl`。正式方向改为：

1. 启动时检查 `~/.renlijia/state.json`。
2. 如果 `migrations.messageShardsToSingleFile != true`，扫描所有 conversations。
3. 把每个会话的 `messages.N.jsonl` 合并进 `messages.jsonl`，按 id 去重、按时间/sequence 排序。
4. 全部成功后写入：

```json
{
  "migrations": {
    "messageShardsToSingleFile": true
  }
}
```

5. 运行时读取和写入都只使用 `messages.jsonl`。

旧 shard 第一版保留不删除，避免迁移失败后不可恢复；因为有 state 标记，后续不会重复迁移。
