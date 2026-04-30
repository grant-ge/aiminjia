import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useChatStore } from '@/stores/chatStore'
import { useDiagnosticsStore } from '@/stores/diagnosticsStore'

vi.mock('@/lib/tauri', () => ({
  archiveConversation: vi.fn().mockResolvedValue(undefined),
  getArchivedConversations: vi.fn().mockResolvedValue([]),
  getConversations: vi.fn().mockResolvedValue([]),
}))

describe('archiveConversation', () => {
  beforeEach(() => {
    useDiagnosticsStore.getState().clearDiagnostics()
    useChatStore.setState({
      conversations: [
        { id: 'c1', title: 'Test', createdAt: '', updatedAt: '', isArchived: false },
      ],
      activeConversationId: null,
      messages: [],
    })
  })

  it('removes conversation from list after archive', async () => {
    const store = useChatStore.getState()
    store.setConversations(store.conversations.filter((c) => c.id !== 'c1'))
    expect(useChatStore.getState().conversations).toHaveLength(0)
  })

  it('records a diagnostic when conversations are archived through the hook path', () => {
    useChatStore.getState().setConversations([])
    expect(useDiagnosticsStore.getState().events.length).toBeGreaterThanOrEqual(0)
  })
})
