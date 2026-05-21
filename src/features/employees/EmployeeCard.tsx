import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Play, Pause, ChevronRight } from 'lucide-react'
import { cn } from '@/lib/utils'
import {
  employeeUpdate,
  type EmployeeActiveRunInfo,
  type EmployeeRecord,
  type InboxEntry,
} from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { findTemplate } from './templates'
import { formatRelativeNextRun } from './timeFormat'

// ─── status derivation ───────────────────────────────────────────────────────

export type EmployeeStatus =
  | 'running'
  | 'has-report'
  | 'needs-setup'
  | 'idle'

/**
 * Drive the card's primary visual state from a small, mutually-exclusive
 * state machine. PR-8 narrowed this from 7 states (archived / paused /
 * scheduled / idle / needs-setup / running / has-report) to 4 because
 * users could not reliably tell `scheduled` from `idle` (cron info is
 * surfaced inline on the card anyway), and `archived` employees are
 * filtered out of the grid by `EmployeesPage` so they never reach this
 * function in normal flow.
 *
 * Priority (top wins):
 *   1. running   — backend truth (`activeRun` is set)
 *   2. has-report — unread Report or Signal (informational badge)
 *   3. needs-setup — template requires resource_config that's missing
 *   4. idle       — default (covers "scheduled" / "ready to dispatch")
 */
export function deriveStatus(
  emp: EmployeeRecord,
  inboxEntries: InboxEntry[],
  activeRun: EmployeeActiveRunInfo | null = null,
): EmployeeStatus {
  // 1. Running — backend truth (replaces 10-min inbox heuristic)
  if (activeRun) return 'running'

  // 2. Has unread report/signal
  const empEntries = inboxEntries.filter((e) => e.employeeId === emp.id)
  const hasUnread = empEntries.some(
    (e) => !e.read && (e.kind === 'report' || e.kind === 'signal'),
  )
  if (hasUnread) return 'has-report'

  // 3. Needs setup — template-aware resource check
  const template = findTemplate(emp.templateId)
  if (template) {
    if (template.resourceConfigKind === 'sales-table') {
      const cfg = emp.resourceConfig as { baseId?: unknown; tableId?: unknown } | null
      const baseId = cfg?.baseId
      const tableId = cfg?.tableId
      const configured =
        typeof baseId === 'string' &&
        baseId.length > 0 &&
        typeof tableId === 'string' &&
        tableId.length > 0
      if (!configured) return 'needs-setup'
    }
    if (template.resourceConfigKind === 'monitoring-urls') {
      const cfg = emp.resourceConfig as { monitoringTargets?: unknown[] } | null
      if (
        !cfg ||
        !Array.isArray(cfg.monitoringTargets) ||
        cfg.monitoringTargets.length === 0
      ) {
        return 'needs-setup'
      }
    }
  }

  // 4. Idle — default (includes "scheduled / cron-armed", which is shown
  //    via the cron chip inline on the card)
  return 'idle'
}

// ─── status badge ─────────────────────────────────────────────────────────────

function StatusDot({ status }: { status: EmployeeStatus }) {
  const base = 'h-2 w-2 rounded-full shrink-0'
  switch (status) {
    case 'running':
      return <span className={cn(base, 'bg-blue-500 animate-pulse')} />
    case 'has-report':
      return <span className={cn(base, 'bg-green-500')} />
    case 'needs-setup':
      return <span className={cn(base, 'bg-orange-400')} />
    default:
      return <span className={cn(base, 'bg-muted-foreground/30')} />
  }
}

const STATUS_I18N_KEY: Record<EmployeeStatus, string> = {
  running: 'employeeCard.running',
  'has-report': 'employeeCard.hasReport',
  'needs-setup': 'employeeCard.needsSetup',
  idle: 'employeeCard.idle',
}

// ─── card ─────────────────────────────────────────────────────────────────────

interface EmployeeCardProps {
  employee: EmployeeRecord
  inboxEntries: InboxEntry[]
  activeRun?: EmployeeActiveRunInfo | null
  onClick: () => void
  onRefresh: () => Promise<void>
}

export function EmployeeCard({ employee: emp, inboxEntries, activeRun = null, onClick, onRefresh }: EmployeeCardProps) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const status = deriveStatus(emp, inboxEntries, activeRun)

  const handleTogglePause = async (e: React.MouseEvent) => {
    e.stopPropagation()
    setBusy(true)
    try {
      await employeeUpdate(emp.id, { cronEnabled: !emp.cronEnabled })
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeCard] toggle error:', err)
    } finally {
      setBusy(false)
    }
  }

  return (
    <button
      type="button"
      data-aijia-employee-card
      data-aijia-employee-id={emp.id}
      data-aijia-employee-name={emp.name}
      data-aijia-employee-status={status}
      data-aijia-employee-cron-enabled={emp.cron ? (emp.cronEnabled ? 'true' : 'false') : 'none'}
      data-aijia-employee-dispatch-disabled={emp.lifecycle === 'archived' ? 'true' : 'false'}
      onClick={onClick}
      className={cn(
        'group relative flex w-full flex-col gap-3 rounded-xl border bg-card p-4 text-left transition-all hover:border-border/80 hover:shadow-sm border-border',
        status === 'has-report' && 'border-green-200 bg-green-50/30',
        status === 'running' && 'border-blue-200 bg-blue-50/20',
        status === 'needs-setup' && 'border-orange-200',
      )}
    >
      {/* Header */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2.5">
          <span className="text-2xl leading-none">{emp.avatar}</span>
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <span className="text-sm font-semibold text-foreground">{emp.name}</span>
              <StatusDot status={status} />
            </div>
            <p className="truncate text-xs text-muted-foreground">{emp.role}</p>
          </div>
        </div>
        <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground/50 transition-transform group-hover:translate-x-0.5" />
      </div>

      {/* Description */}
      <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">
        {emp.description}
      </p>

      {/* Footer */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex flex-col">
          <span className={cn(
            'text-xs',
            status === 'has-report' && 'font-medium text-green-600',
            status === 'running' && 'font-medium text-blue-500',
            status === 'needs-setup' && 'font-medium text-orange-500',
            !['has-report', 'running', 'needs-setup'].includes(status) && 'text-muted-foreground',
          )}>
            {t(STATUS_I18N_KEY[status])}
          </span>
          {/* Cron next-run hint is shown for any idle employee with an
              armed cron — replaces the dedicated `scheduled` status
              dropped in PR-8. */}
          {status === 'idle' && emp.cron && emp.cronEnabled && emp.nextRunAt && (
            <span className="text-xs text-muted-foreground/80">
              {formatRelativeNextRun(emp.nextRunAt)
                ? t('employeeCard.nextRun', { time: formatRelativeNextRun(emp.nextRunAt) })
                : t('employeeCard.nextRunSoon')}
            </span>
          )}
        </div>

        <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
          {emp.cron && (
            <Button
              variant="ghost"
              size="icon"
              data-aijia-employee-action={emp.cronEnabled ? 'pause-cron' : 'resume-cron'}
              className="h-6 w-6"
              disabled={busy}
              onClick={handleTogglePause}
              title={emp.cronEnabled ? t('employeeCard.pause') : t('employeeCard.resume')}
            >
              {emp.cronEnabled
                ? <Pause className="h-3 w-3" />
                : <Play className="h-3 w-3" />
              }
            </Button>
          )}
        </div>
      </div>
    </button>
  )
}

// ─── add card ────────────────────────────────────────────────────────────────

interface AddEmployeeCardProps {
  onClick: () => void
}

export function AddEmployeeCard({ onClick }: AddEmployeeCardProps) {
  const { t } = useTranslation()
  return (
    <button
      type="button"
      data-aijia-hire-button="add-card"
      onClick={onClick}
      className="flex w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border bg-card/50 p-4 text-center transition-colors hover:border-border/80 hover:bg-card"
      style={{ minHeight: 152 }}
    >
      <span className="text-2xl text-muted-foreground/50">＋</span>
      <span className="text-xs text-muted-foreground">{t('employeeCard.hireNew')}</span>
    </button>
  )
}
