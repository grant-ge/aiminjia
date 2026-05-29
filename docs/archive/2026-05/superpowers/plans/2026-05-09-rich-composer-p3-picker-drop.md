# RichComposer P3 — Picker + Drop 直插 Token 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 file picker 和 drop inbox 的附件直接进入 `RichComposer` 的 editor（作为 attachmentToken inserted at selection），替换原 `pendingFiles` 队列消费方式。本期仍**不接入页面**（HomeTaskComposerCard / ChatBottomArea 是 P5），只实现 hooks + 集成测试。

**Architecture:** 加一个 `useComposerDropInbox(handleRef)` hook —— 订阅 `useDropInbox` store，当 `pending` 非空且 composer ref 可用时调用 `handleRef.current?.insertAttachmentTokens(...)`。Picker 路径：导出一个 `pickAttachmentsToComposer(handleRef)` helper，包装现有 `useChatAttachments.pickAttachments()` + token 转换。`PendingAttachment` 与 `ComposerAttachmentToken` 字段几乎完全同构，加一个 `pendingAttachmentToToken()` 转换函数。

**Tech Stack:** React 19、`useDropInbox` (zustand)、`useChatAttachments`（Tauri picker）、P2 的 `RichComposer` ref 与 `insertAttachmentTokens`。spec：`docs/superpowers/specs/2026-05-07-rich-composer-tiptap-design.md`。

---

## 文件结构

新增：
- `src/components/rich-composer/pendingAttachmentToToken.ts` — 纯函数转换器。
- `src/components/rich-composer/useComposerDropInbox.ts` — drop inbox subscriber hook。
- `src/components/rich-composer/__tests__/pendingAttachmentToToken.test.ts` — 转换器单测。
- `src/components/rich-composer/__tests__/useComposerDropInbox.test.tsx` — drop hook 集成测试。

修改：
- `src/components/rich-composer/index.ts` — re-export 上述两个新模块。

不修改：
- `src/stores/dropInbox.ts`（保持不动；本期只换 consumer 方）。
- `src/hooks/useChatAttachments.ts`（不动；picker 仍由该 hook 提供，只做适配）。
- `src/hooks/useDragDropListener.ts`（不动；它仍然 `push` 到 `dropInbox`）。
- `RichComposer.tsx`（已经在 P2 提供 ref 接口）。
- 任何页面文件（HomeTaskComposerCard / ChatBottomArea — P5 才接入）。

## 模块设计

### `pendingAttachmentToToken.ts`

```ts
import type { ComposerAttachmentToken } from './types'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

export function pendingAttachmentToToken(
  attachment: PendingAttachment,
): ComposerAttachmentToken {
  return {
    id: attachment.id,
    fileName: attachment.fileName,
    path: attachment.path,
    kind: attachment.kind,
    fileType: attachment.fileType,
    fileSize: attachment.fileSize,
    mimeType: attachment.mimeType,
    source: attachment.source,
  }
}

export function pendingAttachmentsToTokens(
  attachments: PendingAttachment[],
): ComposerAttachmentToken[] {
  return attachments.map(pendingAttachmentToToken)
}
```

是字段对字段的拷贝，但是把它放在 rich-composer 模块边界处显式声明：**`PendingAttachment` 是 page-side 的存储模型，`ComposerAttachmentToken` 是 editor-side 的 schema**。两个类型字段同构是约定，不是保证。一个独立的转换器让未来其中一个发生 drift 时不会静默错位。

### `useComposerDropInbox.ts`

```ts
import { useEffect } from 'react'
import type { RefObject } from 'react'
import { useDropInbox } from '@/stores/dropInbox'
import { pendingAttachmentsToTokens } from './pendingAttachmentToToken'
import type { RichComposerHandle } from './RichComposer'

export function useComposerDropInbox(
  composerRef: RefObject<RichComposerHandle | null>,
): void {
  const pending = useDropInbox((s) => s.pending)
  const consume = useDropInbox((s) => s.consume)

  useEffect(() => {
    if (pending.length === 0) return
    const handle = composerRef.current
    if (!handle) return
    const taken = consume()
    if (taken.length === 0) return
    handle.insertAttachmentTokens(pendingAttachmentsToTokens(taken))
  }, [pending, consume, composerRef])
}
```

这里关键点：
- `consume()` 是 store 的 self-clearing read（已验证现有代码就这样）。
- 我们订阅 `pending` 数组（zustand selector）只是为了在它从空变非空时触发 effect rerun。
- 如果 ref 不可用（composer 还没 mount 完），effect 早返回；下一次 store 变更（或 ref 就绪后的 rerender）会重新尝试。**潜在风险**：ref 永远不可用时 dropInbox 永远不会被 consume — 但这是页面层的 bug（页面没正确 mount RichComposer），不是 hook 的问题。

为防御 corner case（ref 在第一次 render 时还未 attach，但 pending 已有数据）：用一个轻量轮询？**不**——React 在 ref 挂载完成后会触发 effect 重跑，前提是 effect 的依赖数组包含 ref 的 `current`。但 `RefObject.current` 不会触发 rerender。可接受的 trade-off：consumer 本身要确保 RichComposer 在 dropInbox 推送之前 mount，或在 useDragDropListener 里把推送时机推迟到下一帧。当前 `useDragDropListener` 已经是异步路径（path resolution 是 await），实际场景 ref 一定就绪。**这条 trade-off 在 hook 文件顶部用 1-2 行注释解释**。

### Picker 集成

不写一个新的 picker hook —— 现有 `useChatAttachments.pickAttachments()` 已经返回 `PendingAttachment[]`。在页面接入时（P5）页面会写：

```ts
const pending = await pickAttachments()
composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(pending))
```

不需要 P3 写包装。**只导出 `pendingAttachmentsToTokens`** 就够了。

不写 picker 端到端测试：现有 `useChatAttachments` 已有覆盖；page-side 集成留给 P5。

## 测试策略

### `pendingAttachmentToToken.test.ts`

5 项纯函数测试：
1. file kind 完整字段映射
2. image kind 映射
3. folder kind 映射
4. mimeType 是 undefined 时保留 undefined
5. 数组版本 `pendingAttachmentsToTokens` 顺序保留

### `useComposerDropInbox.test.tsx`

集成测试：用一个 minimal harness 组件 `<TestHarness />` 内含 `<RichComposer>` + 调用 `useComposerDropInbox(ref)`。然后通过 `useDropInbox.getState().push([...])` 模拟 drop，断言 `serializeComposerDoc(editor.getJSON())` 输出包含对应附件 token。

5 项测试：
1. push 单 attachment → editor 中出现 attachmentToken；serializer payload 含 attachment
2. push 多个 attachment → 全部插入，order 正确
3. push 之后再 push → 第二批继续插入（不丢）
4. ref.current 是 null 时不抛错（store 保留 pending，等 ref 就绪）
5. push 空数组 → noop（store 内部已有保护，hook 不访问 ref）

测试中需要直接读写 zustand store —— `useDropInbox.setState` / `useDropInbox.getState`。

## 风险与对策

- **ref-not-attached race**：上面注释解释；不写防御代码（YAGNI）。
- **重复 consume**：`useDropInbox.consume()` 是 self-clearing；多个 consumer 实例同时挂载（理论上：HomePage + ChatPage 都同时 mount），先到先得。本期不解决（页面层只有一个实例 mount，因为 routing 互斥）。在 hook 注释里提一下。
- **`useDropInbox` 测试的隔离性**：每个测试要 `useDropInbox.setState({ pending: [] })` 重置 store。

---

### Task 1: pendingAttachmentToToken 转换器

**Files:**
- Create: `src/components/rich-composer/pendingAttachmentToToken.ts`
- Create: `src/components/rich-composer/__tests__/pendingAttachmentToToken.test.ts`

- [ ] **Step 1: 写失败测试**

```ts
// src/components/rich-composer/__tests__/pendingAttachmentToToken.test.ts
import { describe, expect, it } from 'vitest'
import {
  pendingAttachmentToToken,
  pendingAttachmentsToTokens,
} from '../pendingAttachmentToToken'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

const mkPending = (overrides: Partial<PendingAttachment> = {}): PendingAttachment => ({
  id: '/abs/plan.pdf',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 1024,
  mimeType: undefined,
  source: 'picker',
  ...overrides,
})

describe('pendingAttachmentToToken', () => {
  it('file kind 完整字段映射', () => {
    const pending = mkPending()
    const token = pendingAttachmentToToken(pending)
    expect(token).toEqual({
      id: '/abs/plan.pdf',
      fileName: 'plan.pdf',
      path: '/abs/plan.pdf',
      kind: 'file',
      fileType: 'pdf',
      fileSize: 1024,
      mimeType: undefined,
      source: 'picker',
    })
  })

  it('image kind 映射', () => {
    const token = pendingAttachmentToToken(
      mkPending({ kind: 'image', fileType: 'image', source: 'clipboard-image' }),
    )
    expect(token.kind).toBe('image')
    expect(token.fileType).toBe('image')
    expect(token.source).toBe('clipboard-image')
  })

  it('folder kind 映射', () => {
    const token = pendingAttachmentToToken(
      mkPending({ kind: 'folder', fileType: 'folder', source: 'drop' }),
    )
    expect(token.kind).toBe('folder')
    expect(token.fileType).toBe('folder')
    expect(token.source).toBe('drop')
  })

  it('mimeType present', () => {
    const token = pendingAttachmentToToken(mkPending({ mimeType: 'application/pdf' }))
    expect(token.mimeType).toBe('application/pdf')
  })

  it('pendingAttachmentsToTokens 顺序保留', () => {
    const list = [
      mkPending({ id: 'a' }),
      mkPending({ id: 'b' }),
      mkPending({ id: 'c' }),
    ]
    const tokens = pendingAttachmentsToTokens(list)
    expect(tokens.map((t) => t.id)).toEqual(['a', 'b', 'c'])
  })
})
```

- [ ] **Step 2: 验证测试失败**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/pendingAttachmentToToken.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: 实现 pendingAttachmentToToken.ts**

```ts
// src/components/rich-composer/pendingAttachmentToToken.ts
import type { ComposerAttachmentToken } from './types'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

export function pendingAttachmentToToken(
  attachment: PendingAttachment,
): ComposerAttachmentToken {
  return {
    id: attachment.id,
    fileName: attachment.fileName,
    path: attachment.path,
    kind: attachment.kind,
    fileType: attachment.fileType,
    fileSize: attachment.fileSize,
    mimeType: attachment.mimeType,
    source: attachment.source,
  }
}

export function pendingAttachmentsToTokens(
  attachments: PendingAttachment[],
): ComposerAttachmentToken[] {
  return attachments.map(pendingAttachmentToToken)
}
```

- [ ] **Step 4: 验证测试通过**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/pendingAttachmentToToken.test.ts`
Expected: PASS — 5 tests.

- [ ] **Step 5: 提交**

```bash
git add src/components/rich-composer/pendingAttachmentToToken.ts src/components/rich-composer/__tests__/pendingAttachmentToToken.test.ts
git commit -m "feat(rich-composer): pendingAttachmentToToken converter (PendingAttachment → ComposerAttachmentToken)"
```

---

### Task 2: useComposerDropInbox hook

**Files:**
- Create: `src/components/rich-composer/useComposerDropInbox.ts`
- Create: `src/components/rich-composer/__tests__/useComposerDropInbox.test.tsx`

- [ ] **Step 1: 写失败测试**

```tsx
// src/components/rich-composer/__tests__/useComposerDropInbox.test.tsx
import '@testing-library/jest-dom'
import { useRef, useEffect } from 'react'
import { describe, expect, it, beforeEach } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { vi } from 'vitest'
import { RichComposer } from '../RichComposer'
import type { RichComposerHandle } from '../RichComposer'
import { useComposerDropInbox } from '../useComposerDropInbox'
import { useDropInbox } from '@/stores/dropInbox'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

// Same NodeView mock as the RichComposer test file — ReactNodeViewRenderer fights jsdom.
vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return {
    ...mod,
    ReactNodeViewRenderer: () => () => ({}),
  }
})

const mkPending = (overrides: Partial<PendingAttachment> = {}): PendingAttachment => ({
  id: '/abs/plan.pdf',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 1024,
  mimeType: undefined,
  source: 'drop',
  ...overrides,
})

interface HarnessProps {
  onSubmit?: (markdown: string) => void
  onReady?: () => void
}

function Harness({ onSubmit, onReady }: HarnessProps) {
  const ref = useRef<RichComposerHandle>(null)
  useComposerDropInbox(ref)
  useEffect(() => {
    onReady?.()
  }, [onReady])
  return (
    <RichComposer
      ref={ref}
      onSubmit={(payload) => onSubmit?.(payload.markdown)}
    />
  )
}

beforeEach(() => {
  useDropInbox.setState({ pending: [] })
})

describe('useComposerDropInbox', () => {
  it('push single attachment → editor inserts attachmentToken', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([mkPending()])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('plan.pdf')
    })
    expect(useDropInbox.getState().pending).toEqual([])
    unmount()
  })

  it('push multiple attachments → all inserted in order', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([
      mkPending({ id: 'a', fileName: 'a.pdf', path: '/p/a.pdf' }),
      mkPending({ id: 'b', fileName: 'b.pdf', path: '/p/b.pdf' }),
      mkPending({ id: 'c', fileName: 'c.pdf', path: '/p/c.pdf' }),
    ])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('a.pdf')
      expect(html).toContain('b.pdf')
      expect(html).toContain('c.pdf')
    })
    // verify order via data-id appearance
    const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
    const idxA = html.indexOf('data-id="a"')
    const idxB = html.indexOf('data-id="b"')
    const idxC = html.indexOf('data-id="c"')
    expect(idxA).toBeGreaterThan(-1)
    expect(idxA).toBeLessThan(idxB)
    expect(idxB).toBeLessThan(idxC)
    unmount()
  })

  it('push twice → second batch appended, not lost', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([mkPending({ id: 'first', fileName: 'first.pdf', path: '/p/first.pdf' })])
    await waitFor(() => {
      expect(document.querySelector('.ProseMirror')?.innerHTML).toContain('first.pdf')
    })
    useDropInbox.getState().push([mkPending({ id: 'second', fileName: 'second.pdf', path: '/p/second.pdf' })])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('first.pdf')
      expect(html).toContain('second.pdf')
    })
    unmount()
  })

  it('push empty array → noop, store stays empty', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([])
    expect(useDropInbox.getState().pending).toEqual([])
    expect(document.querySelector('.ProseMirror')?.innerHTML).not.toContain('plan.pdf')
    unmount()
  })

  it('store is reset between tests (no leak)', () => {
    expect(useDropInbox.getState().pending).toEqual([])
  })
})
```

- [ ] **Step 2: 验证测试失败**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/useComposerDropInbox.test.tsx`
Expected: FAIL — `Failed to resolve "../useComposerDropInbox"`.

- [ ] **Step 3: 实现 useComposerDropInbox.ts**

```ts
// src/components/rich-composer/useComposerDropInbox.ts
import { useEffect } from 'react'
import type { RefObject } from 'react'
import { useDropInbox } from '@/stores/dropInbox'
import { pendingAttachmentsToTokens } from './pendingAttachmentToToken'
import type { RichComposerHandle } from './RichComposer'

/**
 * Drains the global drop-inbox into the composer pointed to by `composerRef`.
 *
 * Trade-off: if the ref's `.current` is null when `pending` arrives (e.g. the
 * composer hasn't finished mounting yet), this effect early-returns and leaves
 * `pending` untouched in the store. The next store change (or the next
 * rerender that includes pending items) will retry. In practice the native
 * drag-drop listener resolves paths asynchronously, so the composer is
 * always mounted by the time `pending` arrives — this is just defensive.
 *
 * Multiple consumers: if two RichComposer instances mount simultaneously,
 * whichever effect fires first wins (consume() is self-clearing). The Home /
 * Chat pages route-mutex this, so it doesn't happen in practice.
 */
export function useComposerDropInbox(
  composerRef: RefObject<RichComposerHandle | null>,
): void {
  const pending = useDropInbox((s) => s.pending)
  const consume = useDropInbox((s) => s.consume)

  useEffect(() => {
    if (pending.length === 0) return
    const handle = composerRef.current
    if (!handle) return
    const taken = consume()
    if (taken.length === 0) return
    handle.insertAttachmentTokens(pendingAttachmentsToTokens(taken))
  }, [pending, consume, composerRef])
}
```

- [ ] **Step 4: 验证测试通过**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/useComposerDropInbox.test.tsx`
Expected: PASS — 5 tests.

#### 常见失败诊断

- "push single attachment → editor inserts attachmentToken" 失败：检查 mock for `ReactNodeViewRenderer` 是否复用了 P2 的方式。如果 NodeView 抛错，整个 editor.view 损坏。
- "push multiple → all inserted in order" 失败：检查 `pendingAttachmentsToTokens` 是 array.map（保序）。
- "push twice" 失败：第二次 push 没触发 effect。检查 `useDropInbox((s) => s.pending)` selector 是否引用相等（push 用 `[...s.pending, ...new]` spread，应该返回新数组）。

- [ ] **Step 5: 验证全部 rich-composer 测试**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: PASS — 78 (existing) + 5 (Task 1) + 5 (Task 2) = 88 tests.

- [ ] **Step 6: tsc**

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0.

- [ ] **Step 7: 提交**

```bash
git add src/components/rich-composer/useComposerDropInbox.ts src/components/rich-composer/__tests__/useComposerDropInbox.test.tsx
git commit -m "feat(rich-composer): useComposerDropInbox drains drop inbox into editor as attachment tokens"
```

---

### Task 3: index re-export + 验证

**Files:**
- Modify: `src/components/rich-composer/index.ts`

- [ ] **Step 1: 更新 index.ts**

```ts
// src/components/rich-composer/index.ts
export * from './types'
export { serializeComposerDoc } from './serializer'
export { AttachmentTokenExtension } from './attachmentTokenExtension'
export { AttachmentTokenView } from './AttachmentTokenView'
export { buildComposerExtensions } from './composerSchema'
export type { BuildComposerExtensionsOptions } from './composerSchema'
export { parseMarkdownToComposerJson } from './parseMarkdown'
export { RichComposer } from './RichComposer'
export type {
  RichComposerProps,
  RichComposerHandle,
  ComposerSkillCommand,
} from './RichComposer'
export {
  pendingAttachmentToToken,
  pendingAttachmentsToTokens,
} from './pendingAttachmentToToken'
export { useComposerDropInbox } from './useComposerDropInbox'
```

- [ ] **Step 2: 全量验证**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: PASS — 88 tests.

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0.

Run: `pnpm lint 2>&1 | grep -E "rich-composer"`
Expected: 0 errors / warnings within rich-composer scope.

- [ ] **Step 3: 提交**

```bash
git add src/components/rich-composer/index.ts
git commit -m "chore(rich-composer): export pendingAttachmentToToken + useComposerDropInbox"
```

---

## Self-Review

**1. Spec coverage（spec 的 "粘贴与附件管线 / Picker 和 Drop"）：**

- ✅ picker 选中文件后，由页面调 `composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(pending))` → Task 1 提供转换器；P5 接入页面。
- ✅ drop inbox 被消费后，直接插入当前 editor selection → Task 2 hook。
- ✅ 保留 `useDragDropListener` + `dropInbox` 的全局接收模型，只改 consumer → 我们没动 store / listener；只新增了 hook。
- ⚠️ 不做：editor 未 focus 时插入"文档末尾"——`insertAttachmentTokens` 命令默认就是在当前 selection 后插入；如果 editor 未 focus，selection 默认在文档末尾。这是 Tiptap 行为，spec 已隐含。无需特殊代码。
- ⚠️ 不做：粘贴管线（P4）；HTML 富文本粘贴（P4）；图片 blob 粘贴（P4）；混合粘贴（P4）。

**2. Placeholder scan：** 全部 step 含完整代码。无 TBD / TODO / "implement later"。commit messages 全部具体。

**3. Type consistency：**
- `PendingAttachment`（来自 `@/hooks/useChatAttachments`）与 `ComposerAttachmentToken`（rich-composer/types.ts）字段同构，`pendingAttachmentToToken` 显式声明这个映射边界。
- `RichComposerHandle.insertAttachmentTokens(tokens: ComposerAttachmentToken[])` 来自 P2，hook 内部一致使用。
- `useDropInbox` API（`pending` selector + `push` + `consume`）来自现有 store，没改。

**4. 范围：** 严格 P3。RichComposer.tsx 不动。页面文件不动。Paste pipeline 留给 P4。
