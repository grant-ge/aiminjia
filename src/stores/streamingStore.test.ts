import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useChatStore } from './chatStore'
import { useStreamingStore } from './streamingStore'
import type { PendingAsk } from './streamingStore'
import { useDiagnosticsStore } from './diagnosticsStore'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}))

function resetChatStore() {
  useDiagnosticsStore.getState().clearDiagnostics()
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
}

function buildPendingAsk(overrides: Partial<PendingAsk> = {}): PendingAsk {
  return {
    conversationId: 'conv-1',
    runId: 'run-1',
    toolCallId: 'tc-abc',
    toolName: 'execute_python',
    message: 'Run code?',
    suggestions: null,
    mode: 'default',
    rememberOptions: ['session'],
    defaultDestination: 'session',
    ...overrides,
  }
}

describe('streamingStore view', () => {
  beforeEach(() => {
    resetChatStore()
  })

  it('updates per-conversation streaming state through the same underlying chat store', () => {
    useStreamingStore.getState().setConversationStreaming('c1', true)
    useStreamingStore.getState().appendConversationStreamingContent('c1', 'hello world')

    expect(useChatStore.getState().streamStates['c1']?.isStreaming).toBe(true)
    expect(useChatStore.getState().streamStates['c1']?.streamingContent).toBe('hello world')
    expect(useDiagnosticsStore.getState().events.some((event) => event.event === 'store.streaming.append')).toBe(true)
  })

  it('clears tool executions when clearing a conversation stream state', () => {
    const store = useStreamingStore.getState()
    store.setConversationStreaming('c1', true)
    store.addConversationToolExecution('c1', {
      toolName: 'execute_python',
      toolId: 'tool-1',
      status: 'executing',
    })

    store.clearConversationStreamState('c1')

    expect(useChatStore.getState().streamStates['c1']?.isStreaming).toBe(false)
    expect(useChatStore.getState().streamStates['c1']?.toolExecutions).toHaveLength(0)
    expect(useDiagnosticsStore.getState().events.some((event) => event.event === 'store.streaming.clear')).toBe(true)
  })

  it('recomputes legacy tool execution fields for the active conversation', () => {
    useChatStore.getState().setActiveConversation('c1')

    useStreamingStore.getState().addConversationToolExecution('c1', {
      toolName: 'search_web',
      toolId: 'tool-1',
      status: 'executing',
    })

    expect(useChatStore.getState().toolExecutions).toHaveLength(1)
    expect(useChatStore.getState().toolExecutions[0].toolId).toBe('tool-1')
  })
})

describe('pendingAsks state', () => {
  beforeEach(() => {
    resetChatStore()
  })

  it('addPendingAsk stores ask keyed by toolCallId', () => {
    const store = useStreamingStore.getState()
    store.addPendingAsk(buildPendingAsk())

    const next = useStreamingStore.getState()
    expect(next.pendingAsks.get('tc-abc')).toBeDefined()
    expect(next.pendingAsks.get('tc-abc')?.toolName).toBe('execute_python')
  })

  it('removePendingAsk removes by toolCallId', () => {
    const store = useStreamingStore.getState()
    store.addPendingAsk(buildPendingAsk())

    store.removePendingAsk('tc-abc')

    expect(useStreamingStore.getState().pendingAsks.has('tc-abc')).toBe(false)
  })

  it('clearConversationPendingAsks removes all asks for a given conversationId', () => {
    const store = useStreamingStore.getState()
    store.addPendingAsk(buildPendingAsk({ runId: 'r1', toolCallId: 'tc-1', toolName: 'a', message: 'm' }))
    store.addPendingAsk(buildPendingAsk({ runId: 'r1', toolCallId: 'tc-2', toolName: 'b', message: 'm' }))
    store.addPendingAsk(buildPendingAsk({ conversationId: 'conv-2', runId: 'r2', toolCallId: 'tc-3', toolName: 'c', message: 'm' }))

    store.clearConversationPendingAsks('conv-1')

    const next = useStreamingStore.getState()
    expect(next.pendingAsks.has('tc-1')).toBe(false)
    expect(next.pendingAsks.has('tc-2')).toBe(false)
    expect(next.pendingAsks.has('tc-3')).toBe(true)
  })
})

describe('turn summaries', () => {
  beforeEach(() => {
    resetChatStore()
  })

  it('stores the latest turn summary per conversation', () => {
    const store = useStreamingStore.getState()

    store.setLastTurnSummary('conv-summary', {
      outcome: 'BudgetExceeded',
      totalInputTokens: 1200,
      totalOutputTokens: 300,
      totalCostUsd: 0.24,
      completedAt: 1_713_000_000,
    })

    expect(useStreamingStore.getState().streamStates['conv-summary']?.lastTurnSummary).toEqual({
      outcome: 'BudgetExceeded',
      totalInputTokens: 1200,
      totalOutputTokens: 300,
      totalCostUsd: 0.24,
      completedAt: 1_713_000_000,
    })
  })
})
