# RichComposer P5 — Page Integration 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Replace `ChatComposerCompact`-based composer in `ChatBottomArea` and `HomeTaskComposerCard` with `<RichComposer>` from P0–P4. All page-specific business logic (workspace auth, conversation creation, skill chip, drop inbox, picker, paste) is kept; only the input shell flips from textarea to Tiptap.

**Architecture:** Each page mounts a `RichComposerHandle` ref, calls `useComposerDropInbox(ref)` and `useComposerAttachmentPaste(ref)`, wires `onSubmit` to the existing send flow (passing `payload.markdown` and `payload.attachments`). Skill chip / project button / picker open / streaming stop all map to `RichComposer` props. The skill prefill flow (clicking a skill in popover sets a trigger string in the textarea) becomes inserting the trigger via `editor.commands.insertContent` after `clear`.

**Tech Stack:** P0–P4 modules; `useChat` / `useChatAttachments` / `useChatStore` / `useDropInbox` / `useHomeStore` / `useUiStore` (existing).

---

## 关键转换决策

### Skill prefill 流程

旧：`setInput('/skill ')` 然后用 textareaRef 把光标定位到末尾。
新：`composerRef.current?.clear()` 然后 `composerRef.current?.getEditor()?.commands.insertContent('/skill ')`。`focus('end')` 走 ref handle。

### prefill text（来自 useUiStore.consumePrefillText）

旧：`setInput(prefill)` 然后 textareaRef.focus + setSelectionRange。
新：`<RichComposer initialMarkdown={prefill} autoFocus />` —— 一次性 prefill 通过 prop 传入，由 `parseMarkdownToComposerJson` 解析。

### "请分析附件" 默认文案

旧：`sendUserMessage(trimmed || t('inputBar.analyzeFile'), ...)` —— 文本���空时填默认。
新：spec 明确 "只附件提交时，markdown 就是一个或多个附件占位符本身，不再额外补默认文案"。**所以删掉这个 fallback**。serializer 在只附件时 `markdown` 是 `[附件: ...](file://...)`，发出去就是它。

### `placeholderWithFile` 切换

旧：`pendingFiles.length > 0 ? placeholderWithFile : placeholder`。
新：因为附件现在 inline 在 editor 里，"已经看到附件 chip"本身就是视觉提示，不需要 placeholder 切换。统一用 `placeholder`。

### isStreaming + stopCurrentStream

直接映射到 `<RichComposer isStreaming onStop>` props。

### 空 payload submit guard

`RichComposer` 已经在 `payload.isEmpty=true` 时阻止 onSubmit。页面层不再需要 `handleSend` 的 trim 检查；保留 isSending 互斥即可（防止重复点击）。

### Drop inbox + paste pipeline

替换原有 `useEffect drain` + `useComposerPaste` + `setPendingFiles` 整套：
```ts
const composerRef = useRef<RichComposerHandle>(null)
useComposerDropInbox(composerRef)
useComposerAttachmentPaste(composerRef)
```

### Picker

旧：`pickAttachments()` → `setPendingFiles(prev => [...prev, ...new])` → 渲染 chips。
新：`pickAttachments()` → `composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(results))`。

### `pendingFiles` 状态、`PendingAttachmentChips`、`allowAttachmentOnlySubmit`

全部删除 —— editor 内的 attachmentToken 就是状态本身。

## 文件结构

修改：
- `src/components/chat-scene/ChatBottomArea.tsx` — Tiptap 接入。
- `src/components/home/HomeTaskComposerCard.tsx` — Tiptap 接入。
- `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx` — 测试更新。
- `src/components/home/__tests__/HomeTaskComposerCard.test.tsx` — 测试更新。

不修改：
- `ChatComposerCompact.tsx` / `useComposerPaste.ts` / `PendingAttachmentChips.tsx` —— 留给 P8 删除。
- `useChat` / `sendUserMessage` 签名（仍接 string + `PendingFileInfo[]`；只是 string 现在是 markdown）。
- 后端 / Tauri command（markdown 字符串走原有 `text` 字段）。

## 测试策略

两个页面的现有 vitest 测试（300+ 行）大量依赖 textarea 行为（`fireEvent.change(textarea)` 等）。换 Tiptap 后这些都要重写。**简化策略**：

1. 删除 textarea 直接交互的测试（不再适用）。
2. 保留行为契约层测试：sendUserMessage 调用参数、conversation creation 流程、workspace authorization、isSending 互斥、skill picker 联动等。把这些测试改成"通过 ref 操作 editor，验证 sendUserMessage 调用参数"。
3. 关键路径用 `vi.mock('@tiptap/react', { ReactNodeViewRenderer })` 同前。

P5 不强求保持原测试覆盖完全等价 —— 测试本身在变。如果某条原测试涉及"placeholderWithFile 切换" 之类已删的特性，删除即可。

## 风险

- **整体 paste pipeline 工作但语义有差异**：旧流程中 paste 后 attachment 到 `setPendingFiles`，再统一 send；新流程 paste 直接进 editor，state 仅在 editor 内。语义上等价但事件时序略不同。集成测试覆盖此处。
- **测试基础设施需要 NodeView mock**：与 P3/P4 的测试用同样的 `vi.mock('@tiptap/react', ...)`。
- **Skill popover prefill**：旧实现走 setSelectionRange，新实现 clear + insertContent。不同点：`/skill ` 的尾部空格在 Tiptap 里也是 text 节点末尾的空格，serialize 时不会丢。
- **HomePage 的 conversation creation race**：原顺序是 `setValue('')` 在 `createConversation` 之后，newPage 切换时 RichComposer 会被 unmount + remount。**关键：`autoFocus` + `clearOnSubmit` 让 RichComposer 在新页面重新 mount 后是空的**。所以新流程不再需要手动 `setValue('')`，由 unmount 自动清理。

## 实施分期

P5 拆成两步，因为页面互独立：

- Task 1: ChatBottomArea 接入 + 测试调整
- Task 2: HomeTaskComposerCard 接入 + 测试调整
- Task 3: 跑 `pnpm test`，确认两个页面相关测试和整库不退化

每个 task 由一个 sonnet subagent 独立完成（保证连续上下文）。

---

### Task 1: ChatBottomArea 接入

**Files:**
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`

#### 替换 ChatBottomArea.tsx 主体

新版本：

```tsx
/**
 * @designSource design.pen#Cbtm1 ChatBottomArea
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { SkillPopover } from '@/components/chat/SkillPopover'
import {
  RichComposer,
  pendingAttachmentsToTokens,
  useComposerAttachmentPaste,
  useComposerDropInbox,
  type RichComposerHandle,
  type RichComposerSubmitPayload,
} from '@/components/rich-composer'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { useChatAttachments } from '@/hooks/useChatAttachments'
import { useChatStore } from '@/stores/chatStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

function BottomTips() {
  return (
    <>
      <span>内容由 AI 生成，请仔细核实回答内容</span>
      <div className="flex items-center gap-3">
        <span>Enter 发送</span>
        <span>Shift+Enter 换行</span>
      </div>
    </>
  )
}

export function ChatBottomArea() {
  const { t } = useTranslation()
  const composerRef = useRef<RichComposerHandle>(null)
  const [isSending, setIsSending] = useState(false)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const getSkillById = useSkillStore((s) => s.getById)

  // One-shot prefill text (e.g., from generated suggestion); read on mount only.
  const [initialMarkdown, setInitialMarkdown] = useState<string | undefined>(undefined)
  useEffect(() => {
    const prefill = useUiStore.getState().consumePrefillText()
    if (prefill) setInitialMarkdown(prefill)
  }, [])

  useComposerDropInbox(composerRef)
  useComposerAttachmentPaste(composerRef)

  useEffect(() => {
    if (!isStreaming) {
      requestAnimationFrame(() => {
        composerRef.current?.focus()
      })
    }
  }, [activeConversationId, isStreaming])

  const handleSkillPick = useCallback((skillId: string) => {
    const skill = getSkillById(skillId)
    const trigger = skill?.triggerText || `/${skillId}`
    const next = trigger.endsWith(' ') ? trigger : `${trigger} `
    composerRef.current?.clear()
    composerRef.current?.getEditor()?.commands.insertContent(next)
    composerRef.current?.focus()
    setShowSkillPopover(false)
  }, [getSkillById])

  const handleSubmit = useCallback(async (payload: RichComposerSubmitPayload) => {
    if (isSending) return
    setIsSending(true)
    const fileInfos: PendingFileInfo[] = payload.attachments.map((f) => ({
      id: f.id,
      fileName: f.fileName,
      filePath: f.path,
      kind: f.kind,
      fileType: f.fileType,
      fileSize: f.fileSize,
      mimeType: f.mimeType,
    }))
    try {
      await sendUserMessage(payload.markdown, fileInfos.length > 0 ? fileInfos : undefined)
    } catch (err) {
      console.error('[ChatBottomArea] sendUserMessage failed:', err)
      throw err
    } finally {
      setIsSending(false)
    }
  }, [isSending, sendUserMessage])

  const handlePickAttachments = useCallback(async () => {
    const results = await pickAttachments()
    if (results.length > 0) {
      composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(results))
    }
  }, [pickAttachments])

  return (
    <footer
      data-testid="chat-bottom-area"
      className="relative h-[148px] shrink-0"
      style={{ background: 'var(--color-bg-main)' }}
    >
      <div
        className="absolute right-0 bottom-0 left-0 px-6 pt-4 pb-5 [scrollbar-gutter:stable_both-edges]"
        style={{ background: 'linear-gradient(transparent, var(--color-bg-main) 30%)' }}
      >
        <div className="relative mx-auto w-full max-w-[736px]">
          <div className="absolute bottom-full left-1/2 z-30 mb-1 -translate-x-1/2">
            <SkillPopover
              open={showSkillPopover}
              onPick={handleSkillPick}
              onClose={() => setShowSkillPopover(false)}
            />
          </div>

          <div className="relative">
            <RichComposer
              ref={composerRef}
              placeholder={t('inputBar.placeholder')}
              onSubmit={handleSubmit}
              disabled={isSending}
              isStreaming={isStreaming}
              onStop={stopCurrentStream}
              clearOnSubmit
              autoFocus
              initialMarkdown={initialMarkdown}
              showProjectButton={false}
              onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
              onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
              tips={<BottomTips />}
            />
          </div>
        </div>
      </div>
    </footer>
  )
}
```

#### 测试更新

读现有 `ChatBottomArea.test.tsx` 与 `ChatBottomArea.prefill.test.tsx`。**整体替换**为以下契约层测试。失败的旧测试用例（如直接断言 `placeholderWithFile`）删除。

```tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, waitFor, fireEvent, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ChatBottomArea } from '../ChatBottomArea'
import { useChatStore } from '@/stores/chatStore'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

const mockSendUserMessage = vi.fn()
const mockStopCurrentStream = vi.fn()
let mockIsStreaming = false
const mockPickAttachments = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: mockSendUserMessage,
    isStreaming: mockIsStreaming,
    stopCurrentStream: mockStopCurrentStream,
  }),
}))

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: mockPickAttachments,
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

beforeEach(() => {
  mockSendUserMessage.mockReset().mockResolvedValue(undefined)
  mockStopCurrentStream.mockReset()
  mockPickAttachments.mockReset().mockResolvedValue([])
  mockIsStreaming = false
  useChatStore.setState({ activeConversationId: 'conv-1' })
})

describe('ChatBottomArea', () => {
  it('renders RichComposer', async () => {
    render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  })

  it('typing + Enter calls sendUserMessage with markdown text and no attachments', async () => {
    const user = userEvent.setup()
    render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'hello')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalledTimes(1))
    expect(mockSendUserMessage.mock.calls[0][0]).toBe('hello')
    expect(mockSendUserMessage.mock.calls[0][1]).toBeUndefined()
  })

  it('attachment-only Enter sends markdown with file:// link and attachment array', async () => {
    const handleRef = { current: null as unknown }
    const { container } = render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    void container
    void handleRef
    // Drive token insertion via picker.
    mockPickAttachments.mockResolvedValueOnce([
      {
        id: 'a',
        fileName: 'a.pdf',
        path: '/p/a.pdf',
        kind: 'file',
        fileType: 'pdf',
        fileSize: 0,
        mimeType: undefined,
        source: 'picker',
      },
    ])
    const attachBtn = await waitFor(() => container.querySelector('[aria-label="添加附件"]') as HTMLElement)
    await act(async () => {
      attachBtn.click()
    })
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('a.pdf')
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    const [text, files] = mockSendUserMessage.mock.calls[0]
    expect(text).toContain('[附件: a.pdf](file:///p/a.pdf)')
    expect(files).toHaveLength(1)
    expect(files[0].id).toBe('a')
  })

  it('isStreaming → shows stop button, click calls stopCurrentStream', async () => {
    mockIsStreaming = true
    const { container } = render(<ChatBottomArea />)
    const stopBtn = await waitFor(() => container.querySelector('[aria-label="停止"]') as HTMLElement)
    fireEvent.click(stopBtn)
    expect(mockStopCurrentStream).toHaveBeenCalledTimes(1)
    expect(mockSendUserMessage).not.toHaveBeenCalled()
  })

  it('empty Enter does not call sendUserMessage', async () => {
    render(<ChatBottomArea />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    expect(mockSendUserMessage).not.toHaveBeenCalled()
  })
})
```

If `ChatBottomArea.prefill.test.tsx` exists and tests prefill behavior, replace it with one test:

```tsx
// src/components/chat-scene/__tests__/ChatBottomArea.prefill.test.tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { ChatBottomArea } from '../ChatBottomArea'
import { useUiStore } from '@/stores/uiStore'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: vi.fn(), isStreaming: false, stopCurrentStream: vi.fn() }),
}))
vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: vi.fn().mockResolvedValue([]),
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }))

beforeEach(() => {
  useUiStore.setState({ prefillText: '帮我看看销售数据' })
})

describe('ChatBottomArea prefill', () => {
  it('consumes prefill text on mount and shows it in the editor', async () => {
    render(<ChatBottomArea />)
    await waitFor(() => {
      const text = document.querySelector('.ProseMirror')?.textContent ?? ''
      expect(text).toContain('帮我看看销售数据')
    })
    expect(useUiStore.getState().prefillText).toBeUndefined()
  })
})
```

#### 步骤

- [ ] **Step 1: Replace ChatBottomArea.tsx with the new version**
- [ ] **Step 2: Replace test file content (delete old, write new — see content above)**
- [ ] **Step 3: Run tests** `pnpm exec vitest run src/components/chat-scene/__tests__/ChatBottomArea`
  - Expected PASS — 5+1=6 tests
  - If any fails, diagnose; do NOT stub Tiptap behavior beyond the existing `ReactNodeViewRenderer` mock.
- [ ] **Step 4: tsc** `pnpm exec tsc -b --noEmit` — exit 0
- [ ] **Step 5: Commit**:
  ```
  feat(chat): ChatBottomArea wires RichComposer + drop/paste pipelines (no more textarea + pendingFiles)
  ```

---

### Task 2: HomeTaskComposerCard 接入

**Files:**
- Modify: `src/components/home/HomeTaskComposerCard.tsx`
- Modify: `src/components/home/__tests__/HomeTaskComposerCard.test.tsx`

#### 替换 HomeTaskComposerCard.tsx 主体

新版本：

```tsx
/**
 * @designSource design.pen#uq6ga ChatComposerCompact (home page variant)
 */
import { useCallback, useEffect, useRef, useState } from 'react'

import { SkillPopover } from '@/components/chat/SkillPopover'
import {
  RichComposer,
  pendingAttachmentsToTokens,
  useComposerAttachmentPaste,
  useComposerDropInbox,
  type RichComposerHandle,
  type RichComposerSubmitPayload,
} from '@/components/rich-composer'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { useChatAttachments } from '@/hooks/useChatAttachments'
import {
  authorizeLocalDirectory,
  createConversation,
  getDefaultFolder,
  pickLocalDirectory,
  type AuthorizedWorkspaceRef,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useHomeStore } from '@/stores/homeStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

export function HomeTaskComposerCard() {
  const composerRef = useRef<RichComposerHandle>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const { sendUserMessage } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()

  useComposerDropInbox(composerRef)
  useComposerAttachmentPaste(composerRef)

  const { selectedWorkspace, setSelectedWorkspace } = useHomeStore()
  const [displayWorkspace, setDisplayWorkspace] = useState<AuthorizedWorkspaceRef | null>(
    selectedWorkspace,
  )
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const getSkillById = useSkillStore((s) => s.getById)

  const [initialMarkdown, setInitialMarkdown] = useState<string | undefined>(undefined)
  useEffect(() => {
    const prefill = useUiStore.getState().consumePrefillText()
    if (prefill) setInitialMarkdown(prefill)
  }, [])

  const handleSkillPick = useCallback((skillId: string) => {
    const skill = getSkillById(skillId)
    const trigger = skill?.triggerText || `/${skillId}`
    const next = trigger.endsWith(' ') ? trigger : `${trigger} `
    composerRef.current?.clear()
    composerRef.current?.getEditor()?.commands.insertContent(next)
    composerRef.current?.focus()
    setShowSkillPopover(false)
  }, [getSkillById])

  useEffect(() => {
    if (selectedWorkspace) {
      setDisplayWorkspace(selectedWorkspace)
      return
    }
    getDefaultFolder()
      .then((ws) => setDisplayWorkspace(ws))
      .catch(() => {
        // fallback: show nothing, user can pick manually
      })
  }, [selectedWorkspace])

  const handlePickProject = async () => {
    const path = await pickLocalDirectory({
      defaultPath: displayWorkspace?.rootPath,
      title: '选择工作目录',
    })
    if (!path) return
    const parts = path.split(/[/\\]/).filter(Boolean)
    const name = parts[parts.length - 1] ?? path
    const ws: AuthorizedWorkspaceRef = { id: name, rootPath: path, displayName: name }
    setSelectedWorkspace(ws)
    setDisplayWorkspace(ws)
  }

  const handlePickAttachments = useCallback(async () => {
    const results = await pickAttachments()
    if (results.length > 0) {
      composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(results))
    }
  }, [pickAttachments])

  const handleSubmit = useCallback(async (payload: RichComposerSubmitPayload) => {
    if (isSubmitting) return
    setIsSubmitting(true)
    try {
      const backendId = await createConversation()
      const now = new Date().toISOString()
      const store = useChatStore.getState()
      store.setConversations([
        { id: backendId, title: '新对话', createdAt: now, updatedAt: now, isArchived: false },
        ...store.conversations,
      ])
      store.setActiveConversation(backendId)
      store.setMessages([])
      useUiStore.getState().setRoute({ kind: 'chat', conversationId: backendId })

      const workspacePath = displayWorkspace?.rootPath
      const isDefaultFolder = displayWorkspace?.id === 'default'
      if (workspacePath && !isDefaultFolder) {
        try {
          await authorizeLocalDirectory(workspacePath, backendId)
          const ws = displayWorkspace
          if (ws?.displayName) {
            const s = useChatStore.getState()
            s.setConversations(
              s.conversations.map((c) =>
                c.id === backendId ? { ...c, workspaceName: ws.displayName } : c,
              ),
            )
          }
        } catch (err) {
          console.error('[HomeTaskComposerCard] Failed to authorize workspace:', err)
        }
      }

      const fileInfos: PendingFileInfo[] = payload.attachments.map((f) => ({
        id: f.id,
        fileName: f.fileName,
        filePath: f.path,
        kind: f.kind,
        fileSize: f.fileSize,
        fileType: f.fileType,
        mimeType: f.mimeType,
      }))
      await sendUserMessage(payload.markdown, fileInfos)
    } finally {
      setIsSubmitting(false)
    }
  }, [displayWorkspace, isSubmitting, sendUserMessage])

  return (
    <div className="relative">
      <div className="absolute top-full left-1/2 z-30 mt-1 -translate-x-1/2">
        <SkillPopover
          open={showSkillPopover}
          onPick={handleSkillPick}
          onClose={() => setShowSkillPopover(false)}
        />
      </div>

      <RichComposer
        ref={composerRef}
        placeholder="描述你的任务，或点击「技能」按钮选择技能..."
        onSubmit={handleSubmit}
        disabled={isSubmitting}
        clearOnSubmit
        autoFocus
        initialMarkdown={initialMarkdown}
        onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
        onPickProject={() => void handlePickProject()}
        projectLabel={displayWorkspace?.displayName ?? '默认项目'}
        onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
      />
    </div>
  )
}
```

#### 测试更新

Replace `HomeTaskComposerCard.test.tsx` with focused contract tests:

```tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, waitFor, fireEvent, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { HomeTaskComposerCard } from '../HomeTaskComposerCard'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore } from '@/stores/uiStore'
import { useHomeStore } from '@/stores/homeStore'

vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) }
})

const mockSendUserMessage = vi.fn()
const mockCreateConversation = vi.fn()
const mockAuthorizeLocalDirectory = vi.fn()
const mockGetDefaultFolder = vi.fn()
const mockPickLocalDirectory = vi.fn()
const mockPickAttachments = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ sendUserMessage: mockSendUserMessage, isStreaming: false, stopCurrentStream: vi.fn() }),
}))

vi.mock('@/hooks/useChatAttachments', () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: mockPickAttachments,
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}))

vi.mock('@/lib/tauri', () => ({
  authorizeLocalDirectory: (...args: unknown[]) => mockAuthorizeLocalDirectory(...args),
  createConversation: () => mockCreateConversation(),
  getDefaultFolder: () => mockGetDefaultFolder(),
  pickLocalDirectory: (opts: unknown) => mockPickLocalDirectory(opts),
  readClipboardFilePaths: vi.fn().mockResolvedValue([]),
  saveClipboardImageToWorkspaceStaging: vi.fn(),
}))

beforeEach(() => {
  mockSendUserMessage.mockReset().mockResolvedValue(undefined)
  mockCreateConversation.mockReset().mockResolvedValue('new-conv-1')
  mockAuthorizeLocalDirectory.mockReset().mockResolvedValue(undefined)
  mockGetDefaultFolder.mockReset().mockResolvedValue({ id: 'default', rootPath: '/home', displayName: '默认' })
  mockPickLocalDirectory.mockReset()
  mockPickAttachments.mockReset().mockResolvedValue([])
  useChatStore.setState({ activeConversationId: null, conversations: [], messages: [] })
  useUiStore.setState({ route: { kind: 'home' }, prefillText: undefined })
  useHomeStore.setState({ selectedWorkspace: null })
})

describe('HomeTaskComposerCard', () => {
  it('renders RichComposer', async () => {
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  })

  it('Enter with text → creates conversation, switches route, sends message', async () => {
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'analyze sales')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockCreateConversation).toHaveBeenCalled()
    expect(useChatStore.getState().activeConversationId).toBe('new-conv-1')
    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'new-conv-1' })
    expect(mockSendUserMessage.mock.calls[0][0]).toBe('analyze sales')
  })

  it('attachment via picker shows token + Enter sends with file refs', async () => {
    mockPickAttachments.mockResolvedValueOnce([
      {
        id: 'p1',
        fileName: 'plan.pdf',
        path: '/p/plan.pdf',
        kind: 'file',
        fileType: 'pdf',
        fileSize: 0,
        mimeType: undefined,
        source: 'picker',
      },
    ])
    const { container } = render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const attachBtn = container.querySelector('[aria-label="添加附件"]') as HTMLElement
    await act(async () => {
      attachBtn.click()
    })
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('plan.pdf')
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    const [text, files] = mockSendUserMessage.mock.calls[0]
    expect(text).toContain('[附件: plan.pdf](file:///p/plan.pdf)')
    expect(files).toHaveLength(1)
    expect(files[0].filePath).toBe('/p/plan.pdf')
  })

  it('non-default workspace → authorizeLocalDirectory called before send', async () => {
    useHomeStore.setState({
      selectedWorkspace: { id: 'proj', rootPath: '/Users/me/proj', displayName: 'proj' },
    })
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'go')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockAuthorizeLocalDirectory).toHaveBeenCalledWith('/Users/me/proj', 'new-conv-1')
  })

  it('default workspace → authorizeLocalDirectory NOT called', async () => {
    // selectedWorkspace null → getDefaultFolder returns id=default
    const user = userEvent.setup()
    render(<HomeTaskComposerCard />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'go')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled())
    expect(mockAuthorizeLocalDirectory).not.toHaveBeenCalled()
  })
})
```

#### 步骤

- [ ] Step 1: 替换 HomeTaskComposerCard.tsx
- [ ] Step 2: 替换测试文件
- [ ] Step 3: 跑 `pnpm exec vitest run src/components/home/__tests__/HomeTaskComposerCard` — 5 tests pass
- [ ] Step 4: tsc 0
- [ ] Step 5: Commit `feat(home): HomeTaskComposerCard wires RichComposer + drop/paste pipelines`

---

### Task 3: 全库测试 + lint

- [ ] Step 1: `pnpm test` — 看 RichComposer 相关测试都过；其它 pre-existing 失败照旧；不能因为 P5 引入新失败。

  对比基线：P4 完成时 `pnpm test` 输出 `17 failed | 740 passed (757)`。P5 完成应该差不多 — 可能因为 ChatBottomArea / HomeTaskComposerCard 测试重写后总数变化，但 RichComposer 范围 0 失败。
  
- [ ] Step 2: `pnpm exec tsc -b --noEmit` 0 errors
- [ ] Step 3: `pnpm lint 2>&1 | grep -E "rich-composer|chat-scene/ChatBottomArea|home/HomeTaskComposerCard"` 0 errors
- [ ] Step 4: 不需要新 commit（这一步是验证）
