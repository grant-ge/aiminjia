import { create } from 'zustand'
import type { TeamOverview } from '@/types/team'

/**
 * Per-conversation slice of UI/data state for the team chat drawer.
 *
 * Nothing here is persisted to disk — the source of truth is the on-disk
 * conversation directory, surfaced via `get_team_overview` IPC. This store
 * caches the most recent result and remembers per-conversation interaction
 * state (drawer open/closed, drill-down target).
 */

interface ConversationTeamState {
  /** Latest fetched overview. `null` = not yet fetched. */
  overview: TeamOverview | null
  /** True while the drawer should be visible to the user. */
  drawerOpen: boolean
  /**
   * The user has manually closed the drawer at least once for this
   * conversation. We use this to honor the "first-time auto-open, then
   * respect user" rule: the streaming auto-open trigger fires once on a
   * fresh conversation, and is suppressed thereafter.
   */
  userClosedDrawer: boolean
  /** Currently drilled-into teammate (agentId), or null for the top-level view. */
  drillAgentId: string | null
}

interface TeamStoreState {
  byConversation: Record<string, ConversationTeamState>

  /** Fetch overview snapshot and merge into store. */
  setOverview: (conversationId: string, overview: TeamOverview) => void

  /** Open the drawer (e.g. user clicked the block, or first stream auto-open). */
  openDrawer: (conversationId: string) => void

  /** Close the drawer. When `userInitiated`, suppress future auto-opens. */
  closeDrawer: (conversationId: string, userInitiated: boolean) => void

  /** Drill into a teammate's transcript view. Null to return to the chat view. */
  setDrillAgent: (conversationId: string, agentId: string | null) => void

  /** Hard reset for a single conversation (e.g. on conversation delete). */
  resetConversation: (conversationId: string) => void
}

const EMPTY_STATE: ConversationTeamState = {
  overview: null,
  drawerOpen: false,
  userClosedDrawer: false,
  drillAgentId: null,
}

function patchConversation(
  state: TeamStoreState,
  conversationId: string,
  patch: Partial<ConversationTeamState>,
): TeamStoreState['byConversation'] {
  const prev = state.byConversation[conversationId] ?? EMPTY_STATE
  return {
    ...state.byConversation,
    [conversationId]: { ...prev, ...patch },
  }
}

export const useTeamStore = create<TeamStoreState>((set) => ({
  byConversation: {},
  setOverview: (conversationId, overview) =>
    set((state) => ({
      byConversation: patchConversation(state, conversationId, { overview }),
    })),
  openDrawer: (conversationId) =>
    set((state) => ({
      byConversation: patchConversation(state, conversationId, { drawerOpen: true }),
    })),
  closeDrawer: (conversationId, userInitiated) =>
    set((state) => ({
      byConversation: patchConversation(state, conversationId, {
        drawerOpen: false,
        userClosedDrawer: userInitiated || state.byConversation[conversationId]?.userClosedDrawer === true,
        drillAgentId: null,
      }),
    })),
  setDrillAgent: (conversationId, agentId) =>
    set((state) => ({
      byConversation: patchConversation(state, conversationId, { drillAgentId: agentId }),
    })),
  resetConversation: (conversationId) =>
    set((state) => {
      const next = { ...state.byConversation }
      delete next[conversationId]
      return { byConversation: next }
    }),
}))

/** Convenience selector: read one conversation's slice with sensible defaults. */
export function useConversationTeamState(conversationId: string | null): ConversationTeamState {
  return useTeamStore((s) => {
    if (!conversationId) return EMPTY_STATE
    return s.byConversation[conversationId] ?? EMPTY_STATE
  })
}

/** Convenience: read just the overview (most common pattern). */
export function useTeamOverviewFromStore(conversationId: string | null): TeamOverview | null {
  return useTeamStore((s) => {
    if (!conversationId) return null
    return s.byConversation[conversationId]?.overview ?? null
  })
}
