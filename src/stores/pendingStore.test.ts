import { describe, it, expect, beforeEach, vi } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  pendingRemoveItem: vi.fn(async () => true),
}))

import { usePendingStore } from './pendingStore'
import { pendingRemoveItem } from '@/lib/tauri'
import type { PendingItem } from '@/types/pending'

const itemA: PendingItem = {
  id: 'a',
  source: 'app',
  text: 'hello',
  senderNick: null,
  attachments: [],
  receivedAt: '2026-05-11T03:21:00Z',
}
const itemB: PendingItem = { ...itemA, id: 'b', text: 'world' }

describe('pendingStore', () => {
  beforeEach(() => {
    usePendingStore.setState({ bySession: {} })
    vi.clearAllMocks()
  })

  it('applySnapshot replaces items per session', () => {
    usePendingStore.getState().applySnapshot('s1', [itemA, itemB])
    expect(usePendingStore.getState().bySession.s1).toHaveLength(2)
    usePendingStore.getState().applySnapshot('s1', [itemA])
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })

  it('applyQueued appends if not present, ignores duplicates', () => {
    usePendingStore.getState().applyQueued('s1', itemA)
    usePendingStore.getState().applyQueued('s1', itemA)
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })

  it('applyDrained clears all items in drainedIds', () => {
    usePendingStore.getState().applySnapshot('s1', [itemA, itemB])
    usePendingStore.getState().applyDrained('s1', ['a'])
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
    expect(usePendingStore.getState().bySession.s1[0].id).toBe('b')
  })

  it('applyRemoved removes single item', () => {
    usePendingStore.getState().applySnapshot('s1', [itemA, itemB])
    usePendingStore.getState().applyRemoved('s1', 'a')
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })

  it('removeItem invokes IPC + waits for event (does not mutate locally)', async () => {
    usePendingStore.getState().applySnapshot('s1', [itemA])
    await usePendingStore.getState().removeItem('s1', 'a')
    expect(pendingRemoveItem).toHaveBeenCalledWith('s1', 'a')
    // Locally still there until event lands
    expect(usePendingStore.getState().bySession.s1).toHaveLength(1)
  })
})
