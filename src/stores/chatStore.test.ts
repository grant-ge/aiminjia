import { describe, it, expect, beforeEach } from 'vitest'
import { useChatStore } from './chatStore'
import type { Message, Conversation } from '@/types/message'

// Reset store between tests
beforeEach(() => {
  useChatStore.setState({
    conversations: [],
    activeConversationId: null,
    messages: [],
    busyConversations: new Set(),
    streamStates: {},
    isStreaming: false,
    streamingContent: '',
    toolExecutions: [],
  })
})

// ---------------------------------------------------------------------------
// Conversation management
// ---------------------------------------------------------------------------

describe('chatStore — conversations', () => {
  it('starts with empty conversations', () => {
    const state = useChatStore.getState()
    expect(state.conversations).toEqual([])
    expect(state.activeConversationId).toBeNull()
  })

  it('sets conversations list', () => {
    const convs: Conversation[] = [
      { id: 'c1', title: 'Test', createdAt: '2025-01-01', updatedAt: '2025-01-01', isArchived: false },
    ]
    useChatStore.getState().setConversations(convs)
    expect(useChatStore.getState().conversations).toHaveLength(1)
    expect(useChatStore.getState().conversations[0].id).toBe('c1')
  })

  it('sets active conversation', () => {
    useChatStore.getState().setActiveConversation('c1')
    expect(useChatStore.getState().activeConversationId).toBe('c1')
  })

  it('clears active conversation', () => {
    useChatStore.getState().setActiveConversation('c1')
    useChatStore.getState().setActiveConversation(null)
    expect(useChatStore.getState().activeConversationId).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// Message management
// ---------------------------------------------------------------------------

describe('chatStore — messages', () => {
  const msg1: Message = {
    id: 'm1',
    conversationId: 'c1',
    role: 'user',
    content: { text: 'Hello' },
    createdAt: '2025-01-01T00:00:00Z',
  }

  const msg2: Message = {
    id: 'm2',
    conversationId: 'c1',
    role: 'assistant',
    content: { text: 'Hi there' },
    createdAt: '2025-01-01T00:00:01Z',
  }

  it('starts with empty messages', () => {
    expect(useChatStore.getState().messages).toEqual([])
  })

  it('sets messages', () => {
    useChatStore.getState().setMessages([msg1, msg2])
    expect(useChatStore.getState().messages).toHaveLength(2)
  })

  it('adds a message', () => {
    useChatStore.getState().addMessage(msg1)
    useChatStore.getState().addMessage(msg2)
    expect(useChatStore.getState().messages).toHaveLength(2)
    expect(useChatStore.getState().messages[0].id).toBe('m1')
    expect(useChatStore.getState().messages[1].id).toBe('m2')
  })

  it('updates a message by id', () => {
    useChatStore.getState().setMessages([msg1, msg2])
    useChatStore.getState().updateMessage('m1', { content: { text: 'Updated' } })

    const updated = useChatStore.getState().messages.find((m) => m.id === 'm1')
    expect(updated?.content.text).toBe('Updated')

    // Other messages should be unaffected
    const other = useChatStore.getState().messages.find((m) => m.id === 'm2')
    expect(other?.content.text).toBe('Hi there')
  })

  it('update with non-existent id leaves messages unchanged', () => {
    useChatStore.getState().setMessages([msg1])
    useChatStore.getState().updateMessage('nonexistent', { content: { text: 'X' } })
    expect(useChatStore.getState().messages[0].content.text).toBe('Hello')
  })
})

// ---------------------------------------------------------------------------
// Streaming state (per-conversation)
// ---------------------------------------------------------------------------

describe('chatStore — streaming', () => {
  beforeEach(() => {
    // Set active conversation so legacy actions work
    useChatStore.getState().setActiveConversation('c1')
  })

  it('starts not streaming', () => {
    expect(useChatStore.getState().isStreaming).toBe(false)
    expect(useChatStore.getState().streamingContent).toBe('')
  })

  it('toggles streaming state', () => {
    useChatStore.getState().setStreaming(true)
    expect(useChatStore.getState().isStreaming).toBe(true)
    useChatStore.getState().setStreaming(false)
    expect(useChatStore.getState().isStreaming).toBe(false)
  })

  it('sets streaming content', () => {
    useChatStore.getState().setStreamingContent('partial')
    expect(useChatStore.getState().streamingContent).toBe('partial')
  })

  it('appends streaming content', () => {
    useChatStore.getState().setStreamingContent('Hello')
    useChatStore.getState().appendStreamingContent(' World')
    expect(useChatStore.getState().streamingContent).toBe('Hello World')
  })

  it('handles multiple appends', () => {
    useChatStore.getState().setStreamingContent('')
    useChatStore.getState().appendStreamingContent('A')
    useChatStore.getState().appendStreamingContent('B')
    useChatStore.getState().appendStreamingContent('C')
    expect(useChatStore.getState().streamingContent).toBe('ABC')
  })
})

// ---------------------------------------------------------------------------
// Per-conversation streaming
// ---------------------------------------------------------------------------

describe('chatStore — per-conversation streaming', () => {
  it('tracks streaming state per conversation', () => {
    const store = useChatStore.getState()

    store.setConversationStreaming('c1', true)
    store.appendConversationStreamingContent('c1', 'Hello')
    store.setConversationStreaming('c2', true)
    store.appendConversationStreamingContent('c2', 'World')

    const s = useChatStore.getState()
    expect(s.streamStates['c1']?.isStreaming).toBe(true)
    expect(s.streamStates['c1']?.streamingContent).toBe('Hello')
    expect(s.streamStates['c2']?.isStreaming).toBe(true)
    expect(s.streamStates['c2']?.streamingContent).toBe('World')
  })

  it('clears stream state for one conversation without affecting others', () => {
    const store = useChatStore.getState()

    store.setConversationStreaming('c1', true)
    store.appendConversationStreamingContent('c1', 'A')
    store.setConversationStreaming('c2', true)
    store.appendConversationStreamingContent('c2', 'B')

    store.clearConversationStreamState('c1')

    const s = useChatStore.getState()
    expect(s.streamStates['c1']).toBeUndefined()
    expect(s.streamStates['c2']?.isStreaming).toBe(true)
    expect(s.streamStates['c2']?.streamingContent).toBe('B')
  })

  it('derives legacy isStreaming from active conversation', () => {
    const store = useChatStore.getState()

    store.setConversationStreaming('c1', true)
    store.setConversationStreaming('c2', true)

    // Active = c1
    store.setActiveConversation('c1')
    expect(useChatStore.getState().isStreaming).toBe(true)

    // Active = null
    store.setActiveConversation(null)
    expect(useChatStore.getState().isStreaming).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// Busy conversations
// ---------------------------------------------------------------------------

describe('chatStore — busy conversations', () => {
  it('starts with empty busy set', () => {
    expect(useChatStore.getState().busyConversations.size).toBe(0)
  })

  it('adds and removes busy conversations', () => {
    const store = useChatStore.getState()
    store.addBusyConversation('c1')
    store.addBusyConversation('c2')
    expect(useChatStore.getState().busyConversations.size).toBe(2)

    store.removeBusyConversation('c1')
    expect(useChatStore.getState().busyConversations.size).toBe(1)
    expect(useChatStore.getState().busyConversations.has('c2')).toBe(true)
  })

  it('setBusyConversations replaces entire set', () => {
    const store = useChatStore.getState()
    store.addBusyConversation('old')
    store.setBusyConversations(['c1', 'c2', 'c3'])

    const s = useChatStore.getState()
    expect(s.busyConversations.size).toBe(3)
    expect(s.busyConversations.has('old')).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// Tool executions (legacy + per-conversation)
// ---------------------------------------------------------------------------

describe('chatStore — tool executions', () => {
  beforeEach(() => {
    useChatStore.getState().setActiveConversation('c1')
  })

  it('starts with empty tool executions', () => {
    expect(useChatStore.getState().toolExecutions).toEqual([])
  })

  it('adds tool execution', () => {
    useChatStore.getState().addToolExecution({
      toolName: 'execute_python',
      toolId: 'tool_1',
      status: 'executing',
    })
    expect(useChatStore.getState().toolExecutions).toHaveLength(1)
    expect(useChatStore.getState().toolExecutions[0].status).toBe('executing')
  })

  it('updates tool execution status', () => {
    useChatStore.getState().addToolExecution({
      toolName: 'execute_python',
      toolId: 'tool_1',
      status: 'executing',
    })
    useChatStore.getState().updateToolExecution('tool_1', {
      status: 'completed',
      summary: 'Done',
    })

    const tool = useChatStore.getState().toolExecutions[0]
    expect(tool.status).toBe('completed')
    expect(tool.summary).toBe('Done')
  })

  it('update with non-existent toolId leaves executions unchanged', () => {
    useChatStore.getState().addToolExecution({
      toolName: 'search_web',
      toolId: 'tool_1',
      status: 'executing',
    })
    useChatStore.getState().updateToolExecution('nonexistent', { status: 'error' })
    expect(useChatStore.getState().toolExecutions[0].status).toBe('executing')
  })
})
