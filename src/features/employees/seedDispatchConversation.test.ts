import { describe, expect, it } from 'vitest'
import type { Conversation } from '@/types/message'
import { seedDispatchConversation } from './seedDispatchConversation'

function conv(id: string): Conversation {
  return {
    id,
    title: id,
    createdAt: '2026-05-15T00:00:00Z',
    updatedAt: '2026-05-15T00:00:00Z',
    isArchived: false,
  }
}

describe('seedDispatchConversation', () => {
  it('prepends a placeholder when convId is new', () => {
    const existing = [conv('a'), conv('b')]
    const result = seedDispatchConversation(existing, 'new-1', '小工', '2026-05-15T14:30:00Z')
    expect(result).toHaveLength(3)
    expect(result[0]).toMatchObject({
      id: 'new-1',
      title: '派活: 小工',
      isArchived: false,
      createdAt: '2026-05-15T14:30:00Z',
    })
    expect(result[1].id).toBe('a')
  })

  it('returns the input array unchanged when convId already exists', () => {
    const existing = [conv('a'), conv('new-1'), conv('b')]
    const result = seedDispatchConversation(existing, 'new-1', '小工')
    expect(result).toBe(existing)
  })

  it('handles empty list', () => {
    const result = seedDispatchConversation([], 'new-1', '小销', '2026-05-15T14:30:00Z')
    expect(result).toHaveLength(1)
    expect(result[0].id).toBe('new-1')
    expect(result[0].title).toBe('派活: 小销')
  })
})
