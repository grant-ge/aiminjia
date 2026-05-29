# RichComposer P6 — User Bubble Markdown 渲染实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace `UserMessageBubble`'s manual `ImageThumbnail` / `AttachmentIcon` / `whitespace-pre-wrap` rendering with markdown rendering. `file://` links render as attachment chips, `file://` images render as thumbnails. Skill chip render path stays.

**Architecture:** New `UserBubbleMarkdown` component that wraps `react-markdown` (already a dep) with custom `a` and `img` renderers. Keep dark-on-primary theme via theme variables. `UserMessageBubble.tsx` becomes a thin shell that dispatches to skill chip + `UserBubbleMarkdown`.

**Tech Stack:** `react-markdown` ^10.1.0 + `remark-gfm` ^4.0.1 (existing deps). `useLocalImageDataUrl` (existing) for `file://` thumbnail data URLs. `openLocalFile` / `useGeneratedFilePreviewStore.openPreview` (existing) for click-through.

---

## 文件结构

新增：
- `src/components/chat-scene/markdown/UserBubbleMarkdown.tsx` — markdown renderer with `file://` chip / image renderer.
- `src/components/chat-scene/markdown/__tests__/UserBubbleMarkdown.test.tsx` — render tests.

修改：
- `src/components/chat-scene/UserMessageBubble.tsx` — drop manual rendering, dispatch to `UserBubbleMarkdown`.

不修改：
- `AssistantMarkdown.tsx` (different theme, different requirements).
- `useLocalImageDataUrl.ts`.
- 后端 / Tauri command (markdown 字符串与 attachments 数组双轨，已经在 P5 落地).

## 关键决策

### Renderer 行为

`UserBubbleMarkdown` 接收 `text: string`、`conversationId?: string`、`files?: FileAttachment[]`。

```text
节点                          → 渲染
text / paragraph             → <p className="...">
**bold** / *italic* / etc.   → 标准 inline
[txt](url)
  url 以 file:// 开头         → 自定义 chip (<button> with FILE_TYPE_CONFIG icon + filename)，点击 openPreview / openLocalFile
  其它 (http/https)           → 普通外链 (className="underline underline-offset-2")
![alt](url)
  url 以 file:// 开头         → <ImageThumbnail> (existing useLocalImageDataUrl)
  其它                        → 原生 <img>
- list / 1. list             → <ul>/<ol>
> quote                      → <blockquote className="border-l-2 border-primary-foreground/40 ...">
``` code block               → <pre><code> with bg-primary-foreground/10
```

### 主题（深底浅字）

bubble 外层是 `bg-primary text-primary-foreground`（已有）。`UserBubbleMarkdown` 内只用 primary-foreground 系列变量与半透明叠加：

| 节点 | 样式 |
|---|---|
| 段落 | `leading-relaxed` |
| 内联 code | `rounded bg-primary-foreground/15 px-1` |
| 链接（非 file://） | `underline underline-offset-2` |
| 链接（file://，非 image） | 自定义 chip：`inline-flex items-center gap-1.5 rounded-md px-2 py-1` + 复用 `FileAttachmentChip` 视觉关键元素 |
| 图片（file://） | `<img>` 走 `useLocalImageDataUrl`，`max-h-40 max-w-[200px] rounded-lg object-cover` |
| 图片（http(s)://） | `<img>` 直出 |
| blockquote | `border-l-2 border-primary-foreground/40 pl-3 opacity-90` |
| list | `pl-5 list-{disc\|decimal}` |
| 围栏代码 | `rounded bg-primary-foreground/10 p-2 overflow-x-auto` |

### `files` 数组的角色

bubble 渲染层只读 markdown 字符串。但在 `file://` chip 里，需要原始 `FileAttachment.id` 来调 `openPreview` —— 这条信息 markdown link href 里没有。所以我们让 chip renderer **可以**收 `files` 数组（通过 `path` match），如果找到匹配的 `FileAttachment` 就用它的 id；找不到就 fallback 到 path 直接 openLocalFile。

**简化**：找到匹配 file → 完整功能（openPreview）。找不到 → openLocalFile。两条路径用户体验近似。

### path → file URL 映射

序列化时 `[附件: name](file:///abs/path)`。渲染时 `href` 是 `file:///abs/path`（三斜线），renderer 要把它转回 `/abs/path` 来 match `FileAttachment.filePath`。Windows: `file:///C:/foo` → `C:/foo`（保留正斜线，与 P0 序列化一致）。

```ts
function fileUrlToPath(href: string): string | null {
  if (!href.startsWith('file://')) return null
  // file:///abs/foo → /abs/foo
  // file:///C:/foo → C:/foo
  const stripped = href.slice('file://'.length)
  // remove leading slash if next char is a drive letter (Windows)
  if (/^\/[A-Za-z]:/.test(stripped)) return stripped.slice(1)
  return stripped
}
```

### 旧消息兼容

历史消息的 `content.text` 是纯文本，可能含未 escape 的 `*` `_`。React-markdown 对孤立特殊字符大多无害，直接走同 renderer。`whitespace-pre-wrap` 行为通过 markdown 自身的段落处理覆盖（连续两换行 → 段落边界）。**不做迁移**。

### Skill chip 渲染

`UserMessageBubble` 中 skill chip 渲染逻辑保留独立位置（左上角），不归 markdown 管。

## 测试覆盖（~10 项）

`UserBubbleMarkdown.test.tsx`：

1. plain text → 段落渲染
2. **bold** / *italic* / inline code 渲染正确
3. http link → 普通 `<a>` with underline
4. file:// link with matching file in `files` array → chip with FILE_TYPE_CONFIG label, click → openPreview
5. file:// link without matching file → fallback chip, click → openLocalFile
6. file:// image with `useLocalImageDataUrl` available → `<img src="data:...">`
7. https image → `<img src="https://...">`
8. bullet list / ordered list → `<ul>` / `<ol>`
9. blockquote → `<blockquote>` with border class
10. fenced code block → `<pre><code>` with bg class

`UserMessageBubble.test.tsx`：现有测试若依赖于具体的 thumbnail/chip class 名应该重写。最小测试：
- 显示 markdown 文本
- 含 skill chip 时 skill chip 位于 markdown 之前
- 含 file:// 附件时 chip 渲染正确

如果 `UserMessageBubble.test.tsx` 不存在或测试少，跳过此项；专注 `UserBubbleMarkdown` 单测。

## 风险

- **react-markdown v10 component override API** ：用 `components={{ a: CustomA, img: CustomImg, ... }}` 模式（与 `markdownComponents.ts` 里 AssistantMarkdown 一致）。
- **Code block 不上 syntax highlight**：spec 已说明 user 输入很少需要；不引入 rehype-highlight 减少体积。
- **`files` prop 在迁移期可能为 undefined**：renderer 容错。
- **`useLocalImageDataUrl` SSR / jsdom**：现有测试已经用过它，可以 mock。

---

### Task 1: UserBubbleMarkdown 组件

**Files:**
- Create: `src/components/chat-scene/markdown/UserBubbleMarkdown.tsx`
- Create: `src/components/chat-scene/markdown/__tests__/UserBubbleMarkdown.test.tsx`

#### 实现

```tsx
// src/components/chat-scene/markdown/UserBubbleMarkdown.tsx
import { useMemo } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { openLocalFile } from '@/lib/tauri'
import { isPreviewableFileType } from '@/components/chat/generatedFileActions'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { useLocalImageDataUrl } from '@/hooks/useLocalImageDataUrl'
import type { FileAttachment } from '@/types/message'

const FILE_TYPE_LABEL: Record<FileAttachment['fileType'], string> = {
  excel: 'XLS',
  csv: 'CSV',
  word: 'DOC',
  pdf: 'PDF',
  json: 'JSON',
  image: 'IMG',
  folder: 'DIR',
}

interface UserBubbleMarkdownProps {
  text: string
  conversationId?: string
  files?: FileAttachment[]
}

function fileUrlToPath(href: string | undefined | null): string | null {
  if (!href || !href.startsWith('file://')) return null
  const stripped = href.slice('file://'.length)
  if (/^\/[A-Za-z]:/.test(stripped)) return stripped.slice(1)
  return stripped
}

function FileLinkChip({
  href,
  text,
  files,
  conversationId,
}: {
  href: string
  text: string
  files?: FileAttachment[]
  conversationId?: string
}) {
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  const path = fileUrlToPath(href)
  const matched = useMemo(() => {
    if (!path || !files) return undefined
    return files.find((f) => f.filePath === path)
  }, [path, files])

  const handleClick = () => {
    if (!path) return
    if (matched) {
      if (matched.kind === 'folder') {
        void openLocalFile(path)
        return
      }
      if (isPreviewableFileType(matched.fileType, matched.fileName) && conversationId) {
        openPreview({
          fileId: matched.id,
          conversationId,
          fileName: matched.fileName,
          fileType: matched.fileType,
          localPath: path,
        })
        return
      }
    }
    void openLocalFile(path)
  }

  const fileType = matched?.fileType
  const label = fileType ? FILE_TYPE_LABEL[fileType] : 'FILE'

  return (
    <button
      type="button"
      onClick={handleClick}
      className="inline-flex items-center gap-1.5 rounded-md bg-primary-foreground/15 px-2 py-1 align-middle text-xs leading-none text-primary-foreground transition-opacity hover:opacity-80"
      title={text}
    >
      <span className="rounded bg-primary-foreground/15 px-1 text-[10px] font-bold">{label}</span>
      <span className="max-w-[200px] truncate">{text}</span>
    </button>
  )
}

function FileImage({ href, alt }: { href: string; alt: string }) {
  const path = fileUrlToPath(href) ?? ''
  const { url } = useLocalImageDataUrl(path, undefined)
  if (!url) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-md bg-primary-foreground/15 px-2 py-1 align-middle text-xs">
        <span className="rounded bg-primary-foreground/15 px-1 text-[10px] font-bold">IMG</span>
        <span className="max-w-[200px] truncate">{alt}</span>
      </span>
    )
  }
  return (
    <img
      src={url}
      alt={alt}
      className="max-h-40 max-w-[200px] rounded-lg object-cover"
    />
  )
}

export function UserBubbleMarkdown({ text, conversationId, files }: UserBubbleMarkdownProps) {
  if (!text.trim()) return null
  return (
    <div className="user-bubble-markdown text-sm leading-relaxed">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        skipHtml
        components={{
          p: ({ children }) => <p className="leading-relaxed [&:not(:first-child)]:mt-2">{children}</p>,
          a: ({ href, children }) => {
            const hrefStr = typeof href === 'string' ? href : ''
            if (hrefStr.startsWith('file://')) {
              const text = String(children)
              return (
                <FileLinkChip href={hrefStr} text={text} files={files} conversationId={conversationId} />
              )
            }
            return (
              <a
                href={hrefStr}
                target="_blank"
                rel="noopener noreferrer"
                className="underline underline-offset-2"
              >
                {children}
              </a>
            )
          },
          img: ({ src, alt }) => {
            const srcStr = typeof src === 'string' ? src : ''
            const altStr = typeof alt === 'string' ? alt : ''
            if (srcStr.startsWith('file://')) {
              return <FileImage href={srcStr} alt={altStr} />
            }
            return (
              <img src={srcStr} alt={altStr} className="max-h-40 max-w-[200px] rounded-lg object-cover" />
            )
          },
          code: ({ className, children }) => {
            const isFenced = typeof className === 'string' && className.startsWith('language-')
            if (isFenced) {
              return <code className={className}>{children}</code>
            }
            return (
              <code className="rounded bg-primary-foreground/15 px-1 text-[0.8125em]">
                {children}
              </code>
            )
          },
          pre: ({ children }) => (
            <pre className="overflow-x-auto rounded bg-primary-foreground/10 p-2 text-xs">
              {children}
            </pre>
          ),
          ul: ({ children }) => <ul className="list-disc pl-5">{children}</ul>,
          ol: ({ children }) => <ol className="list-decimal pl-5">{children}</ol>,
          blockquote: ({ children }) => (
            <blockquote className="border-l-2 border-primary-foreground/40 pl-3 opacity-90">
              {children}
            </blockquote>
          ),
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}
```

#### 测试

```tsx
// src/components/chat-scene/markdown/__tests__/UserBubbleMarkdown.test.tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { UserBubbleMarkdown } from '../UserBubbleMarkdown'

const mockOpenLocalFile = vi.fn()
const mockOpenPreview = vi.fn()

vi.mock('@/lib/tauri', () => ({
  openLocalFile: (...args: unknown[]) => mockOpenLocalFile(...args),
}))

vi.mock('@/stores/generatedFilePreviewStore', () => ({
  useGeneratedFilePreviewStore: (selector: (s: { openPreview: typeof mockOpenPreview }) => unknown) =>
    selector({ openPreview: mockOpenPreview }),
}))

vi.mock('@/components/chat/generatedFileActions', () => ({
  isPreviewableFileType: (fileType: string) => fileType === 'pdf' || fileType === 'image',
}))

vi.mock('@/hooks/useLocalImageDataUrl', () => ({
  useLocalImageDataUrl: () => ({ url: 'data:image/png;base64,FAKE' }),
}))

beforeEach(() => {
  mockOpenLocalFile.mockReset()
  mockOpenPreview.mockReset()
})

describe('UserBubbleMarkdown', () => {
  it('renders plain text in a paragraph', () => {
    render(<UserBubbleMarkdown text="hello world" />)
    expect(screen.getByText('hello world')).toBeInTheDocument()
  })

  it('renders bold + italic + inline code', () => {
    const { container } = render(<UserBubbleMarkdown text="**a** *b* `c`" />)
    expect(container.querySelector('strong')).toHaveTextContent('a')
    expect(container.querySelector('em')).toHaveTextContent('b')
    expect(container.querySelector('code')).toHaveTextContent('c')
  })

  it('renders http link as plain anchor', () => {
    const { container } = render(<UserBubbleMarkdown text="[click](https://example.com)" />)
    const a = container.querySelector('a')
    expect(a).toHaveAttribute('href', 'https://example.com')
    expect(a).toHaveAttribute('target', '_blank')
  })

  it('renders file:// link as chip; with matched file → openPreview on click', () => {
    const files = [
      {
        id: 'f1',
        fileName: 'plan.pdf',
        filePath: '/p/plan.pdf',
        kind: 'file' as const,
        fileType: 'pdf' as const,
        fileSize: 0,
        status: 'uploaded' as const,
      },
    ]
    render(
      <UserBubbleMarkdown
        text="[附件: plan.pdf](file:///p/plan.pdf)"
        files={files}
        conversationId="c1"
      />,
    )
    const btn = screen.getByRole('button', { name: '附件: plan.pdf' })
    expect(btn).toBeInTheDocument()
    fireEvent.click(btn)
    expect(mockOpenPreview).toHaveBeenCalled()
    const arg = mockOpenPreview.mock.calls[0][0]
    expect(arg.fileId).toBe('f1')
    expect(arg.localPath).toBe('/p/plan.pdf')
  })

  it('file:// link without matching file → openLocalFile fallback', () => {
    render(<UserBubbleMarkdown text="[附件: x.pdf](file:///p/x.pdf)" />)
    const btn = screen.getByRole('button', { name: '附件: x.pdf' })
    fireEvent.click(btn)
    expect(mockOpenLocalFile).toHaveBeenCalledWith('/p/x.pdf')
    expect(mockOpenPreview).not.toHaveBeenCalled()
  })

  it('renders file:// image as <img> with data URL', () => {
    const { container } = render(<UserBubbleMarkdown text="![chart](file:///p/c.png)" />)
    const img = container.querySelector('img')
    expect(img).toHaveAttribute('src', 'data:image/png;base64,FAKE')
    expect(img).toHaveAttribute('alt', 'chart')
  })

  it('renders https image directly', () => {
    const { container } = render(<UserBubbleMarkdown text="![](https://example.com/x.png)" />)
    const img = container.querySelector('img')
    expect(img).toHaveAttribute('src', 'https://example.com/x.png')
  })

  it('renders bullet list', () => {
    const { container } = render(<UserBubbleMarkdown text="- a\n- b" />)
    expect(container.querySelector('ul')).toBeInTheDocument()
    expect(container.querySelectorAll('li')).toHaveLength(2)
  })

  it('renders blockquote', () => {
    const { container } = render(<UserBubbleMarkdown text="> note" />)
    const bq = container.querySelector('blockquote')
    expect(bq).toBeInTheDocument()
    expect(bq?.className).toContain('border-l-2')
  })

  it('renders fenced code block', () => {
    const { container } = render(<UserBubbleMarkdown text="```\nlet x = 1\n```" />)
    const pre = container.querySelector('pre')
    expect(pre).toBeInTheDocument()
    expect(pre?.querySelector('code')).toHaveTextContent('let x = 1')
  })

  it('empty text → renders nothing', () => {
    const { container } = render(<UserBubbleMarkdown text="   " />)
    expect(container.firstChild).toBeNull()
  })
})
```

#### Steps

- [ ] 写测试文件 → 跑 → 失败（module not found）
- [ ] 写 `UserBubbleMarkdown.tsx`
- [ ] 跑测试 → 11 tests pass
- [ ] tsc 0
- [ ] Commit:
  ```
  feat(chat): UserBubbleMarkdown renders user message markdown with file:// chip + image renderers
  ```

---

### Task 2: 接入 UserMessageBubble

**Files:**
- Modify: `src/components/chat-scene/UserMessageBubble.tsx`

#### 实现

替换 `UserMessageBubble.tsx` 主体（保留 skill chip）。

```tsx
import { Blocks } from 'lucide-react'
import { UserBubbleMarkdown } from './markdown/UserBubbleMarkdown'
import type { FileAttachment, SkillCommandBreadcrumb } from '@/types/message'

interface UserMessageBubbleProps {
  text: string
  commandText?: string
  skillCommand?: SkillCommandBreadcrumb
  files?: FileAttachment[]
  conversationId?: string
}

export function UserMessageBubble({
  text,
  commandText,
  skillCommand,
  files,
  conversationId,
}: UserMessageBubbleProps) {
  const command = skillCommand?.command ?? commandText?.split(/\s+/)[0]
  const tokenLabel = skillCommand?.label ?? skillCommand?.id ?? command?.replace(/^\//, '')

  if (!text && !tokenLabel) return null

  return (
    <div className="flex w-full flex-col items-end gap-1.5">
      <div
        data-testid="user-bubble"
        className="max-w-[80%] rounded-2xl bg-primary px-3 py-2 text-sm leading-relaxed text-primary-foreground"
      >
        {tokenLabel ? (
          <span
            data-testid="user-skill-token"
            className="mr-2 inline-flex translate-y-[1px] items-center gap-1.5 rounded-lg bg-primary-foreground/20 px-2 py-1 text-xs font-semibold leading-none text-primary-foreground shadow-[inset_0_0_0_1px_rgba(255,255,255,0.24)]"
            title={command}
          >
            <Blocks
              aria-hidden="true"
              className="shrink-0"
              style={{ width: '0.75rem', height: '0.75rem', transform: 'translateY(1px)' }}
            />
            <span>{tokenLabel}</span>
          </span>
        ) : null}
        {text ? (
          <UserBubbleMarkdown text={text} files={files} conversationId={conversationId} />
        ) : null}
      </div>
    </div>
  )
}
```

#### Steps

- [ ] 替换 `UserMessageBubble.tsx`
- [ ] 跑全部 chat-scene 测试 → 看是否有破坏
- [ ] tsc 0
- [ ] Commit:
  ```
  feat(chat): UserMessageBubble delegates content rendering to UserBubbleMarkdown
  ```

---

### Task 3: 全库验证

- [ ] `pnpm test` — 与 P5 baseline 比较，新失败 0
- [ ] tsc 0
- [ ] lint scope 0 errors
