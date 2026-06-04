import { act, render, waitFor } from '@testing-library/react'
import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriEventMock = vi.hoisted(() => {
  const listeners = new Map<string, (event: { payload: unknown }) => void>()
  const listen = vi.fn((eventName: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(eventName, handler)
    return Promise.resolve(() => {
      listeners.delete(eventName)
    })
  })
  return { listeners, listen }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: tauriEventMock.listen,
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@/i18n', () => ({
  default: {
    t: (key: string) => key,
  },
}))

import { useStreaming } from './useStreaming'
import { useChatStore } from '@/stores/chatStore'
import { useStreamingStore } from '@/stores/streamingStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useDiagnosticsStore } from '@/stores/diagnosticsStore'

function HookHarness() {
  useStreaming()
  return null
}

async function waitForListeners() {
  await waitFor(() => {
    expect(tauriEventMock.listen).toHaveBeenCalled()
  })
}

describe('useStreaming integration review', () => {
  beforeEach(() => {
    tauriEventMock.listeners.clear()
    tauriEventMock.listen.mockClear()
    useChatStore.setState({
      conversations: [],
      activeConversationId: null,
      messages: [],
      busyConversations: new Set(),
      streamStates: {},
      taskStates: {},
      pendingAsks: new Map(),
      isStreaming: false,
      streamingContent: '',
      toolExecutions: [],
    })
    useDiagnosticsStore.getState().clearDiagnostics()
    useNotificationStore.getState().dismissAll()
    vi.mocked(invoke).mockReset()
  })

  it('registers a frontend listener for runtime task terminal notifications', async () => {
    const view = render(<HookHarness />)
    await waitForListeners()

    expect(tauriEventMock.listeners.has('task:status-changed')).toBe(
      true,
    )

    view.unmount()
  })

  it('appends backend diagnostics events into the diagnostics store', async () => {
    const view = render(<HookHarness />)
    await waitForListeners()

    const diagnosticsHandler = tauriEventMock.listeners.get('diagnostics:event')
    expect(diagnosticsHandler).toBeTypeOf('function')

    act(() => {
      diagnosticsHandler?.({
        payload: {
          ts: '2026-04-25T00:00:00.000Z',
          seq: 101,
          category: 'diagnostics',
          level: 'info',
          source: 'backend',
          event: 'turn.started',
          conversationId: 'conv-diag',
          runId: 'run-diag',
          payload: { phase: 'start' },
        },
      })
    })

    expect(useDiagnosticsStore.getState().events.at(-1)).toMatchObject({
      event: 'turn.started',
      source: 'backend',
      conversationId: 'conv-diag',
      runId: 'run-diag',
    })

    view.unmount()
  })

  it('writes task terminal notifications into the chat store', async () => {
    const view = render(<HookHarness />)
    await waitForListeners()

    const taskStatusChanged = tauriEventMock.listeners.get('task:status-changed')
    expect(taskStatusChanged).toBeTypeOf('function')

    act(() => {
      taskStatusChanged?.({
        payload: {
          conversationId: 'conv-task',
          taskId: 'task-1',
          status: 'completed',
          runId: 'run-task-1',
        },
      })
    })

    const state = useChatStore.getState()
    expect(state.taskStates['conv-task']).toEqual([
      {
        taskId: 'task-1',
        status: 'completed',
        runId: 'run-task-1',
      },
    ])

    view.unmount()
  })

  it('does not clear the parent conversation when a child/background agent becomes idle', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-parent',
      busyConversations: new Set(['conv-parent']),
      streamStates: {
        'conv-parent': {
          isStreaming: true,
          streamingContent: 'still running',
          toolExecutions: [],
        },
      },
      isStreaming: true,
      streamingContent: 'still running',
      toolExecutions: [],
      taskStates: {},
    })

    const view = render(<HookHarness />)
    await waitForListeners()

    const agentIdle = tauriEventMock.listeners.get('agent:idle')
    expect(agentIdle).toBeTypeOf('function')

    act(() => {
      agentIdle?.({
        payload: {
          conversationId: 'conv-parent',
          agentId: 'child-agent-1',
          runId: 'child-run-1',
        },
      })
    })

    const state = useChatStore.getState()
    expect(state.busyConversations.has('conv-parent')).toBe(
      true,
    )
    expect(state.streamStates['conv-parent']?.isStreaming).toBe(
      true,
    )

    view.unmount()
  })

  it('adds pending ask to store when permission:ask event arrives', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('permission:ask')
    expect(handler).toBeTypeOf('function')

    act(() => {
      handler?.({
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

    expect(useStreamingStore.getState().pendingAsks.get('tc-abc')).toBeDefined()
    expect(useDiagnosticsStore.getState().events.some((event) => event.event === 'permission.ask.received')).toBe(true)
  })

  it('clears pending asks for conversation when streaming:done arrives', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const askHandler = tauriEventMock.listeners.get('permission:ask')
    const doneHandler = tauriEventMock.listeners.get('streaming:done')
    expect(askHandler).toBeTypeOf('function')
    expect(doneHandler).toBeTypeOf('function')

    act(() => {
      askHandler?.({
        payload: {
          conversationId: 'conv-1',
          runId: 'r1',
          toolCallId: 'tc-1',
          toolName: 'execute_python',
          message: 'Run code?',
          suggestions: null,
        },
      })
    })

    act(() => {
      doneHandler?.({
        payload: {
          conversationId: 'conv-1',
          messageId: 'msg-1',
        },
      })
    })

    expect(useStreamingStore.getState().pendingAsks.has('tc-1')).toBe(false)
    expect(useDiagnosticsStore.getState().events.some((event) => event.event === 'streaming.done.received')).toBe(true)
  })

  it('stores compact completion token savings from compact:completed', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-compact',
      streamStates: {
        'conv-compact': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [],
        },
      },
    })

    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('compact:completed')
    expect(handler).toBeTypeOf('function')

    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-compact',
          runId: 'run-1',
          boundaryId: 'boundary-1',
          trigger: 'manual',
          createdAt: '2026-06-02T00:00:00.000Z',
          tailMessageId: 'tail-1',
          preTokens: 12000,
          postTokens: 4500,
          tokensSaved: 7500,
          messagesSummarized: 18,
        },
      })
    })

    expect(useChatStore.getState().streamStates['conv-compact']?.lastCompactSummary).toMatchObject({
      preTokens: 12000,
      postTokens: 4500,
      tokensSaved: 7500,
      messagesSummarized: 18,
    })
    expect(useChatStore.getState().messages).toEqual([
      expect.objectContaining({
        id: 'boundary-1',
        conversationId: 'conv-compact',
        role: 'system',
        createdAt: '2026-06-02T00:00:00.000Z',
        subtype: 'compact_boundary',
        content: { text: 'Conversation compacted' },
        compactMetadata: expect.objectContaining({
          trigger: 'manual',
          tailMessageId: 'tail-1',
          preTokens: 12000,
          postTokens: 4500,
          tokensSaved: 7500,
          messagesSummarized: 18,
        }),
      }),
    ])
  })

  it('preserves optimistic user message sender when persisted echo replaces client id', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      messages: [{
        id: 'client-1',
        conversationId: 'conv-1',
        role: 'user',
        createdAt: '2026-04-24T00:00:00Z',
        content: { text: 'hello' },
        sender: { name: 'Alice', isLoggedIn: true },
      }],
    })

    render(<HookHarness />)
    await waitForListeners()

    const messageUpdatedHandler = tauriEventMock.listeners.get('message:updated')
    act(() => {
      messageUpdatedHandler?.({
        payload: {
          id: 'msg-1',
          conversationId: 'conv-1',
          role: 'user',
          createdAt: '2026-04-24T00:00:01Z',
          content: { text: 'hello' },
          clientMessageId: 'client-1',
        },
      })
    })

    expect(useChatStore.getState().messages).toEqual([{
      id: 'msg-1',
      conversationId: 'conv-1',
      role: 'user',
      createdAt: '2026-04-24T00:00:01Z',
      content: { text: 'hello' },
      sender: { name: 'Alice', isLoggedIn: true },
      clientMessageId: 'client-1',
    }])
  })

  it('does not remove optimistic user message on streaming:done', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-done',
      messages: [{
        id: 'client-1',
        conversationId: 'conv-done',
        role: 'user',
        createdAt: '2026-04-24T00:00:00Z',
        content: { text: 'hello' },
      }],
      streamStates: {
        'conv-done': { isStreaming: true, streamingContent: 'ok', toolExecutions: [] },
      },
      isStreaming: true,
      streamingContent: 'ok',
      toolExecutions: [],
    })

    render(<HookHarness />)
    await waitForListeners()

    const doneHandler = tauriEventMock.listeners.get('streaming:done')
    act(() => {
      doneHandler?.({ payload: { conversationId: 'conv-done' } })
    })

    expect(useChatStore.getState().messages.map((m) => m.id)).toEqual(['client-1'])
  })

  it('rolls back only same-conversation optimistic user message on streaming:error', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-active',
      messages: [{
        id: 'client-active',
        conversationId: 'conv-active',
        role: 'user',
        createdAt: '2026-04-24T00:00:00Z',
        content: { text: 'keep me' },
      }],
      streamStates: {
        'conv-background': { isStreaming: true, streamingContent: 'bad', toolExecutions: [] },
      },
      isStreaming: false,
      streamingContent: '',
      toolExecutions: [],
    })

    render(<HookHarness />)
    await waitForListeners()

    const errorHandler = tauriEventMock.listeners.get('streaming:error')
    act(() => {
      errorHandler?.({ payload: { conversationId: 'conv-background', error: 'failed' } })
    })

    expect(useChatStore.getState().messages.map((m) => m.id)).toEqual(['client-active'])
  })

  it('clears busy and records prior content when streaming:error arrives', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-error',
      busyConversations: new Set(['conv-error']),
      streamStates: {
        'conv-error': {
          isStreaming: true,
          streamingContent: 'partial answer',
          toolExecutions: [],
        },
      },
      isStreaming: true,
      streamingContent: 'partial answer',
      toolExecutions: [],
    })

    render(<HookHarness />)
    await waitForListeners()

    const errorHandler = tauriEventMock.listeners.get('streaming:error')
    act(() => {
      errorHandler?.({
        payload: {
          conversationId: 'conv-error',
          error: 'AIjia v2 stream ended without response.completed',
          rawError: 'AIjia v2 stream ended without response.completed',
        },
      })
    })

    const state = useChatStore.getState()
    const clearDiagnostic = useDiagnosticsStore.getState().events.find((event) =>
      event.event === 'store.streaming.clear' &&
      event.conversationId === 'conv-error'
    )
    expect(state.busyConversations.has('conv-error')).toBe(false)
    expect(state.streamStates['conv-error']?.isStreaming).toBe(false)
    expect(state.streamStates['conv-error']?.streamingContent).toBe('')
    expect(clearDiagnostic?.payload).toMatchObject({ hadContent: true })
    expect(useDiagnosticsStore.getState().events.some((event) => event.event === 'streaming.error.received')).toBe(true)
  })

  it('registers a listener for turn:completed events', async () => {
    render(<HookHarness />)
    await waitForListeners()

    expect(tauriEventMock.listeners.has('turn:completed')).toBe(true)
  })

  it('clears busy state for MaxIterationsReached (no toast, PR2: error rendered as in-bubble callout)', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-turn',
      busyConversations: new Set(['conv-turn']),
      streamStates: {
        'conv-turn': {
          isStreaming: true,
          streamingContent: 'thinking',
          toolExecutions: [],
        },
      },
      isStreaming: true,
      streamingContent: 'thinking',
      toolExecutions: [],
    })

    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:completed')
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-turn',
          runId: 'run-turn',
          outcome: 'MaxIterationsReached',
          totalInputTokens: 100,
          totalOutputTokens: 20,
          totalCostUsd: 0.01,
          permissionDenialCount: 0,
          iterations: 30,
        },
      })
    })

    const chatState = useChatStore.getState()
    const notifications = useNotificationStore.getState().notifications
    expect(chatState.busyConversations.has('conv-turn')).toBe(false)
    expect(chatState.streamStates['conv-turn']?.isStreaming).toBe(false)
    // PR2 D' 原则：toast 已删，错误由 AiBubble ErrorCallout 在会话流中渲染
    expect(notifications.some((n) => n.title === 'turnOutcome.maxIterationsTitle')).toBe(false)
  })

  it('clears busy state for BudgetExceeded (no toast, PR2: error rendered as in-bubble callout)', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:completed')
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-budget',
          runId: 'run-budget',
          outcome: 'BudgetExceeded',
          totalInputTokens: 100,
          totalOutputTokens: 20,
          totalCostUsd: 0.12,
          permissionDenialCount: 0,
          reason: 'Reached maximum budget ($0.10)',
        },
      })
    })

    // PR2 D' 原则：toast 已删，错误由 AiBubble ErrorCallout 在会话流中渲染
    expect(
      useNotificationStore.getState().notifications.some(
        (n) => n.title === 'turnOutcome.budgetExceededTitle',
      ),
    ).toBe(false)
  })

  it('clears busy state for ExecutionError (no toast, PR2: error rendered as in-bubble callout)', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:completed')
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-error',
          runId: 'run-error',
          outcome: 'ExecutionError',
          totalInputTokens: 100,
          totalOutputTokens: 20,
          totalCostUsd: 0.02,
          permissionDenialCount: 0,
          message: 'gateway timeout',
        },
      })
    })

    // PR2 D' 原则：toast 已删，错误由 AiBubble ErrorCallout 在会话流中渲染
    expect(
      useNotificationStore.getState().notifications.some(
        (n) => n.title === 'turnOutcome.executionErrorTitle',
      ),
    ).toBe(false)
  })

  it('pushes info toast for Success when cost is present', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:completed')
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-success',
          runId: 'run-success',
          outcome: 'Success',
          totalInputTokens: 100,
          totalOutputTokens: 20,
          totalCostUsd: 0.001,
          permissionDenialCount: 0,
        },
      })
    })

    expect(
      useNotificationStore.getState().notifications.some(
        (n) => n.level === 'info' && n.title === 'turnOutcome.successSummaryTitle',
      ),
    ).toBe(true)
  })

  it('does not push Success toast when cost is null', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:completed')
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-success-null',
          runId: 'run-success-null',
          outcome: 'Success',
          totalInputTokens: 100,
          totalOutputTokens: 20,
          totalCostUsd: null,
          permissionDenialCount: 0,
        },
      })
    })

    expect(
      useNotificationStore.getState().notifications.some(
        (n) => n.title === 'turnOutcome.successSummaryTitle',
      ),
    ).toBe(false)
  })

  it('stores the latest turn summary when turn:completed arrives', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:completed')
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-summary',
          runId: 'run-summary',
          outcome: 'ExecutionError',
          totalInputTokens: 250,
          totalOutputTokens: 50,
          totalCostUsd: 0.003,
          permissionDenialCount: 0,
          message: 'bad gateway',
        },
      })
    })

    expect(useChatStore.getState().streamStates['conv-summary']?.lastTurnSummary).toMatchObject({
      outcome: 'ExecutionError',
      totalInputTokens: 250,
      totalOutputTokens: 50,
      totalCostUsd: 0.003,
    })
  })

  it('adds Skill tool events to streaming bubbles', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('tool:executing')
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-load-skill',
          toolName: 'Skill',
          toolId: 'tool-load-skill-1',
          purpose: 'Load salary query skill',
          input: { skill_name: 'salary-query' },
        },
      })
    })

    expect(useChatStore.getState().streamStates['conv-load-skill']?.toolExecutions).toContainEqual(
      expect.objectContaining({
        toolName: 'Skill',
        toolId: 'tool-load-skill-1',
        status: 'executing',
        summary: 'Load salary query skill',
      }),
    )
  })

  it('silently uploads diagnostics when a primary tool completes with an error', async () => {
    vi.mocked(invoke).mockResolvedValue({
      session_id: 'diag-1',
      chunks_uploaded: 1,
      chunks_total: 1,
      events_uploaded: 0,
      app_log_lines_uploaded: 1,
      bad_metrics_lines: 0,
    })
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('tool:completed')
    act(() => {
      handler?.({
        payload: {
          id: 'msg-tool-error',
          conversationId: 'conv-tool-error',
          role: 'tool',
          createdAt: '2026-05-08T00:00:00.000Z',
          content: { text: '' },
          toolResult: {
            toolCallId: 'tool-error-1',
            name: 'get_file_info',
            content: 'tool execution failed',
            isError: true,
            durationMs: 12,
          },
        },
      })
    })

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('upload_diagnostic_logs')
    })
  })

  it('does not upload diagnostics twice for the same failed tool call', async () => {
    vi.mocked(invoke).mockResolvedValue({
      session_id: 'diag-1',
      chunks_uploaded: 1,
      chunks_total: 1,
      events_uploaded: 0,
      app_log_lines_uploaded: 1,
      bad_metrics_lines: 0,
    })
    render(<HookHarness />)
    await waitForListeners()

    const payload = {
      id: 'msg-tool-error',
      conversationId: 'conv-tool-error',
      role: 'tool',
      createdAt: '2026-05-08T00:00:00.000Z',
      content: { text: '' },
      toolResult: {
        toolCallId: 'tool-error-dedup',
        name: 'get_file_info',
        content: 'tool execution failed',
        isError: true,
        durationMs: 12,
      },
    }
    const handler = tauriEventMock.listeners.get('tool:completed')
    act(() => {
      handler?.({ payload })
      handler?.({ payload: { ...payload, id: 'msg-tool-error-replay' } })
    })

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('upload_diagnostic_logs')
    })
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'upload_diagnostic_logs')).toHaveLength(1)
  })

  // ── Turn-stage events (spec docs/superpowers/specs/2026-05-17-turn-stages.md) ──

  it('hydrates turnStage on turn:stage event', async () => {
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:stage')
    expect(handler).toBeTypeOf('function')

    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-stage',
          runId: 'run-1',
          stage: {
            kind: 'tools',
            iteration: 1,
            running: [
              { toolName: 'Bash', toolCallId: 'tc-1', startedAtMs: 1_700_000_000_000 },
            ],
            completedInBatch: 0,
          },
          stageStartedAtMs: 1_700_000_000_000,
        },
      })
    })

    const state = useChatStore.getState().streamStates['conv-stage']
    expect(state?.turnStage?.kind).toBe('tools')
    expect(state?.stageStartedAt).toBe(1_700_000_000_000)
    expect(state?.turnStartedAt).toBeGreaterThan(0)
    expect(useDiagnosticsStore.getState().events.some((e) => e.event === 'turn.stage.received')).toBe(true)
  })

  it('marks conversation busy and streaming when a drained pending turn starts', async () => {
    useChatStore.setState({ activeConversationId: 'conv-drain-start' })
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:stage')
    expect(handler).toBeTypeOf('function')

    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-drain-start',
          runId: 'run-drain',
          stage: { kind: 'submitted' },
          stageStartedAtMs: 1_700_000_000_001,
        },
      })
    })

    const state = useChatStore.getState()
    expect(state.busyConversations.has('conv-drain-start')).toBe(true)
    expect(state.streamStates['conv-drain-start']?.isStreaming).toBe(true)
    expect(state.isStreaming).toBe(true)
  })

  it('refreshes lastHeartbeatAt on turn:heartbeat event', async () => {
    useChatStore.setState({
      streamStates: {
        'conv-hb': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [],
          turnStage: { kind: 'waitingLlm', iteration: 0 },
          stageStartedAt: 1_000,
          lastHeartbeatAt: 1_000,
          turnStartedAt: 1_000,
        },
      },
    })
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('turn:heartbeat')
    expect(handler).toBeTypeOf('function')

    const before = useChatStore.getState().streamStates['conv-hb'].lastHeartbeatAt!
    act(() => {
      handler?.({
        payload: {
          conversationId: 'conv-hb',
          runId: 'run-hb',
          stageElapsedMs: 2400,
          turnElapsedMs: 5000,
        },
      })
    })
    const after = useChatStore.getState().streamStates['conv-hb'].lastHeartbeatAt!
    expect(after).toBeGreaterThan(before)
  })

  it('clears turnStage fields on streaming:done', async () => {
    useChatStore.setState({
      activeConversationId: 'conv-clear',
      streamStates: {
        'conv-clear': {
          isStreaming: true,
          streamingContent: 'partial text',
          toolExecutions: [],
          turnStage: { kind: 'streaming', iteration: 0 },
          stageStartedAt: 1_000,
          lastHeartbeatAt: 1_000,
          turnStartedAt: 1_000,
        },
      },
    })
    render(<HookHarness />)
    await waitForListeners()

    const handler = tauriEventMock.listeners.get('streaming:done')
    act(() => {
      handler?.({ payload: { conversationId: 'conv-clear' } })
    })

    const state = useChatStore.getState().streamStates['conv-clear']
    expect(state.turnStage).toBeNull()
    expect(state.stageStartedAt).toBeNull()
    expect(state.lastHeartbeatAt).toBeNull()
    expect(state.turnStartedAt).toBeNull()
    expect(state.isStreaming).toBe(false)
  })
})
