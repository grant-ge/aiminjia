import { beforeEach, describe, expect, it, vi } from 'vitest'

const mockSetExpertTeam = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
const mockClearSource = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
const mockGetSource = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri', () => ({
  setConversationExpertTeam: mockSetExpertTeam,
  clearConversationSource: mockClearSource,
  getConversationSource: mockGetSource,
}))

// Mock chatStore — use Zustand directly but inject test conversations
import { useChatStore } from '@/stores/chatStore'
import {
  __resetExpertTeamRegistryCacheForTesting,
  clearExpertTeam,
  ensureExpertTeam,
  getExpertTeam,
  hasExpertTeam,
  setExpertTeam,
} from '../expertTeamRegistry'

const MARKETING = 'marketing'

beforeEach(() => {
  useChatStore.setState((state: any) => ({
    ...state,
    conversations: [
      { id: 'c-1', title: 'team conv', kind: 'expertTeam', sourceLabel: '市场专家团' },
      { id: 'c-2', title: 'normal' /* no kind */ },
      { id: 'c-3', title: 'employee', kind: 'employee' },
    ],
  }))
  mockSetExpertTeam.mockClear()
  mockClearSource.mockClear()
  mockGetSource.mockReset()
  __resetExpertTeamRegistryCacheForTesting()
})

describe('expertTeamRegistry', () => {
  it('setExpertTeam optimistically updates store + seeds cache + calls IPC', async () => {
    await setExpertTeam('c-2', MARKETING as any)
    const conv = useChatStore.getState().conversations.find((c) => c.id === 'c-2')
    expect(conv?.kind).toBe('expertTeam')
    expect(conv?.sourceLabel).toBeTruthy()
    expect(getExpertTeam('c-2')).toBe(MARKETING)
    expect(mockSetExpertTeam).toHaveBeenCalledWith('c-2', MARKETING, expect.any(String))
  })

  it('hasExpertTeam returns true for kind=expertTeam', () => {
    expect(hasExpertTeam('c-1')).toBe(true)
  })

  it('hasExpertTeam returns false for kind=user (missing kind)', () => {
    expect(hasExpertTeam('c-2')).toBe(false)
  })

  it('hasExpertTeam returns false for kind=employee', () => {
    expect(hasExpertTeam('c-3')).toBe(false)
  })

  it('hasExpertTeam returns false for unknown convId', () => {
    expect(hasExpertTeam('nope')).toBe(false)
  })

  it('getExpertTeam returns undefined until cache is seeded', () => {
    // c-1 has kind=expertTeam in store but no id in cache yet
    expect(getExpertTeam('c-1')).toBeUndefined()
  })

  it('ensureExpertTeam fetches from conv.json + caches', async () => {
    mockGetSource.mockResolvedValueOnce({ kind: 'expertTeam', expertTeamId: MARKETING })
    const id = await ensureExpertTeam('c-1')
    expect(id).toBe(MARKETING)
    expect(mockGetSource).toHaveBeenCalledWith('c-1')
    // Second call uses cache
    const id2 = await ensureExpertTeam('c-1')
    expect(id2).toBe(MARKETING)
    expect(mockGetSource).toHaveBeenCalledTimes(1)
    // Sync getter now hits
    expect(getExpertTeam('c-1')).toBe(MARKETING)
  })

  it('ensureExpertTeam returns undefined for non-expert-team conv', async () => {
    mockGetSource.mockResolvedValueOnce({ kind: 'user' })
    expect(await ensureExpertTeam('c-2')).toBeUndefined()
  })

  it('clearExpertTeam resets store + cache + calls IPC', async () => {
    // Seed cache first so we can prove it gets cleared
    await setExpertTeam('c-2', MARKETING as any)
    expect(getExpertTeam('c-2')).toBe(MARKETING)

    await clearExpertTeam('c-2')
    const conv = useChatStore.getState().conversations.find((c) => c.id === 'c-2')
    expect(conv?.kind).toBe('user')
    expect(getExpertTeam('c-2')).toBeUndefined()
    expect(mockClearSource).toHaveBeenCalledWith('c-2')
  })
})
