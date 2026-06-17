import { create } from 'zustand'

export type SidebarCachedStatusKind = 'permission-review' | 'waiting-reply'

export const SIDEBAR_STATUS_SESSION_KEY =
  'aijia.sidebarConversationStatuses.v1'

export interface SidebarCachedStatus {
  kind: SidebarCachedStatusKind
  updatedAt: number
  runId?: string
  toolCallId?: string
  interactionId?: string
}

interface SidebarStatusState {
  statuses: Record<string, SidebarCachedStatus>
  hydrateFromSession: () => void
  setStatus: (
    conversationId: string,
    status: Omit<SidebarCachedStatus, 'updatedAt'> & { updatedAt?: number },
  ) => Promise<void>
  clearStatus: (conversationId: string) => Promise<void>
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

function readSessionStatuses(): Record<string, SidebarCachedStatus> {
  if (typeof window === 'undefined') return {}
  try {
    return parseStatuses(
      window.sessionStorage.getItem(SIDEBAR_STATUS_SESSION_KEY) ?? undefined,
    )
  } catch {
    return {}
  }
}

function persistStatuses(statuses: Record<string, SidebarCachedStatus>) {
  if (typeof window === 'undefined') return
  try {
    window.sessionStorage.setItem(
      SIDEBAR_STATUS_SESSION_KEY,
      JSON.stringify(statuses),
    )
  } catch {
    // Session cache is only a UI hint; losing it must not block the chat flow.
  }
}

function clearSessionStatuses() {
  if (typeof window === 'undefined') return
  try {
    window.sessionStorage.removeItem(SIDEBAR_STATUS_SESSION_KEY)
  } catch {
    // Session cache is only a UI hint; losing it must not block the chat flow.
  }
}

export const useSidebarStatusStore = create<SidebarStatusState>((set, get) => ({
  statuses: readSessionStatuses(),

  hydrateFromSession: () => set({ statuses: readSessionStatuses() }),

  setStatus: async (conversationId, status) => {
    const next = {
      ...get().statuses,
      [conversationId]: {
        ...status,
        updatedAt: status.updatedAt ?? Date.now(),
      },
    }
    set({ statuses: next })
    persistStatuses(next)
  },

  clearStatus: async (conversationId) => {
    const current = get().statuses
    if (!current[conversationId]) return
    const next = { ...current }
    delete next[conversationId]
    set({ statuses: next })
    persistStatuses(next)
  },

  reset: () => {
    set({ statuses: {} })
    clearSessionStatuses()
  },
}))
