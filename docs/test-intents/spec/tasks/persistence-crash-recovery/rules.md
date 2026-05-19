# rules.md — persistence-crash-recovery 持久化原子性与崩溃恢复测试意图

来源：[LUT-8](mention://issue/8cb3deac-6e6a-478e-96f2-cdaa609f0f53)

涉及核心模块：
- `storage/fs_atomic.rs` — tmp+rename 原子写
- `storage/message_write_queue.rs` — 异步写队列（channel 容量 128，`try_send` 满时返回 Full 错误）
- `storage/file_store/messages.rs` — 分片 JSONL（v1，`_current` 跟踪活跃分片，支持 `.tmp` 回退）+ 单文件（v2，`messages.jsonl`，id-based last-writer-wins）
- `storage/aijia_home.rs` — 存储根目录 `~/.renlijia/`，`turn_stages_dir()` / `interrupted_turns_dir()` 路径管理
- `runtime/run_registry.rs` — 纯内存 `Mutex<HashMap<String, ActiveRun>>`，重启后自动清空
- `runtime/chat/turn_stage.rs` — `TurnStageEmitter` 每次状态转换原子写 `turn_stages/{conv_id}.json`；`run_recovery_sweep()` 启动时扫描孤儿文件转换为 `interrupted_turns/{conv_id}.json`

推荐验证方式：模块 1～5 的核心路径均可用 `cargo test` 集成测试覆盖（`TempDir` 隔离真实文件系统），无需调 LLM。

---

## 模块 1：崩溃后对话历史完整性 + 恢复 Banner 触发

### 意图 1.1：进程正常完成写入后被强制终止，重启后对话历史完整且 interrupted_turns 哨兵存在

**场景**
对话完成一轮正常响应后，进程被 `kill -9`。`run_recovery_sweep` 应在下次启动时把 `turn_stages/{conv_id}.json` 孤儿文件转换为 `interrupted_turns/{conv_id}.json`，历史消息完整可读。

**前提**
- 用 `TempDir::new()` 创建隔离根目录 `root`
- 在 `root/turn_stages/` 目录下写入一个合法的 `PersistedTurnStage` 文件，文件名 `conv-abc123.json`，内容：
  ```json
  {
    "schemaVersion": 1,
    "conversationId": "conv-abc123",
    "runId": "run-xyz",
    "stage": { "kind": "completing" },
    "stageStartedAtMs": 1700000000000,
    "turnStartedAtMs": 1700000000000,
    "lastHeartbeatAtMs": 1700000000000
  }
  ```
- `root/interrupted_turns/` 目录不存在（模拟全新启动）
- 在 `root/users/t_1__u_1/conversations/conv-abc123/messages.jsonl` 写入两行合法 JSON（user + assistant 各一条）

**操作**
1. 调用 `run_recovery_sweep(&root.join("turn_stages"), &root.join("interrupted_turns"))`
2. 读取 `root/interrupted_turns/conv-abc123.json`
3. 调用 `AppStorage::new(&root.join("users/t_1__u_1"))` 后调用 `storage.get_messages("conv-abc123")`

**断言**
- `run_recovery_sweep` 返回的 `RecoverySweepResult` 满足：`orphans_found == 1`、`interrupted_written == 1`、`deleted == 1`、`errors == 0`
- `root/turn_stages/conv-abc123.json` 不存在（已被 sweep 删除）
- `root/interrupted_turns/conv-abc123.json` 存在且可被 `serde_json::from_slice::<InterruptedTurnDisk>` 解析
- 解析后 `record.conversation_id == "conv-abc123"`、`record.run_id == "run-xyz"`
- `record.last_stage` 序列化后值为 `{"kind":"completing"}`
- `get_messages("conv-abc123")` 返回长度为 2 的列表，第 0 条 `role == "user"`，第 1 条 `role == "assistant"`

---

### 意图 1.2：流式输出中途崩溃，已 append 的消息行可独立解析，_current 无损

**场景**
流式写入时 `append_jsonl` 只完成了部分行写入，进程崩溃。恢复后，已完成写入的行必须可独立解析为合法 JSON，不能因未完成的行影响其他行。

**前提**
- 用 `TempDir::new()` 创建隔离根目录
- 用 `AppStorage::new(root.path())` + `storage.create_conversation("conv-stream", "流式测试")` 创建对话
- 通过 `storage.insert_message("m1", "conv-stream", "user", r#"{"text":"你好"}"#)` 写入一条完整消息
- 直接在 `messages.jsonl`（或 `messages.1.jsonl`）文件末尾 append 一段**不完整的 JSON**（模拟写入中途崩溃）：`b"{\"id\":\"m2\",\"role\":\"assistant\",\"content\":{\"text\":\"未完"`（不含换行和闭括号）

**操作**
1. 调用 `storage.get_messages("conv-stream")`

**断言**
- 返回结果不是 `Err`（`get_messages_v2` 的 `read_jsonl` 对损坏行应跳过或忽略，不整体 panic）
- 返回列表中包含 `id == "m1"` 的消息，`content["text"] == "你好"`
- 列表中不包含 `id == "m2"`（未完成行不应出现，或若出现也不影响 m1 的完整性）

---

### 意图 1.3：run_recovery_sweep 对 turn_stages 目录下格式损坏的 .json 文件只 log 不 panic，损坏文件被删除

**场景**
`turn_stages/` 下有一个 `.json` 文件但内容是随机字节，`run_recovery_sweep` 不应因此 panic，应跳过并删除该文件，继续处理其他文件。

**前提**
- 用 `TempDir::new()` 创建根目录
- 在 `root/turn_stages/` 写两个文件：
  - `corrupt.json`：内容 `b"{invalid json"`
  - `conv-valid.json`：内容为合法 `PersistedTurnStage`（同意图 1.1）

**操作**
1. 调用 `run_recovery_sweep(&root.join("turn_stages"), &root.join("interrupted_turns"))`

**断言**
- 函数返回（不 panic）
- 返回值 `errors == 1`（corrupt.json 导致一次错误）
- 返回值 `interrupted_written == 1`（valid 文件被正常处理）
- `root/turn_stages/corrupt.json` 不存在（被删除）
- `root/turn_stages/conv-valid.json` 不存在（被正常 sweep 后删除）
- `root/interrupted_turns/conv-valid.json` 存在且可解析

---

### 意图 1.4：进程正常完成 turn 时 turn_stage 文件被删除，重启后无 interrupted_turns 哨兵

**场景**
正常完成的 turn 在 `mark_turn_complete()` 时删除 `turn_stages/{conv_id}.json`。重启后 `turn_stages/` 为空，`run_recovery_sweep` 不产生任何哨兵。

**前提**
- 用 `TempDir::new()` 创建根目录
- `root/turn_stages/` 目录存在但为空（无任何 `.json` 文件）

**操作**
1. 调用 `run_recovery_sweep(&root.join("turn_stages"), &root.join("interrupted_turns"))`

**断言**
- 返回值：`orphans_found == 0`、`interrupted_written == 0`、`deleted == 0`、`errors == 0`
- `root/interrupted_turns/` 目录不存在或存在但为空

---

## 模块 2：写操作失败时数据保护

### 意图 2.1：write_atomic 写入 .tmp 成功但 rename 前崩溃，原有文件保持不变

**场景**
`write_atomic` 先写 `.tmp` 再 rename。如果 rename 未执行，下一次 `write_atomic` 覆盖 `.tmp` 后完成 rename，原有文件保持 v1 内容，不出现半写状态。

**前提**
- 用 `TempDir::new()` 创建目录
- 先调用 `write_atomic(&path, b"v1")` 写入初始值
- 手动写入 `.tmp` 文件：`fs::write(path.with_extension("tmp"), b"v2-partial")`（模拟崩溃留下孤儿 .tmp）

**操作**
1. 调用 `write_atomic(&path, b"v3")` 完成正常写入

**断言**
- `fs::read(&path).unwrap() == b"v3"`（rename 覆盖了 v1，写入 v3 成功）
- `.tmp` 文件不存在（被本次 write_atomic 的 rename 清理）

---

### 意图 2.2：_current 文件的 .tmp 回退：_current 本体不存在时从 .tmp 恢复分片元数据

**场景**
`write_shard_meta` 写 `.tmp` 后 rename 前崩溃，下次 `read_shard_meta` 能从 `.tmp` 回退读取元数据并 promote 为 `_current`，不丢失分片进度。

**前提**
- 用 `TempDir::new()` 创建根目录
- 调用 `storage.create_conversation("conv-meta", "元数据回退测试")` 初始化对话
- 插入 3 条消息（写入 `_current` 记录 `shard=1:next_seq=4`）
- **删除 `_current` 文件**，**手动写入 `_current.tmp`** 内容为 `"1:4"`（模拟 rename 前崩溃留下孤儿 .tmp）

**操作**
1. 调用 `storage.get_messages("conv-meta")`

**断言**
- 返回列表长度为 3（三条消息全部可读）
- `_current` 文件存在（被 `read_shard_meta` promote 后写回）
- `_current.tmp` 文件不存在（promote 后删除）

---

## 模块 3：重启后 RuntimeRunRegistry 不锁死会话

### 意图 3.1：RuntimeRunRegistry 重建后对任意 session_id 的 is_session_busy 返回 false

**场景**
`RuntimeRunRegistry` 是纯内存结构，每次构造都是空 HashMap。模拟重启（重新 `new()`）后，任何 session_id 不应被标记为 busy，不阻塞新的 `reserve` 调用。

**前提**
- 构造 `registry1 = RuntimeRunRegistry::new()`
- 调用 `registry1.reserve("session-a", RunId::new("run-old"))` 使其 busy
- **丢弃 registry1**，构造 `registry2 = RuntimeRunRegistry::new()`（模拟重启）

**操作**
1. 检查 `registry2.is_session_busy("session-a")`
2. 调用 `registry2.reserve("session-a", RunId::new("run-new"))`

**断言**
- `registry2.is_session_busy("session-a") == false`
- `registry2.reserve("session-a", RunId::new("run-new"))` 返回 `Ok(())`（不报 "already processing"）
- `registry2.is_session_busy("session-a") == true`（新 reserve 成功）

---

### 意图 3.2：进行中的 run 被 cancel 后，同一 session 可以立即 reserve 新 run

**场景**
当一个 run 被取消（`cancel_tx` 发出 true），`reserve` 检测到已有 run 处于 cancelled 状态时应移除旧 run 并允许新 run 占位，不返回 "already processing" 错误。

**前提**
- 构造 `RuntimeRunRegistry::new()`
- 调用 `registry.reserve("session-b", RunId::new("run-old"))` 成功
- 调用 `registry.cancel("session-b")` 标记为已取消

**操作**
1. 调用 `registry.reserve("session-b", RunId::new("run-new"))`

**断言**
- 返回 `Ok(())`
- `registry.is_session_busy("session-b") == true`
- `registry.run_id_for_session("session-b").unwrap().as_str() == "run-new"`（旧 run 已被替换）

---

## 模块 4：并发写入时消息完整性

### 意图 4.1：同一对话 5 条消息串行快速写入，全部消息可读且无内容混叠

**场景**
`MessageWriteQueue` 的 channel 容量 128，5 条消息串行写入不会触发 Full 错误。每条消息应独立存为一行合法 JSON，id 不重复，内容与写入时一致。

**前提**
- 用 `TempDir::new()` + `AppStorage::new` 创建存储
- 调用 `storage.create_conversation("conv-bulk", "并发写入测试")`

**操作**
1. 循环 5 次，依次调用 `storage.insert_message(&format!("msg-{i}"), "conv-bulk", "user", &format!(r#"{{"text":"消息{i}"}}"#))` for i in 0..5
2. 调用 `storage.get_messages("conv-bulk")`
3. 读取底层文件（`messages.jsonl` 或 `messages.1.jsonl`）逐行解析

**断言**
- `get_messages` 返回长度为 5 的列表
- 列表中 `content["text"]` 依次为 `"消息0"`、`"消息1"`、`"消息2"`、`"消息3"`、`"消息4"`
- 底层文件每行均为合法 JSON（`serde_json::from_str::<serde_json::Value>(line)` 成功）
- 底层文件中不存在两条消息内容交织在同一行的情况（每行只包含一个 `id` 字段）

---

### 意图 4.2：append_jsonl 追加内容后文件每行独立可解析，追加不破坏已有行

**场景**
`append_jsonl` 使用 `OpenOptions::append(true)` + 换行分隔。追加新行后，已有行不受影响，每行仍可独立解析。

**前提**
- 用 `TempDir::new()` 创建目录
- 在 `path/messages.jsonl` 写入合法 JSONL 两行：
  ```
  {"id":"m1","role":"user"}
  {"id":"m2","role":"assistant"}
  ```

**操作**
1. 调用 `append_jsonl(&path, &serde_json::json!({"id":"m3","role":"user"}))` 追加第三行
2. 逐行读取文件，对每行调用 `serde_json::from_str::<serde_json::Value>`

**断言**
- 文件共 3 行
- 第 1 行解析后 `id == "m1"`
- 第 2 行解析后 `id == "m2"`
- 第 3 行解析后 `id == "m3"`
- 不存在内容跨行（不存在解析失败的行）

---

### 意图 4.3：MessageWriteQueue channel 满时 try_send 返回 Full 错误，不阻塞调用方

**场景**
当 `MessageWriteQueue` 的 channel（容量 128）已满，`enqueue_insert` 应立即返回 `Err`（Full），不阻塞调用线程，错误信息包含 "full"。

**前提**
- 构造一个 `MessageWriteTarget` 的测试 stub，`insert_message` 永不返回（阻塞在 `std::thread::sleep(Duration::MAX)` 或等待一个 Condvar）
- 构造 `MessageWriteQueue::new(Arc::new(stub))`

**操作**
1. 循环 129 次调用 `queue.enqueue_insert(format!("id-{i}"), "conv-x".to_string(), "user".to_string(), r#"{"text":"x"}"#.to_string())` for i in 0..129
2. 记录第 129 次（index 128）的返回值

**断言**
- 前 128 次返回 `Ok(())`（channel 容量刚好容纳）
- 第 129 次返回 `Err`，错误字符串包含 `"full"` 或 `"message write queue is full"`

---

## 模块 5：存储文件损坏后的启动容错

### 意图 5.1：messages.jsonl 被截断后 get_messages_v2 不 panic，返回可解析的部分消息

**场景**
`messages.jsonl` 最后一行被截断（模拟写入中途崩溃）。`get_messages_v2` 使用 `read_jsonl` 读取时，损坏行应被跳过，已完成的行正常返回。

**前提**
- 用 `TempDir::new()` + `AppStorage::new` 创建存储
- 创建对话并插入 2 条完整消息（id `"m1"`, `"m2"`）
- 读取 `messages.jsonl`，在文件末尾 append 一段截断 JSON：`b"\n{\"id\":\"m3\",\"truncated"`

**操作**
1. 调用 `storage.get_messages_v2("conv-damaged")`

**断言**
- 调用不 panic
- 返回 `Ok(messages)`
- `messages` 中包含 `id == "m1"` 和 `id == "m2"` 的消息
- `messages` 中不包含 `id == "m3"`（截断行被跳过）

---

### 意图 5.2：_current 文件不存在时 read_shard_meta 返回默认值 shard=1:next_seq=1，读取已有 shard 文件成功

**场景**
`_current` 文件被删除后，`read_shard_meta` 返回默认值 `ShardMeta { shard: 1, next_seq: 1 }`，`get_legacy_shard_messages` 仍能读取 `messages.1.jsonl`。

**前提**
- 用 `TempDir::new()` 创建根目录
- 在 `root/conversations/conv-no-current/messages.1.jsonl` 写入 2 行合法 JSONL（id `"m1"`, `"m2"`）
- **不创建** `_current` 文件

**操作**
1. 调用 `AppStorage::new(&root)` 后 `storage.get_messages("conv-no-current")`

**断言**
- 调用不 panic
- 返回 `Ok(messages)`，`messages.len() == 2`
- `messages[0]["id"].as_str() == Some("m1")`
- `messages[1]["id"].as_str() == Some("m2")`

---

### 意图 5.3：run_recovery_sweep 在 turn_stages 目录不存在时返回零结果，不 panic

**场景**
全新设备或首次启动时 `turn_stages/` 目录不存在，`run_recovery_sweep` 应安全返回零结果，不因 `read_dir` 失败而 panic。

**前提**
- 用 `TempDir::new()` 创建根目录
- `root/turn_stages/` **不创建**（目录不存在）

**操作**
1. 调用 `run_recovery_sweep(&root.join("turn_stages"), &root.join("interrupted_turns"))`

**断言**
- 函数返回（不 panic）
- 返回值等于 `RecoverySweepResult::default()`（`orphans_found == 0`，`interrupted_written == 0`，`deleted == 0`，`errors == 0`）
- `root/interrupted_turns/` 目录不存在（没有任何写操作）

---

### 意图 5.4：v2 messages.jsonl 为空时自动 fallback 读取 v1 shard 文件

**场景**
`get_messages_v2` 先读 `messages.jsonl`，若为空则 fallback 到 `get_legacy_shard_messages` 读取 `messages.N.jsonl`。保证从 v1 迁移 v2 的对话在 v2 文件为空时不丢失历史。

**前提**
- 用 `TempDir::new()` + `AppStorage::new` 创建存储
- 调用 `storage.create_conversation("conv-fallback", "fallback 测试")`
- 通过底层 `insert_message` 函数（v1 路径）写入 2 条消息到 `messages.1.jsonl`
- 创建空的 `messages.jsonl` 文件（`fs::write(&messages_v2_path, b"")`，长度为 0）

**操作**
1. 调用 `storage.get_messages_v2("conv-fallback")`

**断言**
- 返回 `Ok(messages)`
- `messages.len() == 2`（通过 fallback 从 v1 shard 读取）
- `messages[0].id == "m1"`（与 v1 写入的 id 一致）
