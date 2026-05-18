# Chat Streaming Perf PR1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除长对话 LLM 流式输出期间输入框卡顿 —— 让 `AssistantMarkdown` 在 streaming 时关闭 `rehype-highlight` 高亮，并让历史 `AiBubble` 通过 `React.memo` 锁住，避免每次 `messages` 引用变更全量重渲。

**Architecture:** 三处独立、TDD、可分别 ship 的小改动。① `AssistantMarkdown` 加 `disableCodeHighlight` 开关并把 plugin 数组提到模块作用域常量。② `StreamingBubble` 在 streaming 期间传 `disableCodeHighlight` —— 关掉每帧重新 tokenize 全段 markdown 的开销。③ `AiBubble` 套 `React.memo` —— 由于 `useTurnRenderModel` 透传 store 原始 `Message` 引用，历史消息浅比较稳定，memo 后切对话 / 新增 message 时历史不重渲。**不动**数据流、不引依赖、不改交互。

**Tech Stack:** React 18, TypeScript, Vitest, react-markdown / rehype-highlight, Zustand（仅引用，不改）。

**Spec:** [docs/superpowers/specs/2026-05-15-chat-streaming-perf-design.md](../specs/2026-05-15-chat-streaming-perf-design.md)

---

## File Structure

- **修改** `src/components/chat-scene/AssistantMarkdown.tsx` — 加 `disableCodeHighlight?: boolean` prop；plugin 数组提到模块作用域常量
- **修改** `src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx` — 新增 2 个 case 覆盖 prop 行为
- **修改** `src/components/chat/StreamingBubble.tsx` — `<AssistantMarkdown disableCodeHighlight />`
- **修改** `src/components/chat/StreamingBubble.test.tsx` — 新增 1 个 case 验证 streaming 不开高亮
- **修改** `src/components/chat/AiBubble.tsx` — 改为 `React.memo(function AiBubble(...) {...})`
- **新建** `src/components/chat/__tests__/AiBubble.memo.test.tsx` — 用 render-count spy 验证 memo

注：`src/components/chat-scene/markdown/__tests__/AssistantMarkdown.test.tsx` 是另一个**已存在**的 markdown 子模块测试，与本次改的 `chat-scene/__tests__/AssistantMarkdown.test.tsx` 不是同一份；本计划只动后者。

---

## Task 1: `AssistantMarkdown` 加 `disableCodeHighlight` prop

**Files:**
- Modify: `src/components/chat-scene/AssistantMarkdown.tsx`
- Modify: `src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx`

**关键点：**
- `rehypePlugins` 数组从每次渲染都新建 → 提到模块作用域常量（稳定引用，避免 react-markdown 内部依赖追踪误判）。
- 默认 `disableCodeHighlight = false`（保持现有所有 callsite 行为不变）。
- 关闭高亮时传**空数组** `[]`，react-markdown 支持空数组，不会报错（spec §风险 已确认）。
- `markdownComponents` 不依赖 `hljs-*` className，关高亮后视觉等宽灰色 `<code>` 正常。

- [ ] **Step 1: 写失败测试 —— `disableCodeHighlight=true` 不注入 hljs class**

把下面两个新 case **追加**到 `src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx` 已有 `describe('AssistantMarkdown', () => { ... })` 块内（保留现有 2 个 case 不动）：

```tsx
  it('disableCodeHighlight=true → 不注入 hljs-* className', () => {
    const { container } = render(
      <AssistantMarkdown text={'```ts\nconst x = 1\n```'} disableCodeHighlight />,
    )
    const code = container.querySelector('pre code')
    expect(code).not.toBeNull()
    expect(code?.className ?? '').not.toMatch(/hljs/)
  })

  it('默认开启高亮（注入 hljs-* 或 language-* className）', () => {
    const { container } = render(
      <AssistantMarkdown text={'```ts\nconst x = 1\n```'} />,
    )
    const code = container.querySelector('pre code')
    expect(code).not.toBeNull()
    expect(code?.className ?? '').toMatch(/hljs|language-ts/)
  })
```

- [ ] **Step 2: 运行测试，确认两个新 case 失败**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx`

Expected: 原 2 个 case 通过；新增"disableCodeHighlight=true → 不注入 hljs-* className"失败（当前实现强开高亮）。新增"默认开启高亮"应当通过（与现状一致），如果失败说明本地 hljs 未识别 `ts` —— 把断言里 `/hljs|language-ts/` 放宽到 `/hljs|language-/` 即可（`react-markdown` 对 fenced code 默认会加 `language-*` className）。

- [ ] **Step 3: 实现 `disableCodeHighlight` prop**

把 `src/components/chat-scene/AssistantMarkdown.tsx` **完整替换**为：

```tsx
import ReactMarkdown from 'react-markdown'
import rehypeHighlight from 'rehype-highlight'
import remarkGfm from 'remark-gfm'
import { markdownComponents } from './markdown/markdownComponents'

interface AssistantMarkdownProps {
  text: string
  /**
   * Disable rehype-highlight (syntax highlighting).
   *
   * Why: streaming 时每帧把累积全段 markdown 重新 token 化是主要卡顿源。
   * 关闭后代码块仍以等宽灰色 <code> 渲染，done 后由 AiBubble 接管再开高亮。
   */
  disableCodeHighlight?: boolean
}

const REMARK_PLUGINS = [remarkGfm]
const REHYPE_PLUGINS_WITH_HIGHLIGHT: Parameters<typeof ReactMarkdown>[0]['rehypePlugins'] = [
  [rehypeHighlight, { detect: true }],
]
const REHYPE_PLUGINS_NO_HIGHLIGHT: Parameters<typeof ReactMarkdown>[0]['rehypePlugins'] = []

export function AssistantMarkdown({ text, disableCodeHighlight = false }: AssistantMarkdownProps) {
  if (!text.trim()) return null

  return (
    <div className="assistant-markdown text-sm leading-7">
      <ReactMarkdown
        remarkPlugins={REMARK_PLUGINS}
        rehypePlugins={
          disableCodeHighlight ? REHYPE_PLUGINS_NO_HIGHLIGHT : REHYPE_PLUGINS_WITH_HIGHLIGHT
        }
        skipHtml
        components={markdownComponents}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}
```

注：上面 `Parameters<typeof ReactMarkdown>[0]['rehypePlugins']` 拿的是 react-markdown 的 `rehypePlugins` 真实类型，避免硬编码 `unknown[]` / `any[]`。如果你那边 TypeScript 推断报错（react-markdown 早期版本类型不完整），fallback 写法是 `import type { PluggableList } from 'unified'` 然后用 `PluggableList` 标常量类型。

- [ ] **Step 4: 运行测试，确认全部通过**

Run: `pnpm exec vitest run src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx`

Expected: 4 个 case 全部 PASS。

- [ ] **Step 5: 类型检查**

Run: `pnpm tsc --noEmit`

Expected: 0 errors。如果 `Parameters<...>` 写法报错，按 Step 3 备注换成 `PluggableList`。

- [ ] **Step 6: 提交**

```bash
git add src/components/chat-scene/AssistantMarkdown.tsx \
        src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx
git commit -m "feat(chat): AssistantMarkdown 支持 disableCodeHighlight 关闭语法高亮"
```

---

## Task 2: `StreamingBubble` 关闭流式高亮

**Files:**
- Modify: `src/components/chat/StreamingBubble.tsx:45`
- Modify: `src/components/chat/StreamingBubble.test.tsx`

**关键点：** 仅在 streaming 期间关高亮。stream:done 后 `StreamingBubble` 卸载，最后一段由 `AiBubble`（默认开高亮）接管渲染，所以用户看到的最终态仍带彩色高亮。

- [ ] **Step 1: 写失败测试 —— streaming 内容代码块不含 hljs class**

把下面这个新 case **追加**到 `src/components/chat/StreamingBubble.test.tsx` 已有 `describe('StreamingBubble', () => { ... })` 块内（保留 `beforeEach` 和现有 2 个 case 不动）：

```tsx
  it('streaming 内容的代码块不含 hljs 高亮 className', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '```ts\nlet a = 1\n```',
          toolExecutions: [],
        },
      },
    })

    const { container } = render(
      <StreamingBubble content={'```ts\nlet a = 1\n```'} />,
    )
    const code = container.querySelector('pre code')
    expect(code).not.toBeNull()
    expect(code?.className ?? '').not.toMatch(/hljs/)
  })
```

- [ ] **Step 2: 运行测试，确认新 case 失败**

Run: `pnpm exec vitest run src/components/chat/StreamingBubble.test.tsx`

Expected: 原 2 个 case PASS；新 case FAIL（当前 StreamingBubble 调用 `<AssistantMarkdown>` 不带 `disableCodeHighlight`，仍会注入 `hljs-*`）。

- [ ] **Step 3: 实现 —— 传 `disableCodeHighlight`**

编辑 `src/components/chat/StreamingBubble.tsx`，把第 45 行：

```tsx
          <AssistantMarkdown text={cleanContent} />
```

改为：

```tsx
          <AssistantMarkdown text={cleanContent} disableCodeHighlight />
```

不动其它任何代码。

- [ ] **Step 4: 运行测试，确认全部通过**

Run: `pnpm exec vitest run src/components/chat/StreamingBubble.test.tsx`

Expected: 3 个 case 全部 PASS。

- [ ] **Step 5: 类型检查**

Run: `pnpm tsc --noEmit`

Expected: 0 errors。

- [ ] **Step 6: 提交**

```bash
git add src/components/chat/StreamingBubble.tsx src/components/chat/StreamingBubble.test.tsx
git commit -m "perf(chat): streaming 期间关闭代码高亮，避免每帧重新 tokenize"
```

---

## Task 3: `AiBubble` 套 `React.memo`

**Files:**
- Modify: `src/components/chat/AiBubble.tsx:30`
- Create: `src/components/chat/__tests__/AiBubble.memo.test.tsx`

**关键点：**
- `useTurnRenderModel.ts:338` 把 store 原始 `Message` 引用透传到 `aiSegments[].message`，所以同一 message 多次渲染时引用稳定 → 默认浅比较即可命中 memo。
- `isStreaming` prop 是布尔，浅比较自然 OK。
- 用 mock `AssistantMarkdown` 注入 render spy 验证 —— 比直接 mock React 内部更稳。
- mock `chatStore` 的写法参照已有 `src/components/chat/AiBubble.subagent.test.tsx:14-19`（`AiBubble` 自身不订阅 store，但 `SubAgentResultCard` 等子组件可能订阅，所以仍要 mock）。

- [ ] **Step 1: 创建新测试文件**

**新建** `src/components/chat/__tests__/AiBubble.memo.test.tsx`：

```tsx
import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import type { Message } from '@/types/message'

const renderSpy = vi.fn()

vi.mock('@/components/chat-scene/AssistantMarkdown', () => ({
  AssistantMarkdown: (p: { text: string }) => {
    renderSpy(p.text)
    return <div data-testid="md-stub">{p.text}</div>
  },
}))

vi.mock('@/lib/tauri', () => ({
  sendMessage: vi.fn(),
  openGeneratedFile: vi.fn(),
  revealFileInFolder: vi.fn(),
  getSubagentTranscript: vi.fn(),
}))

vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn(
    (selector: (state: { activeConversationId: string | null }) => unknown) =>
      selector({ activeConversationId: 'conv-1' }),
  ),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: {
    getState: () => ({ push: vi.fn() }),
  },
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (
      key: string,
      fallbackOrOptions?: string | { defaultValue?: string },
    ) => {
      if (typeof fallbackOrOptions === 'string') return fallbackOrOptions
      return fallbackOrOptions?.defaultValue ?? key
    },
  }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}))

import { AiBubble } from '../AiBubble'

function makeMsg(id: string, text: string): Message {
  return {
    id,
    conversationId: 'conv-1',
    role: 'assistant',
    createdAt: '2026-05-15T00:00:00Z',
    content: { text },
  }
}

describe('AiBubble — React.memo', () => {
  it('相同 message 引用 + 相同 isStreaming → 不重渲', () => {
    renderSpy.mockClear()
    const msg = makeMsg('m1', 'hello')

    const { rerender } = render(<AiBubble message={msg} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)

    rerender(<AiBubble message={msg} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)
  })

  it('不同 message 对象引用 → 重渲', () => {
    renderSpy.mockClear()
    const m1 = makeMsg('m1', 'hello')
    const m2 = makeMsg('m1', 'hello') // 内容相同但是新对象

    const { rerender } = render(<AiBubble message={m1} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)

    rerender(<AiBubble message={m2} />)
    expect(renderSpy).toHaveBeenCalledTimes(2)
  })

  it('isStreaming 变化 → 重渲', () => {
    renderSpy.mockClear()
    const msg = makeMsg('m1', 'hello')

    const { rerender } = render(<AiBubble message={msg} isStreaming={false} />)
    expect(renderSpy).toHaveBeenCalledTimes(1)

    rerender(<AiBubble message={msg} isStreaming={true} />)
    expect(renderSpy).toHaveBeenCalledTimes(2)
  })
})
```

- [ ] **Step 2: 运行测试，确认"相同引用不重渲"失败**

Run: `pnpm exec vitest run src/components/chat/__tests__/AiBubble.memo.test.tsx`

Expected:
- "相同 message 引用 + 相同 isStreaming → 不重渲" FAIL（当前 `AiBubble` 是普通函数组件，rerender 时一定重渲，`renderSpy` 会被叫到 2 次）
- "不同 message 对象引用 → 重渲" PASS
- "isStreaming 变化 → 重渲" PASS

- [ ] **Step 3: 给 `AiBubble` 套 `React.memo`**

编辑 `src/components/chat/AiBubble.tsx`：

① 顶部 import 加上 `memo`：

```tsx
import { memo } from 'react'
```

② 第 30 行函数声明 + 末尾导出改为：把 `export function AiBubble(...)` 改成内部 `function AiBubbleImpl(...)`，并在文件末尾导出 memo 包装。具体把：

```tsx
export function AiBubble({ message, isStreaming }: AiBubbleProps) {
```

改为：

```tsx
function AiBubbleImpl({ message, isStreaming }: AiBubbleProps) {
```

然后在闭合 `}` 的下一行（即原 `ContentRenderer` 定义之前）插入：

```tsx
export const AiBubble = memo(AiBubbleImpl)
```

`ContentRenderer` 子组件不动 —— memo 浅比较是在 `AiBubble` 外层比较 props，子组件每次正常重渲不影响（且只在 memo 没命中时才会执行到子组件）。

- [ ] **Step 4: 运行 memo 测试，确认全部通过**

Run: `pnpm exec vitest run src/components/chat/__tests__/AiBubble.memo.test.tsx`

Expected: 3 个 case 全部 PASS。

- [ ] **Step 5: 跑现有 `AiBubble.subagent.test.tsx` 回归**

Run: `pnpm exec vitest run src/components/chat/AiBubble.subagent.test.tsx`

Expected: 现有 case 全部 PASS（memo 不影响首次渲染语义，已有 case 都是 `render(...)` 单次渲染，浅比较与否不影响结果）。如果失败，最可能的原因是 export 改名后引入侧效应 —— 检查是否多写或漏写了 `memo` 的 import / export 行。

- [ ] **Step 6: 类型检查**

Run: `pnpm tsc --noEmit`

Expected: 0 errors。

- [ ] **Step 7: 提交**

```bash
git add src/components/chat/AiBubble.tsx src/components/chat/__tests__/AiBubble.memo.test.tsx
git commit -m "perf(chat): AiBubble 套 React.memo，避免长对话历史消息重渲"
```

---

## Task 4: 全量回归 + 手测

**Files:** （无代码改动）

- [ ] **Step 1: 单元测试全量回归**

Run: `pnpm exec vitest run src/components/chat src/components/chat-scene`

Expected: 全部 PASS。如有失败，先看是不是上面 3 个 task 之外的测试受影响 —— `AssistantMarkdown` / `StreamingBubble` / `AiBubble` 三处改动接口都是**严格扩展**（新增可选 prop / memo 包装），不应破坏现有 case。

- [ ] **Step 2: 仓库级单测**

Run: `pnpm test -- --run`

Expected: 全部 PASS。

注：`pnpm test` 在仓库内默认是 `vitest`（watch 模式），加 `-- --run` 走 CI 单次执行。如果你的 `package.json` `test` script 已经写死 `vitest run`，直接 `pnpm test` 即可。

- [ ] **Step 3: TypeScript 全量类型检查**

Run: `pnpm tsc --noEmit`

Expected: 0 errors。

- [ ] **Step 4: 手测 —— 长流式 + 输入框不卡**

启动 dev：

```bash
pnpm tauri:dev
```

进入任意工作目录，发一条会让 LLM 输出至少 30s 长回复的提问（例如"分步骤详细解释 Rust async runtime 的工作原理，并给出代码示例"，要确保回复里包含至少一个 ``` 围栏代码块）。

LLM 流式输出过程中：
- ✅ 期望：底部输入框打字即时响应、光标不卡顿
- ✅ 期望：流式期间代码块呈**灰色等宽**（无彩色高亮）
- ✅ 期望：stream 结束瞬间，最后一段代码块切换为**彩色高亮**（`AiBubble` 接管渲染并开高亮）

如果流式期间仍卡顿：打开 React DevTools Profiler 录一段，确认 `AiBubble` 历史条目是否还在重渲。如果还在重渲，看 `useTurnRenderModel` 返回的 `aiSegments[].message` 引用是否每次都变 —— 临时在 `AiBubble.tsx` 第一行加 `console.log('AiBubble render', message.id, message)`（**不要提交**），切对话观察。spec §风险表对应这一项有应急方案。

- [ ] **Step 5: 手测 —— 跨对话切换不重渲历史**

在一个对话产生 20+ 条 AI 消息后，切到另一个对话，再切回。

- ✅ 期望：切换流畅、无明显卡顿
- ✅ 期望：React DevTools Profiler 录一次切换，`AiBubble` 历史条目的 "Why did this render?" 显示 "Did not render"（memo 命中）

- [ ] **Step 6: 留 PR**

```bash
# 已经在 worktree 分支上，commits 都已落好
git log --oneline -4
```

Expected: 看到 Task 1 / Task 2 / Task 3 的 3 个 commit（顺序由实际执行决定）。然后按需通过 `commit-commands:commit-push-pr` 或手工开 PR。PR 描述用 spec 的"目标 + 三步独立改动 + 不做清单"做 summary，引用 [docs/superpowers/specs/2026-05-15-chat-streaming-perf-design.md](../specs/2026-05-15-chat-streaming-perf-design.md)。

---

## 不做（来自 spec §不做，提醒执行者别越界）

- ❌ 不加额外节流（`useStreaming` 现有 rAF 节流足够）
- ❌ 不动 streamingStore / `useStreaming` 数据流
- ❌ 不引 incremark / streamdown / shiki
- ❌ 不动滚动行为（spec §后续 已规划到 PR 2）
- ❌ 不 memo `ContentRenderer`（`AiBubble` memo 后冗余）
- ❌ 不动 `FilePreviewPane` / `TeamChatEvents` / `TeammateDetailPanel` 里的 `AssistantMarkdown` 调用（它们不在 streaming 热路径上，保持默认开高亮）

---

## 风险与应急（来自 spec §风险）

| 风险 | 应急 |
|---|---|
| `useTurnRenderModel` 实际返回 message 引用不稳定，memo 命不中 | Task 4 Step 4 手测时临时 `console.log` 观察；若引用每次变，本 PR memo 失效但**无 bug**（退化到现状），后续在 `useTurnRenderModel` 内修引用稳定性 |
| `rehypePlugins={[]}` 引发 react-markdown 报错 | 已确认 react-markdown 支持空数组；`markdownComponents` 不依赖 `hljs-*` className。如果实际报错，把 `REHYPE_PLUGINS_NO_HIGHLIGHT` 改成 `undefined` 走 react-markdown 默认（默认就是没 rehype plugin） |
| `Parameters<typeof ReactMarkdown>[0]['rehypePlugins']` 类型推断失败 | 改用 `import type { PluggableList } from 'unified'` 后标常量类型 |
