# Team 协作事件渲染 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** peer 消息和 task notification 持久化为带 XML 包裹的 user message，前端识别后渲染成左对齐浅色 banner，用户能看到团队成员间的协作消息流。

**Architecture:** 照搬 cc-best 的 UserTeammateMessage 设计——后端 drain 时额外 persist XML 到 messages.jsonl（best-effort），前端 `UserMessageBubble` 用正则识别后走 banner 组件；出站 `SendMessage` 工具调用从 `useTurnRenderModel` 的 `RenderTurn` 结构里提取，渲染成同一 `PeerMessageBanner` 组件。

**Tech Stack:** Rust (chat_turn_driver.rs), React/TypeScript (useTurnRenderModel, UserMessageBubble, 新 PeerMessageBanner/TaskNotificationBanner 组件), Vitest

---

## 文件变更清单

| 文件 | 动作 | 说明 |
|---|---|---|
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | Modify | drain 后 persist XML |
| `src/hooks/useTurnRenderModel.ts` | Modify | 新增 `RenderPeerBanner` 类型 + 出站 SendMessage 提取 |
| `src/components/chat-scene/PeerMessageBanner.tsx` | Create | 入站/出站通用 banner |
| `src/components/chat-scene/TaskNotificationBanner.tsx` | Create | task-notification banner |
| `src/components/chat-scene/UserMessageBubble.tsx` | Modify | 识别 XML 走 banner |
| `src/components/chat/MessageList.tsx` | Modify | 渲染 turn.peerBanners |
| `src/components/chat-scene/__tests__/UserMessageBubble.test.tsx` | Modify | 加 XML 识别测试（return null 验证） |
| `src/hooks/__tests__/useTurnRenderModel.test.ts` | Create/Modify | classifyTeamEventMessage + SendMessage 提取测试 |

---

## Task 1: 后端 — drain 后 persist peer messages XML

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs:1402-1417`

### 背景

`drain_and_inject_lead_inbox_messages`（line 431）是 free function，拿不到 executor。
`drain_and_inject_task_notifications`（line 369）同上。
两个函数在 `run_chat_turn_s4` 里被调用（line 1402-1417），那里有 `executor` 参数。
**方案：在调用点之后紧跟 persist，不改 free function 签名。**

- [ ] **Step 1: 在 task notification drain 之后加 persist**

找到 `src-tauri/src/runtime/chat/chat_turn_driver.rs` line ~1402，在 `drain_and_inject_task_notifications` 调用之后加：

```rust
        // ★ NEW: persist each task-notification XML as user message (best-effort)
        for notification in &pending_task_notifications {
            if let Err(e) = executor
                .persist_user_message(
                    request.conversation_id.as_str(),
                    &notification.xml,
                    &[],
                    None,
                )
                .await
            {
                log::warn!(
                    "[chat_turn_driver] persist task-notification failed (best-effort): {e}"
                );
            }
        }
```

- [ ] **Step 2: 在 peer messages drain 之后加 persist**

找到 `drain_and_inject_lead_inbox_messages` 调用之后（line ~1412-1417），先用局部变量捕获 xml，再 persist：

```rust
        // ★ NEW: persist peer messages XML as user message (best-effort)
        // xml is already constructed inside drain_and_inject_lead_inbox_messages;
        // we need to reconstruct it here or refactor to return the xml.
        // Simpler: refactor drain_and_inject_lead_inbox_messages to return
        // (count, Option<String>) where the String is the xml if any was produced.
```

**注意**：`drain_and_inject_lead_inbox_messages` 目前返回 `usize`（消息数量），不返回 xml string。需要修改签名返回 `(usize, Option<String>)`。

修改 `drain_and_inject_lead_inbox_messages` 签名（line 431）：

```rust
async fn drain_and_inject_lead_inbox_messages(
    query_engine: &QueryEngine,
    session_id: &SessionId,
    messages: &mut Vec<serde_json::Value>,
) -> (usize, Option<String>) {  // ← 改：返回 (count, xml_if_any)
```

修改函数末尾返回值：

```rust
    let xml = render_peer_messages_xml(&drained);
    messages.push(serde_json::json!({
        "role": "user",
        "content": xml.clone(),  // ← clone 留给调用方 persist
    }));

    let count = drained.len();
    log::info!(
        "[chat_turn_driver] drained {} peer message(s) into Lead's next user message",
        count
    );
    // ... diagnostics unchanged ...
    (count, Some(xml))  // ← 改：返回 xml
}
```

原来返回 `0` 的 early-return 处全改为 `(0, None)`：

```rust
    // 改所有 early return:
    return (0, None);
```

- [ ] **Step 3: 调用点更新**

在 `run_chat_turn_s4` 里（line ~1412），改为：

```rust
        let (drained_peer_messages, peer_xml) = drain_and_inject_lead_inbox_messages(
            &self.query_engine,
            turn.session_id(),
            &mut initial_messages,
        )
        .await;

        // ★ NEW: persist peer messages XML (best-effort)
        if let Some(xml) = peer_xml {
            if let Err(e) = executor
                .persist_user_message(
                    request.conversation_id.as_str(),
                    &xml,
                    &[],
                    None,
                )
                .await
            {
                log::warn!(
                    "[chat_turn_driver] persist peer-messages failed (best-effort): {e}"
                );
            }
        }
```

- [ ] **Step 4: cargo check**

```bash
cd src-tauri && cargo check --lib 2>&1 | tail -20
```

期望：0 errors。

- [ ] **Step 5: 已有单测仍通过**

```bash
cd src-tauri && cargo test --lib -- chat_turn_driver 2>&1 | tail -20
```

期望：全部 pass（基线 982 passing）。

- [ ] **Step 6: 补单测 — persist 被调用**

在 `chat_turn_driver.rs` 测试模块末尾加：

```rust
    #[tokio::test]
    async fn task_notification_xml_is_persisted_as_user_message() {
        // 构造一个带 task_notification_queue 的 driver
        // mock executor 记录 persist_user_message 调用
        // 验证:notification.xml 出现在 persisted 列表里
        // 实现略(见现有 mock executor 模式)
    }

    #[tokio::test]
    async fn peer_messages_xml_is_persisted_as_user_message() {
        // 构造 driver + inbox_registry + lead_inbox 有 1 条消息
        // 验证:drain 后 persist_user_message 被调用,content 含 <peer-messages>
    }

    #[tokio::test]
    async fn persist_failure_does_not_abort_turn() {
        // mock executor.persist_user_message 返回 Err
        // 验证:turn 仍正常完成(返回 Ok)
    }
```

- [ ] **Step 7: cargo test --lib 全部 pass**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -5
```

期望：`test result: ok. N passed`（N ≥ 982）。

- [ ] **Step 8: commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs
git commit -m "fix(ltr): persist peer-messages and task-notification XML to messages.jsonl"
```

---

## Task 2: 前端类型 — 在 RenderTurn 加 peerBanners

**Files:**
- Modify: `src/hooks/useTurnRenderModel.ts`

- [ ] **Step 1: 新增类型**

在 `useTurnRenderModel.ts` 的 interface 区域（line ~50 附近），加：

```typescript
export interface RenderPeerBanner {
  /** 'peer' = SendMessage 消息; 'task' = task-notification */
  kind: 'peer' | 'task'
  from: string
  to?: string
  body: string
  summary?: string
  /** task 专用 */
  agent?: string
  status?: string
}
```

- [ ] **Step 2: RenderTurn 加 peerBanners 字段**

在 `RenderTurn` interface 里加：

```typescript
export interface RenderTurn {
  userMessage?: { ... }   // 不变
  aiSegments: RenderAiSegment[]
  toolGroup?: RenderToolGroup
  generatedFiles: RenderGeneratedFile[]
  suggestions: string[]
  peerBanners: RenderPeerBanner[]   // ← 新增
}
```

- [ ] **Step 3: buildTurnsFromMessages 初始化 peerBanners**

所有 `current = { ..., suggestions: [] }` 处加 `peerBanners: []`：

```typescript
    current = {
      userMessage: undefined,
      aiSegments: [],
      toolGroup: undefined,
      generatedFiles: [],
      suggestions: [],
      peerBanners: [],   // ← 新增
    }
```

（两处，line ~210 和 ~222）

- [ ] **Step 4: user message 识别 peer/task XML**

在 `buildTurnsFromMessages` 的 `if (m.role === 'user')` 分支里（line ~209），user message push 进 turn 后，加识别逻辑：

```typescript
    if (m.role === 'user') {
      const classified = classifyTeamEventMessage(m.content.text ?? '')
      if (classified) {
        // 这是系统 inject 的 team event message，不渲染成 user bubble
        current = {
          userMessage: undefined,
          aiSegments: [],
          toolGroup: undefined,
          generatedFiles: [],
          suggestions: [],
          peerBanners: classified,
        }
        turns.push(current)
        continue
      }
      // 普通 user message
      current = {
        userMessage: normalizeUserMessageForRender(m),
        ...
      }
      turns.push(current)
      continue
    }
```

- [ ] **Step 5: 新增 classifyTeamEventMessage 工具函数**

在文件顶部（import 之后，functions 之前）加：

```typescript
const PEER_MESSAGES_RE = /^<peer-messages>([\s\S]*?)<\/peer-messages>$/
const PEER_MESSAGE_ITEM_RE = /<peer-message\s+from="([^"]*)"(?:\s+variant="[^"]*")?>([\s\S]*?)<\/peer-message>/g
const TASK_NOTIFICATION_RE = /^<task-notification\s+agent="([^"]*)"\s+status="([^"]*)">([\s\S]*?)<\/task-notification>$/

/**
 * 如果 text 是 team event XML（整段匹配），返回 RenderPeerBanner[]；否则 null。
 */
function classifyTeamEventMessage(text: string): RenderPeerBanner[] | null {
  const trimmed = text.trim()

  const peerMatch = trimmed.match(PEER_MESSAGES_RE)
  if (peerMatch) {
    const banners: RenderPeerBanner[] = []
    const inner = peerMatch[1]
    let m: RegExpExecArray | null
    PEER_MESSAGE_ITEM_RE.lastIndex = 0
    while ((m = PEER_MESSAGE_ITEM_RE.exec(inner)) !== null) {
      banners.push({
        kind: 'peer',
        from: m[1],
        to: 'team-lead',
        body: m[2].trim(),
      })
    }
    return banners.length > 0 ? banners : null
  }

  const taskMatch = trimmed.match(TASK_NOTIFICATION_RE)
  if (taskMatch) {
    return [{
      kind: 'task',
      from: 'system',
      agent: taskMatch[1],
      status: taskMatch[2],
      body: taskMatch[3].trim(),
    }]
  }

  return null
}
```

- [ ] **Step 6: 出站 SendMessage 从 assistant tool_calls 提取**

在 `buildTurnsFromMessages` 的 `if (m.role === 'assistant')` 分支里（line ~232），处理 `m.toolCalls` 时，对 `SendMessage` 单独提取：

```typescript
    if (m.role === 'assistant') {
      if (m.toolCalls?.length) {
        const group = ensureToolGroup(current)
        for (const tc of m.toolCalls) {
          // ★ NEW: SendMessage → peerBanner instead of toolGroup step
          if (tc.name === 'SendMessage') {
            const args = tc.arguments as { to?: string; message?: unknown; summary?: string }
            const body = typeof args.message === 'string'
              ? args.message
              : JSON.stringify(args.message ?? '')
            current.peerBanners.push({
              kind: 'peer',
              from: 'team-lead',
              to: args.to ?? '?',
              body,
              summary: args.summary,
            })
            continue  // 不进 toolGroup
          }
          // 其余 tool_calls 走原有逻辑
          const existing = group.steps.find((s) => s.toolCallId === tc.id)
          // ...不变...
        }
      }
      // ...不变...
    }
```

- [ ] **Step 7: tsc check**

```bash
pnpm exec tsc --noEmit 2>&1 | tail -20
```

期望：0 errors。

- [ ] **Step 8: 补测试**

在 `src/hooks/__tests__/useTurnRenderModel.test.ts`（如不存在则新建）加：

```typescript
import { buildTurnsFromMessages } from '../useTurnRenderModel'

describe('classifyTeamEventMessage via buildTurnsFromMessages', () => {
  it('peer-messages XML becomes peerBanners, not userMessage', () => {
    const messages = [{
      id: 'm1', role: 'user' as const,
      content: { text: '<peer-messages>\n  <peer-message from="小研" variant="text">调研完成</peer-message>\n</peer-messages>' },
      conversationId: 'c1', createdAt: '',
    }]
    const turns = buildTurnsFromMessages(messages, [])
    expect(turns[0].userMessage).toBeUndefined()
    expect(turns[0].peerBanners).toHaveLength(1)
    expect(turns[0].peerBanners[0]).toMatchObject({ kind: 'peer', from: '小研', body: '调研完成' })
  })

  it('task-notification XML becomes peerBanners', () => {
    const messages = [{
      id: 'm2', role: 'user' as const,
      content: { text: '<task-notification agent="小算" status="completed">分析完成</task-notification>' },
      conversationId: 'c1', createdAt: '',
    }]
    const turns = buildTurnsFromMessages(messages, [])
    expect(turns[0].peerBanners[0]).toMatchObject({ kind: 'task', agent: '小算', status: 'completed' })
  })

  it('plain user message stays as userMessage', () => {
    const messages = [{
      id: 'm3', role: 'user' as const,
      content: { text: '普通消息' },
      conversationId: 'c1', createdAt: '',
    }]
    const turns = buildTurnsFromMessages(messages, [])
    expect(turns[0].userMessage?.text).toBe('普通消息')
    expect(turns[0].peerBanners).toHaveLength(0)
  })

  it('SendMessage tool_call produces peerBanner not toolGroup step', () => {
    const messages = [
      { id: 'm1', role: 'user' as const, content: { text: '开始' }, conversationId: 'c1', createdAt: '' },
      {
        id: 'm2', role: 'assistant' as const,
        content: { text: '' },
        toolCalls: [{ id: 'tc1', name: 'SendMessage', arguments: { to: '小研', message: '去调研', summary: '派活' } }],
        conversationId: 'c1', createdAt: '',
      },
    ]
    const turns = buildTurnsFromMessages(messages as any, [])
    expect(turns[0].peerBanners[0]).toMatchObject({ kind: 'peer', from: 'team-lead', to: '小研', body: '去调研' })
    expect(turns[0].toolGroup).toBeUndefined()
  })
})
```

- [ ] **Step 9: vitest run**

```bash
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts 2>&1 | tail -20
```

期望：4 passed。

- [ ] **Step 10: commit**

```bash
git add src/hooks/useTurnRenderModel.ts src/hooks/__tests__/useTurnRenderModel.test.ts
git commit -m "feat(ltr): RenderTurn.peerBanners — extract SendMessage + team event XML"
```

---

## Task 3: 前端组件 — PeerMessageBanner + TaskNotificationBanner

**Files:**
- Create: `src/components/chat-scene/PeerMessageBanner.tsx`
- Create: `src/components/chat-scene/TaskNotificationBanner.tsx`

- [ ] **Step 1: 新建 PeerMessageBanner**

```typescript
// src/components/chat-scene/PeerMessageBanner.tsx
import type { RenderPeerBanner } from '@/hooks/useTurnRenderModel'

interface Props {
  banners: RenderPeerBanner[]
}

export function PeerMessageBanner({ banners }: Props) {
  const peerItems = banners.filter((b) => b.kind === 'peer')
  const taskItems = banners.filter((b) => b.kind === 'task')

  if (banners.length === 0) return null

  return (
    <div className="flex flex-col gap-1.5 w-full">
      {peerItems.length > 0 && (
        <div className="rounded-lg border border-border bg-muted px-3 py-2 text-sm text-muted-foreground">
          <div className="mb-1.5 flex items-center gap-1.5 font-medium text-foreground">
            <span>🔔</span>
            <span>团队消息</span>
          </div>
          <div className="flex flex-col gap-1">
            {peerItems.map((b, i) => (
              <div key={i}>
                <span className="font-medium text-foreground">
                  {b.from} → {b.to ?? 'Lead'}
                </span>
                <p className="mt-0.5 text-muted-foreground">{b.body}</p>
              </div>
            ))}
          </div>
        </div>
      )}
      {taskItems.map((b, i) => (
        <div key={i} className="rounded-lg border border-border bg-muted px-3 py-2 text-sm text-muted-foreground">
          <div className="mb-1 flex items-center gap-1.5 font-medium text-foreground">
            <span>✅</span>
            <span>子任务完成</span>
          </div>
          <div className="text-muted-foreground">
            <span className="font-medium text-foreground">{b.agent}</span>
            {b.body && <p className="mt-0.5">{b.body}</p>}
          </div>
        </div>
      ))}
    </div>
  )
}
```

- [ ] **Step 2: tsc check**

```bash
pnpm exec tsc --noEmit 2>&1 | tail -10
```

期望：0 errors。

- [ ] **Step 3: commit**

```bash
git add src/components/chat-scene/PeerMessageBanner.tsx
git commit -m "feat(ltr): add PeerMessageBanner component for team event rendering"
```

---

## Task 4: 前端 — MessageList 渲染 peerBanners

**Files:**
- Modify: `src/components/chat/MessageList.tsx`
- Modify: `src/components/chat-scene/UserMessageBubble.tsx`

- [ ] **Step 1: MessageList 渲染 peerBanners**

在 `MessageList.tsx` 里 import PeerMessageBanner：

```typescript
import { PeerMessageBanner } from '@/components/chat-scene/PeerMessageBanner'
```

在 turn 渲染区域（line ~94），`userMessage` 之前渲染 peerBanners：

```typescript
          {t.peerBanners.length > 0 ? (
            <PeerMessageBanner banners={t.peerBanners} />
          ) : null}
          {t.userMessage ? (
            <UserMessageBubble ... />
          ) : null}
```

- [ ] **Step 2: UserMessageBubble — 跳过纯 XML 内容**

`UserMessageBubble` 里，如果 `text` 被识别为 team event XML（整段匹配 `<peer-messages>` 或 `<task-notification>`），**直接 return null**（因为 `buildTurnsFromMessages` 已经把这条 message 转成 `peerBanners`，不会产生 userMessage；但历史消息 load 时可能走到这里）。

在 `UserMessageBubble` 顶部加：

```typescript
const TEAM_EVENT_RE = /^(?:<peer-messages>[\s\S]*<\/peer-messages>|<task-notification[\s\S]*<\/task-notification>)$/

export function UserMessageBubble({ text, ... }) {
  if (TEAM_EVENT_RE.test(text?.trim() ?? '')) return null
  // ...原有逻辑不变
```

- [ ] **Step 3: tsc check**

```bash
pnpm exec tsc --noEmit 2>&1 | tail -10
```

期望：0 errors。

- [ ] **Step 4: 补 UserMessageBubble 测试**

在 `src/components/chat-scene/__tests__/UserMessageBubble.test.tsx` 加：

```typescript
it('returns null for peer-messages XML', () => {
  const { container } = render(
    <UserMessageBubble text="<peer-messages><peer-message from=\"小研\" variant=\"text\">hi</peer-message></peer-messages>" />
  )
  expect(container.firstChild).toBeNull()
})

it('returns null for task-notification XML', () => {
  const { container } = render(
    <UserMessageBubble text='<task-notification agent="小研" status="completed">done</task-notification>' />
  )
  expect(container.firstChild).toBeNull()
})

it('renders normally for plain text', () => {
  const { getByTestId } = render(<UserMessageBubble text="普通消息" />)
  expect(getByTestId('user-bubble')).toBeTruthy()
})
```

- [ ] **Step 5: vitest run**

```bash
pnpm exec vitest run src/components/chat-scene/__tests__/UserMessageBubble.test.tsx 2>&1 | tail -20
```

期望：all passed。

- [ ] **Step 6: commit**

```bash
git add src/components/chat/MessageList.tsx src/components/chat-scene/UserMessageBubble.tsx
git commit -m "feat(ltr): render peerBanners in MessageList; hide team-event XML in UserMessageBubble"
```

---

## Task 5: 端到端手测 + 收尾

- [ ] **Step 1: 启动 dev server**

```bash
pnpm tauri:dev
```

- [ ] **Step 2: 跑 Team 模式场景**

1. 新建会话，让 Lead 调 `TeamCreate` + 派 teammate
2. teammate 执行完后调 `SendMessage(to="team-lead", message="xxx")`
3. 前端聊天界面应看到：
   - 🔔 团队消息 / 小研 → Lead / xxx  ← 左对齐浅色 banner
4. Lead 调 `SendMessage(to="小研", message="yyy")` 时，assistant 消息区域应看到：
   - 🔔 团队消息 / team-lead → 小研 / yyy ← 出站 banner（在 AI 那一侧渲染，不是 toolGroup 折叠卡片）
5. Sub-agent 完成时应看到 ✅ 子任务完成 banner

- [ ] **Step 3: 刷新页面，历史消息里 banner 仍在**

验证：重新打开会话，peer 消息 banner 和 task notification banner 均正常显示（不消失）。

- [ ] **Step 4: 全量测试**

```bash
pnpm exec vitest run 2>&1 | tail -10
cd src-tauri && cargo test --lib 2>&1 | tail -5
```

期望：全部 pass。

- [ ] **Step 5: 最终 commit**

```bash
git add -A
git commit -m "feat(ltr): Team 协作事件 banner — peer messages + task notifications 可视化"
```
