// code/src/features/expert-teams/expertTeamRegistry.ts
// 会话 ↔ 专家团映射（进程内，无持久化）。spec §2 硬约束：
// 不新增 Tauri 命令、不新增 zustand store、不持久化。
// 重启后映射丢失符合预期：导演 prompt 只在 messages.length===0 注入一次。
import type { ExpertTeamId } from './teams'

const map = new Map<string, ExpertTeamId>()
const listeners = new Set<() => void>()

export function setExpertTeam(conversationId: string, teamId: ExpertTeamId): void {
  map.set(conversationId, teamId)
  listeners.forEach((fn) => fn())
}

export function getExpertTeam(conversationId: string): ExpertTeamId | undefined {
  return map.get(conversationId)
}

export function clearExpertTeam(conversationId: string): void {
  if (map.delete(conversationId)) {
    listeners.forEach((fn) => fn())
  }
}

/** Subscribe; returns unsubscribe. Used by useExpertTeamForConversation. */
export function subscribe(fn: () => void): () => void {
  listeners.add(fn)
  return () => {
    listeners.delete(fn)
  }
}

// React hook: re-render when registry for this conversation changes.
import { useSyncExternalStore } from 'react'

export function useExpertTeamForConversation(conversationId: string | null | undefined): ExpertTeamId | undefined {
  return useSyncExternalStore(
    subscribe,
    () => (conversationId ? map.get(conversationId) : undefined),
    () => undefined,
  )
}
