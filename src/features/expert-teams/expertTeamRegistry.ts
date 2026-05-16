// code/src/features/expert-teams/expertTeamRegistry.ts
// 会话 ↔ 专家团映射。轻量 localStorage 持久化，保证刷新 / 重启后仍能识别会话所属团队。
// spec §2 硬约束：不新增 Tauri 命令、不新增 zustand store、不动后端。localStorage 是
// 纯前端持久化，不算"新存储"。
import { useSyncExternalStore } from 'react'
import { EXPERT_TEAMS, type ExpertTeamId } from './teams'

const STORAGE_KEY = 'aijia-expert-team-registry'
const VALID_IDS = new Set<ExpertTeamId>(EXPERT_TEAMS.map((t) => t.id))

function loadFromStorage(): Map<string, ExpertTeamId> {
  if (typeof localStorage === 'undefined') return new Map()
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return new Map()
    const parsed = JSON.parse(raw) as Record<string, string>
    const out = new Map<string, ExpertTeamId>()
    for (const [convId, teamId] of Object.entries(parsed)) {
      if (VALID_IDS.has(teamId as ExpertTeamId)) {
        out.set(convId, teamId as ExpertTeamId)
      }
    }
    return out
  } catch {
    return new Map()
  }
}

function persist(map: Map<string, ExpertTeamId>) {
  if (typeof localStorage === 'undefined') return
  try {
    const obj: Record<string, string> = {}
    for (const [k, v] of map) obj[k] = v
    localStorage.setItem(STORAGE_KEY, JSON.stringify(obj))
  } catch {
    // Ignore quota / private mode failures.
  }
}

const map = loadFromStorage()
const listeners = new Set<() => void>()

function notify() {
  persist(map)
  listeners.forEach((fn) => fn())
}

export function setExpertTeam(conversationId: string, teamId: ExpertTeamId): void {
  map.set(conversationId, teamId)
  notify()
}

export function getExpertTeam(conversationId: string): ExpertTeamId | undefined {
  return map.get(conversationId)
}

export function clearExpertTeam(conversationId: string): void {
  if (map.delete(conversationId)) {
    notify()
  }
}

/** Subscribe; returns unsubscribe. Used by useExpertTeamForConversation. */
export function subscribe(fn: () => void): () => void {
  listeners.add(fn)
  return () => {
    listeners.delete(fn)
  }
}

export function useExpertTeamForConversation(conversationId: string | null | undefined): ExpertTeamId | undefined {
  return useSyncExternalStore(
    subscribe,
    () => (conversationId ? map.get(conversationId) : undefined),
    () => undefined,
  )
}
