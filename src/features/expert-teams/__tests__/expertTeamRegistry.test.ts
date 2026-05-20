import { beforeEach, describe, expect, it, vi } from 'vitest'

const mockSetExpertTeam = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
const mockClearSource = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
vi.mock('@/lib/tauri', () => ({
  setConversationExpertTeam: mockSetExpertTeam,
  clearConversationSource: mockClearSource,
}))

// Mock chatStore — use Zustand directly but inject test conversations
import { useChatStore } from '@/stores/chatStore'
import {
  clearExpertTeam,
  getExpertTeam,
  hasExpertTeam,
  setExpertTeam,
} from '../expertTeamRegistry'

const MARKETING = 'marketing'

beforeEach(() => {
  useChatStore.setState((state: any) => ({
    ...state,
    conversations: [
      { id: 'c-1', title: 'team conv', kind: 'expertTeam', expertTeamId: MARKETING, sourceLabel: '市场专家团' },
      { id: 'c-2', title: 'normal' /* no kind */ },
      { id: 'c-3', title: 'employee', kind: 'employee', employeeId: 'emp-001' },
    ],
  }))
  mockSetExpertTeam.mockClear()
  mockClearSource.mockClear()
})

describe('expertTeamRegistry', () => {
  it('setExpertTeam optimistically updates store + calls IPC', async () => {
    await setExpertTeam('c-2', MARKETING as any)
    const conv = useChatStore.getState().conversations.find((c) => c.id === 'c-2')
    expect(conv?.kind).toBe('expertTeam')
    expect(conv?.expertTeamId).toBe(MARKETING)
    expect(conv?.sourceLabel).toBeTruthy()
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

  it('getExpertTeam returns id for expert team conv', () => {
    expect(getExpertTeam('c-1')).toBe(MARKETING)
  })

  it('getExpertTeam returns undefined for non-team conv', () => {
    expect(getExpertTeam('c-2')).toBeUndefined()
  })

  it('clearExpertTeam optimistically resets store + calls IPC', async () => {
    await clearExpertTeam('c-1')
    const conv = useChatStore.getState().conversations.find((c) => c.id === 'c-1')
    expect(conv?.kind).toBe('user')
    expect(conv?.expertTeamId).toBeUndefined()
    expect(mockClearSource).toHaveBeenCalledWith('c-1')
  })
})
