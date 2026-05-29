# GUI 桌面应用体验改进（Plan-Z）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:test-driven-development — 每个 Task 必须先写失败测试，再写实现。

**Goal:** 补齐消息时间戳、重新生成、侧边栏搜索、TaskStatusList 语义标签等 GUI 必备交互（Permission Ask 弹窗和代码高亮不在此计划范围内）。

**Architecture:** Z2、`Sidebar` 合并任务（Z3 与 AF1/AF2 一次性处理）、Z5 分块推进；其中 Sidebar 相关项共享写集，不应拆成重复实现。

**Tech Stack:** React, TypeScript, Tauri v2, Zustand, Vitest

**Worktree branch:** pzc

---

## 对标修订 / 实施边界（2026-04-19）

- `Z3` 与 `Plan-AF/AF1` 属于同一 Sidebar 搜索任务，后续实现时必须合并，避免重复改 `Sidebar.tsx`。
- `Z2a` 优先复用现有 `src/lib/format.ts` 中的 `formatRelativeTime`；只有现有实现不满足时才扩展，不再默认新建同名文件。
- `Z5b` 落地前需先核实 `GeneratedFile` 是否有可用文件路径以及 Tauri 资产协议是否已开通；若条件不满足，先补数据链路再做预览。

---

## 调研发现汇总

| 文件 | 问题 |
|---|---|
| `src/components/chat/AiBubble.tsx` | 无"重新生成"按钮，CopyButton 模式在 L319-347 |
| `src/components/chat/UserBubble.tsx` | 无"编辑并重发"入口 |
| `src/components/chat/MessageItem.tsx` | 无时间戳渲染 |
| `src/components/layout/Sidebar.tsx` | 无对话搜索框 |
| `src/components/chat/TaskStatusList.tsx:79` | `task.taskId.slice(-8)` 显示裸 ID 后缀，无语义 |
| `src/components/rich-content/GeneratedFileCard.tsx` | 图片类文件无内联预览 |

---

---

## ~~Z1：Permission Ask 重设计~~ （已取消，维持现有全屏模态实现）

---

## Z2：消息时间戳 + AI 消息重新生成按钮 + 用户消息编辑重发

**依赖：** 无

### Z2a：消息时间戳

**文件：** `src/components/chat/MessageItem.tsx`

在 `UserBubble` 和 `AiBubble` 外层各自加时间戳 `<time>` 元素，展示相对时间（如"刚刚"、"3 分钟前"、"昨天 14:30"）。

新增工具函数 `src/lib/formatRelativeTime.ts`：

```typescript
export function formatRelativeTime(isoString: string | undefined): string {
  if (!isoString) return ''
  const date = new Date(isoString)
  if (Number.isNaN(date.getTime())) return ''
  const diffMs = Date.now() - date.getTime()
  const diffMin = Math.floor(diffMs / 60_000)
  if (diffMin < 1) return '刚刚'
  if (diffMin < 60) return `${diffMin} 分钟前`
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24) return `${diffHour} 小时前`
  const diffDay = Math.floor(diffHour / 24)
  if (diffDay === 1) return '昨天'
  if (diffDay < 7) return `${diffDay} 天前`
  return date.toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' })
}
```

`MessageItem.tsx` 渲染变更（在气泡下方 `justify-end` / `justify-start` 各自对齐）：

```tsx
import { formatRelativeTime } from '@/lib/formatRelativeTime'

// user 分支
<div>
  <UserBubble message={message} />
  {message.createdAt && (
    <div className="mt-0.5 pr-9 text-right">
      <time className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
        {formatRelativeTime(message.createdAt)}
      </time>
    </div>
  )}
</div>

// assistant 分支
<div>
  <AiBubble message={message} isStreaming={isStreaming} />
  {message.createdAt && !isStreaming && (
    <div className="mt-0.5 pl-9">
      <time className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
        {formatRelativeTime(message.createdAt)}
      </time>
    </div>
  )}
</div>
```

**测试文件：** `src/lib/formatRelativeTime.test.ts`

```typescript
import { describe, it, expect, vi, afterEach } from 'vitest'
import { formatRelativeTime } from './formatRelativeTime'

describe('formatRelativeTime', () => {
  afterEach(() => vi.useRealTimers())

  it('returns 刚刚 for < 1 min ago', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-19T10:00:30Z'))
    expect(formatRelativeTime('2026-04-19T10:00:00Z')).toBe('刚刚')
  })

  it('returns N 分钟前 for 1-59 min ago', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-19T10:05:00Z'))
    expect(formatRelativeTime('2026-04-19T10:00:00Z')).toBe('5 分钟前')
  })

  it('returns N 小时前 for 1-23 hours ago', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-19T13:00:00Z'))
    expect(formatRelativeTime('2026-04-19T10:00:00Z')).toBe('3 小时前')
  })

  it('returns 昨天 for 1 day ago', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-20T10:00:00Z'))
    expect(formatRelativeTime('2026-04-19T10:00:00Z')).toBe('昨天')
  })

  it('returns empty string for undefined', () => {
    expect(formatRelativeTime(undefined)).toBe('')
  })

  it('returns empty string for invalid date string', () => {
    expect(formatRelativeTime('not-a-date')).toBe('')
  })
})
```

**测试命令：**
```bash
pnpm exec vitest run src/lib/formatRelativeTime.test.ts
```

### Z2b：AI 消息"重新生成"按钮

**文件：** `src/components/chat/AiBubble.tsx`

在 `CopyButton`（L319-347）旁边新增 `RetryButton`，hover 时显示。点击逻辑：取对应 AI 消息之前最近的 user 消息文本，调用 `sendMessage` 重发（前端不删除历史，后端追加新轮次）。

```tsx
function RetryButton({ messageId }: { messageId: string }) {
  const { t } = useTranslation()
  const messages = useChatStore((s) => s.messages)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const [pending, setPending] = useState(false)

  const handleRetry = useCallback(async () => {
    if (!activeConversationId || pending) return
    // 找该 AI 消息前最近的 user 消息
    const msgIndex = messages.findIndex((m) => m.id === messageId)
    const userMsg = [...messages].slice(0, msgIndex).reverse().find((m) => m.role === 'user')
    if (!userMsg?.content.text) return
    setPending(true)
    try {
      await sendMessage(activeConversationId, userMsg.content.text, [])
    } finally {
      setPending(false)
    }
  }, [activeConversationId, messageId, messages, pending])

  return (
    <button
      onClick={handleRetry}
      disabled={pending}
      className="absolute right-8 top-0 hidden rounded-md px-2 py-1 text-xs transition-colors group-hover:block"
      style={{
        color: 'var(--color-text-muted)',
        background: 'var(--color-bg-elevated)',
        border: '1px solid var(--color-border)',
      }}
      title={t('aiBubble.retry', { defaultValue: '重新生成' })}
    >
      ↻
    </button>
  )
}
```

在 `AiBubble` 渲染 AI 消息气泡外层 `div.group` 处，在 `CopyButton` 同级新增 `<RetryButton messageId={message.id} />`。

### Z2c：用户消息"编辑并重发"入口

**文件：** `src/components/chat/UserBubble.tsx`

hover 时在气泡右上角显示编辑图标；点击后气泡文字切换为 `<textarea>`，提交后：
1. 调用 `sendMessage(conversationId, editedText, files)` 重发（后端追加新轮次，前端不删历史）
2. 取消则恢复显示原文本

```tsx
// UserBubble.tsx 新增内联编辑逻辑
const [editing, setEditing] = useState(false)
const [draft, setDraft] = useState(content.text ?? '')
const activeConversationId = useChatStore((s) => s.activeConversationId)

const handleSubmitEdit = async () => {
  if (!draft.trim() || !activeConversationId) return
  await sendMessage(activeConversationId, draft, [])
  setEditing(false)
}
```

**测试文件：** `src/components/chat/UserBubble.test.tsx`（新建）

```typescript
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({ sendMessage: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@/stores/chatStore', () => ({
  useChatStore: vi.fn((sel) => sel({ activeConversationId: 'conv-1', messages: [] })),
}))

import { UserBubble } from './UserBubble'
import { sendMessage } from '@/lib/tauri'

const makeMsg = (text: string) => ({
  id: 'msg-1',
  role: 'user' as const,
  content: { text },
  createdAt: '2026-04-19T10:00:00Z',
  sender: { name: '我', isLoggedIn: false },
})

describe('UserBubble edit and resend', () => {
  it('shows edit button on hover', async () => {
    render(<UserBubble message={makeMsg('hello')} />)
    const bubble = screen.getByText('hello').closest('div')!
    fireEvent.mouseEnter(bubble.parentElement!)
    expect(screen.getByTitle(/编辑/)).toBeInTheDocument()
  })

  it('switches to textarea on edit click', () => {
    render(<UserBubble message={makeMsg('hello')} />)
    const bubble = screen.getByText('hello').closest('div')!
    fireEvent.mouseEnter(bubble.parentElement!)
    fireEvent.click(screen.getByTitle(/编辑/))
    expect(screen.getByRole('textbox')).toHaveValue('hello')
  })

  it('calls sendMessage with edited text on submit', async () => {
    render(<UserBubble message={makeMsg('hello')} />)
    fireEvent.mouseEnter(screen.getByText('hello').closest('div')!.parentElement!)
    fireEvent.click(screen.getByTitle(/编辑/))
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello world' } })
    fireEvent.click(screen.getByText(/发送/))
    await waitFor(() => {
      expect(sendMessage).toHaveBeenCalledWith('conv-1', 'hello world', [])
    })
  })
})
```

**测试命令：**
```bash
pnpm exec vitest run src/components/chat/UserBubble.test.tsx src/lib/formatRelativeTime.test.ts
```

**Commit：** `feat(chat): add message timestamps, AI retry button, user edit-and-resend - Z2`

---

## Z3：侧边栏对话搜索

**依赖：** 无

**文件：** `src/components/layout/Sidebar.tsx`

在 conversations 列表顶部（新对话按钮下方）新增搜索框，过滤 `conversations` 列表（本轮只按 `title` 匹配，大小写不敏感），匹配文字高亮（`<mark>`）。`preview` 搜索需先补后端/前端数据链路，单列后续任务。

**关键实现片段：**

```tsx
// Sidebar.tsx 内新增
const [searchQuery, setSearchQuery] = useState('')

const filteredConversations = useMemo(() => {
  if (!searchQuery.trim()) return conversations
  const q = searchQuery.toLowerCase()
  return conversations.filter(
    (c) => c.title?.toLowerCase().includes(q),
  )
}, [conversations, searchQuery])

// 高亮工具函数
function highlight(text: string, query: string): React.ReactNode {
  if (!query.trim()) return text
  const idx = text.toLowerCase().indexOf(query.toLowerCase())
  if (idx === -1) return text
  return (
    <>
      {text.slice(0, idx)}
      <mark style={{ background: 'var(--color-accent-light)', color: 'inherit' }}>
        {text.slice(idx, idx + query.length)}
      </mark>
      {text.slice(idx + query.length)}
    </>
  )
}
```

搜索框样式：紧凑，单行，带放大镜图标前缀，`placeholder="搜索对话..."`

**测试文件：** `src/components/layout/Sidebar.test.tsx`（新建）

```typescript
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'

// mock deps
vi.mock('@/hooks/useChat', () => ({ useChat: () => ({
  conversations: [
    { id: '1', title: 'Python 分析', preview: '数据清洗', updatedAt: new Date().toISOString() },
    { id: '2', title: '财务报告', preview: '月度汇总', updatedAt: new Date().toISOString() },
  ],
  activeConversationId: null,
  createNewConversation: vi.fn(),
  switchConversation: vi.fn(),
  deleteConversation: vi.fn(),
  renameConversation: vi.fn(),
}) }))
// ... mock other stores

describe('Sidebar search', () => {
  it('renders search input', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)
    expect(screen.getByPlaceholderText(/搜索对话/)).toBeInTheDocument()
  })

  it('filters conversations by title', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)
    fireEvent.change(screen.getByPlaceholderText(/搜索对话/), { target: { value: 'Python' } })
    expect(screen.getByText(/Python 分析/)).toBeInTheDocument()
    expect(screen.queryByText(/财务报告/)).toBeNull()
  })

  it('shows all conversations when query is cleared', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)
    const input = screen.getByPlaceholderText(/搜索对话/)
    fireEvent.change(input, { target: { value: 'Python' } })
    fireEvent.change(input, { target: { value: '' } })
    expect(screen.getByText(/财务报告/)).toBeInTheDocument()
  })
})
```

**测试命令：**
```bash
pnpm exec vitest run src/components/layout/Sidebar.test.tsx
```

**Commit：** `feat(sidebar): add conversation search with highlight - Z3`

---

## Z5：TaskStatusList 语义标签 + 图表内联预览

**依赖：** 无

### Z5a：TaskStatusList 语义标签

**文件：** `src/components/chat/TaskStatusList.tsx`

`ConversationTaskState` 当前结构只有 `taskId / status / runId`，没有 `description` 字段。需要在前端侧加一个 `index`（任务顺序号）来生成"子任务 #N"。

**变更：**

将当前 `{task.taskId.slice(-8)}` 替换为有语义的标签：

```tsx
// tasks.map 时带 index
{tasks.map((task, index) => (
  <li key={task.taskId} className="flex items-center gap-1.5">
    <StatusIcon status={task.status} />
    <span style={{ color: 'var(--color-text-secondary)' }}>
      子任务 #{index + 1}
    </span>
    <span
      className="font-mono text-xs opacity-40"
      title={task.taskId}  // 完整 ID 仍可 hover 查看
    >
      ({task.taskId.slice(-6)})
    </span>
  </li>
))}
```

若后端将来在 `task:status-changed` payload 中新增 `description` 字段，则在 `ConversationTaskState` 加可选字段，此处优先展示 `description`，回退到"子任务 #N"。

### Z5b：GeneratedFileCard 图片内联预览

**文件：** `src/components/rich-content/GeneratedFileCard.tsx`

图片类型（`png`、`jpg`、`jpeg`、`svg`、`gif`、`webp`）在文件信息下方追加 `<img>` 预览。

Tauri v2 通过 `asset://` 协议加载本地文件，路径格式为 `asset://localhost/<absolute-path>`（需在 `tauri.conf.json` 的 `security.assetProtocol.enable` 确认已开启）。

```tsx
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'svg', 'gif', 'webp'])

function isImageFile(fileType: string, fileName: string): boolean {
  return (
    IMAGE_EXTENSIONS.has(fileType.toLowerCase()) ||
    IMAGE_EXTENSIONS.has(fileName.split('.').pop()?.toLowerCase() ?? '')
  )
}

function toAssetUrl(filePath: string | undefined): string | null {
  if (!filePath) return null
  // Tauri v2: asset://localhost/<path>
  const encoded = encodeURIComponent(filePath).replace(/%2F/g, '/')
  return `asset://localhost${filePath.startsWith('/') ? '' : '/'}${encoded}`
}
```

在 `GeneratedFileCard` 的 degradation notice 前插入图片预览区：

```tsx
{isImageFile(file.fileType, file.fileName) && file.filePath && (
  <div
    className="border-t px-3.5 py-3"
    style={{ borderColor: 'var(--color-border)' }}
  >
    <img
      src={toAssetUrl(file.filePath) ?? ''}
      alt={file.fileName}
      className="max-h-48 max-w-full rounded object-contain"
      loading="lazy"
      onError={(e) => {
        // 加载失败时隐藏预览区
        ;(e.currentTarget.parentElement as HTMLElement).style.display = 'none'
      }}
    />
  </div>
)}
```

注意：`GeneratedFile` 类型需要有 `filePath?: string` 字段（检查 `src/types/message.ts`，若无则补充可选字段）。

**测试文件：** `src/components/chat/TaskStatusList.test.tsx`（新建）

```typescript
import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { TaskStatusList } from './TaskStatusList'

const makeTask = (id: string, status = 'running') => ({
  taskId: id,
  status,
  runId: 'run-1',
})

describe('TaskStatusList semantic labels', () => {
  it('shows 子任务 #1 instead of raw ID suffix', () => {
    render(<TaskStatusList tasks={[makeTask('long-task-id-abc123')]} />)
    expect(screen.getByText('子任务 #1')).toBeInTheDocument()
    expect(screen.queryByText('bc123')).toBeNull()  // 裸 ID 后缀不再主显
  })

  it('numbers multiple tasks sequentially', () => {
    render(<TaskStatusList tasks={[makeTask('t1'), makeTask('t2'), makeTask('t3')]} />)
    expect(screen.getByText('子任务 #1')).toBeInTheDocument()
    expect(screen.getByText('子任务 #2')).toBeInTheDocument()
    expect(screen.getByText('子任务 #3')).toBeInTheDocument()
  })

  it('renders nothing for empty tasks', () => {
    const { container } = render(<TaskStatusList tasks={[]} />)
    expect(container.firstChild).toBeNull()
  })
})
```

**测试命令：**
```bash
pnpm exec vitest run src/components/chat/TaskStatusList.test.tsx
```

**Commit：** `feat(task-list): semantic labels + image inline preview in GeneratedFileCard - Z5`

---

## 执行顺序建议

Z2/Z3/Z5（前端）可并行启动。

| Task | 前置 | 估时 |
|---|---|---|
| Z2a 时间戳 + 工具函数 | 无 | 0.5h |
| Z2b AI 重新生成按钮 | 无 | 1h |
| Z2c 用户编辑重发 | 无 | 1.5h |
| Z3 侧边栏搜索 | 无 | 1h |
| Z5a TaskStatusList 语义标签 | 无 | 0.5h |
| Z5b 图片内联预览 | 无（需确认 filePath 字段）| 1h |

## 验收标准

- [ ] 所有消息气泡下方显示相对时间戳
- [ ] AI 消息 hover 时显示"重新生成"按钮
- [ ] 用户消息 hover 时显示编辑入口，编辑提交后重发
- [ ] 侧边栏搜索框实时过滤，匹配词高亮
- [ ] TaskStatusList 显示"子任务 #N"而非裸 ID
- [ ] 图片类 GeneratedFileCard 内联展示 `<img>` 缩略图
- [ ] 全部单测通过：`pnpm test`
