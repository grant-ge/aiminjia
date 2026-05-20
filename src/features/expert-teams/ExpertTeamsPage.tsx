// code/src/features/expert-teams/ExpertTeamsPage.tsx
import { useRef } from 'react'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { createConversation, renameConversation } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore } from '@/stores/uiStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { ExpertTeamCard } from './ExpertTeamCard'
import { EXPERT_TEAMS, type ExpertTeamId, getExpertTeam } from './teams'
import { setExpertTeam } from './expertTeamRegistry'

export function ExpertTeamsPage() {
  const setRoute = useUiStore((s) => s.setRoute)
  const pushNotification = useNotificationStore((s) => s.push)
  // Synchronous guard: React state updates are batched, so two rapid clicks
  // can both pass a useState-based check before re-render. A ref flips
  // immediately and blocks the second call.
  const busyRef = useRef(false)

  const handleStart = async (id: ExpertTeamId) => {
    if (busyRef.current) return
    busyRef.current = true
    const team = getExpertTeam(id)
    if (!team) return
    try {
      const conversationId = await createConversation()
      const title = `专家团: ${team.name}`
      // Persist the title on the backend so reloads / sidebar reloads keep it.
      // Best-effort: if rename fails the optimistic local title still shows.
      try {
        await renameConversation(conversationId, title)
      } catch (err) {
        console.warn('[ExpertTeamsPage] renameConversation failed', err)
      }
      await setExpertTeam(conversationId, id)
      // Optimistically inject into chatStore so the sidebar shows the new
      // conversation immediately. The backend `conversation:created` event
      // will refresh the list anyway, but it can land after the user has
      // already navigated away, leaving them unable to find the chat.
      const store = useChatStore.getState()
      const now = new Date().toISOString()
      store.setConversations([
        {
          id: conversationId,
          title,
          createdAt: now,
          updatedAt: now,
          isArchived: false,
        },
        ...store.conversations.filter((c) => c.id !== conversationId),
      ])
      setRoute({ kind: 'chat', conversationId })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: '无法启动专家团',
        message: err instanceof Error ? err.message : '创建会话失败，请重试。',
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    } finally {
      busyRef.current = false
    }
  }

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
      <PageTopBar variant="title" title="专家团" />
      <div className="flex-1 overflow-y-auto px-6 py-6">
        <div className="mx-auto grid w-full max-w-[1024px] grid-cols-1 gap-4 sm:grid-cols-2">
          {EXPERT_TEAMS.map((team) => (
            <ExpertTeamCard key={team.id} team={team} onStart={handleStart} />
          ))}
        </div>
      </div>
    </div>
  )
}
