// code/src/features/expert-teams/expertTeamRegistry.ts
//
// 会话 ↔ 专家团映射。**已从 localStorage 改为后端 conv.json 持久化**。
//
// 写入：调用 set_conversation_expert_team / clear_conversation_source 通过 IPC 双写 conv.json + index.json，
//       同时乐观更新 chatStore（kind + expertTeamId 字段），让 React 立即响应。
//
// 读取：
//   - `useExpertTeamForConversation(convId)` 同步返回 ExpertTeamId | undefined，从 chatStore 反查
//   - `hasExpertTeam(convId)` 同步返回 boolean，从 chatStore 反查 kind
//
// 历史 localStorage 数据被丢弃（spec 非目标 1）。老 key 'aijia-expert-team-registry' 不再被读写。

import { useShallow } from 'zustand/react/shallow'

import {
  clearConversationSource,
  setConversationExpertTeam,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { EXPERT_TEAMS, type ExpertTeamId } from './teams'

const VALID_IDS = new Set<ExpertTeamId>(EXPERT_TEAMS.map((t) => t.id))

function labelFor(teamId: ExpertTeamId): string {
  return EXPERT_TEAMS.find((t) => t.id === teamId)?.name ?? teamId
}

/**
 * Set the conversation's expert team. Writes to backend conv.json + index.json,
 * and optimistically updates chatStore so UI responds immediately.
 */
export async function setExpertTeam(
  conversationId: string,
  teamId: ExpertTeamId,
): Promise<void> {
  if (!VALID_IDS.has(teamId)) return
  // Optimistic update first — UI responds immediately
  useChatStore.setState((state) => ({
    conversations: state.conversations.map((c) =>
      c.id === conversationId
        ? { ...c, kind: 'expertTeam' as const, expertTeamId: teamId, sourceLabel: labelFor(teamId) }
        : c,
    ),
  }))
  try {
    await setConversationExpertTeam(conversationId, teamId, labelFor(teamId))
  } catch (err) {
    console.warn('[expertTeamRegistry] setExpertTeam IPC failed:', err)
    // Note: not rolling back the optimistic update — UI stays consistent until next refresh
  }
}

/**
 * Clear the conversation's expert team affiliation (set kind back to user).
 */
export async function clearExpertTeam(conversationId: string): Promise<void> {
  useChatStore.setState((state) => ({
    conversations: state.conversations.map((c) =>
      c.id === conversationId
        ? { ...c, kind: 'user' as const, expertTeamId: undefined, sourceLabel: undefined }
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
 * Sync helper: returns the conversation's expertTeamId (or undefined if not in a team
 * or not yet populated). Reads from useChatStore.conversations.
 */
export function getExpertTeam(conversationId: string): ExpertTeamId | undefined {
  const conv = useChatStore
    .getState()
    .conversations.find((c) => c.id === conversationId)
  const id = conv?.expertTeamId as ExpertTeamId | undefined
  return id && VALID_IDS.has(id) ? id : undefined
}

/**
 * Sync helper: returns whether conversation is in any expert team.
 */
export function hasExpertTeam(conversationId: string): boolean {
  const conv = useChatStore
    .getState()
    .conversations.find((c) => c.id === conversationId)
  return conv?.kind === 'expertTeam'
}

/**
 * React hook returning the expertTeamId synchronously.
 *
 * Subscribes to useChatStore.conversations and returns the teamId for the given
 * conversation, or undefined if not an expert team conversation / id not yet populated.
 */
export function useExpertTeamForConversation(
  conversationId: string | null | undefined,
): ExpertTeamId | undefined {
  return useChatStore(
    useShallow((state) => {
      if (!conversationId) return undefined
      const conv = state.conversations.find((c) => c.id === conversationId)
      const id = conv?.expertTeamId as ExpertTeamId | undefined
      return id && VALID_IDS.has(id) ? id : undefined
    }),
  )
}
