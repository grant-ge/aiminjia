# ChatBottomArea Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让对话页底部按 `design.pen#Cbtm1` 落地为 `ChatBottomArea`，并以升级后的 `ChatComposerCompact` 作为可复用输入核心，完整替换当前 `InputBar` 的交互能力。

**Architecture:** 将现有 `InputBar` 中的页面无关输入 UI 能力上提到 `ChatComposerCompact`，通过 props 暴露按钮显隐、状态与插槽；新增场景化封装 `ChatBottomArea` 承接对话页的上传、授权、slash command、stop streaming、pending files 与 tips。最终由 `ChatPage` 改挂 `ChatBottomArea`，旧 `InputBar` 退出对话区主路径。

**Tech Stack:** React 19、TypeScript、Vitest、Testing Library、Tailwind utility class、现有 `useChat`/`useFileUpload`/`useWorkspaceAuthorization` hooks

---

## File Map

- Create: `src/components/chat-scene/ChatBottomArea.tsx`
- Create: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Modify: `src/components/chat-scene/ChatComposerCompact.tsx`
- Modify: `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
- Modify: `src/features/chat/ChatPage.tsx`
- Optional cleanup: `src/components/layout/InputBar.tsx`
- Optional cleanup: `src/components/layout/InputBar.agent-selector.test.tsx`

### Task 1: 扩展 ChatComposerCompact 的测试

**Files:**
- Modify: `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
- Test: `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`

- [ ] **Step 1: 写失败测试，覆盖可配置展示位与键盘行为**

```tsx
it('supports optional controls and tips content', () => {
  render(
    <ChatComposerCompact
      value=""
      onChange={() => {}}
      onSubmit={() => {}}
      projectLabel="Desktop"
      modelLabel="标准"
      permissionLabel="完全访问权限"
      showProjectButton={false}
      tips={<div>Enter 发送</div>}
    />,
  )

  expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
  expect(screen.getByText('标准')).toBeInTheDocument()
  expect(screen.getByText('完全访问权限')).toBeInTheDocument()
  expect(screen.getByText('Enter 发送')).toBeInTheDocument()
})

it('submits on Enter but not on Shift+Enter', () => {
  const onSubmit = vi.fn()
  render(
    <ChatComposerCompact value="hello" onChange={() => {}} onSubmit={onSubmit} />,
  )

  fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })
  expect(onSubmit).toHaveBeenCalledWith('hello')

  onSubmit.mockClear()
  fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter', shiftKey: true })
  expect(onSubmit).not.toHaveBeenCalled()
})
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `npm test -- src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
Expected: FAIL，提示 `permissionLabel` / `showProjectButton` / `tips` 等 props 不存在，或断言未通过

- [ ] **Step 3: 最小实现 ChatComposerCompact 的新 props 与交互**

```tsx
interface ChatComposerCompactProps {
  permissionLabel?: string
  showProjectButton?: boolean
  tips?: React.ReactNode
}

{permissionLabel ? (
  <button type="button">
    <ShieldCheck className="h-3.5 w-3.5" />
    <span>{permissionLabel}</span>
  </button>
) : null}

{showProjectButton ? (
  <button type="button" onClick={onPickProject}>
    <Folder className="h-3.5 w-3.5" />
    <span>{projectLabel}</span>
  </button>
) : null}

{tips ? <div data-testid="composer-tips">{tips}</div> : null}
```

- [ ] **Step 4: 再跑测试，确认通过**

Run: `npm test -- src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/components/chat-scene/ChatComposerCompact.tsx src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
git commit -m "test: extend chat composer compact coverage"
```

### Task 2: 让 ChatComposerCompact 支持对话页输入核心能力

**Files:**
- Modify: `src/components/chat-scene/ChatComposerCompact.tsx`
- Modify: `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
- Test: `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`

- [ ] **Step 1: 写失败测试，覆盖附件按钮、停止按钮、pending files、textarea ref/autoFocus 所需接口**

```tsx
it('renders pending files and stop state', () => {
  render(
    <ChatComposerCompact
      value=""
      onChange={() => {}}
      onSubmit={() => {}}
      isStreaming
      pendingFilesSlot={<div>draft.txt</div>}
      onOpenAttachment={() => {}}
    />,
  )

  expect(screen.getByText('draft.txt')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: '停止' })).toBeInTheDocument()
})
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `npm test -- src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
Expected: FAIL，提示 `isStreaming` / `pendingFilesSlot` / `onOpenAttachment` 等 props 不存在

- [ ] **Step 3: 最小实现输入核心能力**

```tsx
interface ChatComposerCompactProps {
  isStreaming?: boolean
  onStop?: () => void
  onOpenAttachment?: () => void
  pendingFilesSlot?: React.ReactNode
  topSlot?: React.ReactNode
  textareaRef?: React.RefObject<HTMLTextAreaElement | null>
  onKeyDown?: React.KeyboardEventHandler<HTMLTextAreaElement>
  onCompositionStart?: React.CompositionEventHandler<HTMLTextAreaElement>
  onCompositionEnd?: React.CompositionEventHandler<HTMLTextAreaElement>
}

{topSlot}
{pendingFilesSlot}
<button type="button" aria-label="添加附件" onClick={onOpenAttachment}>...</button>
<textarea ref={textareaRef ?? ref} ... />
<button
  type="button"
  aria-label={isStreaming ? '停止' : '发送'}
  onClick={() => {
    if (isStreaming) onStop?.()
    else if (!submitDisabled && value.trim()) onSubmit(value)
  }}
>
```

- [ ] **Step 4: 再跑测试，确认通过**

Run: `npm test -- src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/components/chat-scene/ChatComposerCompact.tsx src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
git commit -m "feat: make chat composer compact reusable"
```

### Task 3: 为 ChatBottomArea 写失败测试

**Files:**
- Create: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`

- [ ] **Step 1: 写失败测试，约束对话页底部行为**

```tsx
it('hides project button but keeps permission label and tips', () => {
  render(<ChatBottomArea />)

  expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
  expect(screen.getByText('完全访问权限')).toBeInTheDocument()
  expect(screen.getByText('Enter 发送')).toBeInTheDocument()
  expect(screen.getByText('Shift+Enter 换行')).toBeInTheDocument()
})

it('sends message on Enter and shows stop while streaming', async () => {
  render(<ChatBottomArea />)
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
  fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })
  await waitFor(() => expect(sendUserMessageMock).toHaveBeenCalled())
})
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `npm test -- src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
Expected: FAIL，提示找不到 `ChatBottomArea` 组件

- [ ] **Step 3: 搭建测试所需 mocks**

```tsx
vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: sendUserMessageMock,
    isStreaming: false,
    stopCurrentStream: stopCurrentStreamMock,
  }),
}))
```

- [ ] **Step 4: 再跑测试，确认仍因缺实现失败**

Run: `npm test -- src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
Expected: FAIL，组件缺失或断言失败

- [ ] **Step 5: 提交**

```bash
git add src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
git commit -m "test: add chat bottom area coverage"
```

### Task 4: 实现 ChatBottomArea 并迁移 InputBar 行为

**Files:**
- Create: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/chat-scene/ChatComposerCompact.tsx`
- Optional cleanup: `src/components/layout/InputBar.tsx`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`

- [ ] **Step 1: 最小实现 ChatBottomArea 外壳，接入现有 hooks**

```tsx
export function ChatBottomArea() {
  const { t } = useTranslation()
  const [input, setInput] = useState('')
  const [pendingFiles, setPendingFiles] = useState<UploadedFile[]>([])
  const [isSending, setIsSending] = useState(false)
  const isComposingRef = useRef(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isUploading, selectAndUploadFiles } = useFileUpload()
  const { isAuthorizingDirectory, selectAndAuthorizeDirectory } = useWorkspaceAuthorization()
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const { workspace } = useAuthorizedWorkspace(activeConversationId)
}
```

- [ ] **Step 2: 迁移发送、IME、slash command、工作区提示与文件 chips 逻辑**

```tsx
const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
  if (slashOpen) return
  if (e.key === 'Enter' && !e.shiftKey && !isComposingRef.current && !e.nativeEvent.isComposing) {
    e.preventDefault()
    void handleSend()
  }
}

<ChatComposerCompact
  value={input}
  onChange={setInput}
  onSubmit={(value) => void handleSend(value)}
  isStreaming={isStreaming}
  onStop={stopCurrentStream}
  permissionLabel="完全访问权限"
  showProjectButton={false}
  onOpenAttachment={() => setShowAttachmentMenu((prev) => !prev)}
  pendingFilesSlot={pendingFiles.length > 0 ? <PendingFiles ... /> : null}
  topSlot={workspace ? <WorkspaceBanner ... /> : null}
  tips={<BottomTips />}
/>
```

- [ ] **Step 3: 运行 ChatBottomArea 测试，修到通过**

Run: `npm test -- src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
Expected: PASS

- [ ] **Step 4: 删除或降级旧 InputBar 的对话页职责**

```tsx
// ChatPage no longer imports InputBar
import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
```

- [ ] **Step 5: 提交**

```bash
git add src/components/chat-scene/ChatBottomArea.tsx src/components/chat-scene/ChatComposerCompact.tsx src/features/chat/ChatPage.tsx
git commit -m "feat: add chat bottom area"
```

### Task 5: 更新对话页接线并回归验证

**Files:**
- Modify: `src/features/chat/ChatPage.tsx`
- Optional cleanup: `src/components/layout/InputBar.agent-selector.test.tsx`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Test: `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`

- [ ] **Step 1: 写或更新失败测试，约束 ChatPage 使用 ChatBottomArea**

```tsx
expect(screen.getByText('Enter 发送')).toBeInTheDocument()
expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
```

- [ ] **Step 2: 运行相关测试，确认失败点在接线**

Run: `npm test -- src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
Expected: 若未切换接线则至少有一项 FAIL

- [ ] **Step 3: 更新 ChatPage 接线并清理陈旧引用**

```tsx
<div className="flex flex-1 flex-col overflow-hidden">
  <ChatArea />
  <ChatBottomArea />
</div>
```

- [ ] **Step 4: 运行目标测试集，确认全部通过**

Run: `npm test -- src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/components/settings/WorkspaceFirst.integration.test.tsx`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/features/chat/ChatPage.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx
git commit -m "test: verify chat bottom area integration"
```

### Task 6: 最终验证

**Files:**
- Verify only

- [ ] **Step 1: 运行完整相关测试**

Run: `npm test -- src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/components/settings/WorkspaceFirst.integration.test.tsx src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
Expected: PASS

- [ ] **Step 2: 运行构建级验证**

Run: `npm run build`
Expected: exit 0

- [ ] **Step 3: 检查变更范围**

Run: `git diff --stat`
Expected: 只包含聊天底部组件、对话页接线、相关测试与文档

- [ ] **Step 4: 记录验证结果**

```md
- chat composer tests: pass
- chat bottom area tests: pass
- workspace integration test: pass
- build: pass
```

- [ ] **Step 5: 提交**

```bash
git add .
git commit -m "feat: migrate chat page to chat bottom area"
```
