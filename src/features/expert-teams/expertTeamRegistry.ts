// 会话 ↔ 专家团映射。
// 真相源：chatStore.conversations[].expertTeamId（由 get_conversations IPC 注入），
// 写回 conv.json 通过 setConversationExpertTeam IPC。
//
// 历史：早期版本 (≤0.5.27) 把映射存在 localStorage('aijia-expert-team-registry') 里，
// 设计为"前端临时持久化"，副作用是 Settings → 重置会误清、跨租户会泄漏、对话删除
// 不会清理孤儿。本实现把映射搬到 conv.json，跟随对话生命周期。迁移路径见
// migrateExpertTeamRegistry.ts；详细设计见
// docs/plans/2026-05-20-expert-team-storage-migration.md。
//
// 公开 API 与早期版本兼容（setExpertTeam / getExpertTeam / clearExpertTeam /
// useExpertTeamForConversation），call sites 无需改动；但 set / clear 变为 async。
import { setConversationExpertTeam } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { EXPERT_TEAMS, type ExpertTeamId } from './teams'

const VALID_IDS = new Set<ExpertTeamId>(EXPERT_TEAMS.map((t) => t.id))

function patchConversationField(
  conversationId: string,
  expertTeamId: ExpertTeamId | undefined,
): void {
  const store = useChatStore.getState()
  store.setConversations(
    store.conversations.map((c) =>
      c.id === conversationId ? { ...c, expertTeamId } : c,
    ),
  )
}

function readField(conversationId: string): string | undefined {
  return useChatStore
    .getState()
    .conversations.find((c) => c.id === conversationId)?.expertTeamId
}

function asValidTeamId(raw: string | undefined): ExpertTeamId | undefined {
  return raw && VALID_IDS.has(raw as ExpertTeamId) ? (raw as ExpertTeamId) : undefined
}

export async function setExpertTeam(
  conversationId: string,
  teamId: ExpertTeamId,
): Promise<void> {
  const prev = readField(conversationId)
  patchConversationField(conversationId, teamId)
  try {
    await setConversationExpertTeam(conversationId, teamId)
  } catch (err) {
    console.error('[expertTeamRegistry] setConversationExpertTeam failed', err)
    patchConversationField(conversationId, prev as ExpertTeamId | undefined)
    throw err
  }
}

export function getExpertTeam(conversationId: string): ExpertTeamId | undefined {
  return asValidTeamId(readField(conversationId))
}

export async function clearExpertTeam(conversationId: string): Promise<void> {
  const prev = readField(conversationId)
  patchConversationField(conversationId, undefined)
  try {
    await setConversationExpertTeam(conversationId, null)
  } catch (err) {
    console.error('[expertTeamRegistry] clearConversationExpertTeam failed', err)
    patchConversationField(conversationId, prev as ExpertTeamId | undefined)
    throw err
  }
}

export function useExpertTeamForConversation(
  conversationId: string | null | undefined,
): ExpertTeamId | undefined {
  return useChatStore((s) => {
    if (!conversationId) return undefined
    const raw = s.conversations.find((c) => c.id === conversationId)?.expertTeamId
    return raw && VALID_IDS.has(raw as ExpertTeamId)
      ? (raw as ExpertTeamId)
      : undefined
  })
}
