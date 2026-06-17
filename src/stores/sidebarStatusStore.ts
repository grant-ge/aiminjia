import { create } from 'zustand'

import {
  getSettings,
  pendingInteractionSnapshotForSession,
  pendingPermissionSnapshotForSession,
  updateSettings,
} from '@/lib/tauri'
import type { Settings } from '@/types/settings'

export type SidebarCachedStatusKind = 'permission-review' | 'waiting-reply'

export interface SidebarCachedStatus {
  kind: SidebarCachedStatusKind
  updatedAt: number
  runId?: string
  toolCallId?: string
  interactionId?: string
}

interface SidebarStatusState {
  statuses: Record<string, SidebarCachedStatus>
  hydrateFromSettings: (settings: Pick<Settings, 'uiSidebarConversationStatuses'>) => void
  setStatus: (
    conversationId: string,
    status: Omit<SidebarCachedStatus, 'updatedAt'> & { updatedAt?: number },
  ) => Promise<void>
  clearStatus: (conversationId: string) => Promise<void>
  reconcileWithRuntimeSnapshots: () => Promise<void>
  reset: () => void
}

function parseStatuses(raw?: string): Record<string, SidebarCachedStatus> {
  if (!raw) return {}
  try {
    const parsed = JSON.parse(raw) as Record<string, Partial<SidebarCachedStatus>>
    const next: Record<string, SidebarCachedStatus> = {}
    Object.entries(parsed).forEach(([conversationId, status]) => {
      if (
        status?.kind !== 'permission-review' &&
        status?.kind !== 'waiting-reply'
      ) {
        return
      }
      next[conversationId] = {
        kind: status.kind,
        updatedAt:
          typeof status.updatedAt === 'number' ? status.updatedAt : Date.now(),
        runId: typeof status.runId === 'string' ? status.runId : undefined,
        toolCallId:
          typeof status.toolCallId === 'string' ? status.toolCallId : undefined,
        interactionId:
          typeof status.interactionId === 'string'
            ? status.interactionId
            : undefined,
      }
    })
    return next
  } catch {
    return {}
  }
}

async function persistStatuses(statuses: Record<string, SidebarCachedStatus>) {
  const settings = await getSettings()
  await updateSettings({
    ...settings,
    uiSidebarConversationStatuses: JSON.stringify(statuses),
  })
}

async function hasMatchingRuntimeStatus(
  conversationId: string,
  status: SidebarCachedStatus,
): Promise<boolean> {
  try {
    if (status.kind === 'permission-review') {
      const asks = await pendingPermissionSnapshotForSession(conversationId)
      if (!status.toolCallId) return asks.length > 0
      return asks.some((ask) => ask.toolCallId === status.toolCallId)
    }
    const interactions = await pendingInteractionSnapshotForSession(conversationId)
    if (!status.interactionId) return interactions.length > 0
    return interactions.some(
      (interaction) => interaction.interactionId === status.interactionId,
    )
  } catch (err) {
    console.warn('[sidebarStatus] runtime snapshot reconcile failed', err)
    return true
  }
}

export const useSidebarStatusStore = create<SidebarStatusState>((set, get) => ({
  statuses: {},

  hydrateFromSettings: (settings) =>
    set({ statuses: parseStatuses(settings.uiSidebarConversationStatuses) }),

  setStatus: async (conversationId, status) => {
    const next = {
      ...get().statuses,
      [conversationId]: {
        ...status,
        updatedAt: status.updatedAt ?? Date.now(),
      },
    }
    set({ statuses: next })
    await persistStatuses(next)
  },

  clearStatus: async (conversationId) => {
    const current = get().statuses
    if (!current[conversationId]) return
    const next = { ...current }
    delete next[conversationId]
    set({ statuses: next })
    await persistStatuses(next)
  },

  reconcileWithRuntimeSnapshots: async () => {
    const current = get().statuses
    const entries = Object.entries(current)
    if (entries.length === 0) return

    const keepEntries = await Promise.all(
      entries.map(async ([conversationId, status]) => [
        conversationId,
        status,
        await hasMatchingRuntimeStatus(conversationId, status),
      ] as const),
    )
    const next: Record<string, SidebarCachedStatus> = {}
    keepEntries.forEach(([conversationId, status, keep]) => {
      if (keep) next[conversationId] = status
    })
    if (Object.keys(next).length === Object.keys(current).length) return

    set({ statuses: next })
    await persistStatuses(next)
  },

  reset: () => set({ statuses: {} }),
}))
