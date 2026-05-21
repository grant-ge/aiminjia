// One-shot migration: lift the legacy localStorage map into conv.json on disk.
//
// Background: pre-PR the mapping lived in localStorage
// (`aijia-expert-team-registry`) — a tactical "ship fast" choice that turned
// out to be brittle (Settings → 重置 误清 / 跨租户污染 / 对话删除残留孤儿 /
// 后端看不到). This migration runs once at app start, writes each entry
// through the new `set_conversation_expert_team` IPC, then deletes the legacy
// key. Idempotent (marker-based); partial failure leaves the legacy key in
// place to retry next launch.
import {
  getConversations,
  setConversationExpertTeam,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { EXPERT_TEAMS, type ExpertTeamId } from './teams'

const LEGACY_KEY = 'aijia-expert-team-registry'
const MARKER_KEY = 'aijia-expert-team-migration-v1'
const VALID_IDS = new Set<ExpertTeamId>(EXPERT_TEAMS.map((t) => t.id))

export async function migrateExpertTeamRegistryOnce(): Promise<void> {
  if (typeof localStorage === 'undefined') return
  try {
    if (localStorage.getItem(MARKER_KEY) === 'done') {
      // Clean residue from a downgrade-then-upgrade cycle so a stale legacy
      // key can't be re-applied by a future migration with a different marker.
      localStorage.removeItem(LEGACY_KEY)
      return
    }
    const raw = localStorage.getItem(LEGACY_KEY)
    if (!raw) {
      localStorage.setItem(MARKER_KEY, 'done')
      return
    }
    const parsed = JSON.parse(raw) as Record<string, string>

    const liveConvs = await getConversations()
    const liveIds = new Set(liveConvs.map((c) => (c.id as string) ?? ''))

    let allOk = true
    const migrated: Array<[string, ExpertTeamId]> = []
    for (const [convId, teamId] of Object.entries(parsed)) {
      if (!VALID_IDS.has(teamId as ExpertTeamId)) {
        console.info('[migrate-expert-team] skip unknown team', convId, teamId)
        continue
      }
      if (!liveIds.has(convId)) {
        console.info('[migrate-expert-team] skip missing conv', convId)
        continue
      }
      try {
        await setConversationExpertTeam(convId, teamId)
        migrated.push([convId, teamId as ExpertTeamId])
      } catch (err) {
        console.error('[migrate-expert-team] write failed', convId, err)
        allOk = false
      }
    }

    if (allOk) {
      localStorage.removeItem(LEGACY_KEY)
      localStorage.setItem(MARKER_KEY, 'done')
      // Patch chatStore so the banner shows up immediately on next route entry,
      // without waiting for the next get_conversations refresh.
      const store = useChatStore.getState()
      const migMap = new Map(migrated)
      store.setConversations(
        store.conversations.map((c) =>
          migMap.has(c.id) ? { ...c, expertTeamId: migMap.get(c.id) } : c,
        ),
      )
    }
  } catch (err) {
    console.error('[migrate-expert-team] migration aborted', err)
    // intentionally do not set marker — retry next launch
  }
}
