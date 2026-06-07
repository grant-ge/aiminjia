import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RefreshCw, SendHorizontal } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { Button } from '@/components/ui/button'
import { useUiStore } from '@/stores/uiStore'
import { useEmployees } from '@/features/employees/useEmployees'
import { useInbox } from '@/features/employees/useInbox'
import {
  employeeCreate,
  employeeTrigger,
  type EmployeeRecord,
} from '@/lib/tauri'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useChatStore } from '@/stores/chatStore'
import { seedDispatchConversation } from '@/features/employees/seedDispatchConversation'
import {
  groupEmployeeCatalog,
  loadEmployeeTemplateCatalog,
  requiredSkillNames,
  type EmployeeCatalogCategory,
  type EmployeeTemplateCatalogResult,
} from '@/features/employees/employeeCatalog'
import type { EmployeeTemplate } from '@/features/employees/templates'
import { EmployeeTemplateDetailDialog } from '@/features/employees/EmployeeTemplateDetailDialog'
import { getEmployeeVisual } from '@/features/employees/employeeVisual'

// ─── daily feed ──────────────────────────────────────────────────────────────

function kindIcon(kind: string): string {
  switch (kind) {
    case 'report': return '📄'
    case 'signal': return '💡'
    case 'running': return '⚙️'
    case 'error': return '⚠️'
    default: return '•'
  }
}

function useTimeLabel() {
  const { t, i18n } = useTranslation()
  return (iso: string): string => {
    const d = new Date(iso)
    const now = new Date()
    const diffMs = now.getTime() - d.getTime()
    const diffMin = Math.floor(diffMs / 60_000)
    if (diffMin < 1) return t('employeesPage.timeLabel.justNow')
    if (diffMin < 60) return t('employeesPage.timeLabel.minutesAgo', { count: diffMin })
    const diffH = Math.floor(diffMin / 60)
    if (diffH < 24) return t('employeesPage.timeLabel.hoursAgo', { count: diffH })
    const diffD = Math.floor(diffH / 24)
    if (diffD === 1) return t('employeesPage.timeLabel.yesterday')
    return d.toLocaleDateString(i18n.language, { month: 'numeric', day: 'numeric' })
  }
}

// ─── greeting ─────────────────────────────────────────────────────────────────

function useGreeting(): string {
  const { t } = useTranslation()
  const h = new Date().getHours()
  if (h < 6) return t('employeesPage.greeting.lateNight')
  if (h < 12) return t('employeesPage.greeting.morning')
  if (h < 14) return t('employeesPage.greeting.noon')
  if (h < 18) return t('employeesPage.greeting.afternoon')
  return t('employeesPage.greeting.evening')
}

function categoryDescription(category: EmployeeCatalogCategory | null): string | null {
  const text = category?.description?.trim()
  return text && text !== category?.name ? text : null
}

const ALL_CATALOG_GROUP_KEY = '__all__'

interface EmployeeDirectoryCardProps {
  template: EmployeeTemplate
  busy: boolean
  onOpen: (template: EmployeeTemplate) => void
}

function EmployeeDirectoryCard({
  template,
  busy,
  onOpen,
}: EmployeeDirectoryCardProps) {
  const { t } = useTranslation()
  const skills = requiredSkillNames(template)
  const visual = getEmployeeVisual(template)
  const actionLabel = busy
    ? t('employeesPage.summoning')
    : t('employeesPage.viewDetail')

  return (
    <button
      type="button"
      data-aijia-employee-template-card
      data-aijia-employee-template-id={template.templateId}
      data-aijia-employee-template-name={visual.name}
      aria-label={t('employeesPage.openEmployeeDetail', { name: visual.name })}
      aria-busy={busy}
      disabled={busy}
      onClick={() => onOpen(template)}
      className="group flex min-h-[212px] w-full flex-col gap-3 rounded-md border border-border bg-card p-4 text-left text-card-foreground shadow-[var(--shadow-card)] transition-all hover:border-primary/50 hover:shadow-[var(--shadow-card-hover)] disabled:cursor-wait disabled:opacity-70"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <span className={`flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-md ${visual.accent}`}>
            {visual.avatarUrl ? (
              <img src={visual.avatarUrl} alt="" className="h-full w-full object-cover" />
            ) : (
              <span className="text-xl font-semibold leading-none">{visual.avatarText}</span>
            )}
          </span>
          <div className="min-w-0">
            <p className="truncate text-[15px] font-semibold leading-[22px] text-foreground">{visual.name}</p>
            <p className="truncate text-xs leading-4 text-muted-foreground">{visual.title}</p>
          </div>
        </div>
        <span className="flex h-7 shrink-0 items-center gap-1 rounded-[var(--radius)] bg-brand-primary-subtle px-2 text-xs font-medium text-primary">
          {busy ? (
            <RefreshCw className="h-3 w-3 animate-spin" />
          ) : (
            <SendHorizontal className="h-3 w-3" />
          )}
          {actionLabel}
        </span>
      </div>

      <p className="line-clamp-3 text-[13px] leading-5 text-muted-foreground">
        {template.description}
      </p>

      <div className="mt-auto flex flex-wrap gap-1.5">
        {template.badge && (
          <span className="max-w-full truncate rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
            {template.badge}
          </span>
        )}
        {template.version && (
          <span className="rounded-full bg-secondary px-2 py-0.5 text-xs text-muted-foreground">
            v{template.version}
          </span>
        )}
        {skills.slice(0, 3).map((skill) => (
          <span
            key={skill}
            className="max-w-full truncate rounded-full bg-accent px-2 py-0.5 text-xs text-accent-foreground"
          >
            {skill}
          </span>
        ))}
      </div>
    </button>
  )
}

// ─── HomePage ─────────────────────────────────────────────────────────────────

export function EmployeesPage() {
  const { t, i18n } = useTranslation()
  const greetingText = useGreeting()
  const timeLabel = useTimeLabel()
  const setRoute = useUiStore((s) => s.setRoute)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)
  const pushNotification = useNotificationStore((s) => s.push)
  const { employees, activeRuns, refresh: refreshEmp } = useEmployees()
  const { entries, refresh: refreshInbox, markRead } = useInbox()
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)

  const busyTemplateRef = useRef<string | null>(null)
  const [catalog, setCatalog] = useState<EmployeeTemplate[]>([])
  const [catalogCategories, setCatalogCategories] = useState<EmployeeCatalogCategory[]>([])
  const [catalogLoading, setCatalogLoading] = useState(false)
  const [catalogLoadError, setCatalogLoadError] = useState<string | null>(null)
  const [syncingCatalog, setSyncingCatalog] = useState(false)
  const [busyTemplateId, setBusyTemplateId] = useState<string | null>(null)
  const [selectedTemplate, setSelectedTemplate] = useState<EmployeeTemplate | null>(null)
  const [activeCatalogGroupKey, setActiveCatalogGroupKey] = useState(ALL_CATALOG_GROUP_KEY)

  const handleRefreshAll = async () => {
    await Promise.all([refreshEmp(), refreshInbox()])
  }

  const loadCatalog = useCallback(async (): Promise<EmployeeTemplateCatalogResult> => {
    setCatalogLoading(true)
    setCatalogLoadError(null)
    try {
      const result = await loadEmployeeTemplateCatalog(i18n.language)
      setCatalog(result.catalog)
      setCatalogCategories(result.categories)
      setCatalogLoadError(
        result.error ? (result.error instanceof Error ? result.error.message : String(result.error)) : null,
      )
      return result
    } catch (err) {
      console.error('[EmployeesPage] employee catalog load failed:', err)
      setCatalog([])
      setCatalogCategories([])
      setCatalogLoadError(err instanceof Error ? err.message : String(err))
      throw err
    } finally {
      setCatalogLoading(false)
    }
  }, [i18n.language])

  useEffect(() => {
    if (!isLoggedIn) return
    let cancelled = false
    void (async () => {
      try {
        await loadCatalog()
        if (!cancelled) {
          await refreshEmp()
        }
      } catch (err) {
        console.warn('[EmployeesPage] automatic employee catalog sync failed:', err)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [isLoggedIn, loadCatalog, refreshEmp])

  const todayEntries = entries.filter((e) => {
    const d = new Date(e.createdAt)
    const now = new Date()
    return (
      d.getFullYear() === now.getFullYear() &&
      d.getMonth() === now.getMonth() &&
      d.getDate() === now.getDate()
    )
  })

  // PR-7: `archived` is a legacy lifecycle from when delete was a 7-day
  // soft-delete with restore. The recycle bin UI was removed; any
  // remaining `archived` records (from upgrades) are simply hidden — they
  // can't be restored (no UI affordance) and the backend hard-deletes
  // from the next `employee_delete` call onwards.
  const activeEmployees = employees.filter((e) => e.lifecycle !== 'archived')
  const archivedIds = new Set(
    employees.filter((e) => e.lifecycle === 'archived').map((e) => e.id),
  )

  // Count distinct *active* employees that the backend says are mid-dispatch.
  // Previous logic counted "running" inbox entries which (a) double-counted on
  // multi-step runs and (b) included archived employees with stale entries.
  const runningCount = Object.keys(activeRuns).filter((id) => !archivedIds.has(id)).length
  // Reports / signals from archived employees are now filtered server-side
  // (inbox::list_all skips archived dirs) but defend in depth here too.
  const reportCount = entries.filter(
    (e) => e.kind === 'report' && !archivedIds.has(e.employeeId),
  ).length
  const catalogGroups = groupEmployeeCatalog(catalog, catalogCategories)
  const visibleCatalogGroups = useMemo(
    () => catalogGroups.filter((group) => !!group.category),
    [catalogGroups],
  )
  const activeCatalogGroup = activeCatalogGroupKey === ALL_CATALOG_GROUP_KEY
    ? null
    : catalogGroups.find((group) => group.key === activeCatalogGroupKey) ?? null
  const visibleCatalogTemplates = activeCatalogGroup?.templates ?? catalog
  const catalogCategoryItems = useMemo(
    () => [
      { key: ALL_CATALOG_GROUP_KEY, label: t('employeesPage.allCategory') },
      ...visibleCatalogGroups.map((group) => ({
        key: group.key,
        label: group.category?.name ?? group.key,
      })),
    ],
    [t, visibleCatalogGroups],
  )

  useEffect(() => {
    if (activeCatalogGroupKey === ALL_CATALOG_GROUP_KEY) return
    if (!catalogGroups.some((group) => group.key === activeCatalogGroupKey)) {
      setActiveCatalogGroupKey(ALL_CATALOG_GROUP_KEY)
    }
  }, [activeCatalogGroupKey, catalogGroups])

  const handleSyncCatalog = async () => {
    if (syncingCatalog) return
    setSyncingCatalog(true)
    try {
      await loadCatalog()
      await refreshEmp()
      pushNotification({
        level: 'success',
        title: t('employeesPage.refreshDone'),
        message: '',
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: t('employeesPage.refreshFailed'),
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setSyncingCatalog(false)
    }
  }

  const ensureEmployeeForTemplate = async (template: EmployeeTemplate): Promise<EmployeeRecord> => {
    const existing = activeEmployees.find((emp) => emp.templateId === template.templateId)
    if (existing) return existing
    const visual = getEmployeeVisual(template)
    return employeeCreate({
      name: visual.name,
      role: template.role,
      description: template.description,
      avatar: template.avatar,
      templateId: template.templateId,
      toolWhitelist: template.toolWhitelist,
      cron: template.cron ?? undefined,
      timezone: 'Asia/Shanghai',
      lifecycle: 'active',
      cronEnabled: false,
      systemPromptExtra: template.systemPromptExtra,
      defaultSkillId: template.defaultSkillId ?? undefined,
      resourceConfig: {},
    })
  }

  const handleStartEmployee = async (template: EmployeeTemplate) => {
    if (busyTemplateRef.current) return
    busyTemplateRef.current = template.templateId
    setBusyTemplateId(template.templateId)
    try {
      const existing = activeEmployees.find((emp) => emp.templateId === template.templateId)
      const running = existing ? activeRuns[existing.id] : null
      if (running) {
        setSelectedTemplate(null)
        setSidebarTab('employee')
        setRoute({ kind: 'chat', conversationId: running.conversationId })
        return
      }

      const employee = await ensureEmployeeForTemplate(template)
      const convId = await employeeTrigger(employee.id, undefined, [])
      const chatStore = useChatStore.getState()
      const nextConversations = seedDispatchConversation(
        chatStore.conversations,
        convId,
        employee.name,
      )
      if (nextConversations !== chatStore.conversations) {
        chatStore.setConversations(nextConversations)
      }
      chatStore.setMessages([])
      await handleRefreshAll()
      setSelectedTemplate(null)
      setSidebarTab('employee')
      setRoute({ kind: 'chat', conversationId: convId })
    } catch (err) {
      console.error('[EmployeesPage] start employee failed:', err)
      pushNotification({
        level: 'error',
        title: t('employeesPage.startFailed'),
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      busyTemplateRef.current = null
      setBusyTemplateId(null)
    }
  }

  return (
    <PageSectionShell
      topBar={(
        <PageTopBar
          variant="title"
          title={t('nav.employees')}
          trailing={(
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="gap-1.5"
              disabled={syncingCatalog || catalogLoading}
              onClick={() => void handleSyncCatalog()}
            >
              <RefreshCw className={`h-3 w-3 ${syncingCatalog || catalogLoading ? 'animate-spin' : ''}`} />
              {syncingCatalog || catalogLoading ? t('employeesPage.syncing') : t('employeesPage.syncServer')}
            </Button>
          )}
        />
      )}
    >
      {/* ── 顶部 greeting ── */}
      <div>
        <h1 className="text-[22px] font-bold leading-7 text-foreground">{greetingText}{t('employeesPage.greetingToday')}</h1>
        <p className="mt-1 text-sm leading-6 text-muted-foreground">
          {runningCount > 0 ? `${t('employeesPage.workingCount', { count: runningCount })} · ` : ''}
          {reportCount > 0 ? t('employeesPage.reportCount', { count: reportCount }) : t('employeesPage.allIdle')}
        </p>
      </div>

      {/* ── 服务端员工目录 ── */}
      <section>
        {catalogLoadError && catalog.length > 0 && (
          <p className="mb-3 text-xs text-muted-foreground">
            {t('employeesPage.directoryPartialError', { err: catalogLoadError })}
          </p>
        )}

        {catalogLoading && catalog.length === 0 ? (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {[...Array(6)].map((_, i) => (
              <div key={i} className="h-[212px] animate-pulse rounded-md border border-border bg-card" />
            ))}
          </div>
        ) : catalog.length === 0 ? (
          <div
            className="flex min-h-[180px] flex-col items-center justify-center gap-3 rounded-md border border-dashed border-border bg-card px-4 text-center shadow-[var(--shadow-card)]"
            data-aijia-employee-directory-empty
          >
            <p className="text-sm text-muted-foreground">
              {catalogLoadError
                ? t('employeesPage.directoryLoadError', { err: catalogLoadError })
                : t('employeesPage.directoryEmpty')}
            </p>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="gap-1.5"
              disabled={syncingCatalog}
              onClick={() => void handleSyncCatalog()}
            >
              <RefreshCw className={`h-3.5 w-3.5 ${syncingCatalog ? 'animate-spin' : ''}`} />
              {syncingCatalog ? t('employeesPage.syncing') : t('employeesPage.syncServer')}
            </Button>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <SkillCategoryBar
              items={catalogCategoryItems}
              activeKey={activeCatalogGroupKey}
              onSelect={setActiveCatalogGroupKey}
            />
            {activeCatalogGroup?.category && categoryDescription(activeCatalogGroup.category) && (
              <p className="text-xs text-muted-foreground">
                {categoryDescription(activeCatalogGroup.category)}
              </p>
            )}
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              {visibleCatalogTemplates.map((template) => (
                <EmployeeDirectoryCard
                  key={`${template.templateId}:${template.version ?? ''}`}
                  template={template}
                  busy={busyTemplateId === template.templateId}
                  onOpen={setSelectedTemplate}
                />
              ))}
            </div>
          </div>
        )}
      </section>

      <EmployeeTemplateDetailDialog
        template={selectedTemplate}
        open={!!selectedTemplate}
        onOpenChange={(open) => {
          if (!open) setSelectedTemplate(null)
        }}
        existingEmployee={
          selectedTemplate
            ? activeEmployees.find((emp) => emp.templateId === selectedTemplate.templateId) ?? null
            : null
        }
        runningConversationId={
          selectedTemplate
            ? (() => {
                const existing = activeEmployees.find((emp) => emp.templateId === selectedTemplate.templateId)
                return existing ? activeRuns[existing.id]?.conversationId ?? null : null
              })()
            : null
        }
        busy={!!selectedTemplate && busyTemplateId === selectedTemplate.templateId}
        onStart={handleStartEmployee}
      />

      {/* ── 今日动态 ── */}
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-foreground">{t('employeesPage.todayFeed')}</h2>
          {entries.length > todayEntries.length && (
            <button
              type="button"
              className="rounded-[var(--radius)] px-2 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
              onClick={() => setRoute({ kind: 'inbox' })}
            >
              {t('employeesPage.viewAll')}
            </button>
          )}
        </div>

        {todayEntries.length === 0 ? (
          <div className="flex h-[120px] items-center justify-center rounded-md border border-dashed border-border bg-card">
            <p className="text-sm text-muted-foreground">{t('employeesPage.noFeedToday')}</p>
          </div>
        ) : (
          <div className="flex flex-col divide-y divide-border overflow-hidden rounded-md border border-border bg-card shadow-[var(--shadow-card)]">
            {todayEntries.slice(0, 8).map((entry) => {
              const emp = employees.find((e) => e.id === entry.employeeId)
              const clickable = !!entry.conversationId
              const handleClick = () => {
                if (!entry.read) {
                  void markRead(entry.employeeId, entry.id)
                }
                if (entry.conversationId) {
                  setSidebarTab('employee')
                  setRoute({ kind: 'chat', conversationId: entry.conversationId })
                }
              }
              return (
                <button
                  key={entry.id}
                  type="button"
                  onClick={handleClick}
                  disabled={!clickable}
                  className="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-muted disabled:cursor-default disabled:hover:bg-transparent"
                >
                  <span className="mt-0.5 text-base">{kindIcon(entry.kind)}</span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline gap-2">
                      {emp && (
                        <span className="shrink-0 text-xs font-medium text-muted-foreground">
                          {emp.avatar} {emp.name}
                        </span>
                      )}
                      <span className="truncate text-sm text-foreground">{entry.title}</span>
                    </div>
                    {entry.summary && (
                      <p className="mt-0.5 truncate text-xs text-muted-foreground">{entry.summary}</p>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <span className="text-xs text-muted-foreground/60">{timeLabel(entry.createdAt)}</span>
                    {!entry.read && (
                      <span className="h-1.5 w-1.5 rounded-full bg-blue-500" />
                    )}
                  </div>
                </button>
              )
            })}
          </div>
        )}
      </section>

    </PageSectionShell>
  )
}
