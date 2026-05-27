// code/src/features/expert-teams/ExpertTeamsPage.tsx
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RefreshCw } from 'lucide-react'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { Button } from '@/components/ui/button'
import { createConversation, expertTeamTemplateRefresh, renameConversation } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore } from '@/stores/uiStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { ExpertTeamCard } from './ExpertTeamCard'
import { EXPERT_TEAMS, type ExpertTeamId, getExpertTeam } from './teams'
import { setExpertTeam } from './expertTeamRegistry'

export function ExpertTeamsPage() {
  const { t } = useTranslation()
  const setRoute = useUiStore((s) => s.setRoute)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)
  const pushNotification = useNotificationStore((s) => s.push)
  const [syncing, setSyncing] = useState(false)
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
      // Optimistically inject into chatStore FIRST so the sidebar shows the new
      // conversation immediately. The backend `conversation:created` event will
      // refresh the list anyway, but it can land after the user has navigated
      // away. `kind` + `sourceLabel` are set here so the conversation lands in
      // the 专家团 sidebar group instead of falling into 默认项目.
      const store = useChatStore.getState()
      const now = new Date().toISOString()
      store.setConversations([
        {
          id: conversationId,
          title,
          createdAt: now,
          updatedAt: now,
          isArchived: false,
          kind: 'expertTeam',
          sourceLabel: team.name,
        },
        ...store.conversations.filter((c) => c.id !== conversationId),
      ])
      // Persist the title on the backend so reloads / sidebar reloads keep it.
      // Best-effort: if rename fails the optimistic local title still shows.
      try {
        await renameConversation(conversationId, title)
      } catch (err) {
        console.warn('[ExpertTeamsPage] renameConversation failed', err)
      }
      // Await so the chatStore patch lands before navigate — otherwise the
      // ExpertTeamBanner on the chat page would flash empty for a beat.
      // setExpertTeam also seeds the id cache so useExpertTeamForConversation
      // hits synchronously on the first render of ChatPage.
      await setExpertTeam(conversationId, id)
      // Switch sidebar to 专家团 tab so the user lands in the right section.
      setSidebarTab('expert-team')
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

  const handleSync = async () => {
    if (syncing) return
    setSyncing(true)
    try {
      const count = await expertTeamTemplateRefresh()
      pushNotification({
        level: 'success',
        title: count > 0
          ? t('ExpertTeams.syncDone', { count })
          : t('ExpertTeams.syncUpToDate'),
        message: '',
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: t('ExpertTeams.syncFailed'),
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setSyncing(false)
    }
  }

  return (
    <PageSectionShell
      topBar={(
        <PageTopBar
          variant="title"
          title={t('ExpertTeams.pageTitle')}
          trailing={(
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 gap-1.5 px-2 text-xs"
              disabled={syncing}
              onClick={() => void handleSync()}
            >
              <RefreshCw className={`h-3 w-3 ${syncing ? 'animate-spin' : ''}`} />
              {syncing ? t('ExpertTeams.syncing') : t('ExpertTeams.syncServer')}
            </Button>
          )}
        />
      )}
      maxWidthClass="max-w-[1024px]"
    >
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
        {EXPERT_TEAMS.map((team) => (
          <ExpertTeamCard key={team.id} team={team} onStart={handleStart} />
        ))}
      </div>
    </PageSectionShell>
  )
}
