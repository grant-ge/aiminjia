// code/src/features/expert-teams/expertTeamRegistry.ts
//
// 会话 ↔ 专家团映射。
//
// 数据来源：
//   - `kind === 'expertTeam'` 由 index.json mirror，进 chatStore.conversations，
//     侧边栏分类靠它（不读 id，spec §1.3：index 不存 ID）
//   - `expertTeamId` 由 `conv.json::source.expertTeamId` 持有，是权威来源；
//     需要 id 的调用方（如 ChatPage / MessageList 渲染欢迎页、ChatBottomArea
//     拼 director prompt）通过 `getConversationSource` IPC 懒读
//
// 缓存：模块级 Map<convId, ExpertTeamId | null>，避免重复 IPC。
//   - `setExpertTeam` 写入时同步种入 cache
//   - `clearExpertTeam` 写入 null
//   - `useExpertTeamForConversation` hook 在 kind === 'expertTeam' 时按需 fetch

import { useEffect, useState } from 'react'

import {
  clearConversationSource,
  getConversationSource,
  setConversationExpertTeam,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { EXPERT_TEAMS, type ExpertTeamId } from './teams'
import { getCachedExpertTeam } from './useExpertTeamCatalog'

// convId → ExpertTeamId | null (null = 已查过，不是专家团；undefined = 还没查)
const cache = new Map<string, ExpertTeamId | null>()
const inflight = new Map<string, Promise<ExpertTeamId | null>>()
type Subscriber = (teamId: ExpertTeamId | null) => void
const subscribers = new Map<string, Set<Subscriber>>()

function labelFor(teamId: ExpertTeamId): string {
  return getCachedExpertTeam(teamId)?.name ?? EXPERT_TEAMS.find((t) => t.id === teamId)?.name ?? teamId
}

function notify(convId: string, teamId: ExpertTeamId | null) {
  const subs = subscribers.get(convId)
  if (subs) for (const fn of subs) fn(teamId)
}

async function fetchTeamId(conversationId: string): Promise<ExpertTeamId | null> {
  if (cache.has(conversationId)) return cache.get(conversationId)!
  const existing = inflight.get(conversationId)
  if (existing) return existing
  const promise = (async () => {
    try {
      const src = await getConversationSource(conversationId)
      const id = src.kind === 'expertTeam' ? (src.expertTeamId as ExpertTeamId) : null
      const resolved = id && id.trim() ? id : null
      cache.set(conversationId, resolved)
      notify(conversationId, resolved)
      return resolved
    } catch (err) {
      console.warn('[expertTeamRegistry] getConversationSource failed:', err)
      return null
    } finally {
      inflight.delete(conversationId)
    }
  })()
  inflight.set(conversationId, promise)
  return promise
}

/**
 * Set the conversation's expert team. Writes to backend conv.json + index.json
 * (kind mirror), seeds the local id cache, and optimistically flips the
 * conversation's `kind` in chatStore so sidebar grouping responds immediately.
 */
export async function setExpertTeam(
  conversationId: string,
  teamId: ExpertTeamId,
  sourceLabel?: string,
): Promise<void> {
  if (!teamId.trim()) return
  const label = sourceLabel ?? labelFor(teamId)
  // Seed cache so synchronous readers (getExpertTeam) see the id immediately.
  cache.set(conversationId, teamId)
  notify(conversationId, teamId)
  // Optimistic update — flip kind/sourceLabel for sidebar.
  useChatStore.setState((state) => ({
    conversations: state.conversations.map((c) =>
      c.id === conversationId
        ? { ...c, kind: 'expertTeam' as const, sourceLabel: label }
        : c,
    ),
  }))
  try {
    await setConversationExpertTeam(conversationId, teamId, label)
  } catch (err) {
    console.warn('[expertTeamRegistry] setExpertTeam IPC failed:', err)
  }
}

/**
 * Clear the conversation's expert team affiliation (set kind back to user).
 */
export async function clearExpertTeam(conversationId: string): Promise<void> {
  cache.set(conversationId, null)
  notify(conversationId, null)
  useChatStore.setState((state) => ({
    conversations: state.conversations.map((c) =>
      c.id === conversationId
        ? { ...c, kind: 'user' as const, sourceLabel: undefined }
        : c,
    ),
  }))
  try {
    await clearConversationSource(conversationId)
  } catch (err) {
    console.warn('[expertTeamRegistry] clearExpertTeam IPC failed:', err)
  }
}

/**
 * Sync helper: returns the conversation's expertTeamId if it's already in
 * cache (e.g. user just called `setExpertTeam`, or a hook earlier fetched it).
 * Returns undefined if not cached yet — callers that need to *await* the value
 * should call `fetchTeamId` directly or use the React hook.
 */
export function getExpertTeam(conversationId: string): ExpertTeamId | undefined {
  const v = cache.get(conversationId)
  return v ?? undefined
}

/**
 * Sync helper: returns whether conversation is in any expert team.
 * Reads from chatStore (index.json mirror) — does not require id.
 */
export function hasExpertTeam(conversationId: string): boolean {
  const conv = useChatStore
    .getState()
    .conversations.find((c) => c.id === conversationId)
  return conv?.kind === 'expertTeam'
}

/**
 * React hook returning the expertTeamId for a conversation.
 *
 * Reads `kind` from chatStore (synchronous) and lazily fetches the id from
 * conv.json on the first call per conversation. Returns undefined while the
 * fetch is in flight, then the team id once resolved.
 *
 * Subscribes to cache mutations so that `setExpertTeam` / `clearExpertTeam`
 * updates propagate to all mounted hooks for the same conversation.
 */
export function useExpertTeamForConversation(
  conversationId: string | null | undefined,
): ExpertTeamId | undefined {
  const kind = useChatStore((state) =>
    conversationId
      ? state.conversations.find((c) => c.id === conversationId)?.kind
      : undefined,
  )

  const [teamId, setTeamId] = useState<ExpertTeamId | undefined>(() =>
    conversationId ? getExpertTeam(conversationId) : undefined,
  )

  useEffect(() => {
    if (!conversationId || kind !== 'expertTeam') {
      setTeamId(undefined)
      return
    }
    const cached = cache.get(conversationId)
    if (cached !== undefined) {
      setTeamId(cached ?? undefined)
    }
    let cancelled = false
    fetchTeamId(conversationId).then((id) => {
      if (!cancelled) setTeamId(id ?? undefined)
    })
    // Subscribe so set/clear from elsewhere refreshes this hook.
    const sub: Subscriber = (id) => { if (!cancelled) setTeamId(id ?? undefined) }
    let subs = subscribers.get(conversationId)
    if (!subs) { subs = new Set(); subscribers.set(conversationId, subs) }
    subs.add(sub)
    return () => {
      cancelled = true
      subs?.delete(sub)
    }
  }, [conversationId, kind])

  return teamId
}

/**
 * Async helper for non-React callers (e.g. ChatBottomArea's send handler) that
 * need the team id before composing a message. Returns the id from cache if
 * available, otherwise fetches from conv.json.
 */
export async function ensureExpertTeam(
  conversationId: string,
): Promise<ExpertTeamId | undefined> {
  const id = await fetchTeamId(conversationId)
  return id ?? undefined
}

/** Test-only helper. */
export function __resetExpertTeamRegistryCacheForTesting() {
  cache.clear()
  inflight.clear()
  subscribers.clear()
}
