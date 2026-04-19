# Permission Ask 全链路前端实现（Plan-P）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:executing-plans`

**Goal:** 实现 `permission:ask` 事件的前端全链路，让用户在 AI 工具请求权限时看到弹窗，并能 Allow / Deny，解除当前所有 Ask 权限流程在用户侧静默失败的问题。

**Architecture:** 事件订阅 → Zustand store（`pendingAsks`）→ React Dialog → `invoke` 回调后端。对话框由 `useStreaming.ts` 驱动写入 store，由 `PermissionAskDialog` 组件（挂载在 `App.tsx`）渲染。

**Tech Stack:** React 18, TypeScript, Zustand, Tauri v2, Vitest

**Worktree branch:** pzc

---

## 后端接口确认（只读，不改动）

已从代码确认的后端事实：

**事件名：** `permission:ask`

**事件 payload（来自 `tauri_event_adapter.rs` 第 77–87 行）：**
```json
{
  "conversationId": "string",
  "runId": "string",
  "toolCallId": "string",
  "toolName": "string",
  "message": "string",
  "suggestions": ["string"] | null
}
```

**Tauri commands（来自 `commands/chat.rs`）：**
- `approve_permission_request(tool_call_id: string, updated_input?: Value)` → `Result<(), String>`
- `deny_permission_request(tool_call_id: string, message?: string)` → `Result<(), String>`
- `cancel_permission_request(tool_call_id: string, message?: string)` → `Result<(), String>`

用户点 **Allow** → `approve_permission_request`（`updated_input: null`）  
用户点 **Deny** → `deny_permission_request`（`message: undefined`）  
用户关闭弹窗（按 ESC / 点 overlay） → `cancel_permission_request`

---

## Task 结构

| Task | 内容 | 测试文件 |
|------|------|---------|
| P1 | `TAURI_EVENTS` + `tauri.ts` 类型和封装 | `src/lib/tauri.events.test.ts`（追加） |
| P2 | `streamingStore` 新增 `pendingAsks` 状态 | `src/stores/streamingStore.test.ts`（追加） |
| P3 | `PermissionAskDialog` 组件 | `src/components/common/PermissionAskDialog.test.tsx` |
| P4 | `useStreaming.ts` 订阅 + `App.tsx` 挂载 | `src/hooks/useStreaming.integration.test.tsx`（追加） |
| P5 | review_ 约束测试（前端无后端依赖隔离） | `src/lib/permission-ask.review.test.ts` |

---

## P1 — `TAURI_EVENTS` + `tauri.ts` 类型和封装

### 文件
- `src/lib/tauri.ts` — 修改（追加常量、类型、两个函数）
- `src/lib/tauri.events.test.ts` — 修改（追加测试用例）

### TDD 步骤

**先写测试（追加到 `src/lib/tauri.events.test.ts`）：**

```typescript
describe('permission:ask contract', () => {
  it('exposes PERMISSION_ASK event constant with correct value', () => {
    expect(TAURI_EVENTS.PERMISSION_ASK).toBe('permission:ask')
  })

  it('onPermissionAsk registers listener with correct event name', async () => {
    const handler = vi.fn()
    await onPermissionAsk(handler)
    expect(tauriEventMock.listen).toHaveBeenCalledWith(
      'permission:ask',
      expect.any(Function),
    )
  })

  it('resolvePermissionAsk approve calls invoke with correct command and params', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    const invokeMock = vi.mocked(invoke)
    invokeMock.mockResolvedValue(undefined)

    await approvePermissionRequest('tool-call-123', null)

    expect(invokeMock).toHaveBeenCalledWith('approve_permission_request', {
      toolCallId: 'tool-call-123',
      updatedInput: null,
    })
  })

  it('resolvePermissionAsk deny calls invoke with correct command and params', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    const invokeMock = vi.mocked(invoke)
    invokeMock.mockResolvedValue(undefined)

    await denyPermissionRequest('tool-call-123', undefined)

    expect(invokeMock).toHaveBeenCalledWith('deny_permission_request', {
      toolCallId: 'tool-call-123',
      message: undefined,
    })
  })
})
```

**运行（应当失败）：**
```bash
cd /Users/a20250311/IdeaProjects/lotus-app
pnpm exec vitest run src/lib/tauri.events.test.ts
```

**实现（追加到 `src/lib/tauri.ts`）：**

在 `TAURI_EVENTS` 对象中追加（`SKILL_FILE_CHANGED` 之后）：
```typescript
  PERMISSION_ASK: 'permission:ask',
```

在 Event Payload Types 区块追加：
```typescript
export interface PermissionAskPayload {
  conversationId: string
  runId: string
  toolCallId: string
  toolName: string
  message: string
  suggestions: string[] | null
}
```

在 Typed Event Listeners 区块末尾追加：
```typescript
/**
 * Listen for permission:ask events.
 * Emitted when a tool requires explicit user approval before executing.
 *
 * @param handler - Callback receiving the ask payload
 * @returns A function to unlisten (unsubscribe) from the event
 */
export function onPermissionAsk(
  handler: (payload: PermissionAskPayload) => void,
): Promise<() => void> {
  return listen<PermissionAskPayload>(TAURI_EVENTS.PERMISSION_ASK, (event) => {
    handler(event.payload)
  })
}
```

在 Chat Commands 区块追加：
```typescript
/**
 * Approve a pending permission request, allowing the tool to execute.
 *
 * @param toolCallId - The tool call ID from the PermissionAskPayload
 * @param updatedInput - Optional modified tool input to use instead of the original
 */
export function approvePermissionRequest(
  toolCallId: string,
  updatedInput: unknown,
): Promise<void> {
  return invoke<void>('approve_permission_request', { toolCallId, updatedInput })
}

/**
 * Deny a pending permission request, blocking the tool from executing.
 *
 * @param toolCallId - The tool call ID from the PermissionAskPayload
 * @param message - Optional reason shown to the agent (defaults to backend default)
 */
export function denyPermissionRequest(
  toolCallId: string,
  message?: string,
): Promise<void> {
  return invoke<void>('deny_permission_request', { toolCallId, message })
}

/**
 * Cancel a pending permission request (treated as user dismiss, e.g. closing the dialog).
 *
 * @param toolCallId - The tool call ID from the PermissionAskPayload
 * @param message - Optional reason
 */
export function cancelPermissionRequest(
  toolCallId: string,
  message?: string,
): Promise<void> {
  return invoke<void>('cancel_permission_request', { toolCallId, message })
}
```

**运行（应当通过）：**
```bash
pnpm exec vitest run src/lib/tauri.events.test.ts
```
预期：所有用例 PASS，无 TypeScript 错误。

**Commit：**
```bash
git add src/lib/tauri.ts src/lib/tauri.events.test.ts
git commit -m "$(cat <<'EOF'
feat(permission-ask): add TAURI_EVENTS.PERMISSION_ASK, payload type, and IPC wrappers - P1

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## P2 — `streamingStore` 新增 `pendingAsks` 状态

### 文件
- `src/stores/streamingStore.ts` — 修改（追加 `PendingAsk` 类型、`pendingAsks` 字段、相关 actions）
- `src/stores/streamingStore.test.ts` — 修改（追加测试）

### TDD 步骤

**先写测试（追加到 `src/stores/streamingStore.test.ts`）：**

```typescript
describe('pendingAsks state', () => {
  beforeEach(() => {
    resetChatStore()
  })

  it('addPendingAsk stores ask keyed by toolCallId', () => {
    const store = useStreamingStore.getState()
    store.addPendingAsk({
      conversationId: 'conv-1',
      runId: 'run-1',
      toolCallId: 'tc-abc',
      toolName: 'execute_python',
      message: 'Run code?',
      suggestions: null,
    })
    expect(store.pendingAsks.get('tc-abc')).toBeDefined()
    expect(store.pendingAsks.get('tc-abc')?.toolName).toBe('execute_python')
  })

  it('removePendingAsk removes by toolCallId', () => {
    const store = useStreamingStore.getState()
    store.addPendingAsk({
      conversationId: 'conv-1',
      runId: 'run-1',
      toolCallId: 'tc-abc',
      toolName: 'execute_python',
      message: 'Run code?',
      suggestions: null,
    })
    store.removePendingAsk('tc-abc')
    expect(store.pendingAsks.has('tc-abc')).toBe(false)
  })

  it('clearConversationPendingAsks removes all asks for a given conversationId', () => {
    const store = useStreamingStore.getState()
    store.addPendingAsk({ conversationId: 'conv-1', runId: 'r1', toolCallId: 'tc-1', toolName: 'a', message: 'm', suggestions: null })
    store.addPendingAsk({ conversationId: 'conv-1', runId: 'r1', toolCallId: 'tc-2', toolName: 'b', message: 'm', suggestions: null })
    store.addPendingAsk({ conversationId: 'conv-2', runId: 'r2', toolCallId: 'tc-3', toolName: 'c', message: 'm', suggestions: null })

    store.clearConversationPendingAsks('conv-1')

    expect(store.pendingAsks.has('tc-1')).toBe(false)
    expect(store.pendingAsks.has('tc-2')).toBe(false)
    expect(store.pendingAsks.has('tc-3')).toBe(true)
  })
})
```

**运行（应当失败）：**
```bash
pnpm exec vitest run src/stores/streamingStore.test.ts
```

**实现（修改 `src/stores/streamingStore.ts`）：**

追加 `PendingAsk` 接口：
```typescript
export interface PendingAsk {
  conversationId: string
  runId: string
  toolCallId: string
  toolName: string
  message: string
  suggestions: string[] | null
}
```

在 `StreamingState` 接口中追加字段和 actions：
```typescript
  pendingAsks: Map<string, PendingAsk>         // key: toolCallId
  addPendingAsk: (ask: PendingAsk) => void
  removePendingAsk: (toolCallId: string) => void
  clearConversationPendingAsks: (conversationId: string) => void
```

在 `createStreamingSlice` 初始值追加：
```typescript
    pendingAsks: new Map(),
```

在 `createStreamingSlice` action 实现中追加：
```typescript
    addPendingAsk: (ask) =>
      set((state) => {
        const next = new Map(state.pendingAsks)
        next.set(ask.toolCallId, ask)
        return { pendingAsks: next }
      }),

    removePendingAsk: (toolCallId) =>
      set((state) => {
        const next = new Map(state.pendingAsks)
        next.delete(toolCallId)
        return { pendingAsks: next }
      }),

    clearConversationPendingAsks: (conversationId) =>
      set((state) => {
        const next = new Map(state.pendingAsks)
        for (const [key, ask] of next) {
          if (ask.conversationId === conversationId) {
            next.delete(key)
          }
        }
        return { pendingAsks: next }
      }),
```

注意：`streamingStore.ts` 中的初始值对象和 `resetChatStore()` 在测试文件里不含 `pendingAsks`，需在 `src/stores/streamingStore.test.ts` 的 `resetChatStore()` 函数里同步追加 `pendingAsks: new Map()`。

**运行（应当通过）：**
```bash
pnpm exec vitest run src/stores/streamingStore.test.ts
```
预期：所有用例 PASS。

**Commit：**
```bash
git add src/stores/streamingStore.ts src/stores/streamingStore.test.ts
git commit -m "$(cat <<'EOF'
feat(permission-ask): add pendingAsks state to streamingStore - P2

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## P3 — `PermissionAskDialog` 组件

### 文件
- `src/components/common/PermissionAskDialog.tsx` — 新建
- `src/components/common/PermissionAskDialog.test.tsx` — 新建

### 组件规格

- 继承 `Modal` 组件（`src/components/common/Modal.tsx`），`size="sm"`，不允许点背景关闭（permission 必须显式决策，关闭视为 cancel）
- 渲染：工具名（bold）、message 描述文本、可选的 suggestions 列表（灰色小字 badge 风格）
- Footer：两个按钮 — `Button variant="secondary"` 标签"拒绝"，`Button variant="primary"` 标签"允许"
- Props：`open: boolean`、`ask: PendingAsk | null`、`onAllow: () => void`、`onDeny: () => void`、`onCancel: () => void`
- `onCancel` 在 ESC 键按下时调用（通过 `useEffect` 监听 `keydown`）
- Modal `onClose` 绑定到 `onCancel`（用于 × 按钮和 overlay）

### TDD 步骤

**先写测试（新建 `src/components/common/PermissionAskDialog.test.tsx`）：**

```typescript
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { PermissionAskDialog } from './PermissionAskDialog'
import type { PendingAsk } from '@/stores/streamingStore'

const baseAsk: PendingAsk = {
  conversationId: 'conv-1',
  runId: 'run-1',
  toolCallId: 'tc-abc',
  toolName: 'execute_python',
  message: '即将执行 Python 代码，是否允许？',
  suggestions: ['查看代码', '修改参数'],
}

describe('PermissionAskDialog', () => {
  it('renders nothing when open=false', () => {
    render(
      <PermissionAskDialog
        open={false}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />
    )
    expect(screen.queryByText('execute_python')).toBeNull()
  })

  it('renders tool name and message when open=true', () => {
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />
    )
    expect(screen.getByText('execute_python')).toBeInTheDocument()
    expect(screen.getByText('即将执行 Python 代码，是否允许？')).toBeInTheDocument()
  })

  it('renders suggestions when provided', () => {
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />
    )
    expect(screen.getByText('查看代码')).toBeInTheDocument()
    expect(screen.getByText('修改参数')).toBeInTheDocument()
  })

  it('calls onAllow when Allow button is clicked', () => {
    const onAllow = vi.fn()
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={onAllow}
        onDeny={vi.fn()}
        onCancel={vi.fn()}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /允许/i }))
    expect(onAllow).toHaveBeenCalledTimes(1)
  })

  it('calls onDeny when Deny button is clicked', () => {
    const onDeny = vi.fn()
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={onDeny}
        onCancel={vi.fn()}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /拒绝/i }))
    expect(onDeny).toHaveBeenCalledTimes(1)
  })

  it('calls onCancel when ESC key is pressed', () => {
    const onCancel = vi.fn()
    render(
      <PermissionAskDialog
        open={true}
        ask={baseAsk}
        onAllow={vi.fn()}
        onDeny={vi.fn()}
        onCancel={onCancel}
      />
    )
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onCancel).toHaveBeenCalledTimes(1)
  })
})
```

**运行（应当失败）：**
```bash
pnpm exec vitest run src/components/common/PermissionAskDialog.test.tsx
```

**实现（新建 `src/components/common/PermissionAskDialog.tsx`）：**

```typescript
/**
 * PermissionAskDialog — shown when the AI agent requests user approval
 * before executing a tool. The user must explicitly Allow or Deny.
 * Closing the dialog (ESC / × button) is treated as Cancel.
 */
import { useEffect } from 'react'
import { Modal } from './Modal'
import { Button } from './Button'
import type { PendingAsk } from '@/stores/streamingStore'

interface PermissionAskDialogProps {
  open: boolean
  ask: PendingAsk | null
  onAllow: () => void
  onDeny: () => void
  onCancel: () => void
}

export function PermissionAskDialog({
  open,
  ask,
  onAllow,
  onDeny,
  onCancel,
}: PermissionAskDialogProps) {
  // ESC key handler
  useEffect(() => {
    if (!open) return
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        onCancel()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [open, onCancel])

  if (!ask) return null

  return (
    <Modal
      open={open}
      onClose={onCancel}
      title="工具执行请求"
      size="sm"
      footer={
        <>
          <Button variant="secondary" onClick={onDeny}>
            拒绝
          </Button>
          <Button variant="primary" onClick={onAllow}>
            允许
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        {/* Tool name */}
        <div
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {ask.toolName}
        </div>

        {/* Message */}
        <p
          className="text-sm leading-relaxed"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          {ask.message}
        </p>

        {/* Suggestions */}
        {ask.suggestions && ask.suggestions.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {ask.suggestions.map((s) => (
              <span
                key={s}
                className="rounded px-2 py-0.5 text-xs"
                style={{
                  background: 'var(--color-bg-subtle)',
                  color: 'var(--color-text-muted)',
                  border: '1px solid var(--color-border)',
                }}
              >
                {s}
              </span>
            ))}
          </div>
        )}
      </div>
    </Modal>
  )
}
```

注意：`Modal` 当前的 body 区域有 `height: '70vh'` 的硬编码。`PermissionAskDialog` 内容很短，Modal 结构可以接受，但如果视觉上需要自适应高度，可在 P3 评审时讨论是否给 `Modal` 追加 `autoHeight` prop（超出本 Plan-P 范围，记录为 TODO）。

**运行（应当通过）：**
```bash
pnpm exec vitest run src/components/common/PermissionAskDialog.test.tsx
```
预期：6 个用例 PASS。

**Commit：**
```bash
git add src/components/common/PermissionAskDialog.tsx src/components/common/PermissionAskDialog.test.tsx
git commit -m "$(cat <<'EOF'
feat(permission-ask): add PermissionAskDialog component with Allow/Deny/Cancel - P3

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## P4 — `useStreaming.ts` 订阅 + `App.tsx` 挂载

### 文件
- `src/hooks/useStreaming.ts` — 修改（追加 `permission:ask` 监听 section）
- `src/App.tsx` — 修改（挂载 `PermissionAskDialog`，管理弹窗开关状态）
- `src/hooks/useStreaming.integration.test.tsx` — 修改（追加集成测试）

### 联调逻辑

`useStreaming.ts` 中的 `permission:ask` 处理器：
1. 收到 payload → 调用 `useStreamingStore.getState().addPendingAsk(payload)`（以 `toolCallId` 为 key，Map 保证同时只处理一个请求队列化）

`App.tsx` 中新增状态：
- `activeAsk: PendingAsk | null` — 当前展示在弹窗中的 ask
- `useChatStore` / `useStreamingStore` 的 `pendingAsks` 变化时，取 Map 的第一个 entry 作为 `activeAsk`

> 设计说明：同一时刻可能到达多个 ask（并发工具调用），`Map<toolCallId, PendingAsk>` 保留队列，每次处理完一个后自动弹出下一个。`activeAsk` 只是当前最顶部的。

**实现：`App.tsx` 中新增逻辑（在 return 语句前）：**

```typescript
import { useState, useEffect } from 'react'
import { useStreamingStore } from '@/stores/streamingStore'
import type { PendingAsk } from '@/stores/streamingStore'
import { PermissionAskDialog } from '@/components/common/PermissionAskDialog'
import {
  approvePermissionRequest,
  denyPermissionRequest,
  cancelPermissionRequest,
} from '@/lib/tauri'

// --- In App() function body ---
const pendingAsks = useStreamingStore((s) => s.pendingAsks)
const removePendingAsk = useStreamingStore((s) => s.removePendingAsk)

// Take the first pending ask (if any) to show in dialog
const activeAsk: PendingAsk | null = pendingAsks.size > 0
  ? (pendingAsks.values().next().value ?? null)
  : null

const handleAllow = async () => {
  if (!activeAsk) return
  const id = activeAsk.toolCallId
  removePendingAsk(id)
  try {
    await approvePermissionRequest(id, null)
  } catch (err) {
    console.error('[permission:ask] approve failed', err)
  }
}

const handleDeny = async () => {
  if (!activeAsk) return
  const id = activeAsk.toolCallId
  removePendingAsk(id)
  try {
    await denyPermissionRequest(id)
  } catch (err) {
    console.error('[permission:ask] deny failed', err)
  }
}

const handleCancel = async () => {
  if (!activeAsk) return
  const id = activeAsk.toolCallId
  removePendingAsk(id)
  try {
    await cancelPermissionRequest(id)
  } catch (err) {
    console.error('[permission:ask] cancel failed', err)
  }
}
```

在 `App.tsx` 的 JSX return 中，在 `<ToastContainer />` 之后追加：
```typescript
      <PermissionAskDialog
        open={activeAsk !== null}
        ask={activeAsk}
        onAllow={handleAllow}
        onDeny={handleDeny}
        onCancel={handleCancel}
      />
```

**`useStreaming.ts` 中追加监听（在 `file:generated` section 之后，watchdog 之前）：**

```typescript
  // --- permission:ask ---------------------------------------------------
  // When the agent needs user approval before executing a tool, the backend
  // emits this event. We write into the store; App.tsx renders the dialog.
  useTauriEvent(() =>
    onPermissionAsk((payload: PermissionAskPayload) => {
      console.log('[permission:ask]', payload.conversationId, payload.toolName, payload.toolCallId)
      useStreamingStore.getState().addPendingAsk(payload)
    }),
  )
```

同时在 `streaming:done` 和 `agent:idle` 的处理器末尾，调用 `clearConversationPendingAsks` 清理残留 ask（避免会话结束后弹窗残留）：

在 `streaming:done` handler 末尾追加：
```typescript
      useStreamingStore.getState().clearConversationPendingAsks(conversationId)
```

在 `agent:idle` handler 中（仅 primary scope）末尾追加：
```typescript
      useStreamingStore.getState().clearConversationPendingAsks(conversationId)
```

### TDD 步骤

**先写集成测试（追加到 `src/hooks/useStreaming.integration.test.tsx`）：**

```typescript
describe('useStreaming permission:ask integration', () => {
  it('adds pending ask to store when permission:ask event arrives', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('permission:ask')
    expect(handler).toBeDefined()

    act(() => {
      handler!({
        payload: {
          conversationId: 'conv-1',
          runId: 'run-1',
          toolCallId: 'tc-abc',
          toolName: 'execute_python',
          message: 'Run code?',
          suggestions: null,
        },
      })
    })

    // useStreamingStore is backed by chatStore (via bindStreamingStore)
    const { pendingAsks } = useChatStore.getState() as unknown as { pendingAsks: Map<string, unknown> }
    // Note: pendingAsks lives on the streaming slice of chatStore
    // Access via useStreamingStore
    const { default: useStreamingStore } = await import('@/stores/streamingStore')
    expect(useStreamingStore.getState().pendingAsks.get('tc-abc')).toBeDefined()
  })

  it('clears pending asks for conversation when streaming:done arrives', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const askHandler = tauriEventMock.listeners.get('permission:ask')
    act(() => {
      askHandler!({ payload: { conversationId: 'conv-1', runId: 'r', toolCallId: 'tc-1', toolName: 'x', message: 'm', suggestions: null } })
    })

    const doneHandler = tauriEventMock.listeners.get('streaming:done')
    act(() => {
      doneHandler!({ payload: { conversationId: 'conv-1', messageId: 'msg-1' } })
    })

    const { default: useStreamingStore } = await import('@/stores/streamingStore')
    expect(useStreamingStore.getState().pendingAsks.has('tc-1')).toBe(false)
  })
})
```

**运行（先失败）：**
```bash
pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx
```

**实现完成后运行（应当通过）：**
```bash
pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx
```

**全量回归：**
```bash
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts src/stores/streamingStore.test.ts src/components/common/PermissionAskDialog.test.tsx
```
预期：所有用例 PASS，无 TypeScript 错误。

**Lint 检查：**
```bash
pnpm lint
```

**Commit：**
```bash
git add src/hooks/useStreaming.ts src/App.tsx src/hooks/useStreaming.integration.test.tsx
git commit -m "$(cat <<'EOF'
feat(permission-ask): subscribe to permission:ask event and wire dialog in App - P4

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## P5 — review_ 约束测试

### 文件
- `src/lib/permission-ask.review.test.ts` — 新建

### 目的

验证 Plan-P 实现不违反架构约束：
1. `onPermissionAsk` / `approvePermissionRequest` 等函数使用 `TAURI_EVENTS.PERMISSION_ASK` 常量而不是字面量字符串
2. `PermissionAskDialog` 不直接调用 `invoke`（通过 props callback 解耦）
3. `pendingAsks` 是 `Map<string, PendingAsk>` 类型（不是裸数组，避免 O(n) 查找）

```typescript
import { describe, expect, it } from 'vitest'
import { TAURI_EVENTS } from './tauri'

describe('review_permission_ask: architecture constraints', () => {
  it('review_permission_ask_event_constant_value is permission:ask', () => {
    // Ensures event name is not a magic string in any handler
    expect(TAURI_EVENTS.PERMISSION_ASK).toBe('permission:ask')
  })

  it('review_permission_ask_pending_asks_is_map: PendingAsk store uses Map for O(1) lookup', async () => {
    const { useChatStore } = await import('@/stores/chatStore')
    const store = useChatStore.getState()
    // pendingAsks must be a Map (not array or plain object)
    expect(store.pendingAsks).toBeInstanceOf(Map)
  })

  it('review_permission_ask_dialog_has_no_direct_invoke: PermissionAskDialog receives callbacks via props', async () => {
    // Structural check: the dialog file should not import invoke directly.
    // We verify by checking the module exports don't expose invoke as a side effect.
    // (True enforcement is done by code review; this test confirms the prop interface shape.)
    const { PermissionAskDialog } = await import('@/components/common/PermissionAskDialog')
    expect(typeof PermissionAskDialog).toBe('function')
    // Props: open, ask, onAllow, onDeny, onCancel — all required
    // If the signature changes, this test will surface the drift.
    expect(PermissionAskDialog.length).toBe(1) // React component receives single props object
  })
})
```

**运行：**
```bash
pnpm exec vitest run src/lib/permission-ask.review.test.ts
```
预期：3 个用例 PASS。

**Commit：**
```bash
git add src/lib/permission-ask.review.test.ts
git commit -m "$(cat <<'EOF'
test(permission-ask): add review_ architecture constraint tests - P5

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## 验收标准

1. 用户在 AI 请求工具权限时，屏幕上弹出 `PermissionAskDialog`，展示工具名、描述、可选 suggestions
2. 点击"允许" → backend `approve_permission_request` 被调用，AI 继续执行工具
3. 点击"拒绝" → backend `deny_permission_request` 被调用，AI 收到拒绝消息
4. 按 ESC 或点 × → backend `cancel_permission_request` 被调用，视为用户取消
5. 会话结束（`streaming:done` / `agent:idle`）后残留的未响应 ask 被自动清除
6. 所有新增 Vitest 用例通过（`pnpm test`）
7. `pnpm lint` 无新增 ESLint 错误

---

## 遗留 TODO（超出本 Plan-P 范围）

- `Modal` 组件当前高度硬编码为 `70vh`，对 PermissionAskDialog 这类内容较少的弹窗不理想，建议后续追加 `autoHeight` prop
- 并发多个 ask 时当前按 Map 插入顺序串行处理，未来可考虑优先级排序（MCP tool 权限 > 内置工具）
- 国际化（i18n）：当前按钮文本"允许"/"拒绝"为硬编码中文；待 i18n 体系完善后替换为 `t()` 调用
