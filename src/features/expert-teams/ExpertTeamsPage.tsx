// code/src/features/expert-teams/ExpertTeamsPage.tsx
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RefreshCw } from 'lucide-react'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { Button } from '@/components/ui/button'
import {
  createConversation,
  expertTeamTemplateRefresh,
  renameConversation,
  workplaceDirectoryCatalog,
  type WorkplaceDirectoryCategory,
  type WorkplaceDirectoryItem,
} from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore } from '@/stores/uiStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { ExpertTeamCard } from './ExpertTeamCard'
import {
  EXPERT_TEAMS,
  getExpertTeams,
  type ExpertTeam,
  type ExpertTeamId,
  getExpertTeam,
  localizeExpertTeam,
  setRemoteExpertTeams,
} from './teams'
import { setExpertTeam } from './expertTeamRegistry'
import { ExpertTeamDetailDialog } from './ExpertTeamDetailDialog'

interface ExpertTeamCategory {
  categoryId: string
  name: string
  description?: string
  icon?: string
  color?: string
  sortOrder: number
}

interface ExpertTeamCatalog {
  teams: ExpertTeam[]
  categories: ExpertTeamCategory[]
}

interface ExpertTeamGroup {
  key: string
  category: ExpertTeamCategory | null
  teams: ExpertTeam[]
}

function defaultComposerPlaceholder(language?: string): string {
  return language?.toLowerCase().startsWith('en')
    ? 'Share a topic for the team to discuss...'
    : '抛出你的议题，专家团会一起讨论...'
}

function iconEmoji(icon?: string): string {
  if (!icon) return '👥'
  return Array.from(icon).length <= 2 ? icon : '👥'
}

function categoryDescription(category: ExpertTeamCategory | null): string | null {
  const text = category?.description?.trim()
  return text && text !== category?.name ? text : null
}

const ALL_EXPERT_TEAM_GROUP_KEY = '__all__'

function toCatalogCategory(category: WorkplaceDirectoryCategory): ExpertTeamCategory {
  return {
    categoryId: category.categoryId,
    name: category.display.name || category.categoryId,
    description: category.display.description || category.display.tagline,
    icon: category.icon,
    color: category.color,
    sortOrder: category.sortOrder,
  }
}

function directoryItemToTeam(
  item: WorkplaceDirectoryItem,
  category: ExpertTeamCategory | null,
  language?: string,
): ExpertTeam | null {
  if (item.resourceType !== 'expert_team_template') return null
  const builtIn = EXPERT_TEAMS.find((team) => team.id === item.resourceId)
  const base = builtIn ? localizeExpertTeam(builtIn, language) : undefined
  const examples = item.display.examples?.filter(Boolean) ?? []
  return {
    id: item.resourceId,
    name: item.display.name || base?.name || item.resourceId,
    emoji: base?.emoji ?? iconEmoji(item.icon),
    tagline: item.display.tagline || item.display.description || base?.tagline || '',
    experts: base?.experts ?? [],
    examples: examples.length > 0 ? examples : (base?.examples ?? []),
    composerPlaceholder: base?.composerPlaceholder ?? defaultComposerPlaceholder(language),
    facilitationStyle: base?.facilitationStyle ?? 'open',
    workplaceCategoryId: item.workplaceCategoryId ?? null,
    workplaceCategoryName: category?.name ?? null,
    workplaceCategoryDescription: category?.description ?? null,
    workplaceCategoryIcon: category?.icon ?? null,
    workplaceCategoryColor: category?.color ?? null,
    workplaceCategorySortOrder: category?.sortOrder ?? null,
    sortOrder: item.sortOrder ?? null,
  }
}

async function loadDirectoryTeams(language?: string): Promise<ExpertTeamCatalog> {
  const directory = await workplaceDirectoryCatalog(language)
  const categories = directory.categories.map(toCatalogCategory)
  const categoryById = new Map(categories.map((category) => [category.categoryId, category]))
  const teams = directory.items
    .map((item) => directoryItemToTeam(
      item,
      item.workplaceCategoryId ? categoryById.get(item.workplaceCategoryId) ?? null : null,
      language,
    ))
    .filter((team): team is ExpertTeam => !!team)
    .sort((a, b) => {
      const categoryDelta =
        (a.workplaceCategorySortOrder ?? Number.MAX_SAFE_INTEGER) -
        (b.workplaceCategorySortOrder ?? Number.MAX_SAFE_INTEGER)
      if (categoryDelta !== 0) return categoryDelta
      const itemDelta = (a.sortOrder ?? Number.MAX_SAFE_INTEGER) - (b.sortOrder ?? Number.MAX_SAFE_INTEGER)
      if (itemDelta !== 0) return itemDelta
      return a.id.localeCompare(b.id)
    })
  return { teams, categories }
}

function groupExpertTeams(teams: ExpertTeam[], categories: ExpertTeamCategory[]): ExpertTeamGroup[] {
  const categoryById = new Map(categories.map((category) => [category.categoryId, category]))
  const groups = new Map<string, ExpertTeamGroup>()
  for (const team of teams) {
    const key = team.workplaceCategoryId || '__default__'
    let group = groups.get(key)
    if (!group) {
      group = {
        key,
        category: team.workplaceCategoryId
          ? categoryById.get(team.workplaceCategoryId) ?? {
              categoryId: team.workplaceCategoryId,
              name: team.workplaceCategoryName || team.workplaceCategoryId,
              sortOrder: Number.MAX_SAFE_INTEGER,
            }
          : null,
        teams: [],
      }
      groups.set(key, group)
    }
    group.teams.push(team)
  }
  return Array.from(groups.values()).sort((a, b) => {
    const sortDelta =
      (a.category?.sortOrder ?? Number.MAX_SAFE_INTEGER) -
      (b.category?.sortOrder ?? Number.MAX_SAFE_INTEGER)
    if (sortDelta !== 0) return sortDelta
    return (a.category?.name ?? '').localeCompare(b.category?.name ?? '')
  })
}

export function ExpertTeamsPage() {
  const { t, i18n } = useTranslation()
  const setRoute = useUiStore((s) => s.setRoute)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)
  const pushNotification = useNotificationStore((s) => s.push)
  const [syncing, setSyncing] = useState(false)
  const [directoryTeams, setDirectoryTeams] = useState<ExpertTeam[] | null>(null)
  const [directoryCategories, setDirectoryCategories] = useState<ExpertTeamCategory[]>([])
  const [selectedTeamId, setSelectedTeamId] = useState<ExpertTeamId | null>(null)
  const [busyTeamId, setBusyTeamId] = useState<ExpertTeamId | null>(null)
  const [activeGroupKey, setActiveGroupKey] = useState(ALL_EXPERT_TEAM_GROUP_KEY)
  // Synchronous guard: React state updates are batched, so two rapid clicks
  // can both pass a useState-based check before re-render. A ref flips
  // immediately and blocks the second call.
  const busyRef = useRef(false)
  const teams = directoryTeams ?? getExpertTeams(i18n.language)
  const groups = groupExpertTeams(teams, directoryTeams ? directoryCategories : [])
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
    ? teams.find((item) => item.id === selectedTeamId) ?? getExpertTeam(selectedTeamId, i18n.language) ?? null
    : null

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
        const loaded = await loadDirectoryTeams(i18n.language)
        if (cancelled) return
        if (loaded.teams.length > 0) {
          setRemoteExpertTeams(loaded.teams)
          setDirectoryTeams(loaded.teams)
          setDirectoryCategories(loaded.categories)
        } else {
          setDirectoryTeams(null)
          setDirectoryCategories([])
        }
      } catch (err) {
        if (cancelled) return
        console.warn('[ExpertTeamsPage] workplace_directory_catalog failed:', err)
        setDirectoryTeams(null)
        setDirectoryCategories([])
      }
    })()
    return () => {
      cancelled = true
    }
  }, [i18n.language])

  const handleStart = async (id: ExpertTeamId) => {
    if (busyRef.current) return
    busyRef.current = true
    setBusyTeamId(id)
    const team = teams.find((item) => item.id === id) ?? getExpertTeam(id, i18n.language)
    if (!team) {
      busyRef.current = false
      setBusyTeamId(null)
      return
    }
    try {
      const conversationId = await createConversation()
      const title = t('ExpertTeams.conversationTitle', { name: team.name })
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
      await setExpertTeam(conversationId, id, team.name)
      // Switch sidebar to 专家团 tab so the user lands in the right section.
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
      const count = await expertTeamTemplateRefresh()
      try {
        const loaded = await loadDirectoryTeams(i18n.language)
        if (loaded.teams.length > 0) {
          setRemoteExpertTeams(loaded.teams)
          setDirectoryTeams(loaded.teams)
          setDirectoryCategories(loaded.categories)
        }
      } catch (err) {
        console.warn('[ExpertTeamsPage] workplace_directory_catalog after sync failed:', err)
      }
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
      <div className="flex flex-col gap-3">
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
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          {visibleTeams.map((team) => (
            <ExpertTeamCard
              key={team.id}
              team={team}
              onStart={(id) => setSelectedTeamId(id)}
            />
          ))}
        </div>
      </div>
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
