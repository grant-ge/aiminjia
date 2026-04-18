import { act, render, waitFor } from '@testing-library/react'
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
  })

  it('registers a frontend listener for runtime task terminal notifications', async () => {
    const view = render(<HookHarness />)
    await waitForListeners()

    expect(tauriEventMock.listeners.has('task:status-changed')).toBe(
      true,
    )

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
  })
})
