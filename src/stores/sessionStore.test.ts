import { beforeEach, describe, expect, it } from 'vitest'

import { useChatStore } from './chatStore'
import { useSessionStore } from './sessionStore'
import type { Conversation, Message } from '@/types/message'

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

describe('sessionStore view', () => {
  beforeEach(() => {
    resetChatStore()
  })

  it('updates conversations through the same underlying chat store', () => {
    const conversations: Conversation[] = [
      { id: 'c1', title: 'Test', createdAt: '2025-01-01', updatedAt: '2025-01-01', isArchived: false },
    ]

    useSessionStore.getState().setConversations(conversations)

    expect(useChatStore.getState().conversations).toEqual(conversations)
  })

  it('applies message CRUD through the same underlying chat store', () => {
    const message: Message = {
      id: 'm1',
      conversationId: 'c1',
      role: 'user',
      content: { text: 'hello' },
      createdAt: '2025-01-01T00:00:00Z',
    }

    useSessionStore.getState().addMessage(message)
    useSessionStore.getState().updateMessage('m1', { content: { text: 'updated' } })

    expect(useChatStore.getState().messages).toHaveLength(1)
    expect(useChatStore.getState().messages[0].content.text).toBe('updated')
  })

  it('recomputes legacy streaming fields when switching active conversation', () => {
    const store = useChatStore.getState()
    store.setConversationStreaming('c1', true)
    store.appendConversationStreamingContent('c1', 'hello')

    useSessionStore.getState().setActiveConversation('c1')

    expect(useChatStore.getState().isStreaming).toBe(true)
    expect(useChatStore.getState().streamingContent).toBe('hello')
  })
})
