import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  cloudLogin: vi.fn(),
  cloudLogout: vi.fn(),
  getCloudAuth: vi.fn(),
  getCloudModels: vi.fn(),
  getLastBrand: vi.fn().mockResolvedValue(null),
  saveLastBrand: vi.fn().mockResolvedValue(undefined),
}))

import { getCloudAuth, getCloudModels } from '@/lib/tauri'
import { useAuthStore } from './authStore'
import { useGeneratedFilePreviewStore } from './generatedFilePreviewStore'
import { useInteractionStore } from './interactionStore'
import { usePendingStore } from './pendingStore'
import { useTeamStore } from './teamStore'

const tauriMock = vi.mocked({ getCloudAuth, getCloudModels })

describe('authStore user-scoped reset', () => {
  beforeEach(() => {
    useAuthStore.setState({
      isLoggedIn: true,
      user: { id: 1, name: 'User A', username: 'user-a' },
      tenant: { id: 1, name: 'Tenant A', balance: '0' },
      cloudModels: [],
      selectedCloudModel: null,
      redirectFrom: null,
      isAuthPending: false,
    })
    usePendingStore.setState({ bySession: {} })
    useInteractionStore.setState({ pendingInteractions: [] })
    useTeamStore.setState({ byConversation: {} })
    useGeneratedFilePreviewStore.setState({ target: null })
  })

  it('clears transient user-scoped UI state when auth expires', () => {
    usePendingStore.getState().applySnapshot('conv-a', [{
      id: 'pending-a',
      source: 'app',
      text: 'queued text',
      senderNick: null,
      attachments: [],
      receivedAt: '2026-05-11T03:21:00Z',
    }])
    useInteractionStore.getState().addInteraction({
      conversationId: 'conv-a',
      runId: 'run-a',
      interactionId: 'interaction-a',
      toolCallId: 'tool-a',
      toolName: 'AskUserQuestion',
      kind: 'askUserQuestion',
      payload: { questions: [] },
    })
    useTeamStore.getState().setOverview('conv-a', { teams: [] } as never)
    useGeneratedFilePreviewStore.getState().openPreview({
      fileId: 'file-a',
      conversationId: 'conv-a',
      fileName: 'a.md',
      fileType: 'markdown',
    })

    useAuthStore.getState().clearAndRedirect({ kind: 'chat', conversationId: 'conv-a' })

    expect(usePendingStore.getState().bySession).toEqual({})
    expect(useInteractionStore.getState().pendingInteractions).toEqual([])
    expect(useTeamStore.getState().byConversation).toEqual({})
    expect(useGeneratedFilePreviewStore.getState().target).toBeNull()
  })

  it('does not preserve chat routes across auth expiration', () => {
    useAuthStore.getState().clearAndRedirect({ kind: 'chat', conversationId: 'conv-a' })

    expect(useAuthStore.getState().redirectFrom).toBeNull()
  })

  it('preserves non-user-content routes across auth expiration', () => {
    useAuthStore.getState().clearAndRedirect({ kind: 'skill-center' })

    expect(useAuthStore.getState().redirectFrom).toEqual({ kind: 'skill-center' })
  })

  it('does not restore stale auth when model refresh reports login expired', async () => {
    tauriMock.getCloudAuth.mockResolvedValueOnce({
      loggedIn: true,
      user: { id: 43, name: '孙宏伟', username: 'sunhongwei' },
      tenant: { id: 17, name: 'Tenant 17', balance: '0', productName: 'AI小家体验中心' },
      models: [],
    })
    tauriMock.getCloudModels.mockRejectedValueOnce(new Error('登录已过期，请重新登录'))

    await useAuthStore.getState().restoreFromStorage()

    expect(useAuthStore.getState().isLoggedIn).toBe(false)
    expect(useAuthStore.getState().user).toBeNull()
    expect(useAuthStore.getState().tenant).toBeNull()
  })
})
