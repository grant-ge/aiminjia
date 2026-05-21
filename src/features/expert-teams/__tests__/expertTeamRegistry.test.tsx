import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  setConversationExpertTeam: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import {
  clearExpertTeam,
  getExpertTeam,
  setExpertTeam,
  useExpertTeamForConversation,
} from '@/features/expert-teams/expertTeamRegistry'
import { useChatStore } from '@/stores/chatStore'

function seed(conversationId: string, expertTeamId?: string) {
  useChatStore.getState().setConversations([
    {
      id: conversationId,
      title: 'Team Chat',
      createdAt: '2026-05-20T00:00:00Z',
      updatedAt: '2026-05-20T00:00:00Z',
      isArchived: false,
      expertTeamId,
    },
  ])
}

describe('expertTeamRegistry (conv.json-backed)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useChatStore.getState().setConversations([])
  })

  it('setExpertTeam optimistically patches chatStore before the IPC resolves', async () => {
    seed('conv-1')
    let resolveInvoke: () => void = () => {}
    tauriMock.setConversationExpertTeam.mockImplementationOnce(
      () => new Promise<void>((resolve) => { resolveInvoke = resolve }),
    )

    const pending = setExpertTeam('conv-1', 'marketing')

    expect(useChatStore.getState().conversations[0].expertTeamId).toBe('marketing')

    resolveInvoke()
    await pending
    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledWith('conv-1', 'marketing')
  })

  it('setExpertTeam reverts the chatStore patch and rethrows on IPC failure', async () => {
    seed('conv-1', 'strategy')
    tauriMock.setConversationExpertTeam.mockRejectedValueOnce(new Error('boom'))

    await expect(setExpertTeam('conv-1', 'marketing')).rejects.toThrow('boom')

    expect(useChatStore.getState().conversations[0].expertTeamId).toBe('strategy')
  })

  it('getExpertTeam returns undefined for unknown team ids stored in chatStore', () => {
    seed('conv-1', 'unknown-team-from-future-release')

    expect(getExpertTeam('conv-1')).toBeUndefined()
  })

  it('clearExpertTeam writes null via IPC and clears the field optimistically', async () => {
    seed('conv-1', 'marketing')

    await clearExpertTeam('conv-1')

    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledWith('conv-1', null)
    expect(useChatStore.getState().conversations[0].expertTeamId).toBeUndefined()
  })

  it('useExpertTeamForConversation tracks chatStore changes reactively', () => {
    seed('conv-1', 'marketing')

    const { result, rerender } = renderHook(() =>
      useExpertTeamForConversation('conv-1'),
    )
    expect(result.current).toBe('marketing')

    act(() => {
      const store = useChatStore.getState()
      store.setConversations(
        store.conversations.map((c) => ({ ...c, expertTeamId: 'strategy' })),
      )
    })
    rerender()
    expect(result.current).toBe('strategy')

    act(() => {
      const store = useChatStore.getState()
      store.setConversations(
        store.conversations.map((c) => ({ ...c, expertTeamId: undefined })),
      )
    })
    rerender()
    expect(result.current).toBeUndefined()
  })

  it('useExpertTeamForConversation returns undefined for null/undefined ids', () => {
    seed('conv-1', 'marketing')

    const { result: nullResult } = renderHook(() => useExpertTeamForConversation(null))
    expect(nullResult.current).toBeUndefined()

    const { result: undefResult } = renderHook(() =>
      useExpertTeamForConversation(undefined),
    )
    expect(undefResult.current).toBeUndefined()
  })
})
