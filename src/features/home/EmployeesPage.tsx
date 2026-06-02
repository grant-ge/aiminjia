import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { useUiStore } from '@/stores/uiStore'
import { useEmployees } from '@/features/employees/useEmployees'
import { useInbox } from '@/features/employees/useInbox'
import { EmployeeCard, AddEmployeeCard } from '@/features/employees/EmployeeCard'
import { EmployeeDrawer } from '@/features/employees/EmployeeDrawer'
import { HireWizard } from '@/features/employees/HireWizard'
import { employeeTemplateRefresh, type EmployeeRecord } from '@/lib/tauri'
import { useAuthStore } from '@/stores/authStore'

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

// ─── HomePage ─────────────────────────────────────────────────────────────────

export function EmployeesPage() {
  const { t } = useTranslation()
  const greetingText = useGreeting()
  const timeLabel = useTimeLabel()
  const setRoute = useUiStore((s) => s.setRoute)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)
  const { employees, activeRuns, loading: empLoading, refresh: refreshEmp } = useEmployees()
  const { entries, refresh: refreshInbox, markRead } = useInbox()
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)

  const [selectedEmp, setSelectedEmp] = useState<EmployeeRecord | null>(null)
  const [hireOpen, setHireOpen] = useState(false)

  const handleRefreshAll = async () => {
    await Promise.all([refreshEmp(), refreshInbox()])
  }

  useEffect(() => {
    if (!isLoggedIn) return
    let cancelled = false
    void (async () => {
      try {
        await employeeTemplateRefresh()
        if (!cancelled) {
          await refreshEmp()
        }
      } catch (err) {
        console.warn('[EmployeesPage] automatic employee template sync failed:', err)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [isLoggedIn, refreshEmp])

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

  return (
    <PageSectionShell
      topBar={<div data-tauri-drag-region className="h-10 shrink-0" />}
      maxWidthClass="max-w-[900px]"
    >
      {/* ── 顶部 greeting ── */}
      <div>
        <h1 className="text-xl font-bold text-foreground">{greetingText}{t('employeesPage.greetingToday')}</h1>
        <p className="mt-0.5 text-sm text-muted-foreground">
          {runningCount > 0 ? `${t('employeesPage.workingCount', { count: runningCount })} · ` : ''}
          {reportCount > 0 ? t('employeesPage.reportCount', { count: reportCount }) : t('employeesPage.allIdle')}
        </p>
      </div>

      {/* ── 员工卡片栏 ── */}
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-foreground">{t('employeesPage.myEmployees')}</h2>
          <div className="flex items-center gap-2">
            <button
              type="button"
              data-aijia-hire-button="template-market"
              className="text-xs text-muted-foreground hover:text-foreground"
              onClick={() => setHireOpen(true)}
            >
              {t('employeesPage.templateMarket')}
            </button>
          </div>
        </div>

        {empLoading ? (
          <div className="grid grid-cols-3 gap-3">
            {[...Array(3)].map((_, i) => (
              <div key={i} className="h-[152px] animate-pulse rounded-xl bg-muted/60" />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-3 gap-3">
            {activeEmployees.map((emp) => (
              <EmployeeCard
                key={emp.id}
                employee={emp}
                inboxEntries={entries}
                activeRun={activeRuns[emp.id] ?? null}
                onClick={() => setSelectedEmp(emp)}
                onRefresh={handleRefreshAll}
              />
            ))}
            <AddEmployeeCard onClick={() => setHireOpen(true)} />
          </div>
        )}
      </section>

      {/* ── 今日动态 ── */}
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-foreground">{t('employeesPage.todayFeed')}</h2>
          {entries.length > todayEntries.length && (
            <button
              type="button"
              className="text-xs text-muted-foreground hover:text-foreground"
              onClick={() => setRoute({ kind: 'inbox' })}
            >
              {t('employeesPage.viewAll')}
            </button>
          )}
        </div>

        {todayEntries.length === 0 ? (
          <div className="flex h-[120px] items-center justify-center rounded-xl border border-dashed border-border">
            <p className="text-sm text-muted-foreground">{t('employeesPage.noFeedToday')}</p>
          </div>
        ) : (
          <div className="flex flex-col divide-y divide-border overflow-hidden rounded-xl border border-border">
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
                  className="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-accent/30 disabled:cursor-default disabled:hover:bg-transparent"
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

      {/* ── Drawer ── */}
      <EmployeeDrawer
        employee={selectedEmp}
        inboxEntries={entries}
        activeRun={selectedEmp ? activeRuns[selectedEmp.id] ?? null : null}
        onClose={() => setSelectedEmp(null)}
        onRefresh={handleRefreshAll}
      />

      {/* ── HireWizard ── */}
      <HireWizard
        open={hireOpen}
        onClose={() => setHireOpen(false)}
        onHired={handleRefreshAll}
      />
    </PageSectionShell>
  )
}
