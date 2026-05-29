# 对话页视觉接入 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把聊天页的消息流、富内容渲染、输入区、ToolGroup 时长全部接通，对齐 design.pen 视觉。

**Architecture:** 按 C→A→B→D 顺序实施。C（容器宽度）2 行改动无风险；A（AiBubble 接回）在 useTurnRenderModel 把 Message 全量传下去，MessageList 改用 AiBubble 渲染 AI 段；B（InputBar 视觉）只改 className/style，功能代码不动；D（ToolGroup 时长）在 streamingStore 前端计算时间差。

**Tech Stack:** React 18 + TypeScript + Tailwind v4 + Zustand + Vitest

---

## 文件结构

| 文件 | 改动类型 | 改动内容 |
|---|---|---|
| `src/components/layout/ChatArea.tsx` | Modify | 去掉自身 px-6，max-w 从 860 改 1032 |
| `src/components/chat/MessageList.tsx` | Modify | aiSegments 改用 AiBubble，传 message 对象 |
| `src/hooks/useTurnRenderModel.ts` | Modify | RenderAiSegment 携带 message: Message |
| `src/components/chat/AiBubble.tsx` | Modify | 加 hideHeader?: boolean prop |
| `src/components/layout/InputBar.tsx` | Modify | 只改外层容器和发送按钮 className/style |
| `src/stores/streamingStore.ts` | Modify | ToolExecution 加 startedAt/durationMs，addToolExecution 记时间，updateToolExecution 完成时算 delta |
| `src/hooks/__tests__/useTurnRenderModel.test.ts` | Modify | 更新 RenderAiSegment 断言适配 message 字段 |

---

## Task 1: 消息流容器宽度对齐（C）

**Files:**
- Modify: `src/components/layout/ChatArea.tsx`

- [ ] **Step 1: 修改 ChatArea 容器**

找到 `ChatArea.tsx` 里第 120 行左右的 `<div className="mx-auto max-w-[860px] px-6 pt-6 pb-40">`，改为：

```tsx
<div className="mx-auto max-w-[1032px] pt-6 pb-40">
```

去掉 `px-6`（MessageList 的 `px-10` 负责水平 padding），max-w 从 860 改到 1032。

同时找同文件里 `authorizedWorkspace` banner 的 `max-w-[860px]`（约 198 行），也改为 `max-w-[1032px]`。

- [ ] **Step 2: tsc + lint + 全测**

```bash
pnpm exec tsc --noEmit
pnpm lint
pnpm test 2>&1 | tail -5
```

Expected: 0 error，全绿。

- [ ] **Step 3: commit**

```bash
git add src/components/layout/ChatArea.tsx
git commit -m "fix(frontend): align ChatArea container to design.pen max-w-1032 rhythm

Remove ChatArea's own px-6 (MessageList handles px-10) and increase
max-w from 860 to 1032 per design.pen.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: useTurnRenderModel 携带完整 Message 对象（A 前置）

**Files:**
- Modify: `src/hooks/useTurnRenderModel.ts`
- Modify: `src/hooks/__tests__/useTurnRenderModel.test.ts`

- [ ] **Step 1: 更新 RenderAiSegment 类型**

在 `src/hooks/useTurnRenderModel.ts` 里，把：

```ts
export interface RenderAiSegment {
  id: string
  text: string
}
```

改为：

```ts
export interface RenderAiSegment {
  id: string
  text: string
  message: Message
}
```

- [ ] **Step 2: 更新 buildTurnsFromMessages 填充 message 字段**

找到 `buildTurnsFromMessages` 里的这一行：

```ts
current.aiSegments.push({ id: m.id, text: m.content.text })
```

改为：

```ts
current.aiSegments.push({ id: m.id, text: m.content.text, message: m })
```

- [ ] **Step 3: 运行测试确认失败**

```bash
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts
```

Expected: 类型测试可能通过，但 `RenderAiSegment` shape 断言如果有 `text` 检查会继续通过。若 MessageList 的测试引用了 `aiSegments[].text`，可能出现类型警告。

- [ ] **Step 4: 更新 useTurnRenderModel 测试**

在 `src/hooks/__tests__/useTurnRenderModel.test.ts` 里，找到 `RenderTurn shape smoke` 测试，更新 `aiSegments` 的样例：

```ts
describe('RenderTurn shape smoke', () => {
  it('aiSegment carries the full message object', () => {
    const msg = aiMsg('a1', 'hello')
    const turns = buildTurnsFromMessages([userMsg('u1', 'hi'), msg], [])
    expect(turns[0].aiSegments[0].message).toBe(msg)
    expect(turns[0].aiSegments[0].id).toBe('a1')
  })
})
```

- [ ] **Step 5: 运行测试确认通过**

```bash
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts
pnpm exec tsc --noEmit
```

Expected: PASS / 0 error。

- [ ] **Step 6: commit**

```bash
git add src/hooks/useTurnRenderModel.ts src/hooks/__tests__/useTurnRenderModel.test.ts
git commit -m "feat(frontend): RenderAiSegment carries full Message object

Needed so MessageList can pass the full Message to AiBubble for rich
content rendering.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: AiBubble 加 hideHeader prop（A）

**Files:**
- Modify: `src/components/chat/AiBubble.tsx`

- [ ] **Step 1: 在 AiBubbleProps 加 hideHeader**

找到 `src/components/chat/AiBubble.tsx` 里的：

```tsx
interface AiBubbleProps {
  message: Message
  isStreaming?: boolean
}
```

改为：

```tsx
interface AiBubbleProps {
  message: Message
  isStreaming?: boolean
  /** When true, hides the Avatar + product name header row.
   *  Used by MessageList turn-based rendering where headers are not shown per message. */
  hideHeader?: boolean
}
```

- [ ] **Step 2: 在 render 里使用 hideHeader**

在 `AiBubble` 函数里，找到渲染 header 的代码段（约 157-170 行）：

```tsx
return (
  <div className="mb-7 animate-[fadeUp_0.3s_ease]">
    {/* Header: avatar + name */}
    <div className="mb-2 flex items-center gap-2">
      <Avatar variant="ai" />
      <span
        className="text-sm font-semibold"
        style={{ color: 'var(--color-text-primary)' }}
      >
        {productName}
      </span>
    </div>
    {/* Body — offset by avatar width */}
    <div className="group relative pl-9">
```

改为：

```tsx
return (
  <div className="animate-[fadeUp_0.3s_ease]" style={{ marginBottom: hideHeader ? 0 : '1.75rem' }}>
    {/* Header: avatar + name */}
    {!hideHeader && (
      <div className="mb-2 flex items-center gap-2">
        <Avatar variant="ai" />
        <span
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {productName}
        </span>
      </div>
    )}
    {/* Body — no avatar offset when header hidden */}
    <div className={`group relative ${hideHeader ? '' : 'pl-9'}`}>
```

- [ ] **Step 3: tsc + 全测**

```bash
pnpm exec tsc --noEmit
pnpm test 2>&1 | tail -5
```

Expected: 0 error，全绿（AiBubble 的现有测试 AiBubble.subagent.test.tsx / AiBubble.actions.test.tsx 不传 hideHeader，行为不变）。

- [ ] **Step 4: commit**

```bash
git add src/components/chat/AiBubble.tsx
git commit -m "feat(frontend): add hideHeader prop to AiBubble

When hideHeader=true, hides Avatar + product name row and removes
pl-9 body offset. Used by MessageList turn-based rendering where
each AI turn is shown as a content block without repeated headers.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: MessageList 改用 AiBubble 渲染 AI 段（A）

**Files:**
- Modify: `src/components/chat/MessageList.tsx`

- [ ] **Step 1: 更新 imports**

在 `src/components/chat/MessageList.tsx` 里，把：

```tsx
import { AiSegmentText } from '@/components/chat-scene/AiSegmentText'
```

替换为：

```tsx
import { AiBubble } from '@/components/chat/AiBubble'
```

（`AiSegmentText` 仍保留文件本体，只是 MessageList 不再用它）

- [ ] **Step 2: 把 aiSegments 渲染改用 AiBubble**

找到 MessageList 里的这段：

```tsx
{t.aiSegments.map((s) => (
  <AiSegmentText key={s.id} text={s.text} />
))}
```

改为：

```tsx
{t.aiSegments.map((s) => (
  <AiBubble key={s.id} message={s.message} hideHeader />
))}
```

- [ ] **Step 3: 同步处理 streaming TypingIndicator**

MessageList 底部已有：

```tsx
{isStreaming ? <TypingIndicator variant="organize" /> : null}
```

保持不动。streaming 时 AiBubble 会接收 `isStreaming` 吗？不需要——streaming 内容通过 `StreamingBubble` 在 `ChatArea` 里另外渲染（检查 ChatArea.tsx 里是否还调用了 StreamingBubble，如果是，保留不动）。

- [ ] **Step 4: tsc + 全测**

```bash
pnpm exec tsc --noEmit
pnpm test 2>&1 | tail -5
```

Expected: 0 error，全绿。

- [ ] **Step 5: commit**

```bash
git add src/components/chat/MessageList.tsx
git commit -m "feat(frontend): MessageList uses AiBubble for rich content rendering

Replace AiSegmentText with AiBubble(hideHeader) in the turn-based
render loop. AiBubble handles all rich content types (code blocks,
tables, report cards, file cards, etc.) while hiding the repeated
avatar+name header row.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: InputBar 视觉对齐 design.pen（B）

**Files:**
- Modify: `src/components/layout/InputBar.tsx`

**原则：只改 className / style，不动任何 handler、state、hook。**

- [ ] **Step 1: 改最外层输入卡容器**

找到约 231 行的外层 `<div>` 容器（含 `mx-auto max-w-[860px] rounded-xl`）：

```tsx
<div
  className="mx-auto max-w-[860px] rounded-xl"
  style={{
    background: 'var(--color-bg-input)',
    boxShadow: 'var(--shadow-input)',
  }}
>
```

改为：

```tsx
<div className="mx-auto max-w-[1032px] rounded-[18px] border border-border bg-card">
```

- [ ] **Step 2: 改输入行 gap 和 padding**

找到输入行（约 280 行）`<div className="flex items-center gap-2 px-4 py-3">`，改为：

```tsx
<div className="flex items-center gap-2 px-4 pb-3.5 pt-3">
```

- [ ] **Step 3: 改发送/停止按钮**

找到发送/停止按钮（约 400 行）：

```tsx
<button
  className="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-lg border-none outline-none transition-colors duration-150"
  style={{
    background:
      isStreaming || hasPendingContent
        ? accentColor
        : 'var(--color-border)',
    cursor: isSendDisabled ? 'default' : 'pointer',
  }}
```

改为：

```tsx
<button
  className="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full border-none outline-none transition-colors duration-150"
  style={{
    background:
      isStreaming || hasPendingContent
        ? 'var(--primary)'
        : '#D4D4D8',
    cursor: isSendDisabled ? 'default' : 'pointer',
  }}
```

- [ ] **Step 4: 把 authorizedWorkspace banner 的 max-w 也改成 1032**

约 198 行：`max-w-[860px]` → `max-w-[1032px]`

- [ ] **Step 5: tsc + lint + 全测**

```bash
pnpm exec tsc --noEmit
pnpm lint
pnpm test 2>&1 | tail -5
```

Expected: 0 error，全绿。

- [ ] **Step 6: commit**

```bash
git add src/components/layout/InputBar.tsx
git commit -m "fix(frontend): align InputBar to design.pen ChatComposerCompact style

Container: rounded-[18px] border border-border bg-card (replaces
legacy bg-input + shadow-input). Send button: rounded-full with
--primary when active, #D4D4D8 when disabled. No functional changes.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: ToolGroup 接通前端 durationMs（D）

**Files:**
- Modify: `src/stores/streamingStore.ts`
- Modify: `src/hooks/useTurnRenderModel.ts`

- [ ] **Step 1: ToolExecution 加 startedAt 和 durationMs**

在 `src/stores/streamingStore.ts` 里，把：

```ts
export interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
}
```

改为：

```ts
export interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
  /** Unix ms timestamp when status changed to 'executing'. */
  startedAt?: number
  /** Elapsed ms from executing → completed/error. Set automatically. */
  durationMs?: number
}
```

- [ ] **Step 2: addToolExecution 记录 startedAt**

找到 `addToolExecution` 函数（约 370 行），改为：

```ts
addToolExecution: (execution) => {
  const { activeConversationId } = get()
  if (activeConversationId) {
    get().addConversationToolExecution(activeConversationId, {
      ...execution,
      startedAt: execution.status === 'executing' ? Date.now() : execution.startedAt,
    })
  }
},
```

- [ ] **Step 3: updateToolExecution 完成时计算 durationMs**

找到 `updateToolExecution` 函数（约 377 行），改为：

```ts
updateToolExecution: (toolId, updates) => {
  const { activeConversationId } = get()
  if (activeConversationId) {
    // If transitioning to completed/error, compute durationMs from startedAt
    if (updates.status === 'completed' || updates.status === 'error') {
      const existing = get().streamStates[activeConversationId]?.toolExecutions
        .find((t) => t.toolId === toolId)
      if (existing?.startedAt && !updates.durationMs) {
        updates = { ...updates, durationMs: Date.now() - existing.startedAt }
      }
    }
    get().updateConversationToolExecution(activeConversationId, toolId, updates)
  }
},
```

- [ ] **Step 4: useTurnRenderModel 传递 durationMs**

在 `src/hooks/useTurnRenderModel.ts` 里，找到 steps 构建段：

```ts
const steps: RenderToolStep[] = toolExecutions.map((t, i) => ({
  index: i + 1,
  name: t.toolName,
  status: toolExecStatusToStep(t.status),
}))
```

改为：

```ts
const steps: RenderToolStep[] = toolExecutions.map((t, i) => ({
  index: i + 1,
  name: t.toolName,
  status: toolExecStatusToStep(t.status),
  durationMs: t.durationMs,
}))
```

同理修改 `durationMs: 0` 的 toolGroup 聚合：

```ts
target.toolGroup = {
  status: running ? 'running' : 'done',
  steps,
  durationMs: steps.reduce((acc, s) => acc + (s.durationMs ?? 0), 0),
}
```

- [ ] **Step 5: 更新相关测试**

在 `src/hooks/__tests__/useTurnRenderModel.test.ts` 里，确认 `attaches tool executions` 测试里的 `toolGroup.durationMs` 断言。现在 `durationMs` 是 steps 的总和，两个 `completed` 工具的 `durationMs` 都是 `undefined`，所以 `durationMs` 应该是 `0`：

```ts
expect(turns[0].toolGroup?.status).toBe('done')
// durationMs is 0 when no timestamps are available (test fixtures)
expect(turns[0].toolGroup?.durationMs).toBe(0)
```

- [ ] **Step 6: tsc + 全测**

```bash
pnpm exec tsc --noEmit
pnpm test 2>&1 | tail -5
```

Expected: 0 error，全绿。

- [ ] **Step 7: commit**

```bash
git add src/stores/streamingStore.ts src/hooks/useTurnRenderModel.ts src/hooks/__tests__/useTurnRenderModel.test.ts
git commit -m "feat(frontend): compute ToolGroup durationMs from frontend timestamps

Record startedAt when tool begins executing; calculate durationMs
when status transitions to completed/error. useTurnRenderModel sums
per-step durationMs for the aggregate group timer.

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: 全流程验收

- [ ] **Step 1: 跑完整测试套件**

```bash
pnpm test
pnpm lint
pnpm exec tsc --noEmit
```

Expected: 全 PASS / 0 error。

- [ ] **Step 2: dev 目视验收**

```bash
pnpm tauri:dev
```

验收检查点：

1. **容器宽度**：消息区宽度明显变宽（1032px），左右各 40px padding，与首页宽度一致
2. **富内容显示**：发一条让 AI 生成代码/表格/报告的消息，确认代码块、数据表格、生成文件卡正常渲染（不再只显示纯文本）
3. **InputBar 外观**：输入框变成 r-18 border bg-card，发送按钮在有内容时变金色圆
4. **ToolGroup 时长**：有工具调用的对话里，工具卡顶部显示每步时长（格式 `1.3s`）
5. **无功能回归**：文件上传、技能弹层、停止流式、斜杠命令仍正常工作

---

## 自审

**Spec coverage:**
- C（容器宽度）→ Task 1 ✓
- A（AiBubble 富内容）→ Task 2（类型扩展）+ Task 3（hideHeader）+ Task 4（MessageList）✓
- B（InputBar 视觉）→ Task 5 ✓
- D（ToolGroup 时长）→ Task 6 ✓

**Placeholder scan:** 无 TBD/TODO。每个 step 都有完整代码。

**Type consistency:**
- `RenderAiSegment.message: Message` 在 Task 2 定义，Task 4 消费 `s.message` 传给 AiBubble — ✓
- `AiBubbleProps.hideHeader?: boolean` 在 Task 3 定义，Task 4 传 `hideHeader` 无参数 — ✓
- `ToolExecution.durationMs?: number` 在 Task 6 Step 1 定义，Step 4 取 `t.durationMs` — ✓
- `RenderToolGroup.durationMs` 是 Task 4 里已存在的字段，Task 6 改了赋值逻辑，类型不变 — ✓
