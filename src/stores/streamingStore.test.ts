import { beforeEach, describe, expect, it } from 'vitest'

import { useChatStore } from './chatStore'
import { useStreamingStore } from './streamingStore'

function resetChatStore() {
  useChatStore.setState({
    conversations: [],
    activeConversationId: null,
    messages: [],
    busyConversations: new Set(),
    streamStates: {},
    taskStates: {},
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],
  })
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
  })

  it('preserves tool executions when clearing a conversation stream state', () => {
    const store = useStreamingStore.getState()
    store.setConversationStreaming('c1', true)
    store.addConversationToolExecution('c1', {
      toolName: 'execute_python',
      toolId: 'tool-1',
      status: 'executing',
    })

    store.clearConversationStreamState('c1')

    expect(useChatStore.getState().streamStates['c1']?.isStreaming).toBe(false)
    expect(useChatStore.getState().streamStates['c1']?.toolExecutions).toHaveLength(1)
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
