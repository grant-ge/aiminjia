# RichComposer P2 — RichComposer.tsx 组件实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `RichComposer.tsx` — 包装 Tiptap editor 的共享输入组件，提供 Enter/Shift+Enter/IME/disabled/clearOnSubmit/sending state/stop/SkillPopover/Project button 等行为，对外暴露 `onSubmit({ markdown, attachments, isEmpty })`。本期不接入页面（P5 做）；只产出可独立测试的组件。

**Architecture:** `RichComposer` 用 `useEditor`（`@tiptap/react`）创建 editor 实例，注入 P1 的 `buildComposerExtensions()`；外层壳（按钮区、tips、skill chip、项目按钮、发送/停止按钮）参考现存 `ChatComposerCompact.tsx` 的视觉。提交路径：`onSubmit(serializeComposerDoc(editor.getJSON()))`。所有内部状态（IME、submitting）放 `useRef` 避免不必要 rerender。

**Tech Stack:** `@tiptap/react` `useEditor`、`EditorContent`；React 19；vitest + jsdom + `@testing-library/react`。spec：`docs/superpowers/specs/2026-05-07-rich-composer-tiptap-design.md`。

---

## 文件结构

新增：
- `src/components/rich-composer/RichComposer.tsx` — 组件本体。
- `src/components/rich-composer/__tests__/RichComposer.test.tsx` — 行为单测。
- `src/components/rich-composer/parseMarkdown.ts` — 极小 markdown → Tiptap JSON 反向解析（仅供 `initialMarkdown` prop 用，spec 限定支持范围：纯文本 + 段落 + hardBreak + attachmentToken）。
- `src/components/rich-composer/__tests__/parseMarkdown.test.ts` — parseMarkdown 单测。

修改：
- `src/components/rich-composer/index.ts` — re-export `RichComposer`、`parseMarkdownToComposerJson`、`RichComposerProps`、`RichComposerHandle`。

不创建：页面级接入（P5）；任何 page-side 依赖（chat store / sendUserMessage）。

## 组件契约

### Props

```ts
import type { ReactNode } from 'react'
import type { RichComposerSubmitPayload } from './types'

export interface ComposerSkillCommand {
  command: string
  label: string
  id?: string
}

export interface RichComposerProps {
  placeholder?: string
  disabled?: boolean
  isStreaming?: boolean
  autoFocus?: boolean
  initialMarkdown?: string
  clearOnSubmit?: boolean

  onSubmit: (payload: RichComposerSubmitPayload) => void | Promise<void>
  onStop?: () => void

  topSlot?: ReactNode
  tips?: ReactNode

  onOpenSkill?: () => void
  skillCommand?: ComposerSkillCommand | null
  onClearSkillCommand?: () => void

  projectLabel?: string
  onPickProject?: () => void
  showProjectButton?: boolean

  onOpenAttachment?: () => void
}

export interface RichComposerHandle {
  focus: () => void
  insertAttachmentTokens: (tokens: import('./types').ComposerAttachmentToken[]) => void
  clear: () => void
}
```

`forwardRef` exposes `RichComposerHandle` for P3 (drop/picker) and P5 (page integration) to call `insertAttachmentTokens` and `focus()`. The handle is part of the public API but the **methods themselves** are not used in P2 tests beyond a smoke check.

### 行为规则（从 spec "RichComposer API"）

- Enter 提交（不在 IME composition 期间，且非 Shift）。
- Shift+Enter 插入 hardBreak（Tiptap 默认）。
- IME composition 期间 Enter 不提交（用 `compositionstart` / `compositionend` 跟踪）。
- `disabled` 时 editor `editable=false`，所有按钮 disabled。
- 内部 `submitting` ref：`onSubmit` 是 async 时，并发 Enter 不重复触发；`onSubmit` 完成（resolve 或 reject）后释放。
- `isStreaming` 时发送按钮变停止按钮（方块图标），点击调 `onStop`。
- `clearOnSubmit=true` 时：成功提交后清空 editor；提交失败（Promise reject）保留内容。
- `initialMarkdown` 在挂载时通过 `parseMarkdownToComposerJson` 转 JSON 注入；挂载后**不再响应** `initialMarkdown` 变化（一次性 prefill，避免外部重设导致光标乱跳）。
- `autoFocus=true` 时挂载后 focus 到末尾。
- `payload.isEmpty=true` 时阻止提交（不调 `onSubmit`）。
- Skill chip / project button / attach button 视觉与 `ChatComposerCompact` 等价；用现有主题变量。

### 视觉壳

复用 `ChatComposerCompact` 的视觉（`rounded-[18px] border border-border bg-card px-4 pb-1 pt-4`），但内容区把 `<textarea>` 换成 `<EditorContent editor={editor} />`，加 `prose` 样式。Skill chip / project button / attach button / send button 视觉与现有 composer 等价。Tips 区域同样位于壳外下方。

### parseMarkdownToComposerJson 范围（极小子集）

仅支持：

- 段落（按 `\n\n` 分段）。
- hardBreak（行尾两空格 + `\n` → `hardBreak` 节点）。
- 单换行 `\n`（无前置两空格）→ 也按 `hardBreak` 处理（容错）。
- attachmentToken：识别 `![name](file:///path)` 与 `[附件: name](file:///path)`；解析回 `attachmentToken` 节点（kind / fileType 通过文件扩展名启发式回填，缺省 `file` / `pdf`）。
- 任何其它 markdown 语法（粗体、链接、列表等）不解析，按字面量文本保留。

理由：spec 写明 prefill 仅支持纯文本 + 换行 + attachmentToken，富文本反向解析能力不在本期。

## 测试覆盖

`parseMarkdown.test.ts` ~6 项：
1. 空字符串 → 单空 paragraph 文档
2. 单段纯文本
3. `\n\n` → 两段
4. 行尾两空格 + `\n` → hardBreak
5. `![name](file:///abs/x.png)` → image attachmentToken (path 去掉 `file://` 前缀)
6. `[附件: r.pdf](file:///p/r.pdf)` → file attachmentToken

`RichComposer.test.tsx` ~12 项：
1. 渲染 placeholder
2. 输入文本后 Enter 提交，payload 包含 markdown
3. Shift+Enter 不提交
4. IME composition 中 Enter 不提交
5. 空 payload Enter 不提交（onSubmit 不调用）
6. 只附件 payload Enter 提交
7. `disabled=true` 时 Enter 不提交且发送按钮 disabled
8. `isStreaming=true` 时显示停止按钮，点击调用 `onStop`
9. `clearOnSubmit=true` 成功后清空
10. `clearOnSubmit=true` 失败（onSubmit reject）后保留内容
11. submitting 期间并发 Enter 不重复调用 onSubmit
12. ref.insertAttachmentTokens 插入 token 后再 Enter，attachments 出现在 payload

测试中 mock `onSubmit` 用 `vi.fn()`，必要时返回 Promise。`useEditor` 在 jsdom 下能实例化（已在 P1 测试中验证）。`fireEvent.keyDown` 触发 Enter；`fireEvent.compositionStart` / `compositionEnd` 触发 IME 状态切换。

## 风险与对策

- jsdom 下 ProseMirror 的某些 selection 操作可能不工作 → 测试只断言 onSubmit 调用 / clearOnSubmit 行为 / disabled 状态等可观测面，不直接断言光标位置。
- `useEditor` 异步创建 editor 实例 → 在测试里用 `await waitFor(() => expect(...))` 等 editor 就绪。
- Skill chip 的视觉主要是装饰，不写 visual regression 测试，只做 "渲染 / 不渲染" 的存在性断言。

---

### Task 1: parseMarkdownToComposerJson

**Files:**
- Create: `src/components/rich-composer/parseMarkdown.ts`
- Create: `src/components/rich-composer/__tests__/parseMarkdown.test.ts`

- [ ] **Step 1: Write failing tests**

```ts
// src/components/rich-composer/__tests__/parseMarkdown.test.ts
import { describe, expect, it } from 'vitest'
import { parseMarkdownToComposerJson } from '../parseMarkdown'

describe('parseMarkdownToComposerJson', () => {
  it('空字符串 → 单空 paragraph 文档', () => {
    const json = parseMarkdownToComposerJson('')
    expect(json).toEqual({
      type: 'doc',
      content: [{ type: 'paragraph' }],
    })
  })

  it('单段纯文本', () => {
    const json = parseMarkdownToComposerJson('hello world')
    expect(json).toEqual({
      type: 'doc',
      content: [
        { type: 'paragraph', content: [{ type: 'text', text: 'hello world' }] },
      ],
    })
  })

  it('\\n\\n → 两段', () => {
    const json = parseMarkdownToComposerJson('a\n\nb')
    expect(json.content).toEqual([
      { type: 'paragraph', content: [{ type: 'text', text: 'a' }] },
      { type: 'paragraph', content: [{ type: 'text', text: 'b' }] },
    ])
  })

  it('行尾两空格 + \\n → hardBreak', () => {
    const json = parseMarkdownToComposerJson('line1  \nline2')
    expect(json.content).toEqual([
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'line1' },
          { type: 'hardBreak' },
          { type: 'text', text: 'line2' },
        ],
      },
    ])
  })

  it('单换行 \\n → 同样作为 hardBreak（容错）', () => {
    const json = parseMarkdownToComposerJson('a\nb')
    expect(json.content).toEqual([
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'a' },
          { type: 'hardBreak' },
          { type: 'text', text: 'b' },
        ],
      },
    ])
  })

  it('image attachment: ![name](file:///abs/x.png) → image attachmentToken', () => {
    const json = parseMarkdownToComposerJson('![chart.png](file:///abs/chart.png)')
    const para = json.content?.[0]
    expect(para?.content?.[0]).toMatchObject({
      type: 'attachmentToken',
      attrs: {
        fileName: 'chart.png',
        path: '/abs/chart.png',
        kind: 'image',
        fileType: 'image',
        source: 'paste',
      },
    })
    expect(typeof (para?.content?.[0] as { attrs: { id: string } }).attrs.id).toBe('string')
  })

  it('file attachment: [附件: r.pdf](file:///p/r.pdf) → pdf attachmentToken', () => {
    const json = parseMarkdownToComposerJson('[附件: r.pdf](file:///p/r.pdf)')
    const para = json.content?.[0]
    expect(para?.content?.[0]).toMatchObject({
      type: 'attachmentToken',
      attrs: {
        fileName: 'r.pdf',
        path: '/p/r.pdf',
        kind: 'file',
        fileType: 'pdf',
      },
    })
  })

  it('文本 + token + 文本', () => {
    const json = parseMarkdownToComposerJson('请分析 [附件: r.pdf](file:///p/r.pdf) 谢谢')
    const para = json.content?.[0]
    expect(para?.content?.map((n) => n.type)).toEqual(['text', 'attachmentToken', 'text'])
  })
})
```

- [ ] **Step 2: Run tests to verify failure**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/parseMarkdown.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement parseMarkdownToComposerJson**

Create `src/components/rich-composer/parseMarkdown.ts`:

```ts
import type { ComposerJsonNode, ComposerAttachmentToken } from './types'

const ATTACHMENT_RE = /(!?)\[(?:附件: )?([^\]]+)\]\(file:\/\/([^)]+)\)/g

const EXT_TO_FILE_TYPE: Record<string, ComposerAttachmentToken['fileType']> = {
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  svg: 'image',
  pdf: 'pdf',
  xlsx: 'excel',
  xls: 'excel',
  doc: 'word',
  docx: 'word',
  json: 'json',
  csv: 'csv',
}

function inferFileType(fileName: string): ComposerAttachmentToken['fileType'] {
  const ext = fileName.toLowerCase().split('.').pop() ?? ''
  return EXT_TO_FILE_TYPE[ext] ?? 'pdf'
}

let counter = 0
function genId(): string {
  counter += 1
  return `prefill-${Date.now().toString(36)}-${counter}`
}

function buildAttachmentTokenAttrs(
  isImage: boolean,
  fileName: string,
  path: string,
): ComposerAttachmentToken {
  if (isImage) {
    return {
      id: genId(),
      fileName,
      path,
      kind: 'image',
      fileType: 'image',
      fileSize: 0,
      source: 'paste',
    }
  }
  return {
    id: genId(),
    fileName,
    path,
    kind: 'file',
    fileType: inferFileType(fileName),
    fileSize: 0,
    source: 'paste',
  }
}

function parseInline(line: string): ComposerJsonNode[] {
  const out: ComposerJsonNode[] = []
  let lastIndex = 0
  ATTACHMENT_RE.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = ATTACHMENT_RE.exec(line)) !== null) {
    const [whole, bang, fileName, rawPath] = match
    if (match.index > lastIndex) {
      out.push({ type: 'text', text: line.slice(lastIndex, match.index) })
    }
    out.push({
      type: 'attachmentToken',
      attrs: buildAttachmentTokenAttrs(bang === '!', fileName, rawPath),
    })
    lastIndex = match.index + whole.length
  }
  if (lastIndex < line.length) {
    out.push({ type: 'text', text: line.slice(lastIndex) })
  }
  return out
}

function parseLineWithBreaks(line: string): ComposerJsonNode[] {
  // markdown soft break: line ending with two spaces + \n → hardBreak.
  // We also fall back to single \n → hardBreak for robustness (composer prefill
  // is intentionally permissive — see plan).
  const segments = line.split(/  ?\n/)
  const out: ComposerJsonNode[] = []
  segments.forEach((seg, idx) => {
    out.push(...parseInline(seg))
    if (idx < segments.length - 1) out.push({ type: 'hardBreak' })
  })
  return out
}

export function parseMarkdownToComposerJson(markdown: string): ComposerJsonNode {
  if (markdown.length === 0) {
    return { type: 'doc', content: [{ type: 'paragraph' }] }
  }
  const paragraphs = markdown.split(/\n\n+/)
  const content: ComposerJsonNode[] = paragraphs.map((para) => {
    const inline = parseLineWithBreaks(para)
    return inline.length > 0
      ? { type: 'paragraph', content: inline }
      : { type: 'paragraph' }
  })
  return { type: 'doc', content }
}
```

- [ ] **Step 4: Run tests**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/parseMarkdown.test.ts`
Expected: PASS — 8 tests.

- [ ] **Step 5: Commit**

```bash
git add src/components/rich-composer/parseMarkdown.ts src/components/rich-composer/__tests__/parseMarkdown.test.ts
git commit -m "feat(rich-composer): parseMarkdownToComposerJson for initialMarkdown prefill (text/hardBreak/attachmentToken)"
```

---

### Task 2: RichComposer 组件骨架 + Enter/IME/Submit

**Files:**
- Create: `src/components/rich-composer/RichComposer.tsx`
- Create: `src/components/rich-composer/__tests__/RichComposer.test.tsx`

- [ ] **Step 1: Write the first batch of failing tests**

Create `src/components/rich-composer/__tests__/RichComposer.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor, fireEvent, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { RichComposer } from '../RichComposer'

async function typeIntoEditor(user: ReturnType<typeof userEvent.setup>, text: string) {
  const editor = document.querySelector('.ProseMirror') as HTMLElement
  await user.click(editor)
  await user.type(editor, text)
}

async function pressEnter(user: ReturnType<typeof userEvent.setup>) {
  const editor = document.querySelector('.ProseMirror') as HTMLElement
  await user.click(editor)
  fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
}

describe('RichComposer — basic submit', () => {
  it('renders placeholder', async () => {
    render(<RichComposer placeholder="say something" onSubmit={() => {}} />)
    await waitFor(() => {
      expect(document.querySelector('.ProseMirror')).toBeTruthy()
    })
  })

  it('Enter submits payload with markdown', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await typeIntoEditor(user, 'hello')
    await pressEnter(user)
    expect(onSubmit).toHaveBeenCalledTimes(1)
    expect(onSubmit.mock.calls[0][0].markdown).toBe('hello')
    expect(onSubmit.mock.calls[0][0].isEmpty).toBe(false)
  })

  it('empty document Enter does not submit', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await pressEnter(user)
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('Shift+Enter does not submit', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await typeIntoEditor(user, 'hi')
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter', shiftKey: true })
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('IME composition + Enter does not submit', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await typeIntoEditor(user, 'hi')
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.compositionStart(editor)
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    expect(onSubmit).not.toHaveBeenCalled()
    fireEvent.compositionEnd(editor)
  })
})
```

- [ ] **Step 2: Run tests, confirm failure**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx`
Expected: FAIL — `Failed to resolve "../RichComposer"`.

- [ ] **Step 3: Create RichComposer.tsx skeleton with Enter/IME logic**

Create `src/components/rich-composer/RichComposer.tsx`:

```tsx
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'
import type { ReactNode } from 'react'
import { ArrowUp, Blocks, Folder, Plus, Sparkles, X } from 'lucide-react'
import { EditorContent, useEditor } from '@tiptap/react'
import { buildComposerExtensions } from './composerSchema'
import { serializeComposerDoc } from './serializer'
import { parseMarkdownToComposerJson } from './parseMarkdown'
import type { ComposerAttachmentToken, RichComposerSubmitPayload } from './types'

export interface ComposerSkillCommand {
  command: string
  label: string
  id?: string
}

export interface RichComposerProps {
  placeholder?: string
  disabled?: boolean
  isStreaming?: boolean
  autoFocus?: boolean
  initialMarkdown?: string
  clearOnSubmit?: boolean

  onSubmit: (payload: RichComposerSubmitPayload) => void | Promise<void>
  onStop?: () => void

  topSlot?: ReactNode
  tips?: ReactNode

  onOpenSkill?: () => void
  skillCommand?: ComposerSkillCommand | null
  onClearSkillCommand?: () => void

  projectLabel?: string
  onPickProject?: () => void
  showProjectButton?: boolean

  onOpenAttachment?: () => void
}

export interface RichComposerHandle {
  focus: () => void
  insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => void
  clear: () => void
}

export const RichComposer = forwardRef<RichComposerHandle, RichComposerProps>(function RichComposer(
  {
    placeholder = '',
    disabled = false,
    isStreaming = false,
    autoFocus = false,
    initialMarkdown,
    clearOnSubmit = false,
    onSubmit,
    onStop,
    topSlot,
    tips,
    onOpenSkill,
    skillCommand,
    onClearSkillCommand,
    projectLabel = 'Desktop',
    onPickProject,
    showProjectButton = true,
    onOpenAttachment,
  },
  ref,
) {
  const isComposingRef = useRef(false)
  const submittingRef = useRef(false)
  const [, forceTick] = useState(0) // 用来在 editor 内容变化时触发 send 按钮 disabled 状态重算
  const editor = useEditor({
    extensions: buildComposerExtensions({ placeholder }),
    content: initialMarkdown ? parseMarkdownToComposerJson(initialMarkdown) : undefined,
    editable: !disabled,
    autofocus: autoFocus ? 'end' : false,
    onUpdate: () => forceTick((n) => n + 1),
  })

  useEffect(() => {
    editor?.setEditable(!disabled)
  }, [editor, disabled])

  const trySubmit = useCallback(async () => {
    if (!editor) return
    if (disabled || isStreaming) return
    if (submittingRef.current) return
    const json = editor.getJSON() as Parameters<typeof serializeComposerDoc>[0]
    const payload = serializeComposerDoc(json)
    if (payload.isEmpty) return
    submittingRef.current = true
    try {
      await onSubmit(payload)
      if (clearOnSubmit) editor.commands.clearContent()
    } catch {
      // failure preserves content; controller decides toasts
    } finally {
      submittingRef.current = false
      forceTick((n) => n + 1)
    }
  }, [editor, disabled, isStreaming, onSubmit, clearOnSubmit])

  useEffect(() => {
    if (!editor) return
    const dom = editor.view.dom
    const onCompositionStart = () => {
      isComposingRef.current = true
    }
    const onCompositionEnd = () => {
      // small delay matches existing ChatComposerCompact behavior
      window.setTimeout(() => {
        isComposingRef.current = false
      }, 50)
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Enter' || e.shiftKey) return
      if (isComposingRef.current || e.isComposing) return
      e.preventDefault()
      void trySubmit()
    }
    dom.addEventListener('compositionstart', onCompositionStart)
    dom.addEventListener('compositionend', onCompositionEnd)
    dom.addEventListener('keydown', onKeyDown)
    return () => {
      dom.removeEventListener('compositionstart', onCompositionStart)
      dom.removeEventListener('compositionend', onCompositionEnd)
      dom.removeEventListener('keydown', onKeyDown)
    }
  }, [editor, trySubmit])

  useImperativeHandle(
    ref,
    () => ({
      focus: () => {
        editor?.commands.focus('end')
      },
      insertAttachmentTokens: (tokens) => {
        editor?.commands.insertAttachmentTokens(tokens)
      },
      clear: () => {
        editor?.commands.clearContent()
      },
    }),
    [editor],
  )

  const isEmpty = !editor || editor.isEmpty
  const sendDisabled = !isStreaming && (disabled || isEmpty || submittingRef.current)

  return (
    <div className="flex w-full flex-col gap-2">
      <div
        data-testid="composer-root"
        className="flex w-full flex-col rounded-[18px] border border-border bg-card px-4 pb-1 pt-4"
      >
        {topSlot}
        {skillCommand ? (
          <div className="mb-2 flex items-center gap-2">
            <div
              className="group inline-flex max-w-full items-center gap-2 rounded-full border px-3 py-1.5 text-[0.8125rem] font-semibold shadow-[0_8px_24px_rgba(212,168,67,0.12)]"
              style={{
                borderColor: 'var(--color-accent-border)',
                background: 'var(--color-accent-subtle)',
                color: 'var(--color-accent-700)',
              }}
            >
              <span
                className="flex h-5 w-5 items-center justify-center rounded-full text-white"
                style={{ background: 'var(--color-accent)' }}
              >
                <Sparkles className="h-3.5 w-3.5" />
              </span>
              <span className="truncate">{skillCommand.label}</span>
              <span
                className="rounded-md bg-white/70 px-1.5 py-0.5 text-[0.6875rem] font-medium"
                style={{ color: 'var(--color-accent-600)' }}
              >
                {skillCommand.command}
              </span>
              {onClearSkillCommand ? (
                <button
                  type="button"
                  aria-label={`移除技能 ${skillCommand.label}`}
                  onClick={onClearSkillCommand}
                  className="ml-0.5 flex h-5 w-5 items-center justify-center rounded-full transition-colors hover:bg-[var(--color-accent-muted)]"
                  style={{ color: 'var(--color-accent-700)' }}
                >
                  <X className="h-3 w-3" />
                </button>
              ) : null}
            </div>
          </div>
        ) : null}
        <EditorContent
          editor={editor}
          className="min-h-[40px] w-full text-[0.8125rem] text-foreground [&_.ProseMirror]:outline-none [&_.ProseMirror_p.is-editor-empty:first-child]:before:pointer-events-none [&_.ProseMirror_p.is-editor-empty:first-child]:before:content-[attr(data-placeholder)] [&_.ProseMirror_p.is-editor-empty:first-child]:before:text-muted-foreground [&_.ProseMirror_p.is-editor-empty:first-child]:before:float-left [&_.ProseMirror_p.is-editor-empty:first-child]:before:h-0"
        />
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-0">
            <button
              type="button"
              aria-label="添加附件"
              onClick={onOpenAttachment}
              disabled={disabled}
              className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted disabled:opacity-40"
            >
              <Plus className="h-4 w-4" />
            </button>
            <button
              type="button"
              onClick={onOpenSkill}
              disabled={disabled}
              aria-label={
                skillCommand
                  ? `打开技能选择，当前已加载技能 ${skillCommand.label}`
                  : '打开技能选择'
              }
              aria-pressed={Boolean(skillCommand)}
              className={
                skillCommand
                  ? 'flex items-center gap-1.5 rounded-md px-2 py-1 text-[0.8125rem] font-semibold transition-colors hover:bg-[var(--color-accent-muted)] disabled:opacity-40'
                  : 'flex items-center gap-1.5 rounded-md px-2 py-1 text-[0.8125rem] text-muted-foreground transition-colors hover:bg-muted disabled:opacity-40'
              }
              style={
                skillCommand
                  ? { background: 'var(--color-accent-subtle)', color: 'var(--color-accent-700)' }
                  : undefined
              }
            >
              <Blocks className="h-3.5 w-3.5" />
              <span>{skillCommand ? '技能已加载' : '技能'}</span>
            </button>
            {showProjectButton ? (
              <button
                type="button"
                onClick={onPickProject}
                disabled={disabled}
                className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[0.8125rem] text-muted-foreground transition-colors hover:bg-muted disabled:opacity-40"
              >
                <Folder className="h-3.5 w-3.5" />
                <span>{projectLabel}</span>
              </button>
            ) : null}
          </div>
          <div className="flex items-center gap-3">
            <button
              type="button"
              aria-label={isStreaming ? '停止' : '发送'}
              onClick={() => {
                if (isStreaming) {
                  onStop?.()
                  return
                }
                void trySubmit()
              }}
              disabled={isStreaming ? false : sendDisabled}
              className={
                sendDisabled
                  ? 'flex h-8 w-8 items-center justify-center rounded-full bg-muted text-muted-foreground'
                  : 'flex h-8 w-8 items-center justify-center rounded-full bg-primary text-primary-foreground transition-colors hover:opacity-90'
              }
            >
              {isStreaming ? (
                <span className="block h-3.5 w-3.5 rounded-[2px] bg-current" />
              ) : (
                <ArrowUp className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>
      </div>
      {tips ? (
        <div
          data-testid="composer-tips"
          className="flex items-center justify-between gap-3 px-3 text-[0.6875rem] text-muted-foreground"
        >
          {tips}
        </div>
      ) : null}
    </div>
  )
})
```

Notes for the implementer:
- The `forceTick` state is intentional — `editor.isEmpty` is a runtime read; we only need to trigger a rerender when content changes (`onUpdate`) and when submit completes.
- The `EditorContent` className uses Tailwind arbitrary variants to wire the placeholder pseudo-element via Tiptap's `is-editor-empty` class. This avoids the need for a separate Tiptap Placeholder UI primitive.
- All colors use theme variables OR defined `var(--color-...)` from existing `:root`. Where the existing `ChatComposerCompact` uses `var(--color-accent-...)`, the same vars are used here (consistency with the existing design system).
- The "send button gray bg" uses `bg-muted` (theme variable) instead of the hardcoded `#D4D4D8` from `ChatComposerCompact` — that hardcode is a pre-existing violation and not worth carrying forward.

- [ ] **Step 4: Run tests, confirm pass**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx`
Expected: PASS — 5 tests.

If a test fails:
- "renders placeholder" — confirm `.ProseMirror` element appears in DOM after waitFor. If not, useEditor may not have initialized; check imports.
- "Enter submits" — `await user.type(editor, 'hello')` may not actually insert text into ProseMirror under jsdom. If so, replace with `editor.commands.insertContent('hello')` via a ref — but that requires updating the test helper. Try `user.type` first; jsdom usually handles ProseMirror typing.
- "Shift+Enter does not submit" — ensure `e.shiftKey` check is present.
- "IME composition" — confirm the `compositionstart` listener is wired and the `isComposingRef.current` flag is checked before submit.

Escalate as BLOCKED if `user.type` cannot insert text in jsdom.

- [ ] **Step 5: Verify all rich-composer tests still pass**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: 58 (P0+P1) + 8 (parseMarkdown) + 5 (RichComposer initial) = 71 tests.

- [ ] **Step 6: Commit**

```bash
git add src/components/rich-composer/RichComposer.tsx src/components/rich-composer/__tests__/RichComposer.test.tsx
git commit -m "feat(rich-composer): RichComposer component skeleton with Enter/IME/empty-doc submit gating"
```

---

### Task 3: disabled / isStreaming / clearOnSubmit / onSubmit failure / submitting lock

**Files:**
- Modify: `src/components/rich-composer/__tests__/RichComposer.test.tsx`

The component already implements all this logic in Task 2's code. This task just adds the tests to lock the contract.

- [ ] **Step 1: Append failing tests**

Append to the END of `RichComposer.test.tsx`:

```tsx
describe('RichComposer — disabled / streaming / clearOnSubmit', () => {
  it('disabled=true → Enter does not submit and send button is disabled', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer disabled onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    expect(onSubmit).not.toHaveBeenCalled()
    expect(screen.getByLabelText('发送')).toBeDisabled()
  })

  it('isStreaming=true → shows stop button and clicking calls onStop', async () => {
    const onStop = vi.fn()
    const onSubmit = vi.fn()
    render(<RichComposer isStreaming onStop={onStop} onSubmit={onSubmit} />)
    const stopBtn = await screen.findByLabelText('停止')
    fireEvent.click(stopBtn)
    expect(onStop).toHaveBeenCalledTimes(1)
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('clearOnSubmit=true → editor cleared after successful submit', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    const user = userEvent.setup()
    render(<RichComposer clearOnSubmit onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'hello')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalled())
    await waitFor(() => {
      expect(editor.textContent ?? '').toBe('')
    })
  })

  it('clearOnSubmit=true + onSubmit rejects → editor content preserved', async () => {
    const onSubmit = vi.fn().mockRejectedValue(new Error('boom'))
    const user = userEvent.setup()
    render(<RichComposer clearOnSubmit onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'keepme')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalled())
    expect(editor.textContent).toContain('keepme')
  })

  it('concurrent Enter while submitting → onSubmit is called only once', async () => {
    let resolveOuter: () => void = () => {}
    const onSubmit = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveOuter = resolve
        }),
    )
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'first')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    act(() => resolveOuter())
  })
})
```

- [ ] **Step 2: Run tests**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx`
Expected: PASS — 10 tests (5 from Task 2 + 5 new).

If "concurrent Enter" fails because the second/third Enter races against React reconciliation, increase the keyDown delay or insert a microtask `await Promise.resolve()` between them. The contract is: while `submittingRef.current === true`, additional keydowns must be no-ops.

- [ ] **Step 3: Commit**

```bash
git add src/components/rich-composer/__tests__/RichComposer.test.tsx
git commit -m "test(rich-composer): lock disabled/streaming/clearOnSubmit/concurrent-submit behavior"
```

---

### Task 4: ref handle (insertAttachmentTokens / focus / clear) + only-attachment submit

**Files:**
- Modify: `src/components/rich-composer/__tests__/RichComposer.test.tsx`

- [ ] **Step 1: Append failing tests**

Append to the END of `RichComposer.test.tsx`:

```tsx
import { createRef } from 'react'
import type { RichComposerHandle } from '../RichComposer'

describe('RichComposer — ref handle + attachment-only submit', () => {
  it('ref.insertAttachmentTokens inserts a token; subsequent Enter submits with attachments', async () => {
    const onSubmit = vi.fn()
    const handleRef = createRef<RichComposerHandle>()
    render(<RichComposer ref={handleRef} onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    act(() => {
      handleRef.current?.insertAttachmentTokens([
        {
          id: 'ref-1',
          fileName: 'a.pdf',
          path: '/p/a.pdf',
          kind: 'file',
          fileType: 'pdf',
          fileSize: 1,
          source: 'picker',
        },
      ])
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    const payload = onSubmit.mock.calls[0][0]
    expect(payload.attachments).toHaveLength(1)
    expect(payload.attachments[0].id).toBe('ref-1')
    expect(payload.markdown).toContain('[附件: a.pdf](file:///p/a.pdf)')
  })

  it('ref.clear empties editor', async () => {
    const handleRef = createRef<RichComposerHandle>()
    const user = userEvent.setup()
    render(<RichComposer ref={handleRef} onSubmit={() => {}} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'hello')
    expect(editor.textContent).toContain('hello')
    act(() => handleRef.current?.clear())
    await waitFor(() => expect(editor.textContent).toBe(''))
  })
})
```

- [ ] **Step 2: Run tests**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/RichComposer.test.tsx`
Expected: PASS — 12 tests.

- [ ] **Step 3: Commit**

```bash
git add src/components/rich-composer/__tests__/RichComposer.test.tsx
git commit -m "test(rich-composer): lock ref handle (insertAttachmentTokens/clear) + attachment-only submit"
```

---

### Task 5: index re-export + 验证

**Files:**
- Modify: `src/components/rich-composer/index.ts`

- [ ] **Step 1: Update index.ts**

Replace `src/components/rich-composer/index.ts` with:

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
```

- [ ] **Step 2: Full vitest run**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: PASS — 58 (P0+P1) + 8 (parseMarkdown) + 12 (RichComposer) = 78 tests.

- [ ] **Step 3: tsc**

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0.

- [ ] **Step 4: Lint (rich-composer scope)**

Run: `pnpm lint 2>&1 | grep -E "rich-composer"`
Expected: 0 errors / 0 warnings under `src/components/rich-composer/`. Pre-existing 14 errors elsewhere in the repo are unrelated.

- [ ] **Step 5: Commit**

```bash
git add src/components/rich-composer/index.ts
git commit -m "chore(rich-composer): export RichComposer + parseMarkdownToComposerJson"
```

---

## Self-Review

**1. Spec coverage（spec 的 "RichComposer API" 与 "页面接入"）：**

- ✅ Props: placeholder/disabled/isStreaming/autoFocus/initialMarkdown/clearOnSubmit/onSubmit/onStop/topSlot/tips/onOpenSkill/skillCommand/onClearSkillCommand/projectLabel/onPickProject/showProjectButton/onOpenAttachment → Task 2
- ✅ Enter submits, Shift+Enter inserts hardBreak (Tiptap default), IME blocks Enter → Task 2 + 3
- ✅ disabled disables editor + buttons, doesn't submit → Task 3
- ✅ isStreaming shows stop button → Task 3
- ✅ clearOnSubmit success cleans, failure preserves → Task 3
- ✅ submitting lock prevents double submit → Task 3
- ✅ initialMarkdown one-time prefill via parseMarkdownToComposerJson → Task 1 + 2
- ✅ Skill chip / project / attach buttons present → Task 2
- ✅ Ref handle exposes focus / insertAttachmentTokens / clear → Task 4
- ⚠️ NOT done in P2: pageside integration (HomeTaskComposerCard / ChatBottomArea wiring) — that's P5
- ⚠️ NOT done in P2: drop/picker/paste pipeline integration — P3 + P4
- ⚠️ NOT done in P2: attachment-only "no default '请分析附件' text" page-level removal — P5

**2. Placeholder scan:** Each step has full code; commands are concrete; commit messages are verbatim.

**3. Type consistency:**
- `RichComposerProps`, `RichComposerHandle`, `ComposerSkillCommand` defined in Task 2; re-exported in Task 5.
- `parseMarkdownToComposerJson` signature `(string) => ComposerJsonNode` consistent across Task 1 and Task 2 use.
- `serializeComposerDoc(json)` consumed in Task 2's `trySubmit`.
- `RichComposer` is `forwardRef<RichComposerHandle, RichComposerProps>`; tests in Task 4 use `createRef<RichComposerHandle>()`.

**4. Scope:** Strict P2 — RichComposer.tsx component only. No page integration, no paste pipeline, no drop wiring.
