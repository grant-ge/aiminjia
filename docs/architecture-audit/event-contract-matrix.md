# Event Contract Matrix

> 通过搜索 `src-tauri/src` 所有 `.emit(` 调用 + `src/lib/tauri.ts` 所有 `listen()` 调用生成。
> 实际事件名以代码为准（与需求文档一致，无差异）。

---

## 事件矩阵

| Event | Emitter (file:line) | Consumer (file:line) | Required Payload Fields | Ordering Contract |
|---|---|---|---|---|
| **streaming:delta** | `chat_runtime_impl.rs:2382` (legacy agent_loop, `ContentDelta` 处理); `tauri_event_adapter.rs:L20` (runtime 路径，`RuntimeEventKind::StreamDelta`) | `src/hooks/useStreaming.ts:154` → buffered RAF flush → `useChatStore.appendConversationStreamingContent` | `conversationId: string`, `delta: string` | 必须在 `streaming:done` 之前；chunk 顺序与 SSE 顺序一致；前端用 deltaBuffer + rAF 合批 |
| **streaming:done** | `chat_support.rs:548`（`AgentGuard::clear()`，async）; `chat_support.rs:593`（`AgentGuard::Drop`，sync fallback）; `chat.rs:196`（delete_conversation 中 agent 被取消时）; `tauri_event_adapter.rs:L28`（runtime 路径） | `src/hooks/useStreaming.ts:165` → flush deltas → `clearConversationStreamState` + `removeBusyConversation` | `conversationId: string`, `messageId: string`（AgentGuard 发出时为空字符串）, 可选 `runId: string` | 保证在 `message:updated` 之后（`finish_agent` 先写 DB 再 emit `message:updated`，`streaming:done` 由 AgentGuard 在 agent_loop 返回后 emit）；`streaming:error` 与 `streaming:done` 互斥，两者都会清理 streaming 状态 |
| **streaming:error** | `chat_runtime_impl.rs:219`（cloud 未登录）; `chat_runtime_impl.rs:235`（auth expired）; `chat_runtime_impl.rs:275`（API key 为空）; `chat_runtime_impl.rs:2231`（gateway error）; `chat_runtime_impl.rs:2330`（chunk timeout）; `chat_runtime_impl.rs:2471`（stream error） | `src/hooks/useStreaming.ts:178` → flush deltas → `clearConversationStreamState` + `removeBusyConversation` + 显示错误 toast | `conversationId: string`, `error: string`, 可选 `errorType: 'chunk_timeout'|'stream_error'|'gateway_error'|'agent_timeout'`, 可选 `rawError`, `partialContent`, `timeoutSeconds`, `iteration`, `maxIterations` | 发出后 agent_loop 立即 return；AgentGuard 之后仍会 emit `streaming:done`（safety net），前端需幂等处理重复清理 |
| **tool:executing** | `chat_runtime_impl.rs:2639`（legacy，每个 tool 执行前）; `sub_agent.rs:241`（sub-agent 路径）; `runtime/tools/dispatcher.rs:69`（新 runtime 路径，通过 `event_sink.emit`） | `src/hooks/useStreaming.ts:248` → `addToolExecution({ toolId, toolName, status:'running' })` | `conversationId: string`, `toolName: string`, `toolId: string`, 可选 `purpose: string` | 在 `tool:completed`（同 toolId）之前；同一 turn 内可多个工具并发 emit（并行执行时）；前端用 toolId 区分 |
| **tool:completed** | `chat_runtime_impl.rs:2679`（tool blocked，`success:false`）; `chat_runtime_impl.rs:2858`（tool 执行完成）; `sub_agent.rs:265`（sub-agent 成功）; `sub_agent.rs:329`（sub-agent 失败）; `runtime/tools/dispatcher.rs:71`（新 runtime 路径） | `src/hooks/useStreaming.ts:262` → `updateToolExecution({ toolId, success, summary, status:'done' })` | `conversationId: string`, `toolName: string`, `toolId: string`, `success: boolean`, 可选 `summary: string` | 必须有对应 `tool:executing`（同 toolId）先到；blocked 工具也会 emit completed（`success:false`） |
| **message:updated** | `chat_runtime_impl.rs:3458`（`finish_agent()`，仅在 DB 写入成功后）; `tauri_event_adapter.rs:L60`（runtime 路径，`RuntimeEventKind::MessagePersisted`） | `src/hooks/useStreaming.ts:206` → `upsertMessage(message)` | legacy 路径：完整 Message 对象（`id`, `conversationId`, `role`, `content`, `createdAt`）; runtime 路径：`conversationId`, `messageId`, `runId` | DB 持久化完成后才 emit（保证 UI 不会展示会在刷新后消失的消息）；在 `streaming:done` 之前或同时 |
| **agent:idle** | `chat_support.rs:562`（`AgentGuard::clear()`）; `chat_support.rs:601`（`AgentGuard::Drop`）; `chat.rs:203`（delete_conversation 取消 agent 时）; `tauri_event_adapter.rs:L68`（runtime 路径） | `src/hooks/useStreaming.ts:305` → `removeBusyConversation` + `clearConversationStreamState`（safety net） | `conversationId: string`, 可选 `runId: string`, 可选 `agentId: string`（runtime 路径） | 与 `streaming:done` 一起 emit（同一 `AgentGuard::clear()` 调用中，`streaming:done` 先于 `agent:idle`）；前端幂等处理 |
| **streaming:step-reset** | `chat_runtime_impl.rs:805`（步骤切换时，AdvanceToStep 分支，在 checkpoint 提取前 emit） | `src/lib/tauri.ts:804`（`onStreamingStepReset`，frontend 注册）→ 清除前一步的 streaming content，保持 `isStreaming=true` | `conversationId: string`, `step: number`（新步骤编号） | 在新步骤的第一个 `streaming:delta` 之前；新步骤 agent_loop spawn 前 emit（防止 watchdog 30s 超时） |
| **agent:phase** | `llm/taor.rs:113`（`PhaseTracker::emit()`，在 think/act/observe 切换时调用） | `src/hooks/useStreaming.ts:298` → `setConversationAgentPhase(conversationId, phase)` | `conversationId: string`, `iteration: number`, `phase: 'think'|'act'|'observe'`, `prevPhaseDurationMs: number`, `toolNames: string[]`, `maxIterations: number` | 仅在 `settings.enable_taor_tracking=true` 时 emit；可选事件，UI 用于显示 TAO-R 进度 |
| **conversation:title-updated** | `chat_runtime_impl.rs:3515`（首次 assistant 回复后 auto-generate title）; `chat.rs:225`（rename_conversation 后） | `src/lib/tauri.ts:761`（`onConversationTitleUpdated`，frontend 注册）→ 更新会话列表标题 | `conversationId: string`, `title: string` | 无严格顺序要求；auto-generate 在 `message:updated` 之后 |
| **analysis:step-changed** | `llm/tool_executor/progress.rs:37`（`handle_update_progress()` 工具调用时） | `src/lib/tauri.ts:705`（`onAnalysisStepChanged`） | `step: number`, `status: string` | 在 `update_progress` tool 执行成功后 emit；步骤状态变化的辅助通知 |
| **file:generated** | `chat_runtime_impl.rs:2822`（tool 返回 `file_meta` 时，在 tool 结果处理阶段） | `src/hooks/useStreaming.ts:321` → 显示文件降级 warning toast | `conversationId: string`, `fileId: string`, `fileName: string`, `requestedFormat: string`, `actualFormat: string`, `fileSize: number`, `storedPath: string`, `category: string`, `isDegraded: boolean`, 可选 `degradationNotice: string` | 在 `tool:completed` 之前（同一工具执行循环内，file:generated 先 emit）；多个文件则多次 emit |
| **auth:expired** | `chat_runtime_impl.rs:234`（cloud auth 过期时，get_session_key 失败且非"未登录"） | `src/lib/tauri.ts:836`（`onAuthExpired`） | `message: string` | 发出后立即 return，不继续处理消息 |
| **browser:navigating** | `connector/playwright_browser.rs:87` | 无注册消费者（内部监控用） | `url: string` | 无 |

---

## 注意事项

1. **重复 emit 风险**：`streaming:done` 和 `agent:idle` 在 `AgentGuard::clear()`（async）和 `AgentGuard::Drop`（sync fallback）两处均有 emit，前端需幂等处理。
2. **双路径冲突**：`tool:executing` / `tool:completed` / `streaming:delta` / `streaming:done` / `agent:idle` / `message:updated` 都有 legacy 路径（直接 `app.emit`）和新 runtime 路径（`RuntimeEventBus → TauriEventAdapter`）。目前 legacy 代码是主路径，新路径仅在测试中验证。混用同一 turn 会产生重复事件。
3. **streaming:done messageId**：AgentGuard emit 时 `messageId` 为空字符串；只有 `finish_agent` 的 `message:updated` 携带真实 message id。`streaming:done` 的 messageId 字段目前在前端未使用。
