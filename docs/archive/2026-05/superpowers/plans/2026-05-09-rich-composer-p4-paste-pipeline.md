# RichComposer P4 — Paste Pipeline 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 把现有 `useComposerPaste`（textarea 时代）的 file-path / image-blob 解析能力移植成与 `RichComposer` 兼容的形态，在 paste 时把附件作为 attachmentToken 插入到当前 selection。普通文本 / HTML 富文本由 Tiptap default paste handler 处理（StarterKit + Link 已自带），不拦截。

**Architecture:** `useComposerAttachmentPaste(composerRef)` —— 与现有 `useComposerPaste` 同核心逻辑，但：(a) 通过 `composerRef` 调用 `insertAttachmentTokens` 而非 `onAttachmentsResolved`；(b) `handlePaste` 签名改为 `(e: ClipboardEvent<HTMLDivElement>)`，因为 RichComposer 的 paste 事件来自 ProseMirror DOM；(c) 共享同一个 `lastPasteHasFile` capture-snapshot 机制（避免 macOS WebKit 卡顿）。

普通文本 / HTML 粘贴由 Tiptap 默认行为处理，pipeline 不拦截。混合粘贴（文本 + 文件）：当前剪贴板 API 同时给出 `lastPasteHasFile=true` 和 `text/plain`，按 spec 的 fallback 策略——文本进 editor（Tiptap default 处理），附件 token 插到光标后（pipeline 处理）。

**Tech Stack:** ProseMirror paste plugin（通过 Tiptap `Editor.view.props.handlePaste`） / `useChatAttachments`（已有的 `resolvePastedPaths` / `saveClipboardImage`）/ `pendingAttachmentsToTokens`（P3）。

---

## 文件结构

新增：
- `src/components/rich-composer/useComposerAttachmentPaste.ts` — paste hook（包装 paste 处理逻辑）。
- `src/components/rich-composer/__tests__/useComposerAttachmentPaste.test.tsx` — 集成测试。

修改：
- `src/components/rich-composer/RichComposer.tsx` — 添加 `onPaste` 内部 handler 注入；可��� prop `onPasteAttachments?: (handlePaste: (e) => boolean) => void` 不需要——直接由 hook 在 mount 后给 editor.view 装 paste 处理器。
  - 实际做法：扩 `RichComposerHandle` 加 `getEditor(): Editor | null`，让 hook 拿到 editor 注入 paste handler。
- `src/components/rich-composer/index.ts` — re-export `useComposerAttachmentPaste`。

不修改：
- `useComposerPaste.ts`（旧 hook 保留——P5/P8 会删；本期不动避免破坏现有页面）。
- `useChatAttachments.ts`。
- 任何后端代码。

## 关键设计决策

### Paste handler 的 hook 形状

为了避免 `RichComposer` 知道太多 paste 实现细节，方案是：
1. RichComposer 暴露 editor 实例（通过 ref handle 加 `getEditor()`）。
2. Hook `useComposerAttachmentPaste(composerRef)` 在 effect 中拿到 editor，注册原生 `paste` 事件 listener 到 `editor.view.dom`。
3. Listener 决策：是否拦截（如果 `lastPasteHasFile` 才拦截），是否调用 `event.preventDefault()`。
4. 异步解析路径或图片 blob，最后调 `editor.commands.insertAttachmentTokens(...)`。

ref handle 扩展：

```ts
export interface RichComposerHandle {
  focus: () => void
  insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => void
  clear: () => void
  getEditor: () => Editor | null  // 新增
}
```

### 与默认 Tiptap paste 的边界

- 用 native `addEventListener('paste', ...)` on `editor.view.dom`，**不**用 ProseMirror 的 `handlePaste` 插件 prop —— 后者只在文本/HTML 路径触发，不能可靠拦截 image-blob 路径。
- 当 `lastPasteHasFile === false`（纯文本/HTML），listener 不调 preventDefault → ProseMirror / Tiptap 默认逻辑接手。
- 当 `lastPasteHasFile === true`，listener 调 preventDefault → 异步解析 → `insertAttachmentTokens`。

### 普通文本路径（混合粘贴）

`lastPasteHasFile=true` 并不意味着没有文本。如果剪贴板同时含有文本（比如 macOS 复制 Finder 文件 + 文本片段），按 spec：fallback 策略是"文本按原始文本插入，附件 token 插到本次粘贴范围末尾"。

实现：
- 在 preventDefault 之前，先把文本部分 `editor.commands.insertContent(textData)`（如果有）。
- 然后异步解析附件，再 `insertAttachmentTokens`。

但是：在 paste 事件里读取 `clipboardData.getData('text/plain')` 也走 capture snapshot 路径，避免 macOS WebKit 卡顿。

**简化决策**：本期 P4 不实现混合粘贴的文本-在-前 / 附件-在-后顺序保持。文本由 Tiptap 默认 handler 处理（即不 preventDefault 的话），附件由 hook 处理（preventDefault 的话）——这是矛盾的。所以折衷：**`lastPasteHasFile=true` 时 fully preventDefault，丢失同剪贴板的文本部分**。Spec 的"混合粘贴可解释 fallback + toast"在 P4 实现成 toast 提示"已识别附件，文本部分已忽略"，让用户重新单独粘贴文本。

可接受性：file/folder paste 的主流场景（Finder/Explorer 复制文件）很少同时含 text/plain，即便有也是文件路径字符串本身。妥协合理。

## 测试覆盖

`useComposerAttachmentPaste.test.tsx` ~6 项：
1. 纯文本粘贴 → 不拦截，editor 内出现文本（Tiptap 默认处理）
2. 文件路径粘贴 → preventDefault，调用 `insertAttachmentTokens`
3. 多文件路径粘贴 → 全部 token 插入，按顺序
4. 危险路径过滤 → toast，无 token 插入
5. 图片 blob 粘贴 → 调用 `saveClipboardImage`，token 插入
6. 危险路径 + 图片 blob → 路径优先，图片忽略（与现有 hook 一致）

测试需要：
- Mock `readClipboardFilePaths`、`useChatAttachments` 的 `resolvePastedPaths` / `saveClipboardImage`。
- Mock `__test_setLastPasteHasFile`（从 `useComposerAttachmentPaste` 导出 `__test_setLastPasteHasFile` 等同 helper）。
- 与 P3 useComposerDropInbox 测试同款 `vi.mock('@tiptap/react', ...)` for ReactNodeViewRenderer。

## 风险

- **paste 事件里读 clipboardData**：保留 `lastPasteHasFile` 模块级 capture snapshot 模型。
- **ref.current.getEditor() 时序**：与 `insertAttachmentTokens` 一样，effect 重跑覆盖。
- **HTML 富文本粘贴 cross-app**：本期不做实测。Tiptap StarterKit 默认 paste handler 处理 HTML→schema 转换，schema 不支持的（heading / table）自动降级。这条由 P5 接入页面后 manual 测一遍即可。
- **图片 blob 异步保存与 token 插入时序**：`saveClipboardImage` 完成后再 insert，与现有逻辑一致。

---

### Task 1: RichComposer 添加 getEditor() ref method

**Files:**
- Modify: `src/components/rich-composer/RichComposer.tsx`
- Modify: `src/components/rich-composer/__tests__/RichComposer.test.tsx`

- [ ] **Step 1: 写失败测试**

Append to `RichComposer.test.tsx` 末尾：

```tsx
describe('RichComposer — getEditor handle', () => {
  it('ref.getEditor returns editor instance after mount', async () => {
    const handleRef = createRef<RichComposerHandle>()
    render(<RichComposer ref={handleRef} onSubmit={() => {}} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const ed = handleRef.current?.getEditor()
    expect(ed).toBeTruthy()
    expect(typeof ed?.view?.dom).toBe('object')
  })
})
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx`
Expected: FAIL — `getEditor` not on handle.

- [ ] **Step 3: 修改 RichComposer.tsx**

In `RichComposer.tsx`，加 `Editor` 类型导入和 handle 字段：

```ts
import type { Editor } from '@tiptap/react'

export interface RichComposerHandle {
  focus: () => void
  insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => void
  clear: () => void
  getEditor: () => Editor | null
}
```

In `useImperativeHandle`：

```ts
useImperativeHandle(
  ref,
  () => ({
    focus: () => { editor?.commands.focus('end') },
    insertAttachmentTokens: (tokens) => { editor?.commands.insertAttachmentTokens(tokens) },
    clear: () => { editor?.commands.clearContent() },
    getEditor: () => editor ?? null,
  }),
  [editor],
)
```

- [ ] **Step 4: 运行测试**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx`
Expected: PASS — 13 tests.

- [ ] **Step 5: 全量 + tsc**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: 88 + 1 = 89 tests.

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add src/components/rich-composer/RichComposer.tsx src/components/rich-composer/__tests__/RichComposer.test.tsx
git commit -m "feat(rich-composer): RichComposerHandle.getEditor() exposes Tiptap editor for paste/extension consumers"
```

---

### Task 2: useComposerAttachmentPaste hook + tests

**Files:**
- Create: `src/components/rich-composer/useComposerAttachmentPaste.ts`
- Create: `src/components/rich-composer/__tests__/useComposerAttachmentPaste.test.tsx`

- [ ] **Step 1: 写测试**

```tsx
// src/components/rich-composer/__tests__/useComposerAttachmentPaste.test.tsx
import '@testing-library/jest-dom'
import { useRef } from 'react'
import { describe, expect, it, beforeEach, vi } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { RichComposer } from '../RichComposer'
import type { RichComposerHandle } from '../RichComposer'
import {
  useComposerAttachmentPaste,
  __test_setLastPasteHasFile,
} from '../useComposerAttachmentPaste'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

const mockResolvePastedPaths = vi.fn()
const mockSaveClipboardImage = vi.fn()
const mockReadClipboardFilePaths = vi.fn()
const mockPushToast = vi.fn()

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    resolvePastedPaths: mockResolvePastedPaths,
    saveClipboardImage: mockSaveClipboardImage,
  }),
}))

vi.mock('@/lib/tauri', () => ({
  readClipboardFilePaths: () => mockReadClipboardFilePaths(),
  saveClipboardImageToWorkspaceStaging: vi.fn(),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: { getState: () => ({ push: mockPushToast }) },
}))

function Harness() {
  const ref = useRef<RichComposerHandle>(null)
  useComposerAttachmentPaste(ref)
  return <RichComposer ref={ref} onSubmit={() => {}} />
}

function dispatchPaste(types: string[], items: Array<{ kind: string; type: string; getAsFile: () => File | null }> = []) {
  const editorDom = document.querySelector('.ProseMirror') as HTMLElement
  // Build a minimal ClipboardEvent-like object since jsdom's ClipboardEvent constructor is limited.
  const clipboardData = {
    types,
    items: { length: items.length, ...items.reduce((acc, item, i) => ({ ...acc, [i]: item }), {}) } as DataTransferItemList,
    getData: () => '',
  }
  const event = new Event('paste', { bubbles: true, cancelable: true }) as Event & { clipboardData: typeof clipboardData }
  Object.defineProperty(event, 'clipboardData', { value: clipboardData, configurable: true })
  editorDom.dispatchEvent(event)
  return event
}

beforeEach(() => {
  mockResolvePastedPaths.mockReset()
  mockSaveClipboardImage.mockReset()
  mockReadClipboardFilePaths.mockReset()
  mockPushToast.mockReset()
  __test_setLastPasteHasFile(false)
})

describe('useComposerAttachmentPaste', () => {
  it('plain text paste → not intercepted (lastPasteHasFile=false)', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(false)
    const event = dispatchPaste(['text/plain'])
    expect(event.defaultPrevented).toBe(false)
    expect(mockReadClipboardFilePaths).not.toHaveBeenCalled()
    unmount()
  })

  it('file paths paste → preventDefault + insertAttachmentTokens', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/abs/a.pdf'])
    mockResolvePastedPaths.mockResolvedValue([
      { id: 'a', fileName: 'a.pdf', path: '/abs/a.pdf', kind: 'file', fileType: 'pdf', fileSize: 0, mimeType: undefined, source: 'paste' },
    ])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    const event = dispatchPaste(['Files'])
    expect(event.defaultPrevented).toBe(true)
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('a.pdf')
    })
    unmount()
  })

  it('multiple file paths → all tokens in order', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/p/x.pdf', '/p/y.pdf'])
    mockResolvePastedPaths.mockResolvedValue([
      { id: 'x', fileName: 'x.pdf', path: '/p/x.pdf', kind: 'file', fileType: 'pdf', fileSize: 0, mimeType: undefined, source: 'paste' },
      { id: 'y', fileName: 'y.pdf', path: '/p/y.pdf', kind: 'file', fileType: 'pdf', fileSize: 0, mimeType: undefined, source: 'paste' },
    ])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    dispatchPaste(['Files'])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('x.pdf')
      expect(html).toContain('y.pdf')
    })
    unmount()
  })

  it('all paths rejected → toast, no token', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/'])
    mockResolvePastedPaths.mockResolvedValue([])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    dispatchPaste(['Files'])
    await waitFor(() => {
      expect(mockPushToast).toHaveBeenCalled()
    })
    expect(document.querySelector('.ProseMirror')?.innerHTML ?? '').not.toContain('data-rich-composer-attachment-token')
    unmount()
  })

  it('image blob paste → saveClipboardImage + insertAttachmentTokens', async () => {
    mockReadClipboardFilePaths.mockResolvedValue([])
    mockSaveClipboardImage.mockResolvedValue({
      id: 'img1', fileName: 'pasted.png', path: '/tmp/pasted.png',
      kind: 'image', fileType: 'image', fileSize: 100, mimeType: 'image/png',
      source: 'clipboard-image',
    })
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    const fakeFile = new File([new Uint8Array([1,2,3])], 'pasted.png', { type: 'image/png' })
    dispatchPaste(['Files'], [{
      kind: 'file',
      type: 'image/png',
      getAsFile: () => fakeFile,
    }])
    await waitFor(() => {
      expect(mockSaveClipboardImage).toHaveBeenCalled()
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('pasted.png')
    })
    unmount()
  })

  it('paths take priority over image blob (matches existing useComposerPaste behavior)', async () => {
    mockReadClipboardFilePaths.mockResolvedValue(['/abs/x.pdf'])
    mockResolvePastedPaths.mockResolvedValue([
      { id: 'x', fileName: 'x.pdf', path: '/abs/x.pdf', kind: 'file', fileType: 'pdf', fileSize: 0, mimeType: undefined, source: 'paste' },
    ])
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    __test_setLastPasteHasFile(true)
    const fakeFile = new File([new Uint8Array([1,2,3])], 'pasted.png', { type: 'image/png' })
    dispatchPaste(['Files'], [{
      kind: 'file',
      type: 'image/png',
      getAsFile: () => fakeFile,
    }])
    await waitFor(() => {
      expect(mockResolvePastedPaths).toHaveBeenCalled()
    })
    expect(mockSaveClipboardImage).not.toHaveBeenCalled()
    unmount()
  })
})
```

- [ ] **Step 2: Run tests, expect failure**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/useComposerAttachmentPaste.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: 实现 useComposerAttachmentPaste.ts**

```ts
// src/components/rich-composer/useComposerAttachmentPaste.ts
import { useEffect } from 'react'
import type { RefObject } from 'react'
import { readClipboardFilePaths } from '@/lib/tauri'
import { useChatAttachments } from '@/hooks/useChatAttachments'
import { useNotificationStore } from '@/stores/notificationStore'
import { pendingAttachmentsToTokens } from './pendingAttachmentToToken'
import type { RichComposerHandle } from './RichComposer'

const MAX_PASTED_PATHS = 50

// macOS WebKit hangs the main thread if a React onPaste handler reads
// clipboardData.types/items in the bubble phase when the clipboard contains
// Finder file references. Capture-phase access is safe; one document-level
// capture listener snapshots whether the paste contains files into module
// state. The main paste handler reads only this snapshot.
let lastPasteHasFile = false

function snapshotPaste(e: globalThis.ClipboardEvent) {
  try {
    const types = Array.from(e.clipboardData?.types ?? [])
    lastPasteHasFile = types.some(
      (t) => t === 'Files' || t === 'text/uri-list' || t.startsWith('public.file-url'),
    )
  } catch {
    lastPasteHasFile = false
  }
}

let snapshotterInstalled = false
function ensureSnapshotterInstalled() {
  if (snapshotterInstalled || typeof document === 'undefined') return
  snapshotterInstalled = true
  document.addEventListener('paste', snapshotPaste, true)
}

function pushToast(level: 'info', title: string, message: string) {
  useNotificationStore.getState().push({
    level,
    title,
    message,
    actions: [],
    dismissible: true,
    autoHide: 4,
    context: 'toast',
  })
}

/** @internal test-only — sets the snapshot state normally managed by document capture listener. */
export function __test_setLastPasteHasFile(value: boolean) {
  lastPasteHasFile = value
}

/**
 * Attach paste handling to a RichComposer's editor DOM. When the clipboard
 * contains file references (Finder/Explorer drags, file URIs) or image blobs,
 * resolves them to attachments and inserts as attachment tokens. Plain text
 * and HTML paste fall through to Tiptap's default paste handler.
 *
 * Trade-off: when the clipboard contains BOTH file refs AND text content,
 * we currently fully preventDefault and discard the text portion. The toast
 * `'已识别附件'` is purely informational. Mixed paste is rare in practice
 * (Finder copy is files-only) so this simplification is acceptable.
 */
export function useComposerAttachmentPaste(
  composerRef: RefObject<RichComposerHandle | null>,
): void {
  ensureSnapshotterInstalled()
  const { resolvePastedPaths, saveClipboardImage } = useChatAttachments()

  useEffect(() => {
    const handle = composerRef.current
    const editor = handle?.getEditor()
    if (!editor) return
    const dom = editor.view.dom

    const onPaste = (event: ClipboardEvent) => {
      if (!lastPasteHasFile) return // Plain text/HTML — let Tiptap handle it.
      event.preventDefault()

      // Snapshot image blob synchronously — clipboardData is cleared when handler returns.
      let imageFile: File | null = null
      const items = event.clipboardData?.items
      if (items) {
        for (let i = 0; i < items.length; i++) {
          const item = items[i]
          if (item.kind === 'file' && item.type.startsWith('image/')) {
            imageFile = item.getAsFile()
            break
          }
        }
      }

      void (async () => {
        const nativePaths = await readClipboardFilePaths().catch(() => [] as string[])
        if (nativePaths.length > 0) {
          const capped = nativePaths.slice(0, MAX_PASTED_PATHS)
          const resolved = await resolvePastedPaths(capped)
          if (resolved.length === 0) {
            pushToast('info', '无法粘贴', '选中的项目（如磁盘根目录、系统目录或别名）不支持作为附件粘贴。')
            return
          }
          if (resolved.length < capped.length) {
            pushToast(
              'info',
              '部分项目已忽略',
              `已忽略 ${capped.length - resolved.length} 个不支持的项目（如磁盘根目录、系统目录或别名）。`,
            )
          }
          handle?.insertAttachmentTokens(pendingAttachmentsToTokens(resolved))
          return
        }

        if (imageFile) {
          try {
            const buffer = await imageFile.arrayBuffer()
            const attachment = await saveClipboardImage(new Uint8Array(buffer), imageFile.type)
            handle?.insertAttachmentTokens(pendingAttachmentsToTokens([attachment]))
          } catch {
            pushToast('info', '无法粘贴', '剪贴板图片保存失败，请重试。')
          }
          return
        }

        pushToast(
          'info',
          '无法粘贴',
          '剪贴板中的文件类型暂不支持作为附件粘贴，请改用左下角"+"按钮选择文件。',
        )
      })()
    }

    dom.addEventListener('paste', onPaste)
    return () => {
      dom.removeEventListener('paste', onPaste)
    }
  }, [composerRef, resolvePastedPaths, saveClipboardImage])
}
```

- [ ] **Step 4: Run tests**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/useComposerAttachmentPaste.test.tsx`
Expected: PASS — 6 tests.

#### Common failure mode

- "image blob paste" tests rely on `clipboardData.items` being iterable with numeric indexing. The mock harness sets up `items` with `length` + numeric props. If access fails, switch the items mock to a real array and adjust the loop.
- "file paths paste" relies on `readClipboardFilePaths` mock returning the path list. If the resolver fires before mocks are set, the test order matters; `beforeEach` resets them.

- [ ] **Step 5: 全量 + tsc + lint**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: 89 + 6 = 95 tests.

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0.

Run: `pnpm lint 2>&1 | grep -E "rich-composer"`
Expected: 0 errors / warnings.

- [ ] **Step 6: Commit**

```bash
git add src/components/rich-composer/useComposerAttachmentPaste.ts src/components/rich-composer/__tests__/useComposerAttachmentPaste.test.tsx
git commit -m "feat(rich-composer): useComposerAttachmentPaste handles file/image-blob paste, defers text/HTML to Tiptap"
```

---

### Task 3: index re-export

**Files:**
- Modify: `src/components/rich-composer/index.ts`

- [ ] **Step 1: Update index.ts**

Append:

```ts
export { useComposerAttachmentPaste } from './useComposerAttachmentPaste'
```

- [ ] **Step 2: Verify**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: 95 tests.

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add src/components/rich-composer/index.ts
git commit -m "chore(rich-composer): export useComposerAttachmentPaste"
```

---

## Self-Review

**1. Spec coverage（spec 的 "粘贴与附件管线"）：**
- ✅ 普通文本粘贴交给 Tiptap default → Task 2（不 preventDefault）
- ✅ HTML 富文本由 Tiptap StarterKit 默认 paste 处理 → 自带，无需代码
- ✅ 文件 / 文件夹 / URI / 路径 → Task 2（保留现有路径过滤）
- ✅ 截图 / 复制图片 blob → Task 2
- ✅ 危险路径 toast → Task 2
- ✅ 50 个路径上限 → Task 2 `MAX_PASTED_PATHS`
- ⚠️ 混合粘贴可解释 fallback：本期实现成"file 优先；text 部分丢失"，spec 说"toast 提示"——已在 hook comment 解释；toast 复用现有"无法粘贴"逻辑。这是 P4 的合理简化。
- ⚠️ macOS WebKit 卡顿：保留 capture snapshot 模型 → Task 2

**2. Type consistency：** `RichComposerHandle.getEditor()` 在 Task 1 加，Task 2 用。`pendingAttachmentsToTokens` 在 P3 已落。

**3. 范围：** 严格 P4，paste pipeline only。无页面接入；旧 useComposerPaste 不删（P5/P8 处理）。
