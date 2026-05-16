# Chat 流式输出性能优化设计

## Context

长对话（50 分钟、116 次 streaming append、618 个事件 handler）在 LLM 输出期间输入框基本卡死。已确认：

- streaming setState 已有 rAF 节流（`src/hooks/useStreaming.ts:241-302`），**频率不是瓶颈**
- 真瓶颈是渲染：
  - `rehype-highlight` (highlight.js) 每帧把累积全段 markdown 重新 token 化（`AssistantMarkdown.tsx:17` 硬开）
  - 历史 `AiBubble` 未 `React.memo`，每次 `messages` 数组变就跟着重渲（`AiBubble.tsx:30`，全代码库零 memo 用法）

目标：LLM 输出期间输入流畅；不引新依赖、不改交互、不动滚动（PR 2 才做）。

## 设计

### 组件职责

| 组件 | 职责 | 订阅 store？ |
|---|---|---|
| `AssistantMarkdown` | 把 markdown 渲染成 React 节点；`disableCodeHighlight` 决定是否跑 rehype-highlight | 否（纯 props） |
| `StreamingBubble` | 渲染 live token 流 + tool 状态 | 是（toolExecutions） |
| `AiBubble` | 渲染已完成 message；用 `React.memo` 锁住 | 否（纯 props） |

**关键不变量**：
- `AssistantMarkdown` 与 `AiBubble` 不订阅 store，确保 memo 浅比较有效
- streaming 时 `<AssistantMarkdown disableCodeHighlight />`；done 后 `AiBubble` 接管 → 默认开启高亮

### 数据流

```
streaming:delta → useStreaming rAF flush → streamStates[convId].streamingContent
  → <StreamingBubble> → <AssistantMarkdown disableCodeHighlight />  ★ 不跑 highlight

stream:done（同一帧）：
  store.upsertMessage(message)          → useTurnRenderModel 重算 turns
  store.clearConversationStreamState    → streamingContent='' isStreaming=false
  → StreamingBubble 卸载；最后一个 aiSegment 由 <AiBubble> 接管
  → AiBubble 首次渲染时 <AssistantMarkdown> 跑一次 highlight
  → 后续 messages 变化时 message 引用稳定 + React.memo 浅比较 → 不再重渲
```

### memo 比较器

`React.memo` 默认浅比较即可：

- `useTurnRenderModel.ts:338` 把 store 原始 `Message` 引用透传到 `aiSegments[].message`，历史 message 引用稳定（除非整个 messages 数组 reset）
- `isStreaming` prop 是布尔，浅比较自然 OK

## 实施步骤（TDD，三步独立）

### Step 1: `AssistantMarkdown` 加 `disableCodeHighlight` prop

**测试**：`src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx`

```tsx
it('disableCodeHighlight=true → 不注入 hljs-* className', () => {
  const { container } = render(
    <AssistantMarkdown text="```ts\nconst x = 1\n```" disableCodeHighlight />,
  )
  expect(container.querySelector('pre code')?.className ?? '').not.toMatch(/hljs/)
})

it('默认开启高亮', () => {
  const { container } = render(
    <AssistantMarkdown text="```ts\nconst x = 1\n```" />,
  )
  expect(container.querySelector('pre code')?.className ?? '').toMatch(/hljs|language-ts/)
})
```

**实现**：
- 把 `rehypePlugins` 提到模块作用域常量（避免每渲染新建数组）
- 加 `disableCodeHighlight?: boolean` prop，条件式传 `rehypePlugins`

### Step 2: `StreamingBubble` 传 `disableCodeHighlight`

**测试**：`src/components/chat/StreamingBubble.test.tsx`

```tsx
it('streaming 内容的代码块不含 hljs 高亮', () => {
  const { container } = render(<StreamingBubble content="```ts\nlet a = 1\n```" />)
  expect(container.querySelector('pre code')?.className ?? '').not.toMatch(/hljs/)
})
```

**实现**：`StreamingBubble.tsx:45` 改为 `<AssistantMarkdown text={cleanContent} disableCodeHighlight />`

### Step 3: `AiBubble` 套 `React.memo`（计数器测试）

**测试**：`src/components/chat/__tests__/AiBubble.memo.test.tsx`（新文件）

```tsx
import type { Message } from '@/types/message'

const makeMsg = (text: string): Message => ({
  id: 'm1', role: 'assistant', conversationId: 'c1',
  createdAt: '', updatedAt: '',
  content: { text },
}) as Message

const renderSpy = vi.fn()
vi.mock('@/components/chat-scene/AssistantMarkdown', () => ({
  AssistantMarkdown: (p: { text: string }) => {
    renderSpy(p.text)
    return <div>{p.text}</div>
  },
}))

it('相同 message 引用 → 不重渲', () => {
  const msg = makeMsg('hello')
  const { rerender } = render(<AiBubble message={msg} />)
  expect(renderSpy).toHaveBeenCalledTimes(1)
  rerender(<AiBubble message={msg} />)
  expect(renderSpy).toHaveBeenCalledTimes(1)
})

it('不同 message 引用 → 重渲', () => {
  renderSpy.mockClear()
  const { rerender } = render(<AiBubble message={makeMsg('hello')} />)
  rerender(<AiBubble message={makeMsg('hello')} />)
  expect(renderSpy).toHaveBeenCalledTimes(2)
})
```

**实现**：`AiBubble` 导出改成 `React.memo(function AiBubble(...) {...})`

## 风险

| 风险 | 缓解 |
|---|---|
| `useTurnRenderModel` 实际返回 message 引用不稳定 | 落地后临时 `console.log('AiBubble render', message.id)` 观察；不进 commit |
| `rehypePlugins={[]}` 引发 react-markdown 报错 | 已确认 react-markdown 支持空 plugins；`markdownComponents` 不依赖 `hljs-*` className |
| memo 浅比较失败 | 最坏退化到现在行为，无 bug |

## 不做

- ❌ 不加额外节流（rAF 已够）
- ❌ 不动 streamingStore / useStreaming 数据流
- ❌ 不引 incremark / streamdown / shiki
- ❌ 不动滚动行为（PR 2）
- ❌ 不 memo `ContentRenderer`（AiBubble memo 后冗余）

## 涉及文件

- `src/components/chat-scene/AssistantMarkdown.tsx` — 加 prop + 提常量
- `src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx`（或现有同名测试）— 加 case
- `src/components/chat/StreamingBubble.tsx:45` — 传 `disableCodeHighlight`
- `src/components/chat/StreamingBubble.test.tsx` — 加 case
- `src/components/chat/AiBubble.tsx:30` — 套 `React.memo`
- `src/components/chat/__tests__/AiBubble.memo.test.tsx` — 新建

## 验证

1. 单元测试：`pnpm test src/components/chat-scene src/components/chat`
2. 类型检查：`pnpm tsc --noEmit`
3. 全量回归：`pnpm test`
4. 手测：触发一次长流式回复（含代码块），同时在输入框打字
   - 期望：输入即时响应、不卡顿
   - 期望：streaming 时代码块灰色等宽、done 后变彩色
5. 跨对话：一个对话产生 20+ 条消息后切到另一对话再切回
   - 期望：切换不卡，React DevTools Profiler 验证历史 AiBubble 不重渲

## 后续（PR 2 预览，不在本 spec 实现）

"一屏一 turn"聚焦布局：
- 拆 ChatArea 自动滚底（300ms interval、ResizeObserver、`messages.length` useEffect、"回到底部"按钮语义改为"跳到当前 turn"）
- `RenderTurn` 加稳定 `turnId`
- `MessageList` 每个 turn 加 `data-turn-id` + 稳定 key
- 最后一个 turn 加 `min-h: calc(scrollContainer.clientHeight - userMsgHeight)`
- 提交后 `scrollContainer.scrollTo({ top: userMsgEl.offsetTop, behavior: 'smooth' })`
- `StreamingBubble` 移到当前 turn 容器内
