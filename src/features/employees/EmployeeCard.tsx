import { useState } from 'react'
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
  | 'paused'
  | 'needs-setup'
  | 'scheduled'
  | 'idle'
  | 'archived'

/**
 * Drive the card's primary visual state from a 2-dimensional state machine:
 * - Lifecycle (Active / Paused / Archived): persistent, user-controlled
 * - Activity (running / idle): real-time, derived from activeRun
 *
 * Priority (top wins):
 *   1. archived  — overrides everything for hidden display
 *   2. running   — backend truth, not inbox heuristic
 *   3. has-report — unread Report or Signal (informational badge)
 *   4. paused    — explicit lifecycle pause
 *   5. needs-setup — template requires resource_config that's missing
 *   6. scheduled — has cron + lifecycle=active + cron_enabled + nextRunAt
 *   7. idle      — default
 */
export function deriveStatus(
  emp: EmployeeRecord,
  inboxEntries: InboxEntry[],
  activeRun: EmployeeActiveRunInfo | null = null,
): EmployeeStatus {
  // 1. Archived — hidden in main grid (Task 6); show 🗑 marker if rendered
  if (emp.lifecycle === 'archived') return 'archived'

  // 2. Running — backend truth (replaces 10-min inbox heuristic)
  if (activeRun) return 'running'

  // 3. Has unread report/signal
  const empEntries = inboxEntries.filter((e) => e.employeeId === emp.id)
  const hasUnread = empEntries.some(
    (e) => !e.read && (e.kind === 'report' || e.kind === 'signal'),
  )
  if (hasUnread) return 'has-report'

  // 4. Paused — explicit lifecycle state
  if (emp.lifecycle === 'paused') return 'paused'

  // 5. Needs setup — template-aware resource check
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

  // 6. Scheduled — cron and all gates align
  if (emp.cron && emp.lifecycle === 'active' && emp.cronEnabled && emp.nextRunAt) {
    return 'scheduled'
  }

  // 7. Idle — default
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
    case 'paused':
      return <span className={cn(base, 'bg-slate-400')} />
    case 'needs-setup':
      return <span className={cn(base, 'bg-orange-400')} />
    case 'scheduled':
      return <span className={cn(base, 'bg-amber-400')} />
    case 'archived':
      return <span className={cn(base, 'bg-slate-300')} />
    default:
      return <span className={cn(base, 'bg-muted-foreground/30')} />
  }
}

function statusLabel(status: EmployeeStatus): string {
  switch (status) {
    case 'running': return '运行中'
    case 'has-report': return '有新汇报'
    case 'paused': return '已暂停'
    case 'needs-setup': return '需要配置'
    case 'scheduled': return '定时驻场'
    case 'archived': return '已解雇'
    default: return '空闲'
  }
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
      onClick={onClick}
      className={cn(
        'group relative flex w-full flex-col gap-3 rounded-2xl border bg-card p-4 text-left transition-all hover:border-border/80 hover:shadow-sm border-border',
        status === 'has-report' && 'border-green-200 bg-green-50/30',
        status === 'running' && 'border-blue-200 bg-blue-50/20',
        status === 'needs-setup' && 'border-orange-200',
        status === 'archived' && 'opacity-60',
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
            status === 'paused' && 'text-slate-500',
            status === 'scheduled' && 'text-amber-700',
            !['has-report', 'running', 'needs-setup', 'paused', 'scheduled'].includes(status) && 'text-muted-foreground',
          )}>
            {statusLabel(status)}
          </span>
          {status === 'scheduled' && emp.nextRunAt && (
            <span className="text-xs text-amber-700/80">
              下次：{formatRelativeNextRun(emp.nextRunAt) || '即将'}
            </span>
          )}
        </div>

        <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
          {emp.cron && (
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              disabled={busy}
              onClick={handleTogglePause}
              title={emp.cronEnabled ? '暂停' : '恢复'}
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
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full flex-col items-center justify-center gap-2 rounded-2xl border border-dashed border-border bg-card/50 p-4 text-center transition-colors hover:border-border/80 hover:bg-card"
      style={{ minHeight: 152 }}
    >
      <span className="text-2xl text-muted-foreground/50">＋</span>
      <span className="text-xs text-muted-foreground">雇佣新员工</span>
    </button>
  )
}
