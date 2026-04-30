# 存储层改进（Plan-Y）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:test-driven-development. Each task requires: write test first → run test (expect fail) → write implementation → run test (expect pass) → commit.

**Goal:** 借鉴 `claude-code-best` 的 housekeeping 思路，优化 lotus 本地 file-store：降低消息写入阻塞、修复会话删除遗漏清理，并在启动时 GC 孤儿上传文件。
**Architecture:** Y1 先把 assistant message persistence 改为“有序后台写入”，不能直接裸 `spawn_blocking` fire-and-forget；user message 继续保持同步落盘语义。Y2 本轮只修 `InMemoryConversationStore::delete_conversation` 未清理 `compact_boundaries`；PermissionStore 级联清理因 key/schema 未定，后续单列计划。Y3 改为在应用启动处、拿到 `db + file_mgr` 的边界做 fail-open 的全局 uploads orphan GC，而不是挂在 `AppStorage::initialize()` 上逐会话扫描不存在的 `conversations/{id}/uploads/` 目录。
**Tech Stack:** Rust, tokio, async_trait
**Worktree branch:** pzc

---

## 对标修订（2026-04-19）

- 本计划不是 `claude-code-best` 的直接存储同构，而是借鉴其 startup/background housekeeping 思路。
- Y1 必须保证写入顺序；`spawn_blocking + fire-and-forget` 本身不满足该要求，需要明确落成单 writer / ordered queue 语义。
- Y3 的 orphan upload GC 是 lotus 本地文件存储特有问题，文档需明确为本地架构债清理而非“对标已有实现”。

---

## 背景与现状分析

### Y1 现状

- `AppStorage::insert_message`（`src-tauri/src/storage/file_store/mod.rs:175`）调用时持有 `write_lock: Mutex<()>`，执行同步 IO：`read_shard_meta` → `count_jsonl_lines` → `append_jsonl` → `write_shard_meta` → `atomic_write_json`（更新 conv.json）
- `AppStorage::update_message_content`（`mod.rs:222`）同样同步持锁
- 当前实际调用点位于 `src-tauri/src/transport/tauri_commands/chat.rs` 的 `persist_user_message` / `persist_assistant_message`
- `persist_user_message` 位于 turn 启动前的 durability 边界；assistant message 才是更适合后台化的候选
- 若直接改成裸 `spawn_blocking`，虽然 `AppStorage` 内部锁仍能串行 IO，但 task 调度不保证调用顺序；assistant insert / update 可能乱序提交

### Y2 现状

- `delete_conversation`（`conversation_service.rs:98`）清理了：Python session、LLM gateway task、物理文件（`get_file_paths_for_conversation`）、memory prefix、`db.delete_conversation`
- 计划原先假设 `PermissionStore` session key 带 conversation 前缀，但当前实现并无稳定的 conversation 维度；`runtime/tools/permission.rs` 当前 key 更接近 `"{definition.id}:{scope}"`，因此不能安全地按会话前缀清理
- `InMemoryConversationStore::delete_conversation`（`conversation_store.rs:83`）清理了 `conversations`、`messages`、`active_tasks`，但**未清理** `compact_boundaries` HashMap

### Y3 现状

- 上传流程（`file.rs:108-127`）：`file_mgr.store_upload(source)` → 物理复制到 `workspace/uploads/{uuid}_{name}` → `db.insert_uploaded_file(...)` 写入 `file_index.json`
- 若 `insert_uploaded_file` 失败，代码已回滚物理文件（`file_mgr.delete_file`），此路径已正确
- **漏洞场景**：`store_upload` 物理复制成功后，进程在 `insert_uploaded_file` 之前崩溃（操作系统 kill、电源断开）——物理文件存在但 `file_index.json` 无记录，永远不会被删除
- 物理上传目录是工作区根下全局 `uploads/`，不是 `conversations/{id}/uploads/`
- `AppStorage::initialize()` 只有 storage base_dir，看不到 workspace root；GC 不应挂在这个边界

---

## Task Y1：final assistant message 改为有序后台写入（user message 保持同步）

### 目标

当前 S4 driver 只在 turn 结束后持久化 final assistant message；本轮 Y1 仅覆盖该 final assistant insert 的顺序后台写入，不新增 streaming update 持久化路径。user message 继续在 turn 启动前同步落盘，保持现有 durability 语义。同步文件 IO 应转移到后台 worker 执行，但在 driver 发 `MessagePersisted` 前仍需等待 ack，保证 event/DB 可见性一致。错误需可观测，且 assistant 持久化失败不应产生“message 已持久化”的假事件。

### 设计决策

**方案选择：单 writer / ordered queue（不是裸 `spawn_blocking`）**

单纯把每次写入包进 `tokio::task::spawn_blocking` 无法保证 insert / update 的提交顺序。本轮应采用单 writer / ordered queue（例如后台 worker 顺序消费 message write job），只把 assistant message 相关写入异步化；user message 维持同步写入。

```rust
// 调用方（persist_assistant_message 实现内）
let message_id = uuid::Uuid::new_v4().to_string();
let job = MessageWriteJob::InsertAssistant {
    message_id: message_id.clone(),
    conversation_id: conversation_id.to_string(),
    content_json,
};
if let Err(e) = self.message_write_queue.enqueue(job).await {
    log::error!("[Y1] enqueue assistant persistence failed: {}", e);
}
```

- `AppStorage` 底层 API 保持同步，队列只负责顺序调度
- final assistant insert 必须走顺序通道；保留 update job/ack 原语，供后续如需引入 assistant update 持久化时复用
- user message 维持同步直写，确保 turn 启动前前端可恢复
- `MessagePersisted` / `message:updated` 只能在 write ack 成功后发送，不能把“入队成功”视为“已落盘”
- 退出时执行 best-effort drain；失败仅记录日志，不阻塞关闭

**关键约束：**

- `persist_assistant_message` 仍返回 message_id（在 enqueue 前生成 UUID）
- 优先走 ordered queue；若 enqueue 失败，可降级为 direct write，但只有在最终写入成功后才能返回给 driver 发 terminal 事件
- ack/direct-write 均失败时向 caller 返回 persistence error，防止发出 persisted 语义事件
- `persist_user_message` **不**迁到后台队列，保持同步 durability 边界

### 文件变更

1. **`src-tauri/src/transport/tauri_commands/chat.rs`**
   - 为 final assistant message 引入顺序后台写通道
   - `persist_user_message` 保持同步写入
   - 在 assistant 路径先生成稳定 `message_id`，将写任务入队并等待 ack，再返回给 driver 发 terminal 事件
2. **`src-tauri/src/storage/file_store/messages.rs`**（如需要）
   - 若测试需要，抽出更易验证顺序的同步写入口或 helper
3. **新增测试**（`src-tauri/tests/y1_async_write_test.rs`）

### 测试策略

```rust
// src-tauri/tests/y1_async_write_test.rs
//
// 测试 1：queue 原语保持顺序，后续 update 不会越过先前 insert
// - 人造慢写 worker：insert sleep 100ms，update sleep 0ms
// - 验证 enqueue 端快速返回，但最终 transcript 顺序仍是 insert -> update
//
// 测试 2：final assistant persistence helper 只有在 DB 可见后才返回
// - 人造阻塞写 worker
// - 在 release 前 helper 不得完成；release 后消息可从 DB 读到
//
// 测试 3：退出路径会 flush pending message writes
// - 通过结构回归锁定 lib.rs exit path 调用了 flush_pending_message_writes
//
// 测试 4：assistant 持久化 helper 在最终写入失败时向 caller 返回错误
// - 使用不可写目录或 synthetic failing target
// - 验证 caller 收到 persistence error，driver 不会误发 persisted 语义事件
```

### 实施步骤

1. 写 `y1_async_write_test.rs` 中的测试，`cargo test` 确认失败
2. 修改 executor 实现，引入 ordered queue
3. `cargo test` 确认新测试通过，原有测试不退化
4. `git commit`

---

## Task Y2：修复 InMemoryConversationStore delete_conversation 漏删 compact_boundaries

### 目标

`InMemoryConversationStore` 删除会话时同步清理 `compact_boundaries`。PermissionStore 的 conversation-scoped cascade 因 key/schema 未定，本轮不实现，后续单列计划。

### 设计决策

当前只落 `Y2c`：`InMemoryConversationStore` 补全 delete_conversation。

> **说明**：PermissionStore 目前没有稳定的 conversation-scoped key schema，不能安全按会话删除。本计划删除原 Y2a/Y2b，后续如要实现，需先定义 permission decision key 的生命周期与作用域。

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

1. **`src-tauri/src/runtime/store/conversation_store.rs`**
   - `InMemoryConversationStore::delete_conversation` 补上 `compact_boundaries` 清理
2. **新增测试**（可直接补在 `conversation_store.rs` 的 `#[cfg(test)]` 中，或新建 `src-tauri/tests/y2_cascade_delete_test.rs`）

### 测试策略

```rust
// 测试：InMemoryConversationStore delete_conversation 清理 compact_boundaries
// - create_conversation("c1", "title")
// - append_compact_boundary({ conversation_id: "c1", ... })
// - delete_conversation("c1")
// - list_compact_boundaries("c1") == []
```

### 实施步骤

1. 写测试，`cargo test` 确认失败
2. 修改 `InMemoryConversationStore::delete_conversation`
3. `cargo test y2` 全通过
4. `git commit`

---

## Task Y3：启动时全局 uploads 孤儿文件 GC（fail-open）

### 目标

进程启动时扫描工作区根下全局 `uploads/` 目录，汇总所有会话 `file_index.json` 中 `source == upload` 的 `stored_path` 引用，删除未被任何索引引用的物理文件（孤儿文件）并记录日志；任一索引损坏时 fail-open，避免误删。

### 设计决策

**GC 位置：应用启动处（`lib.rs` 或同级 startup wiring）**

GC 需要同时拿到 `db` 与 `file_mgr.workspace_path()`；`AppStorage::initialize()` 只有 storage base_dir，看不到真实 uploads 根目录，因此不应挂在这里。采用 startup fail-open wiring 更符合边界。

**扫描逻辑**

```
1. 枚举工作区根 `uploads/` 目录中的物理文件；
2. 枚举所有 conversation 的 `file_index.json`，仅收集 `source == upload` 的 `stored_path`；
3. 将 `stored_path` 归一化成相对 uploads 路径集合；
4. 物理文件若不在引用集合中，则视为 orphan，删除并 `log::warn!`；
5. 任一 conversation 索引读取/反序列化失败时，记录 warning 并跳过该会话或该轮，避免误删。
```

**范围限制**

- 仅扫描工作区根 `uploads/` 子目录（用户上传）
- `generated/` 目录不在此次范围（有独立的 `find_expired_temp_files` 机制）
- 孤儿文件定义：物理文件存在，但在**所有会话索引**中都没有 `source == upload` 的 `stored_path` 引用它

**独立入口（可选，CLI/Tauri command 触发）**

```rust
// 由 startup wiring 暴露 helper，例如：
pub fn gc_orphan_upload_files(db: &AppStorage, file_mgr: &FileManager) -> Result<usize>
```

启动时调用；测试也可直接调用。

**失败处理**

- 单个文件删除失败：`log::warn!` 并继续（不中止整体 GC）
- 返回实际删除数量；错误只在日志中记录，不向调用方传播

### 文件变更

1. **`src-tauri/src/lib.rs`**（或同级 startup wiring）
   - 在应用启动完成 `db + file_mgr` 初始化后调用 `gc_orphan_upload_files(...)`
   - fail-open：GC 失败只记录日志，不中止启动
2. **`src-tauri/src/storage/file_manager.rs` / `src-tauri/src/storage/file_store/files.rs` / 新增 helper 模块**
   - 提供枚举全局 `uploads/` 目录与汇总所有 conversation upload 引用所需的 helper
   - 若实现位置更合适，也可新建 `src-tauri/src/storage/upload_gc.rs`

3. **新增测试**（`src-tauri/tests/y3_orphan_gc_test.rs`）

### 测试策略

```rust
// src-tauri/tests/y3_orphan_gc_test.rs

// 测试 1：孤儿文件被删除
// - 创建 AppStorage，create_conversation("c1", ...)
// - 手动在 workspace/uploads/ 写入物理文件 "orphan.csv"（不写 file_index.json）
// - 调用 gc_orphan_upload_files(db, file_mgr)
// - 验证 "orphan.csv" 物理文件已被删除
// - 验证返回值 == 1

// 测试 2：已在 file_index.json 中的文件不被删除
// - insert_uploaded_file("uf1", "c1", "data.csv", "uploads/uuid_data.csv", ...)
// - 在对应物理路径创建文件
// - 调用 gc_orphan_upload_files(db, file_mgr)
// - 验证物理文件仍存在
// - 验证返回值 == 0

// 测试 3：混合场景（有孤儿 + 有合法文件）
// - 创建 1 个合法文件（有 file_index 记录）
// - 创建 2 个孤儿文件（无记录）
// - gc_orphan_upload_files(db, file_mgr) 返回 2
// - 合法文件存在，孤儿文件不存在

// 测试 4：uploads/ 目录不存在时 GC 不 panic（新会话无上传）
// - create_conversation("empty_c", ...) 但不调用 insert_uploaded_file
// - gc_orphan_upload_files(db, file_mgr) 返回 Ok(0)

// 测试 5：某个 conversation 的 file_index 损坏时 fail-open，不误删其它被引用文件
// - 一个会话 file_index.json 写坏
// - 另一个会话有合法 upload 引用
// - 运行 GC 后，合法文件仍存在；GC 只记录 warning
```

### 实施步骤

1. 写 `y3_orphan_gc_test.rs` 四个测试，`cargo test y3` 确认编译失败/测试失败
2. 实现 `gc_orphan_upload_files`
3. 在 startup wiring 中调用
4. `cargo test y3` 全通过
5. `cargo test` 全局回归（确保 GC 逻辑不误删正常文件）
6. `git commit`

---

## 执行顺序建议

1. **Y2c**（最小改动，1 行补丁，先建立 baseline commit）
2. **Y3**（独立，不依赖 Y1）
3. **Y1**（最后，涉及 async 调用链重构，风险最高）

每个 task 独立 commit，commit message 格式：`fix(storage): <Y1/Y2/Y3 描述>`

---

## 关键文件路径速查

| 文件 | 用途 |
|---|---|
| `src-tauri/src/lib.rs` | 应用启动入口，适合挂载 fail-open housekeeping |
| `src-tauri/src/storage/file_store/mod.rs` | `AppStorage` 主体（conversation/file index 读取） |
| `src-tauri/src/storage/file_store/messages.rs` | `insert_message` / `update_message_content` 同步 IO 实现 |
| `src-tauri/src/storage/file_store/files.rs` | `file_index.json` 读写，`insert_uploaded_file` |
| `src-tauri/src/storage/file_manager.rs` | `store_upload`，物理文件复制 |
| `src-tauri/src/runtime/store/conversation_store.rs` | `ConversationStore` trait，`InMemoryConversationStore` |
| `src-tauri/src/runtime/conversation_service.rs` | `delete_conversation`，级联清理主流程 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | `persist_assistant_message` / `persist_user_message` 实现（Y1） |
| `src-tauri/src/transport/tauri_commands/file.rs` | 上传入口，`store_upload` + `insert_uploaded_file`（Y3 背景） |
