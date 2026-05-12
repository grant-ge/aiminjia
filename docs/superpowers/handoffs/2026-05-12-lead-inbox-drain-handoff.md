# Handoff: Lead Inbox + Turn-start Drain (LTR P2 收尾)

**Date**: 2026-05-12
**From session**: ltr-mvp 接 LLM 工作流（上下文已超载）
**Next session**: 继续完成 Lead inbox + turn-start drain

---

## 一句话

让 teammate 通过 `SendMessage(to="team-lead", ...)` 发的消息能**真正出现在前端聊天 UI**。

## 当前到了哪

LTR P2 大部分已通：
- ✅ Teammate 真接 LLM（之前是 P1 stub）
- ✅ Teammate 工具白名单包含 SendMessage/TaskList/TaskGet/TaskUpdate/TaskClaim
- ✅ Teammate 权限默认放行自己的工作目录（is_async + additional_working_dirs）
- ✅ Teammate 的 QueryEngine 注入了 LTR registries（team/names/inbox/lead_idle/cancel）
- ✅ Teammate 真的会调 `SendMessage(to="team-lead", message={...})`，dispatcher permission Allow
- ❌ **Lead 收不到这条消息**——SendMessage 在 send_message.rs:209 报 ToolError "agent `team-lead` is registered but has no inbox"

## 根因

`team_tools.rs::TeamCreate` 只 register Lead 的 **name** 到 `AgentNameRegistry`，**没给 Lead 创建 `AgentInbox`**。
send_message.rs:204-213 的逻辑是：

```
resolve("team-lead") → lead_id ✅
inbox_reg.get(lead_id) → None ❌（Lead 没 inbox）
→ ToolError "agent `team-lead` is registered but has no inbox"
→ 永远走不到 line 247 的 Path C wake
```

## 方向（参照 cc-best 但适配桌面端）

cc-best CLI 把 Lead 跟 teammate 当对等节点，**都有 mailbox**。但 cc-best 用文件 mailbox，我们用内存 mpsc，且 Lead 不是常驻循环（每次 chat turn 跑完就 idle）。

借鉴**思想**不照搬实现：
1. Lead 也有 `AgentInbox`
2. SendMessage 统一走 inbox.send（不区分对象）
3. Lead 在 chat turn 开始前 drain 自己的 inbox，把 unread 拼到 user message
4. 桌面端特有：Lead 的 inbox 写完后调 `LeadIdleSupervisor.enqueue` 唤醒（已存在）

## 具体改动

### Step 1: TeamCreate 给 Lead 分配 inbox

**文件**: `src-tauri/src/runtime/tools/builtin/team_tools.rs:85-115`

现状（line 102-115 register name）后面追加：

```rust
// Create + register the Lead's inbox so teammates can SendMessage(to: "team-lead").
let lead_inbox = crate::runtime::agent::AgentInbox::new(64);
ctx.inbox_registry()
    .register(&session, lead_id.clone(), lead_inbox)
    .await;
```

**注意**:
- `ctx.inbox_registry()` 是否已暴露？看 `runtime/tools/context.rs` ToolExecutionContext
- 如果没有 accessor，需要加（参照 `agent_names()` / `team_registry()`）
- `InboxRegistry::register` 签名见 `src-tauri/src/runtime/agent/inbox_registry.rs`

### Step 2: chat_turn_driver turn 开始 drain Lead inbox

**文件**: `src-tauri/src/runtime/chat/chat_turn_driver.rs:run_chat_turn_s4`（约 1240-1305 行附近，load_history + persist_user_message 之间）

在调 `gateway.stream_message` 之前、把 messages 拼好之后：

```rust
// LTR: drain Lead's inbox and append unread peer messages as a
// system-reminder XML attachment to the current user message.
// Mirrors claude-code-best's getTeammateMailboxAttachments() (which runs
// at attachment phase for every agent including the Lead).
if let (Some(inbox_reg), Some(names_reg)) = (&self.inbox_registry, &self.agent_names) {
    if let Some(lead_id) = names_reg.resolve(turn.session_id(), "team-lead").await {
        if let Some(lead_inbox) = inbox_reg.get(turn.session_id(), &lead_id).await {
            let mut drained: Vec<InboxItem> = Vec::new();
            while let Ok(item) = lead_inbox.try_recv() {  // 需要给 AgentInbox 加 try_recv_all
                drained.push(item);
            }
            if !drained.is_empty() {
                let xml = render_peer_messages_xml(&drained);
                // Append to user message content（在 build_user_content_json 后追加）
                user_message = append_system_reminder(user_message, &xml);
            }
        }
    }
}
```

**关键点**:
- `AgentInbox::try_recv_all()` 当前可能没暴露，看 `src/runtime/agent/inbox.rs` 现有 API。如果没有需要加（一个 loop 调 `try_recv()` 直到 Empty）
- 渲染 XML 格式建议：
  ```xml
  <peer-messages>
    <peer-message from="小研" ts="2026-05-12T17:00:04Z" variant="text">
      调研完成，详见 ...
    </peer-message>
  </peer-messages>
  ```
- chat_turn_driver 已有 `task_notification_queue` 的 drain 逻辑（找 `drain_and_inject_task_notifications`），可以照搬模式

### Step 3: send_message.rs 不需要改

Lead 有 inbox 后，line 209 的 `inbox_reg.get(lead_id)` 自然成功，line 247 的 Path C `lead_idle.enqueue` 会触发。

### Step 4: 编译 + 测试

```bash
cd src-tauri && cargo check --lib
cargo test --lib                  # 应 979 passed / 0 failed
cargo test --test teammate_idle_loop_skeleton_test --test send_message_routing_test  # LTR 核心回归
```

### Step 5: 重启 dev server

当前 dev server 运行中（task id `bhmwh69jh`）：

```bash
# TaskStop bhmwh69jh
# pnpm tauri:dev (run_in_background)
```

让用户在前端测：
1. Lead 派活给小研（spawn_subagent + team_name + employee_id）
2. 等小研调 SendMessage(to="team-lead", ...)
3. **预期看到 UI 上 Lead 续 turn 时把 teammate 消息当 user message 回应**

## 测试日志关键字

成功标志：

```
[spawn_teammate][engine-build] team_reg=true names_reg=true inbox_reg=true lead_sup=true cancel_reg=true
[dispatcher][permission-trace] tool='SendMessage' is_async=true decision=Allow
tool.send_message.entry payload={"to":"team-lead", ...}
tool.send_message.inbox_sent  ← 新方案下应该出现
tool.send_message.path_c_enqueue payload={"transition":"idle_to_running", "wake_fired":true}
[chat_turn_driver] draining N peer messages for Lead ...  ← 新加日志
```

## 上下文文件清单

启动新会话后先读这些：

1. **本文件**（你正在读的）
2. **设计 spec**: `docs/superpowers/specs/2026-05-12-teammate-llm-turn-design.md`
3. **关键代码**:
   - `src-tauri/src/runtime/tools/builtin/team_tools.rs:60-130` (TeamCreate)
   - `src-tauri/src/runtime/tools/builtin/send_message.rs:48-345` (SendMessage)
   - `src-tauri/src/runtime/chat/chat_turn_driver.rs:1240-1330` (run_chat_turn_s4 message 拼装)
   - `src-tauri/src/runtime/agent/inbox.rs` (AgentInbox API)
   - `src-tauri/src/runtime/agent/inbox_registry.rs` (InboxRegistry API)
   - `src-tauri/src/runtime/tools/context.rs:73-87` (ToolExecutionContext LTR 字段)
4. **cc-best 参照**:
   - `~/github/claude-code-best/src/utils/attachments.ts:3533` (getTeammateMailboxAttachments)
   - `~/github/claude-code-best/src/utils/teammateMailbox.ts:134` (writeToMailbox)

## 最近 5 个相关 commit

```
e347a344 fix(teammate): inject TEAMMATE_TOOLS into real_turn whitelist + add TaskUpdate/TaskClaim
53545e78 fix(teammate): inject LTR registries into Teammate QueryEngine
c8c87139 feat(teammate): default-grant permission for Teammate working dirs + diag log
4be2a19b feat(teammate): wire LlmGateway into Teammate idle loop via launcher trait
ac36f5bc feat(teammate): wire LLM engine into TeammateWorkerCtx + idle loop
9f82369b docs(spec): teammate LLM turn design — close LTR P2 gap
```

## 工作模式提醒

- 用户对话语言：中文
- 工作分支：`ltr-mvp`（不是 main）
- 严禁瞎抽象/过度设计，参照 cc-best 但适配桌面端差异
- 改动后必须 `cargo check --lib` + `cargo test --lib` 验证
- commit message 中文，按 `feat(scope): ...` / `fix(scope): ...` 风格
- 前端测试需要重启 dev server（`pnpm tauri:dev`，run_in_background）
- 严禁在没拿到用户确认前自作主张大改

## 风险提醒

1. **`InboxRegistry::register` 是否能注册 Lead** —— 之前只给 Teammate 用过，看看签名是否假设了 `agent_id` 必须是 teammate kind。如果有断言要去掉。
2. **`AgentInbox::try_recv` 是否存在** —— inbox.rs:74-119 只看到 `send` / `recv`（async），需要加 `try_recv` 同步版（`tx.try_recv()`）或者 `drain_pending`。
3. **chat_turn_driver 是否已有 `inbox_registry` 字段** —— 看 line 320-405 的 RuntimeChatTurnDriver 结构，如果没有需要先 wire（chat.rs:2290 那块有 `with_inbox_registry` 已存在）。
4. **Lead inbox 与 task-notification queue 的关系** —— 两个都是"turn 开始前注入 user message"的机制，确认它们能并存不冲突（task-notification 是子 agent 完成时发的，peer-message 是 SendMessage 发的）。
