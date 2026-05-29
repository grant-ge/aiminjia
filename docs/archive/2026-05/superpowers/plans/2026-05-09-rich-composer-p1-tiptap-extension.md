# RichComposer P1 — Tiptap 底座 + attachmentToken Extension 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 装 Tiptap 依赖；建立 `attachmentToken` Tiptap inline atom node 扩展（含 NodeView UI、`insertAttachmentTokens` command、JSON ↔ HTML 序列化）；纯模块层，不接入页面。

**Architecture:** `attachmentTokenExtension.ts` 用 Tiptap 3.x `Node.create()` API 定义 inline atom；`AttachmentTokenView.tsx` 用 Tiptap React `NodeViewWrapper` 渲染 chip + 删除按钮；JSON ↔ Tiptap 文档结构与 P0 serializer 已锁定的 `ComposerJsonNode` 形态一致。本期不写完整 `RichComposer.tsx` 组件（P2 做），但要写出能在隔离 demo 里实例化一个 editor + 插入 token 的最小 sandbox 测试。

**Tech Stack:** `@tiptap/core`、`@tiptap/react`、`@tiptap/starter-kit`、`@tiptap/extension-link`、`@tiptap/extension-placeholder`（全部 3.23.1）。Vitest jsdom 环境。

---

## 文件结构

新增：
- `src/components/rich-composer/attachmentTokenExtension.ts` — Tiptap Node 定义 + `insertAttachmentTokens` command。
- `src/components/rich-composer/AttachmentTokenView.tsx` — NodeView React 组件（chip + 删除按钮）。
- `src/components/rich-composer/composerSchema.ts` — 复用扩展集合（StarterKit 选项 + Link + Placeholder + AttachmentToken）。后续 RichComposer.tsx 直接用。
- `src/components/rich-composer/__tests__/attachmentTokenExtension.test.ts` — extension 行为单测。
- `src/components/rich-composer/__tests__/composerSchema.test.ts` — schema 整体单测（HTML ↔ JSON ↔ markdown 端到端）。

修改：
- `package.json` — 加 5 个 tiptap 依赖。
- `src/components/rich-composer/index.ts` — re-export `attachmentTokenExtension`、`buildComposerExtensions`、`AttachmentTokenView`。

不创建：`RichComposer.tsx`（P2 做）。

## 概念约定

P0 已经把 `ComposerJsonNode` 形态当作 Tiptap JSON 的 strict subset 锁定了。Tiptap 内部表示与 P0 类型字段同构：

```ts
{ type: 'attachmentToken', attrs: { id, fileName, path, kind, fileType, fileSize, mimeType?, source } }
```

P1 要求 attachmentToken 节点：
- 是 `inline: true`、`atom: true`、`selectable: true`、`draggable: true` 的 inline atom node。
- 完整保存所有 attrs（id/fileName/path/kind/fileType/fileSize/mimeType?/source）。
- 不允许编辑内部文字。
- 整体可选中、可删除（Backspace/Delete）、可拖动。
- HTML 反序列化：`<span data-rich-composer-attachment-token data-id="..." data-file-name="..." data-path="..." data-kind="..." data-file-type="..." data-file-size="..." data-source="..." data-mime-type="...?" />` 形态。
- HTML 序列化：同上。
- 提供 chainable command `insertAttachmentTokens(tokens: ComposerAttachmentToken[])`：批量在当前 selection 后插入；token 之间用普通文本 `' '`（一个空格）分隔；插入后光标停在最后一个 token 之后。

## 测试策略

**核心层（attachmentTokenExtension.test.ts）**：构造 ProseMirror Editor（headless 模式 `Editor` from `@tiptap/core`，不挂 DOM）测试：
- attrs 双向 round-trip
- `insertAttachmentTokens` 单 token / 多 token / 空数组
- 删除按 Backspace 行为（在 token 后按 Backspace 删除整个 token）
- HTML round-trip：JSON → `getHTML()` → `setContent(html)` → 同结构 JSON

**集成层（composerSchema.test.ts）**：构造 `buildComposerExtensions()` 返回的扩展集，validate：
- StarterKit 节点（paragraph / bold / italic / list / blockquote / codeBlock）能正常工作
- Link mark 能输入 / 解析 HTML
- 插入 attachmentToken 后调用 P0 `serializeComposerDoc(editor.getJSON())` 输出符合预期 markdown

---

### Task 1: 装 Tiptap 依赖

**Files:**
- Modify: `package.json`

- [ ] **Step 1: 装 5 个依赖（pinned 3.23.1）**

```bash
pnpm add @tiptap/core@3.23.1 @tiptap/pm@3.23.1 @tiptap/react@3.23.1 @tiptap/starter-kit@3.23.1 @tiptap/extension-link@3.23.1 @tiptap/extension-placeholder@3.23.1
```

- [ ] **Step 2: 验证 lockfile 更新**

Run: `git diff --stat package.json pnpm-lock.yaml`
Expected: 两个文件都改动，`package.json` `dependencies` 多 6 条 tiptap。

- [ ] **Step 3: 运行 P0 测试确保未破坏**

Run: `pnpm exec vitest run src/components/rich-composer/`
Expected: PASS — 38 tests, 0 failures

- [ ] **Step 4: 运行 tsc**

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0

- [ ] **Step 5: 提交**

```bash
git add package.json pnpm-lock.yaml
git commit -m "chore(rich-composer): add tiptap 3.23.1 deps (core/pm/react/starter-kit/link/placeholder)"
```

---

### Task 2: AttachmentTokenView NodeView 组件

**Files:**
- Create: `src/components/rich-composer/AttachmentTokenView.tsx`
- Create: `src/components/rich-composer/__tests__/AttachmentTokenView.test.tsx`

NodeView 用来在 editor 里渲染 chip。React 19 + tiptap 3 的 `NodeViewWrapper` API：组件接收 `{ node, updateAttributes, deleteNode, selected, editor }` 属性。

**视觉**：
- 整体 inline chip：`inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 align-middle text-xs`，用主题变量。
- Image kind：左侧 image 小图标（lucide `Image`）。
- File kind：按 `fileType` 显示 `XLS/DOC/PDF/JSON/CSV/IMG` 短标签（与 `FileAttachmentChip` 一致）。
- Folder kind：folder 小图标（lucide `Folder`）。
- 文件名 truncate `max-w-[160px]`。
- 右侧 X 按钮调用 `deleteNode()`。
- `selected` 为 true 时加 `ring-2 ring-primary` 视觉反馈。
- 全部用主题变量，禁止硬编码颜色。

- [ ] **Step 1: 写失败测试**

```tsx
// src/components/rich-composer/__tests__/AttachmentTokenView.test.tsx
import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AttachmentTokenView } from '../AttachmentTokenView'
import type { ComposerAttachmentToken } from '../types'

const mkAttrs = (overrides: Partial<ComposerAttachmentToken> = {}): ComposerAttachmentToken => ({
  id: 'a1',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 2048,
  source: 'picker',
  ...overrides,
})

describe('AttachmentTokenView', () => {
  it('显示文件名', () => {
    const node = { attrs: mkAttrs() } as never
    render(
      <AttachmentTokenView node={node} deleteNode={() => {}} selected={false} />
    )
    expect(screen.getByText('plan.pdf')).toBeInTheDocument()
  })

  it('文件 kind → 显示 fileType 标签', () => {
    const node = { attrs: mkAttrs({ fileType: 'pdf' }) } as never
    render(
      <AttachmentTokenView node={node} deleteNode={() => {}} selected={false} />
    )
    expect(screen.getByText('PDF')).toBeInTheDocument()
  })

  it('image kind → 显示 image 图标 (aria-label "image attachment")', () => {
    const node = { attrs: mkAttrs({ kind: 'image', fileType: 'image' }) } as never
    render(
      <AttachmentTokenView node={node} deleteNode={() => {}} selected={false} />
    )
    expect(screen.getByLabelText('image attachment')).toBeInTheDocument()
  })

  it('folder kind → 显示 folder 图标 (aria-label "folder attachment")', () => {
    const node = { attrs: mkAttrs({ kind: 'folder', fileType: 'folder' }) } as never
    render(
      <AttachmentTokenView node={node} deleteNode={() => {}} selected={false} />
    )
    expect(screen.getByLabelText('folder attachment')).toBeInTheDocument()
  })

  it('点击删除按钮 → 调用 deleteNode', () => {
    const deleteNode = vi.fn()
    const node = { attrs: mkAttrs() } as never
    render(
      <AttachmentTokenView node={node} deleteNode={deleteNode} selected={false} />
    )
    fireEvent.click(screen.getByLabelText('remove attachment'))
    expect(deleteNode).toHaveBeenCalledTimes(1)
  })

  it('selected=true → 容器有 ring class 标记', () => {
    const node = { attrs: mkAttrs() } as never
    const { container } = render(
      <AttachmentTokenView node={node} deleteNode={() => {}} selected={true} />
    )
    const chip = container.querySelector('[data-attachment-chip]')
    expect(chip?.className).toMatch(/ring-2/)
  })
})
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/AttachmentTokenView.test.tsx`
Expected: FAIL — `Failed to resolve "../AttachmentTokenView"`

如果 `@testing-library/react` 没装：先确认。

```bash
ls node_modules/@testing-library/react 2>/dev/null
```

如果没装，跳到 Task 1 旁边附加：`pnpm add -D @testing-library/react@^17.0.0` 并 commit 到 Task 1。但既存项目大概率有，先 ls 验证。

- [ ] **Step 3: 实现 AttachmentTokenView**

```tsx
// src/components/rich-composer/AttachmentTokenView.tsx
import { Image as ImageIcon, Folder, X } from 'lucide-react'
import type { ComposerAttachmentToken } from './types'

const FILE_TYPE_LABEL: Record<ComposerAttachmentToken['fileType'], string> = {
  excel: 'XLS',
  csv: 'CSV',
  word: 'DOC',
  pdf: 'PDF',
  json: 'JSON',
  image: 'IMG',
  folder: 'DIR',
}

interface AttachmentTokenViewProps {
  node: { attrs: ComposerAttachmentToken }
  deleteNode: () => void
  selected: boolean
}

export function AttachmentTokenView({ node, deleteNode, selected }: AttachmentTokenViewProps) {
  const attrs = node.attrs
  const ringClass = selected ? 'ring-2 ring-primary' : ''
  return (
    <span
      data-attachment-chip
      contentEditable={false}
      className={`inline-flex max-w-[180px] items-center gap-1 rounded-md border border-border bg-muted px-1.5 py-0.5 align-middle text-xs leading-none text-foreground ${ringClass}`}
    >
      {attrs.kind === 'image' ? (
        <ImageIcon aria-label="image attachment" className="h-3.5 w-3.5 shrink-0" />
      ) : attrs.kind === 'folder' ? (
        <Folder aria-label="folder attachment" className="h-3.5 w-3.5 shrink-0" />
      ) : (
        <span className="shrink-0 rounded bg-background px-1 text-[10px] font-bold text-muted-foreground">
          {FILE_TYPE_LABEL[attrs.fileType] ?? 'FILE'}
        </span>
      )}
      <span className="truncate">{attrs.fileName}</span>
      <button
        type="button"
        aria-label="remove attachment"
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          e.preventDefault()
          e.stopPropagation()
          deleteNode()
        }}
        className="ml-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded hover:bg-background"
      >
        <X className="h-3 w-3" />
      </button>
    </span>
  )
}
```

注意：
- `contentEditable={false}` 避免 ProseMirror 把 chip 内当成可编辑文本。
- `onMouseDown preventDefault` 避免点击删除按钮把光标移进 chip。
- 用主题变量（`bg-muted` / `text-foreground` / `border-border` / `bg-background` / `text-muted-foreground` / `ring-primary`），不硬编码颜色。

- [ ] **Step 4: 运行测试，确认通过**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/AttachmentTokenView.test.tsx`
Expected: PASS — 6 tests

- [ ] **Step 5: 提交**

```bash
git add src/components/rich-composer/AttachmentTokenView.tsx src/components/rich-composer/__tests__/AttachmentTokenView.test.tsx
git commit -m "feat(rich-composer): AttachmentTokenView NodeView (chip + delete button)"
```

---

### Task 3: attachmentTokenExtension Node 定义

**Files:**
- Create: `src/components/rich-composer/attachmentTokenExtension.ts`
- Create: `src/components/rich-composer/__tests__/attachmentTokenExtension.test.ts`

#### Tiptap Node 定义要点

```ts
// src/components/rich-composer/attachmentTokenExtension.ts
import { Node, mergeAttributes } from '@tiptap/core'
import { ReactNodeViewRenderer } from '@tiptap/react'
import type { ComposerAttachmentToken } from './types'
import { AttachmentTokenView } from './AttachmentTokenView'

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    attachmentToken: {
      insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => ReturnType
    }
  }
}

const DATA_ATTR = 'data-rich-composer-attachment-token'

function readNumber(value: unknown): number | null {
  if (typeof value === 'number') return value
  if (typeof value === 'string') {
    const n = Number(value)
    return Number.isFinite(n) ? n : null
  }
  return null
}

function readKind(value: unknown): ComposerAttachmentToken['kind'] | null {
  return value === 'file' || value === 'folder' || value === 'image' ? value : null
}

function readSource(value: unknown): ComposerAttachmentToken['source'] | null {
  return value === 'picker' || value === 'paste' || value === 'drop' || value === 'clipboard-image'
    ? value
    : null
}

export const AttachmentTokenExtension = Node.create({
  name: 'attachmentToken',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: true,
  draggable: true,

  addAttributes() {
    return {
      id: { default: null },
      fileName: { default: null },
      path: { default: null },
      kind: { default: null },
      fileType: { default: null },
      fileSize: { default: null },
      mimeType: { default: null },
      source: { default: null },
    }
  },

  parseHTML() {
    return [
      {
        tag: `span[${DATA_ATTR}]`,
        getAttrs: (el) => {
          if (!(el instanceof HTMLElement)) return false
          const id = el.getAttribute('data-id')
          const fileName = el.getAttribute('data-file-name')
          const path = el.getAttribute('data-path')
          const kind = readKind(el.getAttribute('data-kind'))
          const fileType = el.getAttribute('data-file-type')
          const fileSize = readNumber(el.getAttribute('data-file-size'))
          const source = readSource(el.getAttribute('data-source'))
          if (!id || !fileName || !path || !kind || !fileType || fileSize === null || !source) {
            return false
          }
          const mimeType = el.getAttribute('data-mime-type')
          return {
            id,
            fileName,
            path,
            kind,
            fileType,
            fileSize,
            source,
            mimeType: mimeType ?? null,
          }
        },
      },
    ]
  },

  renderHTML({ HTMLAttributes, node }) {
    const attrs = node.attrs as ComposerAttachmentToken & { mimeType: string | null }
    const dataset: Record<string, string> = {
      [DATA_ATTR]: '',
      'data-id': String(attrs.id ?? ''),
      'data-file-name': String(attrs.fileName ?? ''),
      'data-path': String(attrs.path ?? ''),
      'data-kind': String(attrs.kind ?? ''),
      'data-file-type': String(attrs.fileType ?? ''),
      'data-file-size': String(attrs.fileSize ?? ''),
      'data-source': String(attrs.source ?? ''),
    }
    if (attrs.mimeType) dataset['data-mime-type'] = String(attrs.mimeType)
    return ['span', mergeAttributes(HTMLAttributes, dataset), '']
  },

  addNodeView() {
    return ReactNodeViewRenderer(AttachmentTokenView)
  },

  addCommands() {
    return {
      insertAttachmentTokens:
        (tokens: ComposerAttachmentToken[]) =>
        ({ chain }) => {
          if (!tokens.length) return false
          let c = chain()
          tokens.forEach((token, idx) => {
            c = c.insertContent({ type: 'attachmentToken', attrs: token })
            if (idx < tokens.length - 1) {
              c = c.insertContent({ type: 'text', text: ' ' })
            }
          })
          return c.run()
        },
    }
  },
})
```

注意：
- `mimeType` 在 attrs 里允许 `null`（与 P0 `ComposerAttachmentToken.mimeType?: string` 协调；Tiptap 不接受 `undefined` attrs，所以用 `null`，序列化前转回 `undefined`）。但 P0 serializer 不读 `mimeType`，所以暂时不需要做转换 layer。
- 第三个数组元素 `''`（空字符串）告诉 Tiptap 这是 self-closing-like（没有 children）。
- `addCommands` 模式：command factory 返回函数 `({ chain }) => ...`，最终 `.run()` 提交事务。

#### 测试

```ts
// src/components/rich-composer/__tests__/attachmentTokenExtension.test.ts
import { describe, expect, it } from 'vitest'
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import { AttachmentTokenExtension } from '../attachmentTokenExtension'
import type { ComposerAttachmentToken } from '../types'

const mkToken = (overrides: Partial<ComposerAttachmentToken> = {}): ComposerAttachmentToken => ({
  id: 'a1',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 2048,
  source: 'picker',
  ...overrides,
})

function makeEditor() {
  return new Editor({
    extensions: [StarterKit, AttachmentTokenExtension],
    content: '<p></p>',
  })
}

describe('attachmentTokenExtension', () => {
  it('insertAttachmentTokens 单 token → JSON 含 attachmentToken 节点', () => {
    const editor = makeEditor()
    editor.commands.insertAttachmentTokens([mkToken()])
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: unknown[] }>)[0]
    const tokenNode = (para.content as Array<{ type: string; attrs: ComposerAttachmentToken }>).find(
      (n) => n.type === 'attachmentToken'
    )
    expect(tokenNode).toBeDefined()
    expect(tokenNode?.attrs.id).toBe('a1')
    expect(tokenNode?.attrs.fileName).toBe('plan.pdf')
    editor.destroy()
  })

  it('insertAttachmentTokens 多 token → 节点之间用空格 text 分隔', () => {
    const editor = makeEditor()
    editor.commands.insertAttachmentTokens([
      mkToken({ id: 'a' }),
      mkToken({ id: 'b' }),
      mkToken({ id: 'c' }),
    ])
    const json = editor.getJSON()
    const para = (json.content as Array<{ content: Array<{ type: string; attrs?: { id?: string }; text?: string }> }>)[0]
    const types = para.content.map((n) => n.type)
    // expect alternating: token, text(' '), token, text(' '), token
    expect(types).toEqual(['attachmentToken', 'text', 'attachmentToken', 'text', 'attachmentToken'])
    expect(para.content[1].text).toBe(' ')
    expect(para.content[3].text).toBe(' ')
    editor.destroy()
  })

  it('insertAttachmentTokens 空数组 → 不修改文档', () => {
    const editor = makeEditor()
    const beforeJson = JSON.stringify(editor.getJSON())
    const result = editor.commands.insertAttachmentTokens([])
    expect(result).toBe(false)
    expect(JSON.stringify(editor.getJSON())).toBe(beforeJson)
    editor.destroy()
  })

  it('HTML round-trip：getHTML 输出 data-* 属性，setContent 还原 attrs', () => {
    const editor = makeEditor()
    const token = mkToken({ id: 'rt1', fileName: 'a (b).pdf', path: '/p/a (b).pdf', mimeType: 'application/pdf' })
    editor.commands.insertAttachmentTokens([token])
    const html = editor.getHTML()
    expect(html).toContain('data-rich-composer-attachment-token')
    expect(html).toContain('data-id="rt1"')
    expect(html).toContain('data-file-name="a (b).pdf"')
    expect(html).toContain('data-mime-type="application/pdf"')

    const editor2 = new Editor({ extensions: [StarterKit, AttachmentTokenExtension], content: html })
    const json = editor2.getJSON()
    const para = (json.content as Array<{ content: Array<{ type: string; attrs?: ComposerAttachmentToken }> }>)[0]
    const node = para.content.find((n) => n.type === 'attachmentToken')
    expect(node?.attrs?.id).toBe('rt1')
    expect(node?.attrs?.fileName).toBe('a (b).pdf')
    expect(node?.attrs?.path).toBe('/p/a (b).pdf')
    expect(node?.attrs?.mimeType).toBe('application/pdf')
    editor2.destroy()
    editor.destroy()
  })

  it('attrs 不全（缺 id）的 HTML → parseHTML 拒绝 (节点不出现)', () => {
    const html = '<p><span data-rich-composer-attachment-token data-file-name="x.pdf"></span></p>'
    const editor = new Editor({ extensions: [StarterKit, AttachmentTokenExtension], content: html })
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: Array<{ type: string }> }>)[0]
    const hasToken = (para.content ?? []).some((n) => n.type === 'attachmentToken')
    expect(hasToken).toBe(false)
    editor.destroy()
  })
})
```

- [ ] **Step 1: 写测试文件 + extension 文件骨架（让测试可运行但失败）**

Create the test file from above.
Create `attachmentTokenExtension.ts` from above.

- [ ] **Step 2: 运行测试**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/attachmentTokenExtension.test.ts`
Expected: 5 tests pass.

如果失败：诊断按下面的常见问题分支处理：
- jsdom 不支持 ProseMirror DOM 操作 → 看错误是否含 `getSelection` / `Range`：通常 vitest jsdom env 已经支持。如果遇到，把测试改成只用 `editor.commands.*` API + `editor.getJSON()`，避免触发 selection serializer。
- `insertAttachmentTokens` 命令未注册 → 检查 `addCommands` 的 declare module 写法。

- [ ] **Step 3: 提交**

```bash
git add src/components/rich-composer/attachmentTokenExtension.ts src/components/rich-composer/__tests__/attachmentTokenExtension.test.ts
git commit -m "feat(rich-composer): attachmentToken Tiptap node + insertAttachmentTokens command + HTML round-trip"
```

---

### Task 4: composerSchema — 扩展集合工厂

**Files:**
- Create: `src/components/rich-composer/composerSchema.ts`
- Create: `src/components/rich-composer/__tests__/composerSchema.test.ts`

把所有要给 RichComposer 用的扩展打包成一个工厂函数，方便 P2 复用 + 测试解耦。

```ts
// src/components/rich-composer/composerSchema.ts
import StarterKit from '@tiptap/starter-kit'
import Link from '@tiptap/extension-link'
import Placeholder from '@tiptap/extension-placeholder'
import { AttachmentTokenExtension } from './attachmentTokenExtension'

export interface BuildComposerExtensionsOptions {
  placeholder?: string
}

export function buildComposerExtensions(options: BuildComposerExtensionsOptions = {}) {
  return [
    StarterKit.configure({
      // We allow blockquote / codeBlock / lists / bold / italic / strike / code via StarterKit defaults.
      // Disable heading and horizontalRule — not allowed by spec.
      heading: false,
      horizontalRule: false,
    }),
    Link.configure({
      openOnClick: false,
      autolink: true,
      linkOnPaste: true,
    }),
    Placeholder.configure({
      placeholder: options.placeholder ?? '',
    }),
    AttachmentTokenExtension,
  ]
}
```

#### 集成测试 — JSON → P0 serializer 端到端

```ts
// src/components/rich-composer/__tests__/composerSchema.test.ts
import { describe, expect, it } from 'vitest'
import { Editor } from '@tiptap/core'
import { buildComposerExtensions } from '../composerSchema'
import { serializeComposerDoc } from '../serializer'
import type { ComposerAttachmentToken, ComposerJsonNode } from '../types'

const mkToken = (overrides: Partial<ComposerAttachmentToken> = {}): ComposerAttachmentToken => ({
  id: 'a1',
  fileName: 'plan.pdf',
  path: '/p/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 1,
  source: 'picker',
  ...overrides,
})

function makeEditor(content = '<p></p>') {
  return new Editor({ extensions: buildComposerExtensions(), content })
}

describe('composerSchema 端到端：editor → P0 serializer', () => {
  it('插入 plain text + 调用 serializer 输出 markdown', () => {
    const editor = makeEditor()
    editor.commands.setContent('<p>hello world</p>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('hello world')
    editor.destroy()
  })

  it('粗体 HTML → markdown **text**', () => {
    const editor = makeEditor()
    editor.commands.setContent('<p>hi <strong>there</strong></p>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('hi **there**')
    editor.destroy()
  })

  it('bullet list → markdown - item', () => {
    const editor = makeEditor()
    editor.commands.setContent('<ul><li><p>a</p></li><li><p>b</p></li></ul>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('- a\n- b')
    editor.destroy()
  })

  it('blockquote → markdown > line', () => {
    const editor = makeEditor()
    editor.commands.setContent('<blockquote><p>note</p></blockquote>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('> note')
    editor.destroy()
  })

  it('codeBlock with language → fenced code block', () => {
    const editor = makeEditor()
    editor.commands.setContent('<pre><code class="language-ts">let x = 1</code></pre>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    const md = serializeComposerDoc(json).markdown
    // StarterKit 默认 codeBlock 不一定回填 language attr — 此处接受任一种
    expect(md.startsWith('```')).toBe(true)
    expect(md).toContain('let x = 1')
    expect(md.endsWith('```')).toBe(true)
    editor.destroy()
  })

  it('link mark → markdown [txt](url)', () => {
    const editor = makeEditor()
    editor.commands.setContent('<p><a href="https://example.com">click</a></p>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('[click](https://example.com)')
    editor.destroy()
  })

  it('插入 attachmentToken + 文本 → markdown 占位符 + 文本', () => {
    const editor = makeEditor()
    editor.commands.insertAttachmentTokens([mkToken()])
    editor.commands.insertContent(' 你好')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    const result = serializeComposerDoc(json)
    expect(result.markdown).toBe('[附件: plan.pdf](file:///p/plan.pdf) 你好')
    expect(result.attachments[0].id).toBe('a1')
    editor.destroy()
  })

  it('禁用 heading：粘贴 h1 → 退化为段落', () => {
    const editor = makeEditor()
    editor.commands.setContent('<h1>Big</h1>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    const md = serializeComposerDoc(json).markdown
    // 不应该以 # 开头
    expect(md.startsWith('#')).toBe(false)
    expect(md).toContain('Big')
    editor.destroy()
  })
})
```

- [ ] **Step 1: 写文件**

Create `composerSchema.ts` and `__tests__/composerSchema.test.ts`.

- [ ] **Step 2: 运行测试**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/composerSchema.test.ts`
Expected: PASS — 8 tests.

如果某个测试失败：
- "禁用 heading" 测试可能受 StarterKit `heading: false` 实现影响，如果 heading 仍出现，把测试断言改成"`md` 不以 `# ` 开头"或调整为 `headings: { levels: [] }`。先尝试 `heading: false`。
- "codeBlock with language" 测试 StarterKit 默认有 `CodeBlock`（不是 `CodeBlockLowlight`），可能不保留 `class="language-ts"`。所以接受 `md.startsWith('```')` 即可，不强求 ts。

- [ ] **Step 3: 提交**

```bash
git add src/components/rich-composer/composerSchema.ts src/components/rich-composer/__tests__/composerSchema.test.ts
git commit -m "feat(rich-composer): buildComposerExtensions factory (StarterKit + Link + Placeholder + AttachmentToken)"
```

---

### Task 5: index.ts re-export 与最终验证

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
```

- [ ] **Step 2: 全量跑测**

Run: `pnpm test src/components/rich-composer/`
Expected: PASS — 38 (P0) + 6 (NodeView) + 5 (extension) + 8 (schema) = 57 tests.

- [ ] **Step 3: tsc**

Run: `pnpm exec tsc -b --noEmit`
Expected: exit 0

- [ ] **Step 4: lint（仅 rich-composer 目录范围）**

Run: `pnpm lint 2>&1 | grep -E "rich-composer"`
Expected: 0 errors / 0 warnings under `src/components/rich-composer/`。仓库其它 14 个 pre-existing 错误与本次无关。

- [ ] **Step 5: 提交**

```bash
git add src/components/rich-composer/index.ts
git commit -m "chore(rich-composer): export tiptap extension + view + schema factory"
```

---

## Self-Review

**1. Spec coverage（spec 的"Tiptap 文档模型"和"架构设计"章节）：**

- ✅ 装 Tiptap 5 个依赖 → Task 1
- ✅ Schema：doc / paragraph / text / hardBreak / blockquote / codeBlock / bulletList / orderedList / listItem 由 StarterKit 提供 → Task 4
- ✅ Marks：bold / italic / code / strike / link 由 StarterKit + Link 提供 → Task 4
- ✅ 不允许 heading / horizontalRule → Task 4 `StarterKit.configure({ heading: false, horizontalRule: false })`
- ✅ attachmentToken inline atom + atom + selectable + draggable → Task 3
- ✅ attrs id/fileName/path/kind/fileType/fileSize/mimeType?/source → Task 3
- ✅ HTML round-trip via `data-*` → Task 3
- ✅ NodeView chip + delete button + selected ring → Task 2
- ✅ `insertAttachmentTokens` command (批量、空格分隔) → Task 3
- ✅ 与 P0 serializer 端到端兼容 → Task 4 集成测试
- ⚠️ 不做：完整 RichComposer.tsx 组件、Placeholder UI 接入页面、IME/Enter/Shift+Enter 行为（这些都在 P2）

**2. Placeholder scan：** 全部 step 含完整代码或完整命令；测试全部含 expect 行；commit message 全部具体。

**3. Type consistency：** 
- `ComposerAttachmentToken` 来自 P0 types.ts，三个 task 全部一致使用。
- `mimeType` 在 NodeView 用 string、在 extension attrs 用 `string | null`、P0 type 是 `string | undefined`：在 `parseHTML` 里把 `el.getAttribute('data-mime-type') ?? null` 显式转 null，避免 attrs 里出现 undefined。NodeView 用 `attrs.mimeType` 时通过 truthy 检查避开 null/undefined 差异。
- `insertAttachmentTokens` 名字一致。
- `buildComposerExtensions` 名字一致，`BuildComposerExtensionsOptions` 名字一致。

**4. 范围：** 严格 P1。RichComposer.tsx 不做。
