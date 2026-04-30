# Chat Responsibility Map — `send_message` 主流程

> 基于 `src-tauri/src/transport/tauri_commands/chat/chat_runtime_impl.rs` 实际代码生成。
> 关键函数：`legacy_send_message_impl`（L4–L1076）和 `agent_loop`（L1504–L3328）。

## 入口路径

```
commands/chat.rs:send_message (L21)
  → TauriChatCommandAdapter::send_message (chat.rs:L149)
    → SessionRuntime::run_chat_request
      → TauriLegacyTurnExecutor::run_chat_turn (chat.rs:L90)
        → legacy_send_message_impl (chat_runtime_impl.rs:L4)
```

---

## 职责地图

| Responsibility | Current Function / Section | Line Range | Dependencies | Side Effects | Target Layer |
|---|---|---|---|---|---|
| **Input normalization** — 为 LLM 构造含文件引用的完整 `llm_content`，不含 sender 信息 | `legacy_send_message_impl` body | L57–L149 | `db.get_uploaded_files_by_ids()` | None（纯构造） | Command |
| **Conversation load / message history** — 从 DB 加载最近 30 条消息，过滤 tool result，按角色重构 `ChatMessage` vec | `legacy_send_message_impl` | L285–L313 | `db.get_recent_messages(conv_id, 30)`, `compress_tool_result()` | None | Command |
| **Auth check (cloud mode)** — 若 `use_cloud=true`，通过 `AuthManager::get_session_key()` 获取 session key；失败则 emit `streaming:error` 或 `auth:expired` 并 early-return | `legacy_send_message_impl` | L202–L283 | `auth_manager.get_session_key()` | emit `streaming:error`; emit `auth:expired`; `gateway.clear_task()` | Command |
| **API key decryption** — 从 `SecureStorage` 解密 primary/tavily/bocha keys；cloud 模式直接用 session key 覆盖 | `legacy_send_message_impl`, `decrypt_key()` | L187–L200, L3836–L3860 | `crypto::SecureStorage` | 覆盖 `settings.primary_api_key` | Command |
| **Model selection** — 根据 `settings.primary_model` + `use_cloud` 确定最终 model；cloud 时强制 `"lotus"` + `cloud_model` | `legacy_send_message_impl` | L202–L256 | `settings.primary_model`, `settings.cloud_model` | `settings.primary_model` 被覆盖 | Command |
| **Busy guard (TOCTOU prevention)** — 先检查会话是否 busy，再 `set_busy_for_run()` 原子占位；全局并发上限 3 | `legacy_send_message_impl` | L27–L54 | `gateway.is_conversation_busy()`, `gateway.set_busy_for_run()` | `RuntimeRunRegistry` 写入 | Command |
| **Conversation mode detection & skill activation** — 读取 `conversation.mode` (`daily`/`confirming`/`analyzing`)；daily 时调用 `SkillRegistry::detect_activation()`，有 workflow 则设 mode=`confirming` | `legacy_send_message_impl` | L338–L967 | `db.get_conversation_mode()`, `skill_registry.detect_activation()`, `orchestrator::advance_step()` | `db.set_conversation_mode()`, `db.set_memory(active_skill)`, `db.insert active_task` | Command |
| **Context build (analysis notes + file context)** — 构造注入系统提示词的动态上下文：file_context、analysis_notes、precompute 结果、connector context | `agent_loop` > `build_file_context()`, `build_analysis_notes_context()` | L1538, L1232–L1310, L1317–L1490 | `db.get_memories_by_prefix()`, `db.get_uploaded_files_for_conversation()` | telemetry 写入 | Agent |
| **Precompute** — 执行 skill 的 Python 预计算脚本，缓存结果 JSON 文件；失败则降级为 agent 模式 | `agent_loop` | L1635–L1872 | `session_mgr.execute_for_run()`, `SandboxConfig` | 生成缓存 JSON 文件，向 DB 注册 `generated_file` | Agent |
| **Stream emit (streaming:delta)** — 每个 `ContentDelta` chunk 经 `strip_thinking_markers` + `prompt_guard::check_for_leak` 后 emit 给前端 | `agent_loop` inner stream loop | L2356–L2388 | `tauri::AppHandle::emit` | Tauri event bus | Agent |
| **Tool loop** — 循环最多 `max_iterations` 次：stream → 收集 tool calls → 执行（单个串行 / 多个并行） → 追加结果到 messages | `agent_loop` for-loop | L2028–L3102 | `tool_registry.execute()`, `gateway.stream_message()` | messages vec 增长；`analysis_ctx` 更新 | Agent |
| **Tool executing/completed events** — 每个 tool 执行前 emit `tool:executing`，执行后 emit `tool:completed` | `agent_loop` | L2638, L2857 | `tauri::AppHandle::emit` | Tauri event bus | Agent |
| **Sub-agent (browse_data)** — 由 `tool_registry.execute("browse_data")` 内部委托给 `sub_agent::run_sub_agent_loop()`，该函数同样 emit `tool:executing` / `tool:completed` | `llm/sub_agent.rs` | L241, L265 | `tool_registry`, `AppHandle` | Tauri event bus | Agent |
| **Context compression** — daily 模式当消息总字符超 24K 时，调用非流式 LLM 压缩旧消息 | `compress_context_if_needed()` | L1087–L1198 | `gateway.send_message()` | messages vec 被替换 | Agent |
| **Context decay** — 每次迭代对旧 tool result 施加字符衰减，非破坏性 | `context_decay::apply_decay()` | L2139 | 无外部依赖 | 仅创建临时 vec | Agent |
| **Persist (finish_agent)** — 解 PII mask，leak 检测，DB 写入 assistant 消息，emit `message:updated`，auto-generate 会话标题 | `finish_agent()` | L3337–L3524 | `db.insert_message()`, `mask_ctx.unmask()` | DB 写入；emit `message:updated`；emit `conversation:title-updated` | Agent |
| **Cancel / cleanup (AgentGuard)** — tokio::spawn 中包 guard；agent_loop 结束或 panic 时 guard.clear() 释放 gateway slot + run.lock，emit `streaming:done` + `agent:idle` | `AgentGuard::clear()` / `Drop` | chat_support.rs:L538–L610 | `gateway.clear_task()`, `session_mgr.destroy_run()` | DB 写入 `run.lock` 删除；emit `streaming:done`；emit `agent:idle` | Agent |
| **Agent timeout** — 15 分钟外层 `tokio::time::timeout` 超时后 emit `streaming:error` (agent_timeout) | `tokio::spawn` wrapper | L1031–L1073 | `tokio::time::timeout` | emit `streaming:error` | Agent |
| **Step advance** — 当前步骤完成时调用 `orchestrator::advance_step` 标记 completed，checkpoint 提取，chat_messages 重置，emit `streaming:step-reset`，进入下一步 | `legacy_send_message_impl` / AdvanceToStep branch | L774–L922 | `orchestrator::advance_step()`, `checkpoint::checkpoint_extract()` | DB 写入步骤状态；emit `streaming:step-reset` | Command |

---

## 关键常量（`chat_runtime_impl.rs` 顶部）

| Constant | Value | Purpose |
|---|---|---|
| `MAX_TOOL_ITERATIONS` | 30 | daily 模式最大迭代次数 |
| `AGENT_TIMEOUT_SECS` | 900 | 全局 agent 超时（15 分钟） |
| `CHUNK_TIMEOUT_SECS` | 90 | 单个 chunk 无数据超时 |
| `MAX_STREAM_RETRIES` | 2 | 迭代内流重试次数 |
| `COMPRESS_THRESHOLD_CHARS` | 24_000 | 触发 context 压缩的字符阈值 |
| `COMPRESS_KEEP_RECENT` | 10 | 压缩时保留的最近消息数 |
| `MAX_HISTORY_MESSAGES` | 30 | 从 DB 加载的历史消息窗口 |
| `MAX_CONCURRENT_AGENTS` | 3 | 最大并发 agent 数（`gateway.rs:L35`） |
