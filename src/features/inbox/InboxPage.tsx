import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckCheck } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { Button } from '@/components/ui/button'
import { useEmployees } from '@/features/employees/useEmployees'
import { useInbox } from '@/features/employees/useInbox'
import { useUiStore } from '@/stores/uiStore'
import type { InboxKind } from '@/lib/tauri'

type KindFilter = 'all' | InboxKind

const KIND_TAB_KEYS: { key: KindFilter; i18nKey: string }[] = [
  { key: 'all', i18nKey: 'inbox.filterAll' },
  { key: 'report', i18nKey: 'inbox.filterReport' },
  { key: 'signal', i18nKey: 'inbox.filterSignal' },
  { key: 'error', i18nKey: 'inbox.filterError' },
]

function kindIcon(kind: InboxKind): string {
  switch (kind) {
    case 'report': return '📄'
    case 'signal': return '💡'
    case 'running': return '⚙️'
    case 'error': return '⚠️'
  }
}

function useInboxTimeLabel() {
  const { t, i18n } = useTranslation()
  return (iso: string): string => {
    const d = new Date(iso)
    const now = new Date()
    const diffMs = now.getTime() - d.getTime()
    const diffMin = Math.floor(diffMs / 60_000)
    if (diffMin < 1) return t('employeesPage.timeLabel.justNow')
    if (diffMin < 60) return t('employeesPage.timeLabel.minutesAgo', { count: diffMin })
    const diffH = Math.floor(diffMin / 60)
    const timeStr = d.toLocaleTimeString(i18n.language, { hour: '2-digit', minute: '2-digit' })
    if (diffH < 24) return t('inbox.timeToday', { time: timeStr })
    if (diffH < 48) return t('inbox.timeYesterday', { time: timeStr })
    return d.toLocaleDateString(i18n.language, { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' })
  }
}

export function InboxPage() {
  const { t } = useTranslation()
  const timeLabel = useInboxTimeLabel()
  const { employees } = useEmployees()
  const { entries, markAllRead, markRead } = useInbox()
  const setRoute = useUiStore((s) => s.setRoute)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)

  const [kindFilter, setKindFilter] = useState<KindFilter>('all')
  const [empFilter, setEmpFilter] = useState<string>('all')
  const [markingAll, setMarkingAll] = useState(false)

  const filtered = entries.filter((e) => {
    if (kindFilter !== 'all' && e.kind !== kindFilter) return false
    if (empFilter !== 'all' && e.employeeId !== empFilter) return false
    return true
  })

  const unread = filtered.filter((e) => !e.read).length

  const handleMarkAll = async () => {
    setMarkingAll(true)
    try {
      // Mark only the currently visible (filtered) entries' employees, not
      // every employee that has ever produced an inbox entry. The button label
      // is "全部已读" within the current filter scope (e.g. when viewing one
      // employee, only their inbox should be touched).
      const ids = [...new Set(filtered.map((e) => e.employeeId))]
      await Promise.all(ids.map((id) => markAllRead(id)))
    } finally {
      setMarkingAll(false)
    }
  }

  return (
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="title"
          title={t('inbox.title')}
          trailing={
            <Button
              variant="ghost"
              size="sm"
              className="gap-1.5"
              disabled={markingAll || unread === 0}
              onClick={handleMarkAll}
            >
              <CheckCheck className="h-3.5 w-3.5" />
              {t('inbox.markAllRead')}
            </Button>
          }
        />
      }
    >
      {/* Filter bar */}
      <div className="flex items-center gap-3">
        {/* Kind tabs */}
        <div className="flex items-center gap-1 rounded-md border border-border bg-card p-1">
          {KIND_TAB_KEYS.map((tab) => (
            <button
              key={tab.key}
              type="button"
              onClick={() => setKindFilter(tab.key)}
              className={`h-8 rounded-[var(--radius)] px-3 text-xs font-medium transition-colors ${
                kindFilter === tab.key
                  ? 'bg-brand-primary-subtle text-primary'
                  : 'text-muted-foreground hover:bg-muted hover:text-foreground'
              }`}
            >
              {t(tab.i18nKey)}
            </button>
          ))}
        </div>

        {/* Employee filter */}
        <select
          value={empFilter}
          onChange={(e) => setEmpFilter(e.target.value)}
          className="h-9 rounded-[var(--radius)] border border-input bg-card px-2.5 text-xs text-foreground"
        >
          <option value="all">{t('inbox.allEmployees')}</option>
          {employees
            .filter((emp) => emp.lifecycle !== 'archived')
            .map((emp) => (
              <option key={emp.id} value={emp.id}>
                {emp.avatar} {emp.name}
              </option>
            ))}
        </select>

        {unread > 0 && (
          <span className="rounded-full bg-brand-primary-subtle px-2 py-0.5 text-xs font-medium text-primary">
            {t('inbox.unreadCount', { count: unread })}
          </span>
        )}
      </div>

      {/* Entry list */}
      {filtered.length === 0 ? (
        <div className="flex h-[240px] items-center justify-center rounded-md border border-dashed border-border bg-card shadow-[var(--shadow-card)]">
          <p className="text-sm text-muted-foreground">{t('inbox.noRecords')}</p>
        </div>
      ) : (
        <div className="flex flex-col divide-y divide-border overflow-hidden rounded-md border border-border bg-card shadow-[var(--shadow-card)]">
          {filtered.map((entry) => {
            const emp = employees.find((e) => e.id === entry.employeeId)
            const clickable = !!entry.conversationId
            const handleClick = async () => {
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
                className={`flex w-full items-start gap-3 px-5 py-4 text-left transition-colors hover:bg-muted disabled:cursor-default disabled:hover:bg-transparent ${
                  !entry.read ? 'bg-brand-primary-subtle/45' : ''
                }`}
              >
                <span className="mt-0.5 text-lg">{kindIcon(entry.kind)}</span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-2">
                    {emp && (
                      <span className="shrink-0 text-xs font-medium text-muted-foreground">
                        {emp.avatar} {emp.name}
                      </span>
                    )}
                    <span className="text-sm font-medium text-foreground">{entry.title}</span>
                  </div>
                  {entry.summary && (
                    <p className="mt-0.5 text-xs text-muted-foreground line-clamp-2">{entry.summary}</p>
                  )}
                  {entry.catchupInfo && (
                    <p className="mt-0.5 text-xs italic text-muted-foreground/60">{entry.catchupInfo}</p>
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
    </PageSectionShell>
  )
}
