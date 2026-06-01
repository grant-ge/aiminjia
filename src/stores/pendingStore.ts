import { create } from 'zustand'

import { pendingRemoveItem } from '@/lib/tauri'
import type { PendingItem } from '@/types/pending'

export interface PendingState {
  bySession: Record<string, PendingItem[]>
  applySnapshot: (sessionId: string, items: PendingItem[]) => void
  applyQueued: (sessionId: string, item: PendingItem) => void
  applyDrained: (sessionId: string, drainedIds: string[]) => void
  applyRemoved: (sessionId: string, itemId: string) => void
  removeItem: (sessionId: string, itemId: string) => Promise<void>
  reset: () => void
}

export const usePendingStore = create<PendingState>((set) => ({
  bySession: {},

  applySnapshot: (sessionId, items) =>
    set((state) => ({
      bySession: { ...state.bySession, [sessionId]: items },
    })),

  applyQueued: (sessionId, item) =>
    set((state) => {
      const list = state.bySession[sessionId] ?? []
      if (list.some((i) => i.id === item.id)) {
        return state
      }
      return {
        bySession: { ...state.bySession, [sessionId]: [...list, item] },
      }
    }),

  applyDrained: (sessionId, drainedIds) =>
    set((state) => {
      const list = state.bySession[sessionId] ?? []
      const drainedSet = new Set(drainedIds)
      return {
        bySession: {
          ...state.bySession,
          [sessionId]: list.filter((i) => !drainedSet.has(i.id)),
        },
      }
    }),

  applyRemoved: (sessionId, itemId) =>
    set((state) => {
      const list = state.bySession[sessionId] ?? []
      return {
        bySession: {
          ...state.bySession,
          [sessionId]: list.filter((i) => i.id !== itemId),
        },
      }
    }),

  removeItem: async (sessionId, itemId) => {
    // Single source of truth: backend emits pending:removed; applyRemoved fires from event.
    await pendingRemoveItem(sessionId, itemId)
  },

  reset: () => set({ bySession: {} }),
}))
