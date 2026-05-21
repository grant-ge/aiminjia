import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  getConversations: vi.fn(),
  setConversationExpertTeam: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { migrateExpertTeamRegistryOnce } from '@/features/expert-teams/migrateExpertTeamRegistry'
import { useChatStore } from '@/stores/chatStore'

const LEGACY_KEY = 'aijia-expert-team-registry'
const MARKER_KEY = 'aijia-expert-team-migration-v1'

describe('migrateExpertTeamRegistryOnce', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    tauriMock.getConversations.mockResolvedValue([])
    tauriMock.setConversationExpertTeam.mockResolvedValue(undefined)
    useChatStore.getState().setConversations([])
  })

  it('happy path: writes each entry via IPC, clears legacy key, sets marker, patches chatStore', async () => {
    localStorage.setItem(
      LEGACY_KEY,
      JSON.stringify({ 'conv-a': 'marketing', 'conv-b': 'strategy' }),
    )
    tauriMock.getConversations.mockResolvedValue([
      { id: 'conv-a', title: 'A' },
      { id: 'conv-b', title: 'B' },
    ])
    useChatStore.getState().setConversations([
      { id: 'conv-a', title: 'A', createdAt: '', updatedAt: '', isArchived: false },
      { id: 'conv-b', title: 'B', createdAt: '', updatedAt: '', isArchived: false },
    ])

    await migrateExpertTeamRegistryOnce()

    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledTimes(2)
    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledWith('conv-a', 'marketing')
    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledWith('conv-b', 'strategy')
    expect(localStorage.getItem(LEGACY_KEY)).toBeNull()
    expect(localStorage.getItem(MARKER_KEY)).toBe('done')

    const convs = useChatStore.getState().conversations
    expect(convs.find((c) => c.id === 'conv-a')?.expertTeamId).toBe('marketing')
    expect(convs.find((c) => c.id === 'conv-b')?.expertTeamId).toBe('strategy')
  })

  it('marker already set: skips IPC, cleans legacy key residue from rollback', async () => {
    localStorage.setItem(MARKER_KEY, 'done')
    localStorage.setItem(LEGACY_KEY, JSON.stringify({ 'conv-a': 'marketing' }))

    await migrateExpertTeamRegistryOnce()

    expect(tauriMock.setConversationExpertTeam).not.toHaveBeenCalled()
    expect(tauriMock.getConversations).not.toHaveBeenCalled()
    expect(localStorage.getItem(LEGACY_KEY)).toBeNull() // residue cleared
    expect(localStorage.getItem(MARKER_KEY)).toBe('done')
  })

  it('no legacy key: writes marker without invoking IPC', async () => {
    await migrateExpertTeamRegistryOnce()

    expect(tauriMock.setConversationExpertTeam).not.toHaveBeenCalled()
    expect(tauriMock.getConversations).not.toHaveBeenCalled()
    expect(localStorage.getItem(MARKER_KEY)).toBe('done')
  })

  it('skips entries whose conv was deleted between sessions', async () => {
    localStorage.setItem(
      LEGACY_KEY,
      JSON.stringify({ 'conv-alive': 'marketing', 'conv-deleted': 'strategy' }),
    )
    tauriMock.getConversations.mockResolvedValue([{ id: 'conv-alive', title: 'Alive' }])

    await migrateExpertTeamRegistryOnce()

    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledTimes(1)
    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledWith('conv-alive', 'marketing')
    // skip is not failure → marker writes, legacy key clears
    expect(localStorage.getItem(LEGACY_KEY)).toBeNull()
    expect(localStorage.getItem(MARKER_KEY)).toBe('done')
  })

  it('skips entries with unknown team ids (release removed the team)', async () => {
    localStorage.setItem(
      LEGACY_KEY,
      JSON.stringify({ 'conv-a': 'marketing', 'conv-b': 'team-deleted-in-future' }),
    )
    tauriMock.getConversations.mockResolvedValue([
      { id: 'conv-a', title: 'A' },
      { id: 'conv-b', title: 'B' },
    ])

    await migrateExpertTeamRegistryOnce()

    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledTimes(1)
    expect(tauriMock.setConversationExpertTeam).toHaveBeenCalledWith('conv-a', 'marketing')
    expect(localStorage.getItem(MARKER_KEY)).toBe('done')
  })

  it('IPC failure on one entry: legacy key + marker untouched (retries next launch)', async () => {
    localStorage.setItem(
      LEGACY_KEY,
      JSON.stringify({ 'conv-a': 'marketing', 'conv-b': 'strategy' }),
    )
    tauriMock.getConversations.mockResolvedValue([
      { id: 'conv-a', title: 'A' },
      { id: 'conv-b', title: 'B' },
    ])
    tauriMock.setConversationExpertTeam.mockResolvedValueOnce(undefined) // conv-a ok
    tauriMock.setConversationExpertTeam.mockRejectedValueOnce(new Error('flaky')) // conv-b fails

    await migrateExpertTeamRegistryOnce()

    expect(localStorage.getItem(LEGACY_KEY)).not.toBeNull()
    expect(localStorage.getItem(MARKER_KEY)).toBeNull()
  })
})
