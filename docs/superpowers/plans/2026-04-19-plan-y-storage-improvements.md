# 存储层改进（Plan-Y）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:test-driven-development. Each task requires: write test first → run test (expect fail) → write implementation → run test (expect pass) → commit.

**Goal:** 借鉴 `claude-code-best` 的 housekeeping 思路，优化 lotus 本地 file-store：降低消息写入阻塞、删除会话时做级联清理、并在启动时 GC 孤儿上传文件。
**Architecture:** Y1 在调用层引入后台写入（优先比较 `spawn_blocking` 与顺序队列的取舍，不能只写 happy path）；Y2 在 `ConversationStore::delete_conversation` 增加 `PermissionStore` 清理调用，并修复 `InMemoryConversationStore::delete_conversation` 未清理 `compact_boundaries`；Y3 在 `AppStorage::new` 初始化尾部扫描 `conversations/{id}/uploads/` 目录，比对 `file_index.json` 中的 `stored_path`，删除孤儿文件并记录日志。
**Tech Stack:** Rust, tokio, async_trait
**Worktree branch:** pzc

---

## 对标修订（2026-04-19）

- 本计划不是 `claude-code-best` 的直接存储同构，而是借鉴其 startup/background housekeeping 思路。
- Y1 需要额外验证消息顺序、退出时丢写和错误可观测性；若 `fire-and-forget` 风险过高，应改为有序后台队列。
- Y3 的 orphan upload GC 是 lotus 本地文件存储特有问题，文档需明确为本地架构债清理而非“对标已有实现”。

---

## 背景与现状分析

### Y1 现状

- `AppStorage::insert_message`（`src-tauri/src/storage/file_store/mod.rs:175`）调用时持有 `write_lock: Mutex<()>`，执行同步 IO：`read_shard_meta` → `count_jsonl_lines` → `append_jsonl` → `write_shard_meta` → `atomic_write_json`（更新 conv.json）
- `AppStorage::update_message_content`（`mod.rs:222`）同样同步持锁
- 调用链：`run_chat_turn_s4`（`chat_turn_driver.rs:905`）→ `executor.persist_assistant_message`（trait 方法）→ 实现中调用 `db.insert_message(...)` —— 在 tokio 异步任务上 `.await` 阻塞等待同步文件 IO

### Y2 现状

- `delete_conversation`（`conversation_service.rs:98`）清理了：Python session、LLM gateway task、物理文件（`get_file_paths_for_conversation`）、memory prefix、`db.delete_conversation`
- **未清理**：`PermissionStore`（`runtime/store/permission_store.rs`）内 `session` HashMap 中以该 conversation_id 为 scope 的决策条目
- `InMemoryConversationStore::delete_conversation`（`conversation_store.rs:83`）清理了 `conversations`、`messages`、`active_tasks`，但**未清理** `compact_boundaries` HashMap

### Y3 现状

- 上传流程（`file.rs:108-127`）：`file_mgr.store_upload(source)` → 物理复制到 `workspace/uploads/{uuid}_{name}` → `db.insert_uploaded_file(...)` 写入 `file_index.json`
- 若 `insert_uploaded_file` 失败，代码已回滚物理文件（`file_mgr.delete_file`），此路径已正确
- **漏洞场景**：`store_upload` 物理复制成功后，进程在 `insert_uploaded_file` 之前崩溃（操作系统 kill、电源断开）——物理文件存在但 `file_index.json` 无记录，永远不会被删除
- `AppStorage::initialize()`（`mod.rs:104`）已有 `reconcile_index`、`cleanup_expired_cache` 等启动期清理逻辑，是放置孤儿 GC 的天然位置

---

## Task Y1：消息写入改为 fire-and-forget 异步

### 目标

streaming 期间调用 `insert_message` / `update_message_content` 不再阻塞 tokio 任务，错误通过日志记录而不是向上 propagate crash。

### 设计决策

**方案选择：`Arc<AppStorage>` + `tokio::task::spawn_blocking`**

`AppStorage` 内部使用 `std::sync::Mutex`，不能在 async 上下文直接持锁跨 `.await`。最简改法：

```
// 调用方（persist_assistant_message 实现内）
let db = Arc::clone(&self.db);
let conv_id = conversation_id.to_string();
let content = content.to_string();
tokio::task::spawn_blocking(move || {
    if let Err(e) = db.insert_message(&msg_id, &conv_id, "assistant", &content) {
        log::error!("[Y1] persist_assistant_message failed: {}", e);
    }
});
```

- 不修改 `AppStorage::insert_message` 签名（保持同步，便于测试）
- 不引入 channel queue（复杂度高，对消息写入不必要）
- `JoinHandle` 由调用方持有但可丢弃（fire-and-forget）；若需要 graceful shutdown，未来可收集 handles

**关键约束：**

- `persist_assistant_message` 仍返回 message_id（在 spawn 前生成 UUID）
- 错误仅 log，不向 caller propagate（messaging 写入失败不应中止 streaming turn）
- `persist_user_message` 同样适用此模式

### 文件变更

1. **`src-tauri/src/runtime/chat/chat_turn_driver.rs`**（或其 production executor 实现）
   - 找到 `persist_assistant_message` 和 `persist_user_message` 的具体实现
   - 将 `db.insert_message(...)?` 改为 `tokio::task::spawn_blocking(move || { ... log error ... })`
   - 在 spawn 前生成 `message_id = uuid::Uuid::new_v4().to_string()`，直接返回

2. **新增测试**（`src-tauri/tests/y1_async_write_test.rs`）
   ```rust
   // 验证：调用 persist_assistant_message 后不等待 IO 完成即返回
   // 方法：使用一个人造 slow-write AppStorage mock，验证函数在 IO 完成前就 return
   // 或者：使用 tokio::time 测量调用延迟 < IO 耗时
   ```

### 测试策略

```rust
// src-tauri/tests/y1_async_write_test.rs
// 
// 测试 1：fire-and-forget 不阻塞 caller
// - 创建包含人造延迟 IO 的测试 executor（spawn_blocking 内 sleep 100ms）
// - 计时调用 persist_assistant_message，验证返回 < 50ms
// - 等待 tokio handle join，验证最终消息落盘
//
// 测试 2：IO 错误不 panic，仅 log
// - 使用只读临时目录触发写入失败
// - 验证函数正常返回，无 panic
```

### 实施步骤

1. `cargo test persist_assistant_message` — 确认现有测试通过（baseline）
2. 写 `y1_async_write_test.rs` 中的两个测试，`cargo test` 确认失败
3. 修改 executor 实现，改为 `spawn_blocking` fire-and-forget
4. `cargo test` 确认两个新测试通过，原有测试不退化
5. `git commit`

---

## Task Y2：会话删除级联清理 PermissionStore + InMemoryConversationStore

### 目标

删除会话时，`PermissionStore` 中属于该会话的 session 级权限决策被清除；`InMemoryConversationStore` 删除会话时同步清理 `compact_boundaries`。

### 设计决策

**子任务 Y2a：PermissionStore 新增清理 API**

```rust
// runtime/store/permission_store.rs
impl PermissionStore {
    /// 清除某 conversation 的所有 session 级决策。
    /// scope_key 约定为 "{conversation_id}:{capability}" 格式，因此按前缀过滤。
    pub fn clear_session_for_conversation(&self, conversation_id: &str) {
        let prefix = format!("{}:", conversation_id);
        self.session
            .write()
            .unwrap()
            .retain(|k, _| !k.starts_with(&prefix));
    }
}
```

> **注意**：`AlwaysAllow` / `AlwaysDeny`（`persistent` map）是跨会话的全局策略，**不应**随会话删除而清除。仅清理 `session` map。

**子任务 Y2b：delete_conversation 调用 clear_session_for_conversation**

定位 `conversation_service.rs::delete_conversation`，在 `db.delete_conversation` 之前（或之后）调用 `permission_store.clear_session_for_conversation(&conversation_id)`。

需要在函数签名中增加 `permission_store: Arc<PermissionStore>` 参数，或通过已有的 `Arc<AppStorage>` 旁路（推荐前者，更清晰）。

调用方（`tauri_commands/chat.rs` 或 `lib.rs` 中的 command handler）需同步更新传参。

**子任务 Y2c：InMemoryConversationStore 补全 delete_conversation**

```rust
// conversation_store.rs InMemoryConversationStore::delete_conversation
fn delete_conversation(&self, id: &str) -> Result<()> {
    self.conversations.lock().unwrap().remove(id);
    self.messages.lock().unwrap().remove(id);
    self.active_tasks.lock().unwrap().remove(id);
    self.compact_boundaries.lock().unwrap().remove(id); // 新增
    Ok(())
}
```

### 文件变更

1. **`src-tauri/src/runtime/store/permission_store.rs`**
   - 新增 `clear_session_for_conversation(&self, conversation_id: &str)`
   - 新增单测

2. **`src-tauri/src/runtime/conversation_service.rs`**
   - `delete_conversation` 签名增加 `permission_store: Arc<PermissionStore>`
   - 调用 `permission_store.clear_session_for_conversation(&conversation_id)`

3. **`src-tauri/src/transport/tauri_commands/chat.rs`**（或 delete_conversation 的调用方）
   - 注入 `permission_store` 参数

4. **`src-tauri/src/runtime/store/conversation_store.rs`**
   - `InMemoryConversationStore::delete_conversation` 补上 `compact_boundaries` 清理

### 测试策略

```rust
// src-tauri/tests/y2_cascade_delete_test.rs

// 测试 1：删除会话后 PermissionStore session 决策被清除
// - 创建 PermissionStore，record("conv1:python:exec", Allow)
// - 调用 clear_session_for_conversation("conv1")
// - 验证 get("conv1:python:exec") == None
// - 验证 AlwaysAllow 决策不受影响

// 测试 2：AlwaysAllow 在会话删除后保留
// - record("conv1:browser", AlwaysAllow)
// - clear_session_for_conversation("conv1")
// - 验证 get("conv1:browser") == Some(AlwaysAllow)

// 测试 3：InMemoryConversationStore delete_conversation 清理 compact_boundaries
// - create_conversation("c1", "title")
// - append_compact_boundary({ conversation_id: "c1", ... })
// - delete_conversation("c1")
// - list_compact_boundaries("c1") == []
```

### 实施步骤

1. 写测试，`cargo test` 确认失败
2. 实现 `clear_session_for_conversation`
3. 修改 `InMemoryConversationStore::delete_conversation`
4. 修改 `delete_conversation` 函数签名和调用链
5. `cargo test y2` 全通过
6. `cargo test review_` 回归测试确认无退化
7. `git commit`

---

## Task Y3：启动时孤儿文件 GC

### 目标

进程启动时扫描每个会话的 `uploads/` 物理目录，比对 `file_index.json` 中已登记的 `stored_path`，删除未在索引中出现的文件（孤儿文件）并记录日志。

### 设计决策

**GC 位置：`AppStorage::initialize()`**

`initialize()` 已是启动期清理的聚集点（reconcile_index、cleanup_expired_cache）。GC 在此调用，保持一致性。

**扫描逻辑**

```
对每个 conversations/{conv_id}/uploads/ 目录：
  1. 读取 file_index.json，提取所有 stored_path（格式："uploads/{uuid}_{name}"）
     → 建立 indexed_names: HashSet<String>（只取文件名部分）
  2. 列举 uploads/ 目录下的所有文件
  3. 对每个物理文件，若文件名不在 indexed_names 中 → 孤儿文件
  4. 删除孤儿文件，log::warn!("[Y3] gc orphan: {:?}", path)
  5. 统计并 log::info!("[Y3] gc: {} orphan(s) removed in conv {}", count, conv_id)
```

**范围限制**

- 仅扫描 `uploads/` 子目录（用户上传）
- `generated/` 目录不在此次范围（有独立的 `find_expired_temp_files` 机制）
- 孤儿文件定义：物理文件存在，但在当前会话的 `file_index.json` 中**无任何条目**的 `stored_path` 字段指向它

**独立入口（可选，CLI/Tauri command 触发）**

```rust
// AppStorage 新增公开方法
pub fn gc_orphan_upload_files(&self) -> Result<usize>
```

`initialize()` 内部调用，同时也可从 Tauri command 或 test 直接调用。

**失败处理**

- 单个文件删除失败：`log::warn!` 并继续（不中止整体 GC）
- 返回实际删除数量；错误只在日志中记录，不向调用方传播

### 文件变更

1. **`src-tauri/src/storage/file_store/mod.rs`**
   - 新增 `pub fn gc_orphan_upload_files(&self) -> Result<usize>`
   - 在 `initialize()` 末尾调用：`let gc_count = self.gc_orphan_upload_files().unwrap_or(0); if gc_count > 0 { info!(...) }`

2. **新增测试**（`src-tauri/tests/y3_orphan_gc_test.rs`）

### 测试策略

```rust
// src-tauri/tests/y3_orphan_gc_test.rs

// 测试 1：孤儿文件被删除
// - 创建 AppStorage，create_conversation("c1", ...)
// - 手动在 conversations/c1/uploads/ 写入物理文件 "orphan.csv"（不写 file_index.json）
// - 调用 storage.gc_orphan_upload_files()
// - 验证 "orphan.csv" 物理文件已被删除
// - 验��返回值 == 1

// 测试 2：已在 file_index.json 中的文件不被删除
// - insert_uploaded_file("uf1", "c1", "data.csv", "uploads/uuid_data.csv", ...)
// - 在对应物理路径创建文件
// - 调用 gc_orphan_upload_files()
// - 验证物理文件仍存在
// - 验证返回值 == 0

// 测试 3：混合场景（有孤儿 + 有合法文件）
// - 创建 1 个合法文件（有 file_index 记录）
// - 创建 2 个孤儿文件（无记录）
// - gc_orphan_upload_files() 返回 2
// - 合法文件存在，孤儿文件不存在

// 测试 4：uploads/ 目录不存在时 GC 不 panic（新会话无上传）
// - create_conversation("empty_c", ...) 但不调用 insert_uploaded_file
// - gc_orphan_upload_files() 返回 Ok(0)
```

### 实施步骤

1. 写 `y3_orphan_gc_test.rs` 四个测试，`cargo test y3` 确认编译失败/测试失败
2. 实现 `gc_orphan_upload_files`
3. 在 `initialize()` 中调用
4. `cargo test y3` 全通过
5. `cargo test` 全局回归（确保 GC 逻辑不误删正常文件）
6. `git commit`

---

## 执行顺序建议

1. **Y2c**（最小改动，1 行补丁，先建立 baseline commit）
2. **Y2a + Y2b**（同批：PermissionStore API + 级联调用，配套测试）
3. **Y3**（独立，不依赖 Y1/Y2）
4. **Y1**（最后，涉及 async 调用链重构，风险最高）

每个 task 独立 commit，commit message 格式：`fix(storage): <Y1/Y2/Y3 描述>`

---

## 关键文件路径速查

| 文件 | 用途 |
|---|---|
| `src-tauri/src/storage/file_store/mod.rs` | `AppStorage` 主体，`initialize()` 入口 |
| `src-tauri/src/storage/file_store/messages.rs` | `insert_message` / `update_message_content` 同步 IO 实现 |
| `src-tauri/src/storage/file_store/files.rs` | `file_index.json` 读写，`insert_uploaded_file` |
| `src-tauri/src/storage/file_manager.rs` | `store_upload`，物理文件复制 |
| `src-tauri/src/runtime/store/permission_store.rs` | `PermissionStore`，需新增 `clear_session_for_conversation` |
| `src-tauri/src/runtime/store/conversation_store.rs` | `ConversationStore` trait，`InMemoryConversationStore` |
| `src-tauri/src/runtime/conversation_service.rs` | `delete_conversation`，级联清理主流程 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | `persist_assistant_message` trait 方法 + 调用点（Y1） |
| `src-tauri/src/transport/tauri_commands/file.rs` | 上传入口，`store_upload` + `insert_uploaded_file`（Y3 背景） |
