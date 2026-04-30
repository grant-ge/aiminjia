# State Owner Matrix

> 基于以下源文件分析生成：
> - `src-tauri/src/llm/gateway.rs`
> - `src-tauri/src/runtime/run_registry.rs`
> - `src-tauri/src/storage/file_store/mod.rs`（含 `conversations.rs` run.lock 实现）
> - `src-tauri/src/python/session.rs`
> - `src-tauri/src/auth/mod.rs`
> - `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs`

---

## 状态拥有者矩阵

| State | Current Source of Truth | Readers | Writers | Conflict / 冲突点 | Future Source |
|---|---|---|---|---|---|
| **busy（会话是否正在处理）** | `RuntimeRunRegistry::active_runs` (in-memory `Mutex<HashMap<String, ActiveRun>>`) — `run_registry.rs:L18` | `gateway.is_conversation_busy()` / `gateway.get_busy_conversations()` / `gateway.is_busy()` | `gateway.set_busy_for_run()` → `registry.reserve()`; `gateway.clear_task()` → `registry.clear()` | **双真相源冲突**：in-memory registry 在进程重启后丢失；`run.lock` 文件（`file_store/mod.rs:L552`）是持久化副本，但两者可能短暂不一致（registry 已 clear 但 lock 文件尚未删除，或进程崩溃后 lock 残留） | `RuntimeRunRegistry` 为主，`run.lock` 仅用于崩溃恢复 |
| **run.lock（崩溃恢复 agent lock）** | 文件系统：`conversations/{id}/run.lock`，格式 `SESSION_ID:UNIX_TIMESTAMP` — `file_store/mod.rs:L552–L583` | `db.get_orphaned_tasks()`（app 启动时扫描）| `db.insert_active_task()`（agent spawn 后）; `db.remove_active_task()`（AgentGuard::clear）| **冲突**：registry.reserve()（内存）与 insert_active_task()（磁盘）之间存在极短窗口，若 insert_active_task 失败则需回滚 registry（`chat_runtime_impl.rs:L996–L1003`）。崩溃时 lock 文件残留，下次启动由 `get_orphaned_tasks()` 检测 | 保持文件系统；考虑原子写入 |
| **streaming（当前是否在流式输出）** | 前端 `useChatStore` 的 `isStreaming` / `busyConversations` state — `useStreaming.ts` | 聊天界面 UI 层 | `streaming:delta` → 进入流；`streaming:done` / `streaming:error` → 退出流；`agent:idle` 也触发 `removeBusyConversation` | **双真相源冲突**：后端用 `RuntimeRunRegistry.active_runs` 跟踪 busy；前端用 `busyConversations` 跟踪。两者通过 event 同步，若事件丢失则前端和后端状态永久不一致（前端认为仍在流，后端已 idle）。`streaming:done` 与 `agent:idle` 分别由不同路径 emit（`finish_agent` 不 emit streaming:done；由 AgentGuard 统一 emit） | 统一到后端单一 source；前端仅做订阅 |
| **python session（每会话 REPL 进程）** | `PythonSessionManager::sessions: std::sync::Mutex<HashMap<String, Arc<PythonSession>>>` — `session.rs:L440` | `session_mgr.execute()` / `execute_for_run()` | `get_or_create()`（lazy spawn）; `destroy()` / `destroy_run()`（对话删除 / agent 结束）; `restart_session()`（崩溃恢复） | **冲突**：session key 有两种命名方式：legacy 用 `conversation_id`，新路径用 `session_key_for_run(run_id)` = `"python-run:{run_id}"`（`session.rs:L58`）。若 legacy 代码与新路径混用同一 conversation_id 则可能创建两个 session | 统一使用 run-scoped key |
| **auth（云端认证状态）** | `AuthManager::state: RwLock<Option<CloudAuth>>` — `auth/mod.rs:L31` | `auth_manager.get_auth_info()` / `get_session_key()` / `is_logged_in()` | `login()`: 写 state + 持久化; `logout()`: 清 state; `refresh_auth_info()`: 可能更新 state; `get_session_key()`: 过期时 write-lock 内更新 | **冲突**：内存 state 与 `config.json` 中加密持久化的 `cloud_auth` 字段（通过 `AppStorage::set_setting`）是双真相源。启动时 `restore()` 同步，但运行时只内存为准；崩溃则重新从文件 restore。`get_auth_info()` 在 refresh_token 过期时会在 read-lock 内检测到、再升级为 write-lock 清空，存在一次重入（`auth/mod.rs:L165–L177`） | 保持现状；确保 persist_auth 在所有写路径上原子执行 |
| **tool executing（工具是否在执行）** | 无持久化状态；仅通过 Tauri events 表达：emit `tool:executing` → emit `tool:completed` | 前端 `useStreaming.ts` 维护 per-tool 状态 | `legacy_send_message_impl` agent_loop（`chat_runtime_impl.rs:L2638, L2857`）; `sub_agent.rs`（`L241, L265`）; `runtime/tools/dispatcher.rs`（`L69, L71`，新路径） | **冲突**：tool 事件有两个 emit 路径：legacy 路径（直接 `app.emit`）和新 runtime 路径（`event_sink.emit` → `TauriEventAdapter`）。若同一 turn 走了混合路径，前端可能收到重复事件 | 统一到 RuntimeEventBus |
| **messages（消息持久化列表）** | `AppStorage` 文件系统：`conversations/{id}/messages.N.jsonl`（分片，每片 100 条）— `file_store/messages.rs` | `db.get_messages()` / `db.get_recent_messages()` | `db.insert_message()`（user msg 在 `chat_runtime_impl.rs:L123`；assistant msg 在 `finish_agent:L3449`） | **双写时序**：user 消息由 `send_message` 同步写入，assistant 消息由 agent_loop 异步写入（在 `finish_agent()` 中）。前端用 optimistic 消息在 DB 写入前先显示 user 消息，再用 `message:updated` event 同步 assistant 消息。若 agent_loop 崩溃，assistant 消息不会写入 DB，但前端可能显示了 streaming content | 保持文件存储；考虑 WAL 以防崩溃丢失 |

---

## 说明

- **双真相源冲突**标注的状态是重构 Phase 1 的优先修复目标：`busy` 与 `streaming` 状态的双真相源是当前架构中最高风险点。
- `run.lock` 的崩溃恢复逻辑已通过 Session UUID（非 PID）解决了 PID 复用问题（`file_store/mod.rs:L71`）。
- Python session 的 key 迁移函数（`migrate_loaded_keys_to_run_scope`，`session.rs:L62`）已提供，但 legacy 代码尚未全部迁移。
