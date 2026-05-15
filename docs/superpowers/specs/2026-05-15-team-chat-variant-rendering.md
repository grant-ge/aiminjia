# Team Chat 抽屉协议消息渲染分流

**日期**：2026-05-15
**作者**：claude (post-mortem of 95eec8dc session)
**目标读者**：前端专家
**状态**：待审核

## 0. 场景与术语（不熟悉 team 功能请先读这里）

**AIjia** 是 Tauri 2.x 桌面 AI 工作台。LLM 可以通过 `TeamCreate` 工具把当前会话标记为"多智能体团队模式"，然后用 `Agent` 工具派几个 Teammate（子 agent）并行干活；干完用 `TeamDelete` 解散。整个过程对用户可见的入口是聊天界面右侧的"**团队过程**"抽屉——展示 team 内 Lead 与 Teammate 之间的消息往来。

```
┌──────────────────────────────┬────────────────────────────┐
│                              │  团队过程         × 关闭   │
│                              │  ─────────────────────── │
│   主聊天列                   │  debate-team ● 进行中     │
│   (Lead 跟用户对话)          │  [pro-debater] [con-...]  │
│                              │                            │
│                              │  ─── 5/15 23:01 ───       │
│                              │  con-debater → team-lead  │
│                              │  ┌─ 立论正文 ─┐           │
│                              │  └────────────┘           │
│                              │  ←── 团队过程的时间线 ──→ │
│                              │                            │
└──────────────────────────────┴────────────────────────────┘
```

**关键术语**：

| 术语 | 含义 |
|---|---|
| Lead | team 内的主 agent，对应 `team-lead`，跟用户对话的那个 |
| Teammate | Lead 派生的子 agent，比如 `pro-debater` / `con-debater` |
| `team-chat.jsonl` | 落在 `<conv>/teams/{team}/team-chat.jsonl`，team 内所有 SendMessage 的事件日志，**抽屉时间线的数据来源** |
| StructuredMessage | SendMessage 工具的 message 入参类型，5 个 variant：`text` + 4 个协议握手类型 |
| variant | StructuredMessage 的 type 字段（snake_case），决定一条消息是"对话"还是"协议信号" |

## TL;DR

抽屉右侧时间线里出现"（空消息）"灰色气泡——根因不是消息真的空，而是**前端把所有 SendMessage 都当对话气泡渲染**。实际上 `team-chat.jsonl` 里混了两类消息：

1. **对话消息**（`variant=text`）：有正文，应渲染为气泡
2. **协议握手消息**（`variant=shutdown_request/response`、`plan_approval_request/response`）：按设计**无正文**，语义全在 `variant` + `approve` 等结构化字段里，应该渲染为 system divider（跟 `team_create` / `agent_spawn` 那种灰色横条对称）

本方案不改后端协议字段，但需要**两边都动**，因为当前数据链路有**两个缺口**：

| 缺口 | 后果 | 修复 |
|---|---|---|
| 缺口 1：variant 字段被后端吃掉 | 前端拿不到 variant 信息 | `team_view.rs` 透传 variant 字段（~5 行）|
| 缺口 2：协议结构化字段未落盘 | 前端拿不到 `approve` / `reason` / `feedback`，无法区分"同意退出"vs"拒绝退出" | scope 决策点 X/Y/Z，详见 §5 |

前端分流改动主要在 `TeamChatEvents.tsx`（~30-50 行 + 5 个 vitest）。详见 §6。

## 1. 复现

### Plan A（推荐，30 秒确定性复现，**不依赖 LLM**）

直接 mock 一段 `team-chat.jsonl` 数据，渲染 `TeamChatEvents` 组件。下面是从真实会话 95eec8dc 抓的 6 行（已脱敏化简）：

```jsonl
{"from":"con-debater","text":"AI 不应该取代初级程序员……（立论正文 1500 字）","to":"team-lead","ts":"2026-05-15T15:01:53Z","variant":"text"}
{"from":"pro-debater","text":"AI 应该取代初级程序员……（立论正文 1500 字）","to":"team-lead","ts":"2026-05-15T15:02:15Z","variant":"text"}
{"from":"team-lead","text":"","to":"pro-debater","ts":"2026-05-15T15:02:23Z","variant":"shutdown_request"}
{"from":"team-lead","text":"","to":"con-debater","ts":"2026-05-15T15:02:23Z","variant":"shutdown_request"}
{"from":"con-debater","text":"","to":"team-lead","ts":"2026-05-15T15:02:28Z","variant":"shutdown_response"}
{"from":"pro-debater","text":"","to":"team-lead","ts":"2026-05-15T15:02:29Z","variant":"shutdown_response"}
```

用这段数据 mock `TeamSession.events`，把 `TeamChatEvents` 渲染出来——你立刻看到 2 条立论气泡 + **4 个"（空消息）"气泡**。后 4 个就是 bug 现场。

### Plan B（真实 LLM 派活，可选验证）

如果你想看完整链路：

1. `pnpm tauri:dev`
2. 在聊天里说："组个辩论团队，正反方各派一个，正方主张 AI 应该取代初级程序员，反方主张 AI 不应该取代，每方写一段 200 字立论，写完结束团队"
3. 等 LLM 派活完成（看主聊天里 `TeamCreate` → `Agent×2` → `TaskOutput×2` → `SendMessage` 工具调用陆续出现）
4. 点开右侧"团队过程"抽屉
5. **注意**：步骤 3 的"LLM 主动结束团队"行为**不是 100% 触发**——换模型 / 改 prompt / 高温度都可能让 LLM 不调 `shutdown_request`。**建议用 Plan A 确认 bug 存在，Plan B 仅作端到端冒烟**

## 2. 改造结果（before / after）

### Before（当前）

```
─── 5/15 23:01 ──────────────────────

🟢 con-debater → team-lead   23:01:53
   ┌─────────────────────────────┐
   │ AI 不应该取代初级程序员…    │   ← 立论正文，正常
   │ （1500 字）                  │
   └─────────────────────────────┘

🟢 pro-debater → team-lead   23:02:15
   ┌─────────────────────────────┐
   │ AI 应该取代初级程序员…       │   ← 立论正文，正常
   │ （1500 字）                  │
   └─────────────────────────────┘

🟡 team-lead → pro-debater   23:02:23
   ┌─────────────────────────────┐
   │ （空消息）                    │   ← ❌ Bug 现场
   └─────────────────────────────┘

🟡 team-lead → con-debater   23:02:23
   ┌─────────────────────────────┐
   │ （空消息）                    │   ← ❌
   └─────────────────────────────┘

🟢 con-debater → team-lead   23:02:28
   ┌─────────────────────────────┐
   │ （空消息）                    │   ← ❌
   └─────────────────────────────┘

🟢 pro-debater → team-lead   23:02:29
   ┌─────────────────────────────┐
   │ （空消息）                    │   ← ❌
   └─────────────────────────────┘
```

### After（期望）

```
─── 5/15 23:01 ──────────────────────

🟢 con-debater → team-lead   23:01:53
   ┌─────────────────────────────┐
   │ AI 不应该取代初级程序员…    │
   │ （1500 字）                  │
   └─────────────────────────────┘

🟢 pro-debater → team-lead   23:02:15
   ┌─────────────────────────────┐
   │ AI 应该取代初级程序员…       │
   │ （1500 字）                  │
   └─────────────────────────────┘

──── ⊙ team-lead 请求 pro-debater 退出团队 · 23:02:23 ────
──── ⊙ team-lead 请求 con-debater 退出团队 · 23:02:23 ────
──── ✓ con-debater 同意退出                · 23:02:28 ────
──── ✓ pro-debater 同意退出                · 23:02:29 ────
```

跟现有 `team_create` / `agent_spawn` / `agent_stop` 的 system divider 视觉对齐：

```
──── ● 团队已创建 · debate-team · 23:00:42 ────
──── ＋ pro-debater 加入团队 · 23:01:05 ────
──── ⊙ team-lead 请求 pro-debater 退出团队 · 23:02:23 ────  ← 新增的协议消息
──── ＋ pro-debater 同意退出 · 23:02:29 ────  ← 新增
──── ○ 团队已解散 · 23:02:30 ────
```

## 3. 根因（已验证）

### 3.1 落盘是对的

`<conv>/teams/{team}/team-chat.jsonl` 每行格式：

```json
{"from":"con-debater","text":"...","to":"team-lead","ts":"...","variant":"text"}
{"from":"team-lead","text":"","to":"pro-debater","ts":"...","variant":"shutdown_request"}
{"from":"con-debater","text":"","to":"team-lead","ts":"...","variant":"shutdown_response"}
```

> ⚠️ **重要警告**：上面 jsonl 只有 5 个字段（from/text/to/ts/variant）。`StructuredMessage` 的协议字段（`approve` / `request_id` / `reason` / `feedback`）**当前完全不在落盘里**——这是数据缺口 2，详见 §3.4 + §5。

- text 行有正文是对的
- 协议行 `text=""` **是对的**，因为协议语义全在 `variant` 字段里（详见下文 StructuredMessage 定义）
- 后端 `append_team_chat_entry` 把所有 SendMessage 都记进去也是对的（审计日志一致性，参考 cc-best）

### 3.2 `StructuredMessage` 5 个 variant

后端定义在 `src-tauri/src/runtime/messaging/structured.rs`：

```rust
pub enum StructuredMessage {
    Text { content: String },
    ShutdownRequest { reason: Option<String> },
    ShutdownResponse { request_id: String, approve: bool, reason: Option<String> },
    PlanApprovalRequest { request_id: String, plan: String },
    PlanApprovalResponse { request_id: String, approve: bool, feedback: Option<String> },
}
```

只有 `Text` 有用户可见正文，其他 4 个是**协议握手**：

- `shutdown_request`：Lead 通知 Teammate "请退出"。可选 `reason`
- `shutdown_response`：Teammate 应答 "approve=true 同意退出 / false 拒绝退出"
- `plan_approval_request`：发起方请求接收方审批 plan
- `plan_approval_response`：接收方应答审批结果

这 4 个 variant 的语义全在结构化字段里，**`text` 字段在 `team-chat.jsonl` 里恒为空** —— `append_team_chat_entry` 只 dump `message.as_text()`，非 Text variant 直接 None → 落盘 `""`。

### 3.3 前端渲染缺陷

`src/components/team/TeamChatEvents.tsx::MessageBubble`（L184-185）：

```tsx
{text ? (
  <AssistantMarkdown text={text} />
) : (
  <span className="italic text-muted-foreground">（空消息）</span>
)}
```

不看 variant，只看 text 空不空 → 协议消息全部撞这个分支。

### 3.4 数据缺口 1：variant 在后端被吃掉

`src/types/team.ts` 的 `TeamEvent.send_message` **没有 variant 字段**：

```typescript
{ kind: 'send_message'; ts: string; from: string; to: string; text: string; isError: boolean; toolCallId: string }
```

不是 TS 类型漏写，是**后端没传**。`src-tauri/src/runtime/team_view.rs::append_events_from_team_chat_jsonl` (L487-494)：

```rust
out.push(TeamEvent::SendMessage {
    ts,
    from,
    to,
    text,
    is_error: false,
    tool_call_id: String::new(),
});
// ↑ 完全没读 jsonl 行里的 "variant" 字段
```

所以前端目前**拿不到 variant**。修复分流前必须先让后端透传。

### 3.5 数据缺口 2：协议字段从未落盘

`src-tauri/src/runtime/tools/builtin/send_message.rs::append_team_chat_entry` 落盘的 entry 只有 5 个字段：

```rust
let entry = serde_json::json!({
    "ts": ...,
    "from": from,
    "to": to,
    "text": body,             // ← message.as_text().unwrap_or("") 永远是空
    "variant": message.variant_name(),
});
```

`StructuredMessage::ShutdownResponse { request_id, approve, reason }` 里的 `approve` / `request_id` / `reason` 等字段**根本没写进 jsonl**。

后果：即使修了缺口 1，前端拿到的也只是 `variant: "shutdown_response"`，**无法区分 approve=true vs false**——也就无法在 UI 区分"同意退出"vs"拒绝退出"。

是否补这个缺口是 §5 的 scope 决策点。

## 4. 后端为什么不改 jsonl 格式（防御性说明）

你可能会问"那让后端只在 text variant 时才写 jsonl 不就行了？"答：不行。

1. **审计一致性**：`team-chat.jsonl` 是 team 内所有 SendMessage 路由的完整事件流，用于排查"为什么 team 解散了 / 为什么 Teammate 退出了"这类问题。协议消息恰恰是这种问题的关键证据
2. **单一管道**：cc-best 设计 SendMessage 是 agent 之间唯一通信管道，不为协议握手单独开通道；落盘策略跟着对齐
3. **跟其他 lifecycle 事件对称**：`team_create` / `agent_spawn` / `agent_stop` 这些已经渲染为 SystemDivider 而非气泡。协议消息走同样模式即可

## 5. variant 清单 + 期望 UI

| variant | 含义 | 当前显示 | 期望显示 |
|---|---|---|---|
| `text` | 自由文本对话 | 对话气泡（保留） | 对话气泡（不动）|
| `shutdown_request` | 发起方请求接收方退出 | "（空消息）"气泡 ❌ | 系统标签：`⊙ {from} 请求 {to} 退出团队`（有 reason 时追加 `· {reason}`）|
| `shutdown_response` (approve=true) | 接收方同意退出 | "（空消息）"气泡 ❌ | 系统标签：`✓ {from} 同意退出` |
| `shutdown_response` (approve=false) | 接收方拒绝退出 | "（空消息）"气泡 ❌ | 系统标签：`✗ {from} 拒绝退出`（有 reason 时追加 `· {reason}`）|
| `plan_approval_request` | 发起方请求审批 plan | "（空消息）"气泡 ❌ | 系统标签：`≪ {from} 请 {to} 审批方案` |
| `plan_approval_response` (approve=true) | 同意 plan | "（空消息）"气泡 ❌ | 系统标签：`✓ {from} 同意方案` |
| `plan_approval_response` (approve=false) | 拒绝 plan | "（空消息）"气泡 ❌ | 系统标签：`✗ {from} 修改建议：{feedback}` |

## 6. 三个待你拍板的设计选择

### A. 协议消息位置策略

| 选项 | 描述 | 我的倾向 |
|---|---|---|
| A1 平铺时间线 | 跟 team_create / agent_spawn 同样的 SystemDivider 风格，按 ts 顺序插在对话气泡之间 | ✅ 推荐 |
| A2 折叠成 "n 条协议事件" | 默认折叠，点击展开。气泡区域更清爽但隐藏了上下文 | |
| A3 完全隐藏 | 极简，但握手出问题时排查困难 | ❌ 不推荐 |

cc-best 走 A1。理由：协议消息密度低（一次 team 生命周期通常 ≤ 4 条），平铺不会污染时间线，反而帮助理解"为什么这时候 Teammate 退出了"。

### B. SystemDivider 是否够用，还是需要新组件

`TeamChatEvents.tsx::SystemDivider` 当前长这样：

```tsx
<div className="flex items-center justify-center gap-2 text-[11px] text-muted-foreground">
  <span className="h-px flex-1 bg-border" />
  <span className="inline-flex items-center gap-1.5">
    <span aria-hidden>{icon}</span>
    <span>{label}</span>
    <span className="opacity-60">{formatClock(ts)}</span>
  </span>
  <span className="h-px flex-1 bg-border" />
</div>
```

我的建议：**直接复用** SystemDivider，icon 用差异化字符区分类别：

- `⊙` for shutdown_request（请求）
- `✓` for shutdown_response approve=true（同意）/ plan_approval_response approve=true
- `✗` for shutdown_response approve=false（拒绝）/ plan_approval_response approve=false
- `≪` for plan_approval_request

请你判断：是否需要新增 `ProtocolEventRow.tsx`？比如想给协议消息加箭头方向（请求 vs 应答）、加 from→to 双 avatar、加 hover 显示 reason/feedback 详情？

### C. i18n / 文案

当前 `TeamChatEvents.tsx` 文案全是中文硬编码（`团队已创建` / `团队已解散` / `加入团队` / `已退出`），没接 react-i18next。

请你确认：

1. 这次分流的协议标签**沿用中文硬编码**？还是顺手把整个 TeamChatEvents 改造接 i18next？
2. 如果接 i18next，命名 key 建议：`teamChat.protocol.shutdownRequest` / `.shutdownResponse.approve` / `.shutdownResponse.reject` / `.planApprovalRequest` / ...

### D. scope 决策：协议附加字段透传策略（与缺口 2 关联）

如 §3.5 所述，jsonl 当前不含 `approve` / `reason` / `feedback`。本期是否补落盘？

| 方案 | 描述 | 影响 |
|---|---|---|
| X | jsonl 加 approve / reason / feedback 三个字段（最小代价）| approve=true/false 可区分；reason/feedback 可显示；不能渲染 plan 正文 |
| Y | jsonl 把整个 StructuredMessage 序列化进去 | 前端拿完整 payload，未来加 variant 不动后端 |
| Z | 本期只按 variant 名分流，不区分 approve；以后真有人提需求再补字段 | 改动最小；但 UI 上 "同意退出" / "拒绝退出" 显示不出来——只能显示 "shutdown 应答" 这种模糊文案 |

我的倾向：**X**。Y 改动面太大、序列化耦合后端类型；Z 用户体验不够（不知道 teammate 是 ack 还是拒绝）。

## 7. 实现指引

### 后端改动 1：透传 variant（必须先做）

**文件**：`src-tauri/src/runtime/team_view.rs`

1. `TeamEvent::SendMessage` 加 `variant: String` 字段
2. `append_events_from_team_chat_jsonl` 读 `v.get("variant").and_then(|x| x.as_str()).unwrap_or("text").to_string()` 透传

**注意**：`TeamEvent` 是 `#[serde(rename_all = "camelCase")]`，TS 端字段名是 `variant`（保持不变）。`PeerMessage` 已经有 variant 字段，可以参考它。

后端单测要补一条：jsonl 含 `variant=shutdown_request` 的行能映射到 `TeamEvent::SendMessage { variant: "shutdown_request" .. }`。

### 后端改动 2：补落盘协议字段（如果选 X 或 Y）

**文件**：`src-tauri/src/runtime/tools/builtin/send_message.rs::append_team_chat_entry`

如果选 X，jsonl entry 加 3 个可选字段：

```rust
let entry = serde_json::json!({
    "ts": ..., "from": ..., "to": ..., "text": ...,
    "variant": message.variant_name(),
    // 新增（按 variant 条件填充，None 时省略）：
    "approve": match message { Self::ShutdownResponse{approve,..} | Self::PlanApprovalResponse{approve,..} => Some(*approve), _ => None },
    "reason": match message { Self::ShutdownRequest{reason} | Self::ShutdownResponse{reason,..} => reason.clone(), _ => None },
    "feedback": match message { Self::PlanApprovalResponse{feedback,..} => feedback.clone(), _ => None },
});
```

如果选 Y，直接 `serde_json::to_value(message)?` 把整个 StructuredMessage 序列化进去；前端拿到 `payload: StructuredMessage` discriminated union 直接 switch。

如果选 Z，跳过本步。

### TS 类型补字段

**文件**：`src/types/team.ts` L21-29

最小版（选 Z）：

```typescript
| {
    kind: 'send_message'
    ts: string
    from: string
    to: string
    text: string
    isError: boolean
    toolCallId: string
    variant: string  // ← 新增
  }
```

带协议字段版（选 X）：

```typescript
| {
    kind: 'send_message'
    ts: string
    from: string
    to: string
    text: string
    isError: boolean
    toolCallId: string
    variant: 'text' | 'shutdown_request' | 'shutdown_response' | 'plan_approval_request' | 'plan_approval_response'
    approve?: boolean
    reason?: string
    feedback?: string
  }
```

union literal vs string 由你选——前者更安全但要求后端任何 variant 新增都同步前端；后者宽松但失类型守卫。

### 前端分流核心改动

**文件**：`src/components/team/TeamChatEvents.tsx`

- `TeamEventRow` switch 内 `'send_message'` 分支按 `event.variant` 二次分流：
  - `text` → 走 `MessageBubble`（保留）
  - 其他 → 走新增的协议系统标签渲染（复用 SystemDivider 或新组件）
- `peer_message` 同理（teammate→teammate 也可能传协议消息，逻辑对称）
- L184 那段"（空消息）"占位**保留**，作为意外情况兜底（譬如 text variant 但 content 真的是空）

需要补 vitest：
1. variant=text + text 非空 → 渲染 MessageBubble
2. variant=shutdown_request → 渲染系统标签，内容包含 "请求" + from + to
3. variant=shutdown_response, approve=true → 渲染 "同意退出"
4. variant=shutdown_response, approve=false → 渲染 "拒绝退出"
5. variant=text + text="" → 兜底"（空消息）"，保留现有行为

## 8. 不在本方案范围内

- 不改 `StructuredMessage` 5 个 variant 的字段
- 不改 `append_team_chat_entry` 的触发时机（每次 SendMessage 都落盘）
- 不调整 `TeamChatDrawer` 整体布局（标题、成员条、宽度、抽屉触发）
- 不改 `TeammateDetailPanel`（drill-down 详情面板独立模块）
- 不动 `useTeamStore` 的 store 结构

## 9. 验收标准

1. 复现 §1 Plan A 的 6 行 jsonl 渲染后：2 行对话气泡 + 4 行系统标签（区分 request / response），符合 §2 After 示意图
2. 关闭团队后回看历史，握手过程一目了然（不再有"空消息"）
3. variant=text 的对话气泡渲染零回归（截图对比）
4. 暗色主题下系统标签可读（`text-muted-foreground` 已是主题变量，本身没问题，但请确认 icon 字符的可读性）
5. 后端单测：jsonl 行 variant 透传到 TeamEvent
6. 前端单测：5 条 variant 分流路径覆盖
7. `pnpm lint` + `pnpm exec tsc --noEmit` 通过
8. `cargo test --lib runtime::team_view::` 通过

## 10. 工作量估算

| 改动 | 文件 | 代码量 | 测试 |
|---|---|---|---|
| 后端 variant 透传 | team_view.rs | ~5 行 | +1 单测 |
| 后端协议字段落盘（X 方案）| send_message.rs | ~15 行 | +1 单测 |
| TS 类型加字段 | types/team.ts | ~5 行 | - |
| 前端分流 | TeamChatEvents.tsx | ~30-50 行（取决于是否复用 SystemDivider）| +5 vitest |
| 文案 / i18n | TeamChatEvents.tsx | 取决于决策 C | - |

总计：半天到一天可完成（选 X / 复用 SystemDivider / 沿用硬编码文案的话）。

## 11. 决策 checklist（请前端专家逐项回复）

1. [ ] 协议消息渲染策略 A1 / A2 / A3？
2. [ ] 复用 SystemDivider 还是新增 ProtocolEventRow？
3. [ ] 文案中文硬编码 还是 接 i18next？接的话 key 命名是否同意 `teamChat.protocol.*`？
4. [ ] variant 字段在 TS 端用 string 还是 union literal？
5. [ ] approve / reason / feedback 字段透传策略 X / Y / Z？
6. [ ] 还有其他没考虑到的点？

