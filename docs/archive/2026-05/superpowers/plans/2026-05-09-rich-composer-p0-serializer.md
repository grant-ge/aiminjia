# RichComposer P0 — Tiptap JSON → Markdown Serializer 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现纯函数 serializer，把 RichComposer 的 Tiptap JSON 文档转换成 markdown 字符串 + 有序 attachments 数组，作为 `RichComposerSubmitPayload`。

**Architecture:** 一份 TypeScript 模块（`src/components/rich-composer/serializer.ts`），输入是 Tiptap 的 ProseMirror-JSON 文档树（`{ type: 'doc', content: [...] }` 形态的 plain object），输出 `{ markdown, attachments, isEmpty }`。**不依赖** `@tiptap/*` 运行时；只对 JSON 做结构化遍历。所有规则与 spec `2026-05-07-rich-composer-tiptap-design.md` 的"提交序列化"章节一致。

**Tech Stack:** TypeScript + Vitest（jsdom 环境，但本模块不需要 DOM）。spec 路径：`docs/superpowers/specs/2026-05-07-rich-composer-tiptap-design.md`。

---

## 文件结构

- 新增：
  - `src/components/rich-composer/types.ts` — 共享类型（`ComposerAttachmentToken`、`RichComposerSubmitPayload`、内部 `ComposerJsonNode`）
  - `src/components/rich-composer/serializer.ts` — 纯函数 `serializeComposerDoc(doc)`
  - `src/components/rich-composer/index.ts` — re-export
  - `src/components/rich-composer/__tests__/serializer.test.ts` — 全量单测

P0 不创建 `RichComposer.tsx` / `pastePipeline.ts` / `attachmentTokenExtension.ts`；这些留给 P1 之后。

## 类型契约

`ComposerAttachmentToken`（与 spec attrs 一致）：

```ts
export type ComposerAttachmentTokenKind = 'file' | 'folder' | 'image'
export type ComposerAttachmentTokenFileType =
  | 'image' | 'excel' | 'word' | 'pdf' | 'json' | 'csv' | 'folder'

export interface ComposerAttachmentToken {
  id: string
  fileName: string
  path: string
  kind: ComposerAttachmentTokenKind
  fileType: ComposerAttachmentTokenFileType
  fileSize: number
  mimeType?: string
  source: 'picker' | 'paste' | 'drop' | 'clipboard-image'
}
```

`RichComposerSubmitPayload`：

```ts
export interface RichComposerSubmitPayload {
  markdown: string
  attachments: ComposerAttachmentToken[]
  isEmpty: boolean
}
```

Tiptap JSON 节点（结构化对象，不引用 `@tiptap/*`）：

```ts
export interface ComposerMark {
  type: 'bold' | 'italic' | 'code' | 'strike' | 'link'
  attrs?: { href?: string; [k: string]: unknown }
}

export interface ComposerJsonNode {
  type:
    | 'doc' | 'paragraph' | 'text' | 'hardBreak'
    | 'blockquote' | 'codeBlock'
    | 'bulletList' | 'orderedList' | 'listItem'
    | 'attachmentToken'
  content?: ComposerJsonNode[]
  text?: string
  marks?: ComposerMark[]
  attrs?: Record<string, unknown>
}
```

## 序列化规则（节点 → markdown）

| 节点类型 | markdown |
|---|---|
| `doc` | 顺序拼接 children，块之间 `\n\n` |
| `paragraph` | inline children 拼接，独立段落 |
| `text` | escape 后输出，按 marks 包裹 |
| `hardBreak` | 行尾两个空格 + `\n` |
| `blockquote` | 每行前缀 `> `（含空行） |
| `codeBlock` | ` ```language\n...\n``` `（无 language attr 时三反引号无后缀） |
| `bulletList` | 每个 `listItem` 行首 `- ` |
| `orderedList` | 每个 `listItem` 行首 `1. ` `2. ` ... |
| `listItem` | 内部段落拼接，多段落时缩进 4 空格 |
| `attachmentToken` (kind ≠ image) | `[附件: fileName](file://path)` |
| `attachmentToken` (kind = image) | `![fileName](file://path)` |

Mark 包裹优先级（从内到外）：`code > bold > italic > strike > link`。理由：`code` 内的 `*` 不应被解释为 italic；link 是最外层，能正确包住带 mark 的链接文本。

Text escape：对 `\` `*` `_` `[` `]` `` ` `` `<` `>` 转义。escape 仅作用于 `text` 节点；`codeBlock`、`code` mark 内的内容不 escape。

文件名/路径里的特殊字符：在生成 `[附件: fileName](file://path)` 时，对 `fileName` 做 `[` / `]` / `\` escape；对 `path` 做 `(` / `)` / `\` escape，并对路径中的非 ASCII 不做编码（保持人类可读，markdown 渲染器接受 unicode URL）。

attachments 数组：按节点遍历顺序收集；同 `id` 去重保留首次出现的位置；同 `id` 的 markdown 占位符保留每次出现（"内容 + 路径双轨"）。

`isEmpty`：当且仅当 markdown 中没有非空白文本、且 attachments 数组为空时为 true。

## 测试覆盖（17 项）

每个测试用一个 helper 直接构造最小 ProseMirror JSON。所有测试独立，不依赖 Tiptap 运行时。

---

### Task 1: 文件骨架与共享类型

**Files:**
- Create: `src/components/rich-composer/types.ts`
- Create: `src/components/rich-composer/index.ts`

- [ ] **Step 1: 创建 `src/components/rich-composer/types.ts`**

```ts
// src/components/rich-composer/types.ts

export type ComposerAttachmentTokenKind = 'file' | 'folder' | 'image'

export type ComposerAttachmentTokenFileType =
  | 'image'
  | 'excel'
  | 'word'
  | 'pdf'
  | 'json'
  | 'csv'
  | 'folder'

export interface ComposerAttachmentToken {
  id: string
  fileName: string
  path: string
  kind: ComposerAttachmentTokenKind
  fileType: ComposerAttachmentTokenFileType
  fileSize: number
  mimeType?: string
  source: 'picker' | 'paste' | 'drop' | 'clipboard-image'
}

export interface RichComposerSubmitPayload {
  markdown: string
  attachments: ComposerAttachmentToken[]
  isEmpty: boolean
}

export type ComposerMarkType = 'bold' | 'italic' | 'code' | 'strike' | 'link'

export interface ComposerMark {
  type: ComposerMarkType
  attrs?: { href?: string; [k: string]: unknown }
}

export type ComposerJsonNodeType =
  | 'doc'
  | 'paragraph'
  | 'text'
  | 'hardBreak'
  | 'blockquote'
  | 'codeBlock'
  | 'bulletList'
  | 'orderedList'
  | 'listItem'
  | 'attachmentToken'

export interface ComposerJsonNode {
  type: ComposerJsonNodeType
  content?: ComposerJsonNode[]
  text?: string
  marks?: ComposerMark[]
  attrs?: Record<string, unknown>
}
```

- [ ] **Step 2: 创建 `src/components/rich-composer/index.ts`**

```ts
// src/components/rich-composer/index.ts
export * from './types'
export { serializeComposerDoc } from './serializer'
```

- [ ] **Step 3: 提交（serializer 文件下一 task 才创建，index.ts 暂时引用未创建模块——本步先不 commit，留到 Task 2 测试-红绿后一起 commit）**

跳过 commit，进入 Task 2。

---

### Task 2: 空文档 / 纯文本基线

**Files:**
- Create: `src/components/rich-composer/serializer.ts`
- Create: `src/components/rich-composer/__tests__/serializer.test.ts`

- [ ] **Step 1: 写第一批失败测试**

```ts
// src/components/rich-composer/__tests__/serializer.test.ts
import { describe, expect, it } from 'vitest'
import { serializeComposerDoc } from '../serializer'
import type { ComposerJsonNode } from '../types'

const doc = (...content: ComposerJsonNode[]): ComposerJsonNode => ({ type: 'doc', content })
const p = (...content: ComposerJsonNode[]): ComposerJsonNode => ({ type: 'paragraph', content })
const t = (text: string, marks?: ComposerJsonNode['marks']): ComposerJsonNode => ({
  type: 'text',
  text,
  marks,
})

describe('serializeComposerDoc — 空 / 纯文本', () => {
  it('空 doc → markdown=""，isEmpty=true', () => {
    const result = serializeComposerDoc(doc())
    expect(result).toEqual({ markdown: '', attachments: [], isEmpty: true })
  })

  it('空 paragraph → markdown=""，isEmpty=true', () => {
    const result = serializeComposerDoc(doc(p()))
    expect(result).toEqual({ markdown: '', attachments: [], isEmpty: true })
  })

  it('单段纯文本 → 原样输出，isEmpty=false', () => {
    const result = serializeComposerDoc(doc(p(t('hello world'))))
    expect(result).toEqual({ markdown: 'hello world', attachments: [], isEmpty: false })
  })

  it('多段段落用 \\n\\n 分隔', () => {
    const result = serializeComposerDoc(doc(p(t('first')), p(t('second'))))
    expect(result.markdown).toBe('first\n\nsecond')
  })

  it('hardBreak → 行尾两空格 + \\n', () => {
    const result = serializeComposerDoc(
      doc(p(t('line1'), { type: 'hardBreak' }, t('line2')))
    )
    expect(result.markdown).toBe('line1  \nline2')
  })
})
```

- [ ] **Step 2: 运行测试，确认全部失败（模块不存在）**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: FAIL — `Failed to resolve "../serializer"`

- [ ] **Step 3: 创建 `src/components/rich-composer/serializer.ts` 最小实现**

```ts
// src/components/rich-composer/serializer.ts
import type {
  ComposerAttachmentToken,
  ComposerJsonNode,
  RichComposerSubmitPayload,
} from './types'

export function serializeComposerDoc(doc: ComposerJsonNode): RichComposerSubmitPayload {
  const attachments: ComposerAttachmentToken[] = []
  const markdown = renderBlocks(doc.content ?? [], attachments)
  const isEmpty = markdown.trim().length === 0 && attachments.length === 0
  return { markdown, attachments, isEmpty }
}

function renderBlocks(nodes: ComposerJsonNode[], attachments: ComposerAttachmentToken[]): string {
  const parts: string[] = []
  for (const node of nodes) {
    parts.push(renderBlock(node, attachments))
  }
  return parts.filter((s) => s.length > 0).join('\n\n')
}

function renderBlock(node: ComposerJsonNode, attachments: ComposerAttachmentToken[]): string {
  switch (node.type) {
    case 'paragraph':
      return renderInline(node.content ?? [], attachments)
    default:
      return ''
  }
}

function renderInline(nodes: ComposerJsonNode[], _attachments: ComposerAttachmentToken[]): string {
  const parts: string[] = []
  for (const node of nodes) {
    if (node.type === 'text') {
      parts.push(node.text ?? '')
    } else if (node.type === 'hardBreak') {
      parts.push('  \n')
    }
  }
  return parts.join('')
}
```

- [ ] **Step 4: 运行测试，确认全部通过**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: PASS — 5 tests

- [ ] **Step 5: 提交**

```bash
git add src/components/rich-composer/
git commit -m "feat(rich-composer): scaffold types + serializer baseline (text + paragraphs + hardBreak)"
```

---

### Task 3: Inline marks（bold / italic / strike / inline code / link）

**Files:**
- Modify: `src/components/rich-composer/serializer.ts`
- Modify: `src/components/rich-composer/__tests__/serializer.test.ts`

- [ ] **Step 1: 追加失败测试**

```ts
// 追加到 serializer.test.ts 末尾
describe('serializeComposerDoc — inline marks', () => {
  it('bold → **text**', () => {
    const result = serializeComposerDoc(doc(p(t('hi', [{ type: 'bold' }]))))
    expect(result.markdown).toBe('**hi**')
  })

  it('italic → *text*', () => {
    const result = serializeComposerDoc(doc(p(t('hi', [{ type: 'italic' }]))))
    expect(result.markdown).toBe('*hi*')
  })

  it('strike → ~~text~~', () => {
    const result = serializeComposerDoc(doc(p(t('hi', [{ type: 'strike' }]))))
    expect(result.markdown).toBe('~~hi~~')
  })

  it('inline code → `text`，且 code 内不被 italic 包', () => {
    const result = serializeComposerDoc(
      doc(p(t('x', [{ type: 'code' }, { type: 'italic' }])))
    )
    // code 是最内层，italic 在外
    expect(result.markdown).toBe('*`x`*')
  })

  it('link → [text](url)', () => {
    const result = serializeComposerDoc(
      doc(p(t('点这里', [{ type: 'link', attrs: { href: 'https://example.com' } }])))
    )
    expect(result.markdown).toBe('[点这里](https://example.com)')
  })

  it('link 包 bold：bold 在内、link 在外', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          t('hi', [
            { type: 'bold' },
            { type: 'link', attrs: { href: 'https://example.com' } },
          ])
        )
      )
    )
    expect(result.markdown).toBe('[**hi**](https://example.com)')
  })

  it('混合 marks：code + bold + italic + strike + link', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          t('x', [
            { type: 'code' },
            { type: 'bold' },
            { type: 'italic' },
            { type: 'strike' },
            { type: 'link', attrs: { href: 'https://e.com' } },
          ])
        )
      )
    )
    // 由内到外：code → bold → italic → strike → link
    expect(result.markdown).toBe('[~~***`x`***~~](https://e.com)')
  })
})
```

- [ ] **Step 2: 运行测试，确认新增 7 项失败**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: FAIL — 至少 7 项 mark 相关失败

- [ ] **Step 3: 替换 `renderInline` 为支持 marks 的版本**

把 `serializer.ts` 里的 `renderInline` 替换为：

```ts
const MARK_ORDER: Array<'code' | 'bold' | 'italic' | 'strike' | 'link'> = [
  'code',
  'bold',
  'italic',
  'strike',
  'link',
]

function renderInline(
  nodes: ComposerJsonNode[],
  attachments: ComposerAttachmentToken[],
): string {
  const parts: string[] = []
  for (const node of nodes) {
    if (node.type === 'text') {
      parts.push(renderText(node))
    } else if (node.type === 'hardBreak') {
      parts.push('  \n')
    } else if (node.type === 'attachmentToken') {
      parts.push(renderAttachmentToken(node, attachments))
    }
  }
  return parts.join('')
}

function renderText(node: ComposerJsonNode): string {
  const raw = node.text ?? ''
  const marks = node.marks ?? []
  const hasCode = marks.some((m) => m.type === 'code')
  // text inside `code` mark must not be markdown-escaped (it's verbatim)
  let result = hasCode ? raw : escapeMarkdownText(raw)
  for (const markType of MARK_ORDER) {
    const mark = marks.find((m) => m.type === markType)
    if (!mark) continue
    result = wrapMark(result, markType, mark)
  }
  return result
}

function wrapMark(
  text: string,
  type: 'code' | 'bold' | 'italic' | 'strike' | 'link',
  mark: ComposerMark,
): string {
  switch (type) {
    case 'code':
      return '`' + text + '`'
    case 'bold':
      return '**' + text + '**'
    case 'italic':
      return '*' + text + '*'
    case 'strike':
      return '~~' + text + '~~'
    case 'link': {
      const href = typeof mark.attrs?.href === 'string' ? mark.attrs.href : ''
      return '[' + text + '](' + escapeUrl(href) + ')'
    }
  }
}

function escapeMarkdownText(text: string): string {
  return text.replace(/([\\*_`\[\]<>])/g, '\\$1')
}

function escapeUrl(url: string): string {
  return url.replace(/([\\()])/g, '\\$1')
}

// 占位实现，Task 6 实现真正逻辑
function renderAttachmentToken(
  _node: ComposerJsonNode,
  _attachments: ComposerAttachmentToken[],
): string {
  return ''
}
```

并在文件顶部 import 中追加：

```ts
import type {
  ComposerAttachmentToken,
  ComposerJsonNode,
  ComposerMark,
  RichComposerSubmitPayload,
} from './types'
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: PASS — 12 tests

- [ ] **Step 5: 提交**

```bash
git add src/components/rich-composer/
git commit -m "feat(rich-composer): serializer supports inline marks (bold/italic/strike/code/link)"
```

---

### Task 4: Markdown 特殊字符 escape

**Files:**
- Modify: `src/components/rich-composer/__tests__/serializer.test.ts`

Task 3 里已实现 escape 函数，本 task 锁定其行为契约。

- [ ] **Step 1: 追加 escape 测试**

```ts
describe('serializeComposerDoc — markdown 特殊字符 escape', () => {
  it('escape * _ ` [ ] \\ < >', () => {
    const result = serializeComposerDoc(
      doc(p(t('a*b_c`d[e]f\\g<h>')))
    )
    expect(result.markdown).toBe('a\\*b\\_c\\`d\\[e\\]f\\\\g\\<h\\>')
  })

  it('inline code mark 内部不 escape markdown 特殊字符', () => {
    const result = serializeComposerDoc(
      doc(p(t('a*b', [{ type: 'code' }])))
    )
    expect(result.markdown).toBe('`a*b`')
  })
})
```

- [ ] **Step 2: 运行测试，应该全部通过（Task 3 已实现）**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: PASS — 14 tests

- [ ] **Step 3: 提交**

```bash
git add src/components/rich-composer/__tests__/serializer.test.ts
git commit -m "test(rich-composer): lock serializer escape contract"
```

---

### Task 5: 块级节点（blockquote / codeBlock / bulletList / orderedList）

**Files:**
- Modify: `src/components/rich-composer/serializer.ts`
- Modify: `src/components/rich-composer/__tests__/serializer.test.ts`

- [ ] **Step 1: 追加失败测试**

```ts
describe('serializeComposerDoc — 块级节点', () => {
  const blockquote = (...content: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'blockquote',
    content,
  })
  const codeBlock = (text: string, language?: string): ComposerJsonNode => ({
    type: 'codeBlock',
    attrs: language ? { language } : undefined,
    content: [{ type: 'text', text }],
  })
  const ul = (...items: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'bulletList',
    content: items,
  })
  const ol = (...items: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'orderedList',
    content: items,
  })
  const li = (...content: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'listItem',
    content,
  })

  it('blockquote 单段 → 行首 > ', () => {
    const result = serializeComposerDoc(doc(blockquote(p(t('hello')))))
    expect(result.markdown).toBe('> hello')
  })

  it('blockquote 多段 → 每行 > 前缀，段间 > 空行', () => {
    const result = serializeComposerDoc(doc(blockquote(p(t('a')), p(t('b')))))
    expect(result.markdown).toBe('> a\n>\n> b')
  })

  it('codeBlock 带 language', () => {
    const result = serializeComposerDoc(doc(codeBlock('let x = 1', 'ts')))
    expect(result.markdown).toBe('```ts\nlet x = 1\n```')
  })

  it('codeBlock 无 language', () => {
    const result = serializeComposerDoc(doc(codeBlock('plain')))
    expect(result.markdown).toBe('```\nplain\n```')
  })

  it('codeBlock 内的 markdown 特殊字符不 escape', () => {
    const result = serializeComposerDoc(doc(codeBlock('a*b_c[d]', 'ts')))
    expect(result.markdown).toBe('```ts\na*b_c[d]\n```')
  })

  it('bulletList 多项', () => {
    const result = serializeComposerDoc(
      doc(ul(li(p(t('a'))), li(p(t('b'))), li(p(t('c')))))
    )
    expect(result.markdown).toBe('- a\n- b\n- c')
  })

  it('orderedList 多项 → 1. 2. 3.', () => {
    const result = serializeComposerDoc(
      doc(ol(li(p(t('a'))), li(p(t('b'))), li(p(t('c')))))
    )
    expect(result.markdown).toBe('1. a\n2. b\n3. c')
  })

  it('listItem 多段 → 续段缩进 4 空格', () => {
    const result = serializeComposerDoc(
      doc(ul(li(p(t('first line')), p(t('second line')))))
    )
    expect(result.markdown).toBe('- first line\n\n    second line')
  })
})
```

- [ ] **Step 2: 运行测试，确认 8 项失败**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: FAIL — 8 项块级失败

- [ ] **Step 3: 在 `serializer.ts` 的 `renderBlock` 里加分支**

把 `renderBlock` 替换为：

```ts
function renderBlock(node: ComposerJsonNode, attachments: ComposerAttachmentToken[]): string {
  switch (node.type) {
    case 'paragraph':
      return renderInline(node.content ?? [], attachments)
    case 'blockquote':
      return renderBlockquote(node, attachments)
    case 'codeBlock':
      return renderCodeBlock(node)
    case 'bulletList':
      return renderList(node, attachments, false)
    case 'orderedList':
      return renderList(node, attachments, true)
    default:
      return ''
  }
}

function renderBlockquote(node: ComposerJsonNode, attachments: ComposerAttachmentToken[]): string {
  const inner = renderBlocks(node.content ?? [], attachments)
  return inner
    .split('\n')
    .map((line) => (line.length === 0 ? '>' : '> ' + line))
    .join('\n')
}

function renderCodeBlock(node: ComposerJsonNode): string {
  const language = typeof node.attrs?.language === 'string' ? node.attrs.language : ''
  const text = (node.content ?? [])
    .filter((c) => c.type === 'text')
    .map((c) => c.text ?? '')
    .join('')
  return '```' + language + '\n' + text + '\n```'
}

function renderList(
  node: ComposerJsonNode,
  attachments: ComposerAttachmentToken[],
  ordered: boolean,
): string {
  const items = node.content ?? []
  const lines: string[] = []
  items.forEach((item, idx) => {
    const marker = ordered ? `${idx + 1}. ` : '- '
    const itemText = renderBlocks(item.content ?? [], attachments)
    const indented = itemText
      .split('\n')
      .map((line, lineIdx) => (lineIdx === 0 ? marker + line : '    ' + line))
      .join('\n')
    lines.push(indented)
  })
  return lines.join('\n')
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: PASS — 22 tests

- [ ] **Step 5: 提交**

```bash
git add src/components/rich-composer/
git commit -m "feat(rich-composer): serializer supports blockquote / codeBlock / bullet & ordered lists"
```

---

### Task 6: AttachmentToken 序列化与 attachments 收集

**Files:**
- Modify: `src/components/rich-composer/serializer.ts`
- Modify: `src/components/rich-composer/__tests__/serializer.test.ts`

- [ ] **Step 1: 追加失败测试**

```ts
describe('serializeComposerDoc — attachmentToken', () => {
  const tokenAttrs = (overrides: Partial<ComposerJsonNode['attrs']> = {}): ComposerJsonNode['attrs'] => ({
    id: 'a1',
    fileName: 'report.pdf',
    path: '/abs/report.pdf',
    kind: 'file',
    fileType: 'pdf',
    fileSize: 1024,
    source: 'picker',
    ...overrides,
  })
  const at = (overrides: Partial<ComposerJsonNode['attrs']> = {}): ComposerJsonNode => ({
    type: 'attachmentToken',
    attrs: tokenAttrs(overrides),
  })

  it('单个非图片 token → [附件: name](file://path)', () => {
    const result = serializeComposerDoc(doc(p(at())))
    expect(result.markdown).toBe('[附件: report.pdf](file:///abs/report.pdf)')
    expect(result.attachments).toHaveLength(1)
    expect(result.attachments[0]).toMatchObject({
      id: 'a1',
      fileName: 'report.pdf',
      path: '/abs/report.pdf',
      kind: 'file',
      fileType: 'pdf',
      fileSize: 1024,
      source: 'picker',
    })
    expect(result.isEmpty).toBe(false)
  })

  it('image token → ![name](file://path)', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          at({
            id: 'img1',
            fileName: 'a.png',
            path: '/abs/a.png',
            kind: 'image',
            fileType: 'image',
          })
        )
      )
    )
    expect(result.markdown).toBe('![a.png](file:///abs/a.png)')
  })

  it('folder token → kind=folder 仍走非图片占位符', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          at({
            id: 'f1',
            fileName: 'docs',
            path: '/abs/docs',
            kind: 'folder',
            fileType: 'folder',
          })
        )
      )
    )
    expect(result.markdown).toBe('[附件: docs](file:///abs/docs)')
  })

  it('文本 + token + 文本，按文档顺序', () => {
    const result = serializeComposerDoc(
      doc(p(t('请分析 '), at(), t(' 谢谢')))
    )
    expect(result.markdown).toBe('请分析 [附件: report.pdf](file:///abs/report.pdf) 谢谢')
    expect(result.attachments.map((a) => a.id)).toEqual(['a1'])
  })

  it('多个不同 token 按出现顺序收集', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'a' }), at({ id: 'b' }), at({ id: 'c' })))
    )
    expect(result.attachments.map((a) => a.id)).toEqual(['a', 'b', 'c'])
  })

  it('同 id token 出现多次：markdown 保留多处占位符，attachments 去重', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'dup' }), t(' 和 '), at({ id: 'dup' })))
    )
    expect(result.markdown.match(/附件: report\.pdf/g)).toHaveLength(2)
    expect(result.attachments).toHaveLength(1)
    expect(result.attachments[0].id).toBe('dup')
  })

  it('只附件提交：markdown 是占位符串联，isEmpty=false', () => {
    const result = serializeComposerDoc(doc(p(at({ id: 'a' }), t(' '), at({ id: 'b' }))))
    expect(result.isEmpty).toBe(false)
    expect(result.attachments.map((a) => a.id)).toEqual(['a', 'b'])
  })

  it('文件名含 [ ] \\ 时 escape', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'x', fileName: 'a[1]\\b.pdf', path: '/p/a[1]\\b.pdf' })))
    )
    expect(result.markdown).toBe('[附件: a\\[1\\]\\\\b.pdf](file:///p/a\\[1\\]\\\\b.pdf)')
  })

  it('路径含 ( ) 时 escape', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'x', fileName: 'name', path: '/a (b)/c.pdf' })))
    )
    expect(result.markdown).toBe('[附件: name](file:///a \\(b\\)/c.pdf)')
  })
})
```

- [ ] **Step 2: 运行测试，确认 9 项失败**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: FAIL — 9 项 attachmentToken 失败

- [ ] **Step 3: 实现 `renderAttachmentToken` 与 attachments 收集**

替换 `serializer.ts` 里的 `renderAttachmentToken` 占位实现，并把 escape 工具扩展到文件名/路径。

```ts
function renderAttachmentToken(
  node: ComposerJsonNode,
  attachments: ComposerAttachmentToken[],
): string {
  const token = readAttachmentTokenAttrs(node)
  if (!token) return ''
  // dedupe by id, preserve first occurrence
  if (!attachments.some((existing) => existing.id === token.id)) {
    attachments.push(token)
  }
  const safeName = escapeMarkdownLinkText(token.fileName)
  const safePath = escapeUrl(toFileUrl(token.path))
  if (token.kind === 'image') {
    return `![${safeName}](${safePath})`
  }
  return `[附件: ${safeName}](${safePath})`
}

function readAttachmentTokenAttrs(node: ComposerJsonNode): ComposerAttachmentToken | null {
  const attrs = node.attrs ?? {}
  const id = typeof attrs.id === 'string' ? attrs.id : null
  const fileName = typeof attrs.fileName === 'string' ? attrs.fileName : null
  const path = typeof attrs.path === 'string' ? attrs.path : null
  const kind = attrs.kind === 'image' || attrs.kind === 'folder' || attrs.kind === 'file' ? attrs.kind : null
  const fileType = typeof attrs.fileType === 'string' ? (attrs.fileType as ComposerAttachmentToken['fileType']) : null
  const fileSize = typeof attrs.fileSize === 'number' ? attrs.fileSize : null
  const source = attrs.source === 'picker' || attrs.source === 'paste' || attrs.source === 'drop' || attrs.source === 'clipboard-image'
    ? attrs.source
    : null
  if (!id || !fileName || !path || !kind || !fileType || fileSize === null || !source) {
    return null
  }
  const mimeType = typeof attrs.mimeType === 'string' ? attrs.mimeType : undefined
  return { id, fileName, path, kind, fileType, fileSize, source, mimeType }
}

function escapeMarkdownLinkText(text: string): string {
  return text.replace(/([\\\[\]])/g, '\\$1')
}

function toFileUrl(path: string): string {
  // POSIX & Windows abs paths — keep human-readable; just ensure leading slash → file:///abs
  // path already starts with `/` on POSIX; Windows `C:\...` becomes `file://C:\...` (renderer-side)
  if (path.startsWith('/')) return 'file://' + path
  return 'file://' + path
}
```

注意：`escapeUrl` 已在 Task 3 实现，escape `\ ( )`。

- [ ] **Step 4: 运行测试，确认通过**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: PASS — 31 tests

- [ ] **Step 5: 提交**

```bash
git add src/components/rich-composer/
git commit -m "feat(rich-composer): serializer renders attachment tokens (file/image/folder) with deduped attachments"
```

---

### Task 7: 综合场景与 isEmpty 边界

**Files:**
- Modify: `src/components/rich-composer/__tests__/serializer.test.ts`

锁定 `isEmpty` 行为，并加一个 spec "请分析 [附件: report.pdf]" 风格的端到端样例。

- [ ] **Step 1: 追加测试**

```ts
describe('serializeComposerDoc — 综合 / isEmpty', () => {
  it('只有空白文本 → isEmpty=true', () => {
    const result = serializeComposerDoc(
      doc(p({ type: 'text', text: '   ' }))
    )
    expect(result.isEmpty).toBe(true)
  })

  it('只有 hardBreak → isEmpty=true', () => {
    const result = serializeComposerDoc(doc(p({ type: 'hardBreak' })))
    expect(result.isEmpty).toBe(true)
  })

  it('hardBreak + token → isEmpty=false', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          { type: 'hardBreak' },
          {
            type: 'attachmentToken',
            attrs: {
              id: 'x',
              fileName: 'a.pdf',
              path: '/p/a.pdf',
              kind: 'file',
              fileType: 'pdf',
              fileSize: 1,
              source: 'picker',
            },
          }
        )
      )
    )
    expect(result.isEmpty).toBe(false)
    expect(result.attachments).toHaveLength(1)
  })

  it('端到端：富文本 + 附件 + 图片 + 列表 + 引用', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          t('请帮我看看 ', undefined),
          {
            type: 'attachmentToken',
            attrs: {
              id: 'pdf1',
              fileName: 'plan.pdf',
              path: '/p/plan.pdf',
              kind: 'file',
              fileType: 'pdf',
              fileSize: 1,
              source: 'picker',
            },
          },
          t(' 和 ', undefined),
          {
            type: 'attachmentToken',
            attrs: {
              id: 'img1',
              fileName: 'chart.png',
              path: '/p/chart.png',
              kind: 'image',
              fileType: 'image',
              fileSize: 1,
              source: 'paste',
            },
          },
          t(' 的关系'),
        ),
        {
          type: 'bulletList',
          content: [
            { type: 'listItem', content: [p(t('重点 1'))] },
            { type: 'listItem', content: [p(t('重点 2'))] },
          ],
        },
        {
          type: 'blockquote',
          content: [p(t('备注'))],
        },
      ),
    )
    expect(result.markdown).toBe(
      '请帮我看看 [附件: plan.pdf](file:///p/plan.pdf) 和 ![chart.png](file:///p/chart.png) 的关系\n\n- 重点 1\n- 重点 2\n\n> 备注'
    )
    expect(result.attachments.map((a) => a.id)).toEqual(['pdf1', 'img1'])
    expect(result.isEmpty).toBe(false)
  })
})
```

- [ ] **Step 2: 运行测试**

Run: `pnpm exec vitest run src/components/rich-composer/__tests__/serializer.test.ts`
Expected: PASS — 35 tests

如果 "只有空白文本" 失败：当前 `isEmpty` 用 `markdown.trim().length === 0`，纯空白文本会先被 escape 写出（escape 不影响空格）。`'   '.trim() === ''` 为 true，应通过。

如果 "只有 hardBreak" 失败：markdown 会是 `'  \n'`，`trim()` 后为空，应通过。

- [ ] **Step 3: 提交**

```bash
git add src/components/rich-composer/__tests__/serializer.test.ts
git commit -m "test(rich-composer): lock isEmpty + end-to-end serializer scenarios"
```

---

### Task 8: 最终验证 + 总结提交

**Files:** 无新增

- [ ] **Step 1: 全量跑测**

Run: `pnpm test src/components/rich-composer/`
Expected: PASS — 35 tests, 0 failures

- [ ] **Step 2: tsc 类型检查**

Run: `pnpm exec tsc -b --noEmit`
Expected: PASS — 0 errors

如果有类型错误，定位到 `src/components/rich-composer/` 内部修复；不要为消错改其它文件。

- [ ] **Step 3: lint**

Run: `pnpm lint`
Expected: 没有 `src/components/rich-composer/` 相关 error/warning

如果有，按 lint 规则修复。

- [ ] **Step 4: 验证 P0 完成度**

打勾确认：
- [ ] `serializeComposerDoc` 是纯函数，不依赖 `@tiptap/*`
- [ ] 文件、image、folder 三种 attachment token 都有正确 markdown 输出
- [ ] attachments 数组按文档顺序、按 id 去重
- [ ] markdown 特殊字符 escape 与 codeBlock / inline code 不 escape 行为一致
- [ ] isEmpty 在"只空白文本/只 hardBreak"为 true，"含 token"为 false
- [ ] 全部 35 个单测通过

- [ ] **Step 5: 这一步无新文件改动，跳过额外 commit**

P0 已完成。下一阶段（P1：装 Tiptap 依赖 + 建 attachmentToken extension）单独立 plan。

---

## Self-Review

**1. Spec coverage（spec 中"提交序列化"章节）：**

- ✅ paragraph / hardBreak / 段落 → Task 2
- ✅ bold / italic / strike / inline code / link → Task 3
- ✅ markdown 特殊字符 escape → Task 4
- ✅ blockquote / codeBlock / bulletList / orderedList / listItem → Task 5
- ✅ attachmentToken (file/image/folder) → Task 6
- ✅ attachments 顺序 + id 去重 → Task 6
- ✅ isEmpty 规则 → Task 7
- ✅ 综合场景 → Task 7

**2. Placeholder scan：** 无 TBD/TODO；每步有完整代码。

**3. Type consistency：** `serializeComposerDoc` / `RichComposerSubmitPayload` / `ComposerAttachmentToken` 在 Task 1 定义，后续 Task 全部一致使用。`renderInline` / `renderBlock` / `renderText` / `wrapMark` / `escapeMarkdownText` / `escapeUrl` / `escapeMarkdownLinkText` / `renderAttachmentToken` / `readAttachmentTokenAttrs` / `toFileUrl` / `renderBlockquote` / `renderCodeBlock` / `renderList` / `renderBlocks` 名字在所有 Task 间一致。

**4. 范围：** 严格 P0；未越界引入 Tiptap / pastePipeline / RichComposer 组件。
