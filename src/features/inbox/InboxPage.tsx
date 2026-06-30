import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  CheckCheck,
} from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { Button } from '@/components/ui/button'
import {
  employeeInitial,
  getLocalEmployeeAvatarUrl,
} from '@/features/employees/employeeVisual'
import { localizeEmployeeDisplay } from '@/features/employees/templates'
import { useEmployees } from '@/features/employees/useEmployees'
import { useInbox } from '@/features/employees/useInbox'
import { useUiStore } from '@/stores/uiStore'
import type { EmployeeRecord, InboxKind } from '@/lib/tauri'
import { cn } from '@/lib/utils'

type KindFilter = 'all' | InboxKind

const KIND_TAB_KEYS: { key: KindFilter; i18nKey: string }[] = [
  { key: 'all', i18nKey: 'inbox.filterAll' },
  { key: 'report', i18nKey: 'inbox.filterReport' },
  { key: 'signal', i18nKey: 'inbox.filterSignal' },
  { key: 'error', i18nKey: 'inbox.filterError' },
]

function EmployeeInboxAvatar({ name }: { name: string }) {
  const avatarUrl = getLocalEmployeeAvatarUrl(name)
  return (
    <span className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-md bg-muted text-sm font-semibold leading-none text-muted-foreground shadow-[var(--shadow-sm)]">
      {avatarUrl ? (
        <img
          alt=""
          className="h-full w-full object-cover"
          draggable={false}
          src={avatarUrl}
        />
      ) : (
        employeeInitial(name)
      )}
    </span>
  )
}

function formatInboxTitleParts(
  title: string,
  employee: EmployeeRecord | undefined,
  language: string,
): { identity: string | null; status: string } {
  if (!employee) return { identity: null, status: title }
  const display = localizeEmployeeDisplay(
    employee.templateId,
    {
      name: employee.name,
      role: employee.role ?? '',
      description: employee.description ?? '',
    },
    language,
  )
  const role = display.role?.trim()
  const name = display.name.trim()
  const identity = role ? `${role} · ${name}` : name
  const spacedPrefix = role ? `${role} ${name}` : name
  const normalizedTitle = title.trim()
  let status = normalizedTitle
  if (normalizedTitle.startsWith(spacedPrefix)) {
    status = normalizedTitle.slice(spacedPrefix.length).trimStart()
  } else if (normalizedTitle.startsWith(identity)) {
    status = normalizedTitle.slice(identity.length).trimStart()
  } else if (normalizedTitle.startsWith(name)) {
    status = normalizedTitle.slice(name.length).trimStart()
  }
  return { identity, status: status || normalizedTitle }
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
  const { t, i18n } = useTranslation()
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
            <div className="flex items-center gap-2">
              {unread > 0 && (
                <span className="rounded-md bg-[rgba(var(--primary-rgb),0.10)] px-2 py-0.5 text-xs font-medium text-primary shadow-[inset_0_0_0_1px_rgba(var(--primary-rgb),0.10)]">
                  {t('inbox.unreadCount', { count: unread })}
                </span>
              )}
              <Button
                variant="ghost"
                size="sm"
                icon={<CheckCheck className="h-3.5 w-3.5" />}
                disabled={markingAll || unread === 0}
                onClick={handleMarkAll}
              >
                {t('inbox.markAllRead')}
              </Button>
            </div>
          }
        />
      }
    >
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex min-w-0 items-center gap-2">
          {KIND_TAB_KEYS.map((tab) => (
            <Button unstyled
              key={tab.key}
              type="button"
              onClick={() => setKindFilter(tab.key)}
              className={cn(
                'h-8 max-w-[120px] shrink-0 truncate rounded-md px-3 text-sm font-semibold transition-colors',
                kindFilter === tab.key
                  ? 'bg-[rgba(var(--primary-rgb),0.10)] text-primary shadow-[inset_0_0_0_1px_rgba(var(--primary-rgb),0.12)]'
                  : 'text-[rgba(var(--muted-foreground-rgb),0.80)] hover:bg-[rgba(var(--muted-rgb),0.45)] hover:text-foreground',
              )}
            >
              {t(tab.i18nKey)}
            </Button>
          ))}
        </div>

        <div className="ml-auto flex items-center gap-3">
          <select
            value={empFilter}
            onChange={(e) => setEmpFilter(e.target.value)}
            className="h-8 rounded-md border border-[rgba(var(--border-rgb),0.70)] bg-card px-2.5 text-sm font-medium text-foreground shadow-[var(--shadow-sm)] outline-none transition-colors hover:border-border focus:border-primary"
          >
            <option value="all">{t('inbox.allEmployees')}</option>
            {employees
              .filter((emp) => emp.lifecycle !== 'archived')
              .map((emp) => (
                <option key={emp.id} value={emp.id}>
                  {emp.name}
                </option>
              ))}
          </select>
        </div>
      </div>

      {/* Entry list */}
      {filtered.length === 0 ? (
        <div className="flex h-[240px] items-center justify-center rounded-md border border-dashed border-[rgba(var(--border-rgb),0.70)] bg-card shadow-[var(--shadow-card)]">
          <p className="text-sm text-muted-foreground">{t('inbox.noRecords')}</p>
        </div>
      ) : (
        <div className="flex flex-col divide-y divide-[rgba(var(--border-rgb),0.60)] overflow-hidden rounded-md border border-[rgba(var(--border-rgb),0.70)] bg-card shadow-[var(--shadow-card)]">
          {filtered.map((entry) => {
            const emp = employees.find((e) => e.id === entry.employeeId)
            const title = formatInboxTitleParts(entry.title, emp, i18n.language)
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
              <Button unstyled
                key={entry.id}
                type="button"
                aria-label={title.identity ? `${title.identity} ${title.status}` : title.status}
                onClick={handleClick}
                disabled={!clickable}
                className={cn(
                  'flex w-full items-start gap-3 px-5 py-4 text-left transition-colors hover:bg-[rgba(var(--muted-rgb),0.40)] disabled:cursor-default disabled:hover:bg-transparent',
                  !entry.read && 'bg-[rgba(var(--primary-rgb),0.055)]',
                )}
              >
                <EmployeeInboxAvatar name={emp?.name ?? entry.employeeId} />
                <div className="min-w-0 flex-1">
                  <div className="flex min-w-0 items-center gap-4">
                    {title.identity && (
                      <span className="max-w-[42%] shrink-0 truncate text-sm font-semibold text-foreground">
                        {title.identity}
                      </span>
                    )}
                    <span className="min-w-0 truncate text-sm font-semibold text-foreground">{title.status}</span>
                  </div>
                  {entry.summary && (
                    <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">{entry.summary}</p>
                  )}
                  {entry.catchupInfo && (
                    <p className="mt-0.5 text-xs italic text-[rgba(var(--muted-foreground-rgb),0.60)]">{entry.catchupInfo}</p>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <span className="text-xs text-[rgba(var(--muted-foreground-rgb),0.60)]">{timeLabel(entry.createdAt)}</span>
                  {!entry.read && (
                    <span className="h-1.5 w-1.5 rounded-md bg-primary" />
                  )}
                </div>
              </Button>
            )
          })}
        </div>
      )}
    </PageSectionShell>
  )
}
