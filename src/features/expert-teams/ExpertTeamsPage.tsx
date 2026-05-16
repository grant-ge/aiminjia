// code/src/features/expert-teams/ExpertTeamsPage.tsx
import { useState } from 'react'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { createConversation } from '@/lib/tauri'
import { useUiStore } from '@/stores/uiStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { ExpertTeamCard } from './ExpertTeamCard'
import { EXPERT_TEAMS, type ExpertTeamId, getExpertTeam } from './teams'
import { setExpertTeam } from './expertTeamRegistry'

export function ExpertTeamsPage() {
  const setRoute = useUiStore((s) => s.setRoute)
  const pushNotification = useNotificationStore((s) => s.push)
  const [starting, setStarting] = useState<ExpertTeamId | null>(null)

  const handleStart = async (id: ExpertTeamId) => {
    if (starting) return
    const team = getExpertTeam(id)
    if (!team) return
    setStarting(id)
    try {
      const conversationId = await createConversation()
      setExpertTeam(conversationId, id)
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
      setStarting(null)
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
