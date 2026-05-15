# Team Chat 抽屉协议消息渲染分流

**日期**：2026-05-15
**作者**：claude (post-mortem of 95eec8dc session)
**目标读者**：前端专家
**状态**：待审核

## TL;DR

"团队过程"抽屉 (`TeamChatDrawer`) 当前把 team-chat.jsonl 每一行都当对话气泡渲染。**协议握手类消息（shutdown_request / shutdown_response / plan_approval_*）按设计没有正文**，气泡里显示成"（空消息）"，用户困惑。

本方案不改后端协议字段（jsonl 落盘本身是对的），但需要**两边协作**：
1. **后端微改**：`runtime/team_view.rs::append_events_from_team_chat_jsonl` 把现在丢掉的 `variant` 字段透传到 `TeamEvent::SendMessage`
2. **前端分流**：`TeamChatEvents.tsx` 按 `variant` 把协议消息渲染成系统标签（类似 team_create / agent_spawn 那种 SystemDivider），不再走 MessageBubble

## 复现步骤

不需要特定会话 ID，任何一次"Lead 派 Teammate → Lead 决定结束团队"的完整流程都会触发：

1. 起 `pnpm tauri:dev`
2. 在聊天里让 LLM 派活：
   > "组个辩论团队，正反方各派一个，正方主张 X，反方主张 Y，每方写 200 字立论，写完结束团队"
3. 等两个 Teammate 各回一条 `SendMessage(type=text)` 后，LLM 会主动调 `SendMessage(type=shutdown_request)` 给两个 Teammate，Teammate 应答 `shutdown_response`
4. 打开右侧"团队过程"抽屉
5. **观察**：两条立论正文正常显示；之后 4 条灰色气泡显示"（空消息）"，from→to 标签是 `team-lead → pro-debater`、`con-debater → team-lead` 等

## 根因（已确认）

### 落盘是对的

`<conv>/teams/{team}/team-chat.jsonl` 每行格式：

```json
{"from":"con-debater","text":"...立论正文...","to":"team-lead","ts":"...","variant":"text"}
{"from":"team-lead","text":"","to":"pro-debater","ts":"...","variant":"shutdown_request"}
{"from":"con-debater","text":"","to":"team-lead","ts":"...","variant":"shutdown_response"}
```

- text 行有正文是对的
- 协议行 `text=""` **是对的**，因为协议语义全在 `variant` 字段里（详见下文 StructuredMessage 定义）
- 后端 `append_team_chat_entry` 把所有 SendMessage 都记进去也是对的（审计日志一致性，参考 cc-best）

### `StructuredMessage` 5 个 variant

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

### 前端渲染缺陷

`src/components/team/TeamChatEvents.tsx::MessageBubble`（L184-185）：

```tsx
{text ? (
  <AssistantMarkdown text={text} />
) : (
  <span className="italic text-muted-foreground">（空消息）</span>
)}
```

不看 variant，只看 text 空不空 → 协议消息全部撞这个分支。

### 数据链路上的小尾巴：variant 在后端被吃掉

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

## 后端为什么不改 jsonl 格式（设计意图）

防御性回答 —— 你可能会问"那让后端只在 text variant 时才写 jsonl 不就行了？"答：不行。

1. **审计一致性**：`team-chat.jsonl` 是 team 内所有 SendMessage 路由的完整事件流，用于排查"为什么 team 解散了 / 为什么 Teammate 退出了"这类问题。协议消息恰恰是这种问题的关键证据
2. **单一管道**：cc-best 设计 SendMessage 是 agent 之间唯一通信管道，不为协议握手单独开通道；落盘策略跟着对齐
3. **跟其他 lifecycle 事件对称**：`team_create` / `agent_spawn` / `agent_stop` 这些已经渲染为 SystemDivider 而非气泡。协议消息走同样模式即可

## variant 清单 + 期望 UI

| variant | 含义 | 当前显示 | 期望显示 |
|---|---|---|---|
| `text` | 自由文本对话 | 对话气泡（保留） | 对话气泡（不动）|
| `shutdown_request` | 发起方请求接收方退出 | "（空消息）"气泡 ❌ | 系统标签：`⊙ {from} 请求 {to} 退出团队`（有 reason 时追加 `· {reason}`）|
| `shutdown_response` (approve=true) | 接收方同意退出 | "（空消息）"气泡 ❌ | 系统标签：`⊙ {from} 同意退出` |
| `shutdown_response` (approve=false) | 接收方拒绝退出 | "（空消息）"气泡 ❌ | 系统标签：`⊙ {from} 拒绝退出`（有 reason 时追加 `· {reason}`）|
| `plan_approval_request` | 发起方请求审批 plan | "（空消息）"气泡 ❌ | 系统标签：`⊙ {from} 请 {to} 审批方案`（如需展示 plan 内容请见下方"待拍板"）|
| `plan_approval_response` (approve=true) | 同意 plan | "（空消息）"气泡 ❌ | 系统标签：`⊙ {from} 同意方案` |
| `plan_approval_response` (approve=false) | 拒绝 plan | "（空消息）"气泡 ❌ | 系统标签：`⊙ {from} 修改建议：{feedback}` |

## 三个待你拍板的设计选择

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
- `✓` for shutdown_response approve=true（同意）
- `✗` for shutdown_response approve=false（拒绝）
- `≪` for plan_approval_request
- `✓` / `✗` for plan_approval_response

请你判断：是否需要新增 `ProtocolEventRow.tsx`？比如想给协议消息加箭头方向（请求 vs 应答）、加 from→to 双 avatar、加 hover 显示 reason/feedback 详情？

### C. i18n / 文案

当前 `TeamChatEvents.tsx` 文案全是中文硬编码（`团队已创建` / `团队已解散` / `加入团队` / `已退出`），没接 react-i18next。

请你确认：

1. 这次分流的协议标签**沿用中文硬编码**？还是顺手把整个 TeamChatEvents 改造接 i18next？
2. 如果接 i18next，命名 key 建议：`teamChat.protocol.shutdownRequest` / `.shutdownResponse.approve` / `.shutdownResponse.reject` / `.planApprovalRequest` / ...

## 实现指引

### 后端改动（必须先做）

**文件**：`src-tauri/src/runtime/team_view.rs`

1. `TeamEvent::SendMessage` 加 `variant: String` 字段
2. `append_events_from_team_chat_jsonl` 读 `v.get("variant").and_then(|x| x.as_str()).unwrap_or("text").to_string()` 透传

**注意**：`TeamEvent` 是 `#[serde(rename_all = "camelCase")]`，TS 端字段名是 `variant`（保持不变）。`PeerMessage` 已经有 variant 字段，可以参考它。

后端单测要补一条：jsonl 含 `variant=shutdown_request` 的行能映射到 `TeamEvent::SendMessage { variant: "shutdown_request" .. }`。

### TS 类型补字段

**文件**：`src/types/team.ts` L21-29

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

考虑用 union literal 限制：`variant: 'text' | 'shutdown_request' | 'shutdown_response' | 'plan_approval_request' | 'plan_approval_response'`（不允许 string 时收窄类型）—— 但这要求后端永远不引入新 variant 不通知前端，请你判断要不要这层强约束。

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

**注意**：`approve` / `reason` / `feedback` 这些字段当前**没有**从后端透传过来（jsonl 只记 text+from+to+ts+variant）。如果协议标签需要展示 approve 状态，**还需要后端 `append_team_chat_entry` 在写 jsonl 时把 message 结构体的额外字段（approve / reason / feedback）也写进去**，否则前端只能按 variant 名字模糊显示，无法区分 approve=true vs false。

这是个 **scope 决策点**：

- **方案 X**：jsonl 只加 approve / reason / feedback 三个字段（最小代价，覆盖 80% 需求）
- **方案 Y**：jsonl 把整个 StructuredMessage 序列化进去（前端拿到完整 payload，未来加 variant 不动后端）
- **方案 Z**：本期只按 variant 名分流，不区分 approve；以后真有人提需求再补字段

请你拍板。

## 不在本方案范围内

- 不改 `StructuredMessage` 5 个 variant 的字段
- 不改 `append_team_chat_entry` 的触发时机（每次 SendMessage 都落盘）
- 不调整 `TeamChatDrawer` 整体布局（标题、成员条、宽度、抽屉触发）
- 不改 `TeammateDetailPanel`（drill-down 详情面板独立模块）
- 不动 `useTeamStore` 的 store 结构

## 验收标准

1. 复现步骤里那条 6 行 jsonl 渲染后：2 行对话气泡 + 4 行系统标签（区分 request / response）
2. 关闭团队后回看历史，握手过程一目了然（不再有"空消息"）
3. variant=text 的对话气泡渲染零回归（截图对比）
4. 暗色主题下系统标签可读（`text-muted-foreground` 已是主题变量，本身没问题，但请确认 icon 字符的可读性）
5. 后端单测：jsonl 行 variant 透传到 TeamEvent
6. 前端单测：5 条 variant 分流路径覆盖
7. `pnpm lint` + `pnpm exec tsc --noEmit` 通过
8. `cargo test --lib runtime::team_view::` 通过

## 工作量估算

| 改动 | 文件 | 代码量 | 测试 |
|---|---|---|---|
| 后端 variant 透传 | team_view.rs | ~5 行 | +1 单测 |
| TS 类型加字段 | types/team.ts | ~1 行 | - |
| 前端分流 | TeamChatEvents.tsx | ~30-50 行（取决于是否复用 SystemDivider）| +5 vitest |
| 文案 / i18n | TeamChatEvents.tsx | 取决于决策 C | - |

总计：半天到一天可完成。

## 决策 checklist（请前端专家逐项回复）

1. [ ] 协议消息渲染策略 A1 / A2 / A3？
2. [ ] 复用 SystemDivider 还是新增 ProtocolEventRow？
3. [ ] 文案中文硬编码 还是 接 i18next？接的话 key 命名是否同意 `teamChat.protocol.*`？
4. [ ] variant 字段在 TS 端用 string 还是 union literal？
5. [ ] approve / reason / feedback 字段透传策略 X / Y / Z？
6. [ ] 还有其他没考虑到的点？
