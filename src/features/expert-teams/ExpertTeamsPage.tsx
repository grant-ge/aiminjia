// code/src/features/expert-teams/ExpertTeamsPage.tsx
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RefreshCw } from 'lucide-react'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import {
  createConversation,
  renameConversation,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore } from '@/stores/uiStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { ExpertTeamCard } from './ExpertTeamCard'
import {
  type ExpertTeam,
  type ExpertTeamId,
  setRemoteExpertTeams,
} from './teams'
import { setExpertTeam } from './expertTeamRegistry'
import { ExpertTeamDetailDialog } from './ExpertTeamDetailDialog'
import {
  groupExpertTeams,
  loadExpertTeamCatalog,
  type ExpertTeamCategory,
} from './expertTeamCatalog'
import { Button } from '@/components/ui/button'

function categoryDescription(category: ExpertTeamCategory | null): string | null {
  const text = category?.description?.trim()
  return text && text !== category?.name ? text : null
}

const ALL_EXPERT_TEAM_GROUP_KEY = '__all__'

export function ExpertTeamsPage() {
  const { t, i18n } = useTranslation()
  const setRoute = useUiStore((s) => s.setRoute)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)
  const pushNotification = useNotificationStore((s) => s.push)
  const [syncing, setSyncing] = useState(false)
  const [catalogLoading, setCatalogLoading] = useState(false)
  const [catalogLoadError, setCatalogLoadError] = useState<string | null>(null)
  const [teams, setTeams] = useState<ExpertTeam[]>([])
  const [directoryCategories, setDirectoryCategories] = useState<ExpertTeamCategory[]>([])
  const [selectedTeamId, setSelectedTeamId] = useState<ExpertTeamId | null>(null)
  const [busyTeamId, setBusyTeamId] = useState<ExpertTeamId | null>(null)
  const [activeGroupKey, setActiveGroupKey] = useState(ALL_EXPERT_TEAM_GROUP_KEY)
  const busyRef = useRef(false)

  const groups = groupExpertTeams(teams, directoryCategories)
  const visibleGroups = useMemo(
    () => groups.filter((group) => !!group.category),
    [groups],
  )
  const activeGroup = activeGroupKey === ALL_EXPERT_TEAM_GROUP_KEY
    ? null
    : groups.find((group) => group.key === activeGroupKey) ?? null
  const visibleTeams = activeGroup?.teams ?? teams
  const categoryItems = useMemo(
    () => [
      { key: ALL_EXPERT_TEAM_GROUP_KEY, label: t('ExpertTeams.allCategory') },
      ...visibleGroups.map((group) => ({
        key: group.key,
        label: group.category?.name ?? group.key,
      })),
    ],
    [t, visibleGroups],
  )
  const selectedTeam = selectedTeamId
    ? teams.find((item) => item.id === selectedTeamId) ?? null
    : null

  const loadCatalog = useCallback(async (options: { forceRefresh?: boolean } = {}) => {
    setCatalogLoading(true)
    setCatalogLoadError(null)
    try {
      const loaded = await loadExpertTeamCatalog(i18n.language, options)
      setTeams(loaded.teams)
      setDirectoryCategories(loaded.categories)
      setRemoteExpertTeams(loaded.teams)
      setCatalogLoadError(
        loaded.error ? (loaded.error instanceof Error ? loaded.error.message : String(loaded.error)) : null,
      )
      return loaded
    } catch (err) {
      console.warn('[ExpertTeamsPage] expert team catalog load failed:', err)
      setTeams([])
      setDirectoryCategories([])
      setRemoteExpertTeams([])
      setCatalogLoadError(err instanceof Error ? err.message : String(err))
      throw err
    } finally {
      setCatalogLoading(false)
    }
  }, [i18n.language])

  useEffect(() => {
    if (activeGroupKey === ALL_EXPERT_TEAM_GROUP_KEY) return
    if (!groups.some((group) => group.key === activeGroupKey)) {
      setActiveGroupKey(ALL_EXPERT_TEAM_GROUP_KEY)
    }
  }, [activeGroupKey, groups])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        await loadCatalog()
      } catch (err) {
        if (cancelled) return
        console.warn('[ExpertTeamsPage] automatic expert team catalog sync failed:', err)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [loadCatalog])

  const handleStart = async (id: ExpertTeamId) => {
    if (busyRef.current) return
    busyRef.current = true
    setBusyTeamId(id)
    const team = teams.find((item) => item.id === id)
    if (!team) {
      busyRef.current = false
      setBusyTeamId(null)
      return
    }
    try {
      const conversationId = await createConversation()
      const title = t('ExpertTeams.conversationTitle', { name: team.name })
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
      try {
        await renameConversation(conversationId, title)
      } catch (err) {
        console.warn('[ExpertTeamsPage] renameConversation failed', err)
      }
      await setExpertTeam(conversationId, id, team.name)
      setSelectedTeamId(null)
      setSidebarTab('expert-team')
      setRoute({ kind: 'chat', conversationId })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: t('ExpertTeams.startFailed'),
        message: err instanceof Error ? err.message : t('ExpertTeams.createConversationFailed'),
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    } finally {
      busyRef.current = false
      setBusyTeamId(null)
    }
  }

  const handleSync = async () => {
    if (syncing) return
    setSyncing(true)
    try {
      await loadCatalog({ forceRefresh: true })
      pushNotification({
        level: 'success',
        title: t('ExpertTeams.syncDone'),
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
              size="md"
              icon={<RefreshCw className={`h-3 w-3 ${syncing || catalogLoading ? 'animate-spin' : ''}`} />}
              disabled={syncing || catalogLoading}
              onClick={() => void handleSync()}
            >
              {syncing || catalogLoading ? t('ExpertTeams.syncing') : t('ExpertTeams.syncServer')}
            </Button>
          )}
        />
      )}
    >
      {catalogLoading && teams.length === 0 ? (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {[...Array(4)].map((_, i) => (
            <div key={i} className="h-[154px] animate-pulse rounded-md border border-border/50 bg-card" />
          ))}
        </div>
      ) : teams.length === 0 ? (
        <div
          className="flex min-h-[180px] flex-col items-center justify-center gap-3 rounded-md border border-dashed border-border bg-card px-4 text-center shadow-[var(--shadow-card)]"
          data-aijia-expert-team-directory-empty
        >
          <p className="text-sm text-muted-foreground">
            {catalogLoadError
              ? t('ExpertTeams.directoryLoadError', { err: catalogLoadError })
              : t('ExpertTeams.directoryEmpty')}
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            icon={<RefreshCw className={`h-3.5 w-3.5 ${syncing ? 'animate-spin' : ''}`} />}
            disabled={syncing}
            onClick={() => void handleSync()}
          >
            {syncing ? t('ExpertTeams.syncing') : t('ExpertTeams.syncServer')}
          </Button>
        </div>
      ) : (
        <div className="flex min-w-0 flex-col gap-3">
          {catalogLoadError && (
            <p className="text-xs text-muted-foreground">
              {t('ExpertTeams.directoryPartialError', { err: catalogLoadError })}
            </p>
          )}
          <SkillCategoryBar
            items={categoryItems}
            activeKey={activeGroupKey}
            onSelect={setActiveGroupKey}
          />
          {activeGroup?.category && categoryDescription(activeGroup.category) && (
            <p className="text-xs text-muted-foreground">
              {categoryDescription(activeGroup.category)}
            </p>
          )}
          <div className="grid min-w-0 grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {visibleTeams.map((team) => (
              <ExpertTeamCard
                key={team.id}
                team={team}
                onStart={(id) => setSelectedTeamId(id)}
              />
            ))}
          </div>
        </div>
      )}
      <ExpertTeamDetailDialog
        team={selectedTeam}
        open={!!selectedTeam}
        onOpenChange={(open) => {
          if (!open) setSelectedTeamId(null)
        }}
        busy={!!selectedTeam && busyTeamId === selectedTeam.id}
        onStart={handleStart}
      />
    </PageSectionShell>
  )
}
