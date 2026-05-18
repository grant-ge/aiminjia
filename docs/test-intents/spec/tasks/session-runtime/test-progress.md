# test-progress.md — session-runtime 主链路事件序列

## 状态

- 已实现测试文件：`src-tauri/tests/review_session_runtime_test.rs`
- 执行命令：`cd src-tauri && cargo test --test review_session_runtime_test -- --nocapture`
- 最近执行结果：5 passed；生产代码无需修改

## 执行记录

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：正常 turn 完成后 EventBus 中包含完整的事件序列且顺序正确 | ✅ 通过 | 覆盖 RunStarted 首位、AgentIdle 末位、StreamDone/TurnCompleted/AgentIdle 顺序、无工具事件 |
| 意图 2：同一 turn 内所有事件携带相同的 run_id | ✅ 通过 | 所有 recorded events run_id 一致 |
| 意图 3：LLM 调用工具后下一轮收到 tool_result，turn 继续推进直至完成 | ✅ 通过 | 覆盖 ToolCallExecuting/Completed 各 1 次、Success、LLM 第二轮收到工具结果 |
| 意图 4：CancellationToken 触发后 turn 正常退出，不挂起 | ✅ 通过 | 5s timeout 内返回，TurnCompleted=Cancelled，最后 AgentIdle |
| 意图 5：达到最大迭代次数时 turn 以 MaxIterationsReached 结束 | ✅ 通过 | max_iterations=2，工具执行 2 次，TurnCompleted=MaxIterationsReached，最后 AgentIdle |

## 执行记录详情

- 2026-04-24：新增 `review_session_runtime_test.rs`，通过顶层 `SessionRuntime::run_chat_request` 覆盖生命周期、run_id、工具推进、取消、最大迭代。
- 2026-04-24：首次运行 4 passed / 1 failed，缺 `StreamDelta`。根因是测试 mock executor 没有模拟 provider streaming adapter 发 delta；按生产职责在 mock 返回 ContentComplete 前 emit `StreamDelta`。
- 2026-04-24：运行 `cd src-tauri && cargo test --test review_session_runtime_test -- --nocapture`，结果 5 passed。仅出现既有 dead_code warning：`FILE_GEN_TOOLS`、`is_last_tool_file_generation` 未使用。
- 2026-05-18：意图 1 补做 agent 跑（产品验收）变体，验证 CLI 端到端真实环境可用。环境：`pnpm tauri:dev` 起在 5173 + 真实 LLM。CLI：`tauri-pilot aijia *` 16 命令组。22/22 断言通过：
  - 状态机：`isStreaming` false→true→false 自然收口（对应 RunStarted→AgentIdle）
  - 流式：`wait-reply` stableTicks=3（对应 StreamDone 在 TurnCompleted 之前）
  - 工具空集：`ui-message --include-tools` 中 role=tool_call 计数=0（对应不发 ToolCallExecuting/Completed）
  - 持久化：`~/.renlijia/users/{scope}/conversations/{id}/messages.jsonl` 含 user+assistant 各 1 条（对应 MessagePersisted）
  - 一致性：UI `last-reply.text` == 磁盘 `assistant.content.text`（对应同 run_id 全程一致，流不串）
  - 唯一标识：用 marker `intent1-{epoch}` 验证 user prompt 和 assistant 回复同时落盘（grep 次数=2）
  - **观察到的 CLI 行为差异（更新到 `context/capabilities.md` 已知边界）**：① `new-task` 是 lazy 创建，`where.sessionId=null`/`messageCount` 上次会话残值，必须 send 后才生成新 conv_id；② `ui-message` 返回**顶层 array** 不是 `{messages:[]}`；③ 消息字段是 `text` 不是 `content`；④ list-sessions 字段是 `active/archived/title` 不是 `isActive/isArchived/name`。
  - conv_id 留档：`569a5aab-d985-457b-8625-4d98b47dae75`（dev workspace t_28__u_54）
- 2026-05-18：意图 4 补做 agent 跑变体，验证 cancel 路径真实环境可用。**15/15 断言通过**：
  - 发 500 字长 prompt 让 LLM 进入慢回复 → 1.5s 时 `isStreaming=True`（RunStarted 已发出）
  - `aijia cancel` 触发后 1s 内：`isStreaming=False` + `hasEditor=True`（对应 TurnCompleted=Cancelled + AgentIdle）
  - `wait-reply --timeout 5` 实测 1s 返回 ok=true（对应原 cargo 断言"5s 内不挂起"）
  - 磁盘 `messages.jsonl` 只含 user 消息（74 字 + marker），**无 assistant 残留**（最早 cancel 路径，流式还没产物落盘就被打断）
  - cancel 后编辑器立即可再次输入（验证 UI 完全解封）
  - **产品视角发现的 2 个非阻塞问题**（值得后续 follow up，不影响 cancel 主链路正确性）：
    1. UI `ui-message --role assistant` 在 cancel 后返回 1 条 **空字符串** assistant 消息（`{id, role:"assistant", text:""}`），磁盘没有 — 是流式占位气泡的残影。空气泡 UI 体验不优雅
    2. DOM 用 `[data-aijia-message-role="assistant"]` 查不到这条 bubble，但 `ui-message` 仍能 dump 出来 — 说明 `ui-message` 子命令内部用的选择器跟 Phase 0 加的 `data-aijia-*` hook 不一致（可能走的是 `.message-assistant` 类名）。要么把空气泡过滤掉，要么把 hook 对齐
  - conv_id 留档：`e95ba4be-680c-4182-ba8c-c4673d8b7de8`
