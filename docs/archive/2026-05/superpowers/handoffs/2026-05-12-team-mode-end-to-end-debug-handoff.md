# Handoff: Team 模式端到端联调 - 多发修复后的最后一个 BUG

**Date**: 2026-05-12
**From session**: 接 `2026-05-12-lead-inbox-drain-handoff.md`，端到端测试 Team 模式 + 修若干 bug
**Next session**: 处理 ae9f39f4 会话暴露的剩余问题（Path C wake 被 set_busy 拒后消息未被自动续 turn 处理 — 已修但还需要再验证）

---

## 当前工作树状态

**未 commit**。改动 9 个文件，505 行新增/106 行修改。

```
src-tauri/src/lib.rs
src-tauri/src/runtime/agent/inbox.rs
src-tauri/src/runtime/chat/chat_turn_driver.rs
src-tauri/src/runtime/chat/tool_round_driver.rs
src-tauri/src/runtime/query_engine.rs
src-tauri/src/runtime/session_runtime.rs
src-tauri/src/runtime/tools/builtin/team_tools.rs
src-tauri/src/transport/tauri_commands/chat.rs
src-tauri/tests/team_tools_test.rs
```

`cargo test --lib` **982 passed / 0 failed**

---

## 这一轮做了哪 5 件事（按顺序）

### 1. Lead inbox 注册 + chat_turn_driver drain（接续上一发 handoff 的 Step 1+2）

**文件**：`team_tools.rs` / `chat_turn_driver.rs` / `query_engine.rs` / `agent/inbox.rs`

- `TeamCreate` 工具执行时，如果 `ctx.inbox_registry` 注入了，**给 Lead 自己也建一个 `AgentInbox::new(64)` 并注册到 InboxRegistry**（之前只注册了 name，没注册 inbox，所以 `SendMessage(to: "team-lead")` 永远报 "agent registered but has no inbox"）
- `AgentInbox::drain_pending() -> Vec<InboxItem>`（非阻塞版的 try_recv loop）
- `QueryEngine::inbox_registry()` accessor（对称 `agent_names()`）
- `chat_turn_driver::run_chat_turn_s4` 紧跟 `drain_and_inject_task_notifications` 之后调 `drain_and_inject_lead_inbox_messages`，把 inbox 里的 `InboxItem::ChatMessage` 渲染成 `<peer-messages><peer-message from=... variant=...>body</peer-message>...</peer-messages>` 注入 user message
- `is_resume_for_task_notification` 的 early-return guard 改成"task_notifications 空 **且** drained_peer_messages == 0"才 skip（否则 Path C wake 续 turn 时只有 inbox 消息没 task notification 会被错误地跳过）
- inline test：`render_peer_messages_xml_includes_from_and_variant_and_escapes_body` + `skips_non_chat_items`
- `team_tools_test.rs` 加 2 个 case：`team_create_registers_lead_inbox_when_registry_is_present` + `succeeds_without_inbox_registry_for_legacy_paths`

### 2. tool_round_driver 错误分支保留真实 tool_call_id

**文件**：`tool_round_driver.rs:217-249`

之前：当 `query_engine.run_tool_call_with_bus` 返回 `Err`（infrastructure error，例如 dispatcher 未注入），错误分支构造的 fallback outcome 把 `tool_call_id` 写成 `String::new()`。空 ID 持久化到 messages.jsonl，下一次 turn load_history 后发给 Anthropic → 400 `tool_use_id: String should match pattern '^[a-zA-Z0-9_-]+$'`。

修法：在 `call` 被 move 之前 clone 出 `tool_call_id` / `tool_name`，错误分支用这两个 clone 作 fallback。

并行路径（line 178-180）本来就这么做，只有 serial 路径漏了。

inline test：`infrastructure_error_preserves_tool_call_id_and_name`。

### 3. Path C wake 用 transport 主路径（关键架构修复）

**文件**：`session_runtime.rs` + `transport/tauri_commands/chat.rs` + `lib.rs`

**根因**：之前 `spawn_continuation`（在 `SessionRuntime` 里）跑续 turn 时用的是 base SessionRuntime 的 QueryEngine —— 它从未注入过 ToolDispatcher（dispatcher 是 transport 层 per-request 构造的，需要 services）。所以 Path C wake 起的续 turn 里**所有工具调用都报 "tool dispatcher not configured"**，error 持久化到 messages.jsonl 污染历史。

**修法**：让 wake_fn 走 transport 主路径 `adapter.send_chat_request(...)` —— 跟用户主动 send_message 同款代码（per-request 构造 dispatcher + set_busy + run_chat_request + clear_task）。

具体改动：
- `SessionRuntime` 删除 `wire_lead_idle_wake_path` / `spawn_continuation` / `with_gateway` / `gateway` 字段
- `SessionRuntime` 新增 `lead_idle_supervisor()` accessor
- `TauriChatCommandAdapter` 新增 `wire_path_c_wake_to_self(self: &Arc<Self>)`，wake 闭包持 `Weak<Self>`，upgrade 后调 `send_chat_request`
- `lib.rs` 在 `chat_adapter = Arc::new(...)` 之后调 `chat_adapter.wire_path_c_wake_to_self()`

被并发 set_busy 拒的情况会 swallow 并 log warn（`continuation turn rejected (likely a concurrent turn already busy)`）。

**实测验证**：会话 ae9f39f4 dev server log line 17512 起 `wake_fn installed` 多次 — 已生效。新会话再没出现过 "tool dispatcher not configured"。

### 4. TeamCreate 同步标记 supervisor 状态（接 #3 暴露的二级问题）

**文件**：`team_tools.rs:138-155`

**问题**：#3 修完后，发现 Path C wake 经常被 set_busy 拒（log warn `continuation turn rejected: This conversation is already processing`）。

**根因**：`LeadIdleSupervisor` 的状态机 和 `RuntimeRunRegistry` 不同步。Lead 在 user turn 中**跑到一半**才调 TeamCreate，但 `lead_key_and_mark_running`（在 turn 入口跑）在 TeamCreate 之前已经跑过——那时 `agent_names.resolve("team-lead")` 返 None → 没 mark_running → supervisor 状态机里 lead 一直是默认的 Idle。

接下来 teammate SendMessage → supervisor 看 lead idle → fire wake_fn → set_busy 拒（因为 user turn 真在跑）→ pending 也没记录 → **inbox 里的消息卡着不会被处理**。

**修法**：`TeamCreate::execute` 末尾，如果 `ctx.lead_idle` 注入了，立刻 `sup.mark_running(&(session, lead_id)).await`。

这样 SendMessage 进来时 supervisor 看到 lead Running → 走 `already_running_pending_recorded` 分支 → **不 fire wake_fn** → Path A 在 user turn 结束 mark_idle 时检测到 pending → 起续 turn 自动 drain inbox。

### 5. 文档：`2026-05-12-lead-inbox-drain-handoff.md`（上一发的）

不动，留作历史。

---

## 当前已知的剩余问题（重要 — 下次会话先验证）

### 问题 A: TeamCreate 修复后实际效果待验证

我做了 #4 的修复**就被你让我做交接了**，没在新会话里验证修复是否生效。

**怎么验证**：
1. 新开会话，让 Lead 调 TeamCreate + spawn teammate
2. teammate 调 SendMessage(to="team-lead")
3. 看 log 是否出现 `[path_c_wake] continuation turn rejected (likely a concurrent turn already busy)` ← **不应该再频繁出现**
4. 应该看到 `[LeadIdleSupervisor] enqueue already-running session=... pending=true`
5. user turn 结束时应该看到 `mark_idle_and_maybe_emit_pending` 触发 pending 续 turn
6. 续 turn 应该看到 `[chat_turn_driver] drained N peer message(s)` 把 teammate 消息读出来

### 问题 B: 网关 60 RPM 限速

ae9f39f4 会话最后失败原因是 lotus 网关返回 `429 Rate limit exceeded: 60 requests per minute`。这是 **lotus 网关给当前账户/key 设的限速**，不是客户端 bug。

短期建议：lotus 后台调高 RPM，或在 team 模式下限制 teammate 滥用 WebSearch（每次 search = 1 个 stream_message）。

### 问题 C: LLM 行为本身（不是 bug 但影响 UX）

我多次观察到 Lead claude-sonnet-4-5 的行为：
- 给 teammate 推一条 SendMessage 后立刻就开始 TeammateStop 重新 spawn
- 调 `Agent` 工具时在"传 team_name = teammate 模式"和"不传 = standalone 一次性子代理"之间反复横跳
- 看到 teammate idle 就觉得"出问题了"

cc-best 用了 113 行的 `TeamCreate prompt.ts` 教 LLM Team workflow（idle 语义、消息自动送达、teammate 是持续在线的、不要 stop、be patient）。我们的 `TeamCreate` 工具描述只有 373 字符，**没教这些**。

用户当前的态度是"先用 user prompt 自己写清楚要求"，不改工具描述。

---

## 下次会话要做的事（按优先级）

### 必做：验证 #4 修复

按"问题 A"的步骤跑一轮，确认 supervisor 状态机这次跟 RunRegistry 一致了。

### 验证完通过后：commit 全部改动

5 个修复都是独立的 + 互相依赖（按 1→2→3→4 时间顺序）。建议 1 个 commit 或拆成 2 个：
- commit 1: Path C wake 主路径迁移 + ToolCallId fallback + Lead inbox 注册 + drain 注入（核心修复）
- commit 2: TeamCreate 同步标记 supervisor（接续问题）

或者一锅端：

```
feat(ltr): Path C wake 走 transport 主路径 + 修若干 inbox/dispatcher bug

- TeamCreate 注册 Lead inbox + 主动 mark_running 同步 supervisor 状态
- chat_turn_driver 加 drain_and_inject_lead_inbox_messages，把 peer SendMessage 注入下一轮 user message
- AgentInbox::drain_pending 非阻塞 try_recv loop
- tool_round_driver serial 错误分支保留真实 tool_call_id，修复 Anthropic 400 tool_use_id 报错
- Path C wake 从 SessionRuntime 搬到 TauriChatCommandAdapter，复用 send_chat_request 主路径
  (per-request 构造 ToolDispatcher，根治 "tool dispatcher not configured")
- SessionRuntime 删 wire_lead_idle_wake_path / spawn_continuation / gateway 字段
- 加 inline tests + team_tools 集成 test 覆盖新行为
```

### 可选：处理"问题 C"

如果跑团队任务还是发现 Lead 乱来（TeammateStop 误用、不耐心等），动 `TeamCreate` 工具描述里塞 cc-best 风格的 workflow 描述（≈3500 字符）。但要慎重，宁愿先用户 prompt 调，不动工具描述。

---

## 上下文文件清单

启动新会话后先读这些：

1. **本文件**（你正在读的）
2. **上一发 handoff**: `docs/superpowers/handoffs/2026-05-12-lead-inbox-drain-handoff.md`
3. **关键代码**:
   - `src-tauri/src/runtime/tools/builtin/team_tools.rs` — TeamCreate 新加的 mark_running
   - `src-tauri/src/runtime/agent/lead_idle.rs` — supervisor 状态机
   - `src-tauri/src/runtime/agent/inbox.rs:79-117` — AgentInbox::drain_pending
   - `src-tauri/src/runtime/chat/chat_turn_driver.rs:400-510` — drain_and_inject_lead_inbox_messages + render_peer_messages_xml
   - `src-tauri/src/transport/tauri_commands/chat.rs:2355-2440` — wire_path_c_wake_to_self
   - `src-tauri/src/lib.rs:589-600` — adapter Arc + wire 调用
   - `src-tauri/src/runtime/chat/tool_round_driver.rs:217-260` — serial dispatch fallback id
4. **cc-best 参照**:
   - `~/github/claude-code-best/src/utils/teammateMailbox.ts:84-192` — readMailbox / writeToMailbox
   - `~/github/claude-code-best/src/utils/attachments.ts:3533-3690` — getTeammateMailboxAttachments
   - `~/github/claude-code-best/src/utils/swarm/inProcessRunner.ts:680-770` — teammate waitForNextPromptOrShutdown
   - `~/github/claude-code-best/src/utils/messageQueueManager.ts` — queueChanged.emit 通知机制
   - `~/github/claude-code-best/src/tools/TeamCreateTool/prompt.ts` — 113 行 Team workflow 描述

---

## 工作模式提醒（沿用上一发 handoff 的）

- 用户对话语言：中文
- 工作分支：`ltr-mvp`（不是 main）
- 严禁瞎抽象/过度设计，参照 cc-best 但适配桌面端差异
- 改动后必须 `cargo check --lib` + `cargo test --lib` 验证（982 passing 是基线）
- 用户不要无意义的单测（"不是端到端的都是假的"），生产代码改动后自己重启 dev server 让用户在前端实测
- 重启 dev server：`pnpm tauri:dev`（run_in_background）
- 严禁在没拿到用户确认前自作主张大改
- 不要做"再改一处补救"式的修补，先理清根因再下手
- commit message 中文，按 `feat(scope): ...` / `fix(scope): ...` 风格
- **commit 前必须先让用户在前端验证修复有效**

---

## 5 个最近相关 commit（这些都是 push 过的）

```
4da8a645 docs(handoff): LTR Lead inbox + turn-start drain — context handoff
53545e78 fix(teammate): inject LTR registries into Teammate QueryEngine
e347a344 fix(teammate): inject TEAMMATE_TOOLS into real_turn whitelist + add TaskUpdate/TaskClaim
c8c87139 feat(teammate): default-grant permission for Teammate working dirs + diag log
4be2a19b feat(teammate): wire LlmGateway into Teammate idle loop via launcher trait
```
