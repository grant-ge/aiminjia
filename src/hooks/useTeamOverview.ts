/**
 * Loads a conversation's team overview and keeps it in sync with the live
 * stream of team events. Backed by `useTeamStore` so multiple consumers
 * (block in main chat + drawer) read the same data.
 *
 * Strategy: fetch once on mount / conversationId change; debounce-refetch
 * whenever a streaming event lands that could plausibly mutate the team
 * (message:updated for peer-messages, tool:completed for SendMessage /
 * TeamCreate / TeamDelete / Agent spawn / TeammateStop). The events are
 * cheap to detect and the underlying IPC is just a JSONL scan — even on
 * a chatty team session this is negligibly small.
 *
 * Drilling-down (per-teammate transcript) is loaded on demand by
 * `useTeammateTranscript` since it's only needed when the user expands
 * a teammate panel.
 */

import { useEffect, useMemo, useRef, useState } from 'react'

import { getTeamOverview, getTeammateTranscript, TAURI_EVENTS } from '@/lib/tauri'
import { onMessageUpdated, onToolCompleted } from '@/lib/tauri'
import { listen } from '@tauri-apps/api/event'
import type { TeamOverview } from '@/types/team'

import { useTauriEvent } from './useTauriEvent'
import { useConversationTeamState, useTeamStore } from '@/stores/teamStore'

const REFETCH_DEBOUNCE_MS = 300

/**
 * Tool names that could change team state and warrant a refetch when their
 * `tool:completed` event arrives.
 */
const TEAM_MUTATING_TOOLS = new Set([
  'SendMessage',
  'TeamCreate',
  'TeamDelete',
  'Agent',
  'TeammateStop',
])

interface UseTeamOverviewResult {
  overview: TeamOverview | null
  /** True after the first fetch attempt completes (regardless of result). */
  loaded: boolean
  /** Force a refetch (e.g. user clicked refresh). */
  refetch: () => Promise<void>
}

export function useTeamOverview(conversationId: string | null): UseTeamOverviewResult {
  const overview = useConversationTeamState(conversationId).overview
  const setOverview = useTeamStore((s) => s.setOverview)
  const loadedRef = useRef(false)
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Fetch immediately when conversationId changes.
  useEffect(() => {
    if (!conversationId) return
    let cancelled = false
    loadedRef.current = false
    void getTeamOverview(conversationId)
      .then((data) => {
        if (cancelled) return
        setOverview(conversationId, data)
      })
      .catch((err) => {
        console.warn('[useTeamOverview] getTeamOverview failed:', err)
      })
      .finally(() => {
        if (!cancelled) loadedRef.current = true
      })
    return () => {
      cancelled = true
    }
  }, [conversationId, setOverview])

  // Debounced refetch helper — called from event handlers.
  const scheduleRefetch = useMemo(() => {
    return (id: string) => {
      if (debounceTimerRef.current != null) clearTimeout(debounceTimerRef.current)
      debounceTimerRef.current = setTimeout(() => {
        debounceTimerRef.current = null
        void getTeamOverview(id)
          .then((data) => setOverview(id, data))
          .catch((err) => {
            console.warn('[useTeamOverview] refetch failed:', err)
          })
      }, REFETCH_DEBOUNCE_MS)
    }
  }, [setOverview])

  // Refetch on peer-messages user XML.
  useTauriEvent(() =>
    onMessageUpdated((message) => {
      if (!conversationId) return
      if (message.conversationId !== conversationId) return
      if (message.role !== 'user') return
      const text = message.content?.text ?? ''
      if (!text.startsWith('<peer-messages>')) return
      scheduleRefetch(conversationId)
    }),
  )

  // Refetch on team-mutating tool completions.
  useTauriEvent(() =>
    onToolCompleted((message) => {
      if (!conversationId) return
      if (message.conversationId !== conversationId) return
      const toolName = message.toolResult?.name
      if (!toolName) return
      if (!TEAM_MUTATING_TOOLS.has(toolName)) return
      scheduleRefetch(conversationId)
    }),
  )

  // PR10: also subscribe to PR9's narrower team:* / team-chat:* events so the
  // overview refreshes the moment the backend writes (not just when the tool
  // round completes).  Wrapped in `useTauriEvent` so test environments without
  // a real Tauri runtime swallow the registration error gracefully (matches
  // how the peer-messages / tool-completed listeners above are wired).
  useTauriEvent(() =>
    listen<{ conversationId?: string }>(TAURI_EVENTS.TEAM_CREATED, (e) => {
      if (conversationId && e.payload?.conversationId === conversationId) {
        scheduleRefetch(conversationId)
      }
    }),
  )
  useTauriEvent(() =>
    listen<{ conversationId?: string }>(TAURI_EVENTS.TEAM_DELETED, (e) => {
      if (conversationId && e.payload?.conversationId === conversationId) {
        scheduleRefetch(conversationId)
      }
    }),
  )
  useTauriEvent(() =>
    listen<{ conversationId?: string }>(TAURI_EVENTS.TEAM_ACTIVE_CHANGED, (e) => {
      if (conversationId && e.payload?.conversationId === conversationId) {
        scheduleRefetch(conversationId)
      }
    }),
  )
  useTauriEvent(() =>
    listen<{ conversationId?: string }>(TAURI_EVENTS.TEAM_CHAT_APPENDED, (e) => {
      if (conversationId && e.payload?.conversationId === conversationId) {
        scheduleRefetch(conversationId)
      }
    }),
  )

  // Cleanup on unmount.
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current != null) clearTimeout(debounceTimerRef.current)
    }
  }, [])

  const refetch = async () => {
    if (!conversationId) return
    try {
      const data = await getTeamOverview(conversationId)
      setOverview(conversationId, data)
    } catch (err) {
      console.warn('[useTeamOverview] manual refetch failed:', err)
    }
  }

  return {
    overview,
    loaded: loadedRef.current,
    refetch,
  }
}

/**
 * Lazy loader for one teammate's full transcript. Returns `null` while
 * loading or if not requested yet.
 *
 * Caches per (conversationId, agentId) in component-local state because
 * transcripts are large and only needed when the user explicitly drills in.
 */
export function useTeammateTranscript(
  conversationId: string | null,
  agentId: string | null,
): { entries: unknown[] | null; loading: boolean } {
  const [entries, setEntries] = useState<unknown[] | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!conversationId || !agentId) {
      setEntries(null)
      setLoading(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setEntries(null)
    void getTeammateTranscript(conversationId, agentId)
      .then((data) => {
        if (!cancelled) setEntries(data)
      })
      .catch((err) => {
        console.warn('[useTeammateTranscript] failed:', err)
        if (!cancelled) setEntries([])
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [conversationId, agentId])

  return { entries, loading }
}
