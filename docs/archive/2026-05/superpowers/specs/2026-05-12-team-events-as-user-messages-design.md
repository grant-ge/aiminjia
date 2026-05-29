# Team 协作事件以"特殊 user message"形式渲染

**Date**: 2026-05-12
**Status**: Draft (pending review)
**Branch**: `ltr-mvp`
**Related**:
- `docs/superpowers/handoffs/2026-05-12-team-mode-end-to-end-debug-handoff.md`
- `docs/superpowers/handoffs/2026-05-12-lead-inbox-drain-handoff.md`
- 参照实现:`~/github/claude-code-best/src/components/messages/UserTeammateMessage.tsx` + `src/utils/teammateMailbox.ts`

---

## 1. Problem

当前 Team 模式下,以下两类系统事件**用户在前端完全看不见**,体验突兀:

1. **Sub-agent 完成通知**(`<task-notification>`):后台 sub-agent 完成时,前端通过 sentinel `__resume_from_task_notification__` 调 `sendMessage` 让 Lead 续 turn drain notification。这个 sentinel 因故意不持久化,用户看到的现象是"Lead 突然又开始说话",不知道为什么。
2. **Peer SendMessage**(team-lead 与 teammate 间的消息):`AgentInbox` 是内存 channel,drain 完即弃。用户看不到"小研给 Lead 说了什么"、"Lead 给小算下了什么指令"——只能看到 Lead 的最终回复。

用户原话:"客户也觉得很奇怪"。

## 2. Goals

- 用户在聊天界面**直观看到**团队成员之间的协作消息流(谁→谁,内容是什么)
- 用户**直观看到** sub-agent 完成事件(哪个 agent 完成了什么)
- Lead 跨 turn / 跨重启 resume 后,LLM history 中**自动保留**这些事件(无需独立 cursor/状态机)
- 实现工作量可控,不引入新文件 / 新 schema 字段

## 3. Non-Goals

- 不做 teammate 之间通信的展示(teammate-to-teammate);本期只覆盖 Lead 视角(用户看到的)
- 不做 shutdown_request / TaskClaim / TeammateStop 的 banner;后期加 XML tag + banner 组件即可
- 不做独立的 `team_events.jsonl` 共享流(brainstorm 时讨论过,被否决——见 §10 Design Notes)
- 不做 cc-best 的 `inboxes/{name}.json` 磁盘文件(brainstorm 时讨论过,被否决——单进程不需要,见 §10)

## 4. Approach (照搬 cc-best 风格)

**核心思想**:peer 消息和 task notification **持久化为带特殊 XML 包裹的 user message**,直接写入现有 `messages.jsonl`。LLM 看到的、UI 看到的、磁盘存的是**同一份数据**。

数据流:

```
后端 drain peer/task event
  → inject 成 user message 给 LLM (现状,已有)
  → ★ NEW: 同时持久化为 user message 写入 messages.jsonl
            content = "<peer-messages>...</peer-messages>"
            或       "<task-notification>...</task-notification>"

前端 UserMessageBubble 渲染前
  → parse text:匹配 <peer-messages> → 走 PeerMessageBanner
  → parse text:匹配 <task-notification> → 走 TaskNotificationBanner
  → 都不匹配 → 普通用户气泡
```

参照 cc-best 的 `UserTeammateMessage.tsx`:**渲染层用正则识别 user message 文本里的特殊 XML tag,命中则走专门的 banner 组件**。这是 cc-best 经过验证的设计——单一数据通道、自然 resume、零 schema 变更。

## 5. Detailed Design

### 5.1 XML 格式(后端,延续现有约定)

后端目前 `chat_turn_driver::render_peer_messages_xml`(已实现)和 `task_notification` queue 已经在用这两种格式包裹注入的 user content。本期**不改格式**,只改"是否持久化"。

**Peer messages**(单次 drain 可能多条,统一包在一个 `<peer-messages>` 里):

```xml
<peer-messages>
  <peer-message from="小研" variant="text">
    调研完成第一部分
  </peer-message>
  <peer-message from="小算" variant="text">
    数据准备好了
  </peer-message>
</peer-messages>
```

**Task notification**(每次 sub-agent 完成一条):

```xml
<task-notification agent="小研" status="completed">
  ... 完成摘要 ...
</task-notification>
```

每个 `<task-notification>` 是独立一条 user message(目前的 inject 逻辑就是逐条 push),不合并。

XML 由后端拼好,前端**只读**不构造。

### 5.2 后端改动

**文件**:`src-tauri/src/runtime/chat/chat_turn_driver.rs`

#### 5.2.1 `drain_and_inject_lead_inbox_messages` (~line 431)

当前逻辑(简化):

```rust
let drained = lead_inbox.drain_pending().await;
if drained.is_empty() { return 0; }
let xml = render_peer_messages_xml(&drained);
messages.push(serde_json::json!({ "role": "user", "content": xml }));
```

**新增**:在 `messages.push` 之后,把 xml 持久化为 user message:

```rust
// best-effort persist; failure must NOT abort the turn
if let Err(e) = executor.persist_user_message(
    request.conversation_id.as_str(),
    &xml,
    &[],          // no attachments
    None,         // no client_message_id
).await {
    log::warn!("[chat_turn_driver] persist peer messages failed: {e}");
}
```

注意:当前 `drain_and_inject_lead_inbox_messages` 是 free function,没拿到 `executor`。需要把 executor 引用传进去(或把这一段挪到调用处之后做)。**实施时**优先选"挪到调用处"——避免函数签名扩散。

#### 5.2.2 `drain_and_inject_task_notifications` (~line 369)

类似处理。当前逻辑逐条 push 到 messages,新增逐条 persist:

```rust
for notification in &notifications {
    messages.push(serde_json::json!({
        "role": "user",
        "content": notification.xml.clone(),
    }));
    // ★ NEW
    if let Err(e) = executor.persist_user_message(
        request.conversation_id.as_str(),
        &notification.xml,
        &[],
        None,
    ).await {
        log::warn!("[chat_turn_driver] persist task notification failed: {e}");
    }
}
```

同样,实施时把这段持久化挪到调用处之后(`run_chat_turn_s4` 内拿到 executor 引用的位置),避免改 free function 签名。

#### 5.2.3 持久化时机与顺序

两次 drain 紧跟在 `initial_messages.extend(history)` 之后(`chat_turn_driver.rs:1394-1417`)。原顺序:

```
1. user_message 注入 LLM messages (in-memory)
2. drain_and_inject_task_notifications → push 到 LLM messages
3. drain_and_inject_lead_inbox_messages → push 到 LLM messages
```

**新顺序(本期)**:

```
1. user_message 注入 LLM messages (in-memory) + persist (现状)
2. drain_and_inject_task_notifications → push to LLM + ★ persist each xml
3. drain_and_inject_lead_inbox_messages → push to LLM + ★ persist xml
```

持久化顺序保证 messages.jsonl 里的时间序与 LLM 看到的顺序一致。

#### 5.2.4 Sentinel 行为不变

`__resume_from_task_notification__` 自身仍然**不持久化**(`is_resume_for_task_notification` 分支跳过 `persist_user_message`)。它只是触发 drain 的载体,真正持久化的是它**触发 drain 出来**的 peer/task event。

#### 5.2.5 失败语义

持久化是 best-effort:写盘失败只 log warn,**不重新入队、不阻断 turn**。理由:
- 内存里 LLM 已经收到这条 message,turn 能正常完成
- 持久化失败大概率是磁盘问题,反复重试只会放大故障
- 用户最坏的体验是"少看到一条 banner",不会数据丢失到 LLM 上下文

### 5.3 前端改动

#### 5.3.1 渲染识别

**文件**:`src/components/chat-scene/UserMessageBubble.tsx`(主入口)

在普通气泡渲染前先 parse text:

```typescript
function classifyUserMessage(text: string): 
  | { kind: 'peer-messages'; items: PeerMessage[] }
  | { kind: 'task-notification'; agent: string; status: string; body: string }
  | { kind: 'plain'; text: string } {
  // peer-messages first (more common)
  const peerMatch = text.match(/^<peer-messages>([\s\S]*?)<\/peer-messages>$/);
  if (peerMatch) {
    return { kind: 'peer-messages', items: parsePeerMessages(peerMatch[1]) };
  }
  const taskMatch = text.match(/^<task-notification\s+agent="([^"]*)"\s+status="([^"]*)">([\s\S]*?)<\/task-notification>$/);
  if (taskMatch) {
    return { 
      kind: 'task-notification', 
      agent: taskMatch[1], 
      status: taskMatch[2], 
      body: taskMatch[3].trim() 
    };
  }
  return { kind: 'plain', text };
}
```

正则要求**整段 trim 后完全匹配**(`^...$`),避免误判用户在普通���息里粘贴了 XML 片段。后端持久化的就是单 XML 块,不会混杂别的内容。

#### 5.3.2 新组件:`PeerMessageBanner`

**文件**:`src/components/chat-scene/PeerMessageBanner.tsx`(新建)

形态:左对齐(跟用户气泡的右对齐区分),浅色卡片背景,列出每条 peer-message 的 `from` + `body`。视觉上明显**不是用户输入**,也明显不是 AI 回复。

**严格遵守 CLAUDE.md UI 规约**:
- 颜色用语义变量:`bg-muted` / `border-border` / `text-muted-foreground` / `text-foreground`
- ❌ 禁止 `bg-white` / `bg-gray-100` / `text-[#xxx]` 等硬编码
- 多条 peer-message 渲染成 list,每条头部展示 `from` 名字(可加 emoji 🔔 或 figures.bullet 等价物)

线框:

```
┌──────────────────────────────────────────────┐
│ 🔔 团队消息                                   │
│ ────────────────────────                     │
│ 小研 → Lead                                   │
│   调研完成第一部分                             │
│                                              │
│ 小算 → Lead                                   │
│   数据准备好了                                 │
└──────────────────────────────────────────────┘
```

#### 5.3.3 新组件:`TaskNotificationBanner`

**文件**:`src/components/chat-scene/TaskNotificationBanner.tsx`(新建)

形态同上,左对齐浅色卡片,展示 agent 名字 + status + body 摘要。

```
┌──────────────────────────────────────────────┐
│ ✅ 子任务完成                                  │
│ Agent: 小研                                   │
│ ────────────────────────                     │
│   ... 完成摘要 ...                             │
└──────────────────────────────────────────────┘
```

#### 5.3.4 出站消息(Lead → teammate)的展示

入站消息(teammate → Lead)走 §5.3.1-5.3.3 的"user message + XML"路径。**出站消息**(Lead 调 `SendMessage(to=...)`)在数据层的现状:

- Lead 的 assistant message 的 `tool_calls` 字段里有完整记录:`{ name: "SendMessage", arguments: { to, message, summary } }`
- `SendMessage` 工具执行时,直接把消息推进对方 AgentInbox(内存),**不会**在 Lead 自己的 messages.jsonl 里再写一条 user/assistant message

**前端处理(后端 0 改动)**:`AiBubble` 渲染 assistant message 时,检查 `tool_calls`,如果有 `name === "SendMessage"` 的项,从 arguments 拎 `to` / `message` / `summary`,渲染出站 banner。

UI 上出站与入站**共用同一个 `PeerMessageBanner` 组件**,只是数据来源不同:

```
🔔 团队消息
Lead → 小研          ← 出站,数据来自 assistant.tool_calls[SendMessage]
  开始数据分析

小研 → Lead          ← 入站,数据来自 user message <peer-messages>
  调研完成第一部分
```

**实施细节**:
- `PeerMessageBanner` 接受统一 props:`{ from: string; to: string; body: string; summary?: string }`
- 入站:从 XML parse 出 from(`peer-message from=...`),to 默认 `team-lead`(因为是写到 lead inbox)
- 出站:从 tool_calls 的 `arguments.to` 拎 to,from 固定 `team-lead`
- `AiBubble` 渲染时:文字内容 + tool_calls 列表;遇到 SendMessage tool_call **不渲染普通"调用了工具"的折叠卡片**,改渲染 PeerMessageBanner;其他 tool_call 走原有渲染

**不在范围**:teammate 之间的 SendMessage(小研 → 小算)的展示,本期不做(见 §9 Out of Scope)。

#### 5.3.5 实时性

不需要新 Tauri event。当前 `message:updated` event 已经覆盖"messages.jsonl 新增了一行"的场景,前端会自然刷新。

历史消息加载也走现有 `load_history` / `list_messages`,无需改动。

### 5.4 兼容性

- **StoredMessage schema 不动**(`schemaVersion: 2` 保持,content 仍是 text,只是文本里有 XML)
- 老会话 load 时,正则匹配不到 = 普通用户气泡渲染,**零回归**
- Lead resume 老会话时,LLM 会从 history 里读到这些 `<peer-messages>` user message——它本来就懂这个格式(后端 drain inject 的 prompt 已经训了 Lead 读 `<peer-messages>` XML)

## 6. Testing

### 6.1 后端

**单测**(在 `chat_turn_driver` 既有测试模块内):

- `peer_messages_persisted_after_drain`:mock executor 验证 `persist_user_message` 被调用,content 是预期的 `<peer-messages>` XML
- `task_notification_persisted_per_item`:多条 notification 时 persist 调用次数 = notification 数
- `persist_failure_does_not_abort_turn`:mock executor 的 persist 返回 Err,验证 turn 仍正常完成,只 log warn
- `sentinel_itself_not_persisted`:`__resume_from_task_notification__` 分支不调用 persist(已有逻辑,加断言巩固)

**集成测试**(`src-tauri/tests/team_tools_test.rs` 或新建):

- 跑一次 Lead+teammate 的 mini turn:teammate SendMessage → Lead drain → 验证 messages.jsonl 多了一行 content=<peer-messages>...

### 6.2 前端

**vitest**(`src/components/chat-scene/__tests__/PeerMessageBanner.test.tsx` 等):

- `classifyUserMessage` 单测:peer-messages / task-notification / plain text 三种 case + edge case(嵌套 XML / 不完整 tag / trim)
- `PeerMessageBanner` snapshot:多条 peer-message 渲染列表
- `UserMessageBubble` 集成:user message text = `<peer-messages>...` 时不渲染普通气泡,渲染 banner

### 6.3 端到端验证

延续上一发 handoff 的流程:`pnpm tauri:dev` → 创建 Team → 派 teammate → teammate SendMessage → 看前端是否出现 banner。

## 7. Implementation Steps

1. **后端**:把两次 drain 的 persist 逻辑挪到 `run_chat_turn_s4` 内的调用点之后(避免改 free function 签名),加 best-effort log warn
2. **后端**:补 4 个单测覆盖持久化路径
3. **前端**:新增 `classifyUserMessage` 工具函数 + 单测
4. **前端**:新增 `PeerMessageBanner` 组件(入站 + 出站共用)+ `TaskNotificationBanner` 组件
5. **前端**:`UserMessageBubble` 在渲染前调 `classifyUserMessage`,命中走对应 banner
6. **前端**:`AiBubble` 渲染 assistant tool_calls 时,识别 `SendMessage` tool_call,改渲染 `PeerMessageBanner`(出站)而非默认"调用工具"折叠卡片
7. **前端**:补 banner 组件单测 + UserMessageBubble / AiBubble 集成测试
8. **前端 vitest**:补"出站 SendMessage tool_call 识别"测试(`AiBubble` 收到含 SendMessage 的 tool_calls 时渲染 PeerMessageBanner,其他 tool_calls 正常)
9. **手测**:`pnpm tauri:dev` 端到端跑 Team 模式,确认 banner 显示正确(入站 + 出站)、历史消息 reload 后 banner 仍在

## 8. Risks / Open Questions

### Risk 1: messages.jsonl 里出现"看起来像 user 但不是用户输入"的消息

**影响**:任何依赖 messages.jsonl 假设"role=user → 用户真实输入"的下游代码会被误导。

**缓解**:
- 持久化的 content 一定是 XML 包裹(`<peer-messages>` / `<task-notification>`),下游可以通过正则区分
- 当前下游消费者主要是前端渲染(本期已处理)和 LLM history(本期就是要让 LLM 看到)
- 没有别的关键消费者依赖"role=user 必为用户输入"——已快速 grep 验证

### Risk 2: 前端正则 false positive

如果用户在普通消息里手动粘贴 `<peer-messages>...</peer-messages>` 字符串,会被错误识别成 banner。

**缓解**:正则要求**整段 trim 后完全匹配**(`^...$`),正常用户不会发出仅由 XML 构成的消息。极端 case 只是渲染异常,不影响数据。

### Open Q1: peer-message variant 字段渲染

后端 `<peer-message variant="text">` 的 variant 字段当前没用。如果后期支持 image / file 类型 peer 消息,前端 banner 要分类型渲染。本期只支持 text,variant 字段读取后忽略。

### Open Q2: 多条 peer-message 折叠

如果一次 drain 出 10+ 条 peer-message,banner 会很长。本期不做折叠(默认全展开)。后期可加"展开/收起",但目前 LLM 一次最多也就被注入几条,不是问题。

## 9. Out of Scope (本期不做,留给后续 spec)

- **Teammate 视角**:用户能看到小研↔小算的私聊。需要 teammate 维度的 UI 视图,不是现有"用户看 Lead"模型。后期补 spec。
- **shutdown_request / TaskClaim / TeammateStop banner**:加新 XML tag 即可,本期不做
- **历史归档 / 清理**:messages.jsonl 累积大量 banner 后是否需要单独归档,后期再看
- **跨重启 unread 标记**:cc-best 有 `read: true/false`,我们暂不做(每次 reload 全部展示,符合"历史对话"语义)

## 10. Design Notes(讨论过程留痕)

Brainstorm 阶段讨论过 3 个备选方案,最终选 C1:

| 方案 | 描述 | 否决理由 |
|---|---|---|
| **A** | 只做 task-notification banner,不做 peer messages | 范围太小,客户体验问题只解一半 |
| **B** | 新建 `team_events.jsonl` 共享事件流 + 双数据通道 | 实现复杂,前端要合并两个数据源排序;重启 resume 要单独处理 cursor;LLM history 注入路径要保留两套 |
| **C1** ✅ | 照搬 cc-best:event 持久化为带 XML 的 user message,单通道 | 单一真相源、自然 resume、零 schema 变更,前端只多一个识别函数 |

也讨论过 `inboxes/{name}.json`(cc-best 风格的磁盘 inbox):
- cc-best 用是因为 swarm 是**多进程**,需要文件 + lockfile 跨进程通信
- 我们是**单进程 Tauri 后端**,内存 channel(`AgentInbox`)就够用
- **不引入磁盘 inbox**;运输层保持现状,只改"渲染层 + 持久化"

## 11. References

- Handoff: `docs/superpowers/handoffs/2026-05-12-team-mode-end-to-end-debug-handoff.md`
- 后端 inbox/notification 现有代码:
  - `src-tauri/src/runtime/agent/inbox.rs`
  - `src-tauri/src/runtime/agent/task_notification.rs`
  - `src-tauri/src/runtime/chat/chat_turn_driver.rs:369-517`
- 前端现有 user 渲染:
  - `src/components/chat-scene/UserMessageBubble.tsx`
- cc-best 参照:
  - `~/github/claude-code-best/src/components/messages/UserTeammateMessage.tsx`
  - `~/github/claude-code-best/src/utils/teammateMailbox.ts`
  - `~/github/claude-code-best/src/constants/xml.ts:52` (`TEAMMATE_MESSAGE_TAG`)
