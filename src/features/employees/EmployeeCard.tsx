import { useState } from 'react'
import { Play, Pause, ChevronRight } from 'lucide-react'
import { cn } from '@/lib/utils'
import { employeeTrigger, employeeUpdate, type EmployeeRecord, type InboxEntry } from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { findTemplate } from './templates'

// ─── status derivation ───────────────────────────────────────────────────────

export type EmployeeStatus = 'working' | 'has-report' | 'scheduled' | 'idle' | 'needs-setup'

const DINGTALK_TEMPLATES = new Set(['builtin:xiaoxiao', 'builtin:xiaoding'])

export function deriveStatus(
  emp: EmployeeRecord,
  inboxEntries: InboxEntry[],
): EmployeeStatus {
  const empEntries = inboxEntries.filter((e) => e.employeeId === emp.id)

  // Needs auth: dingtalk templates with empty resource config
  if (
    emp.templateId &&
    DINGTALK_TEMPLATES.has(emp.templateId) &&
    Object.keys(emp.resourceConfig).length === 0
  ) {
    return 'needs-setup'
  }

  // Working: Running entry in the last 10 min
  const tenMinAgo = new Date(Date.now() - 10 * 60 * 1000).toISOString()
  const isRunning = empEntries.some((e) => e.kind === 'running' && e.createdAt > tenMinAgo)
  if (isRunning) return 'working'

  // Has report: unread Report or Signal
  const hasUnread = empEntries.some((e) => !e.read && (e.kind === 'report' || e.kind === 'signal'))
  if (hasUnread) return 'has-report'

  // Template-aware needs-setup check (after working/has-report so recent activity wins)
  const template = findTemplate(emp.templateId)
  if (template) {
    if (template.resourceConfigKind === 'sales-table') {
      // Stub kind: never considered configured.
      return 'needs-setup'
    }
    if (template.resourceConfigKind === 'monitoring-urls') {
      const cfg = emp.resourceConfig as { monitoringTargets?: unknown[] } | null
      if (!cfg || !Array.isArray(cfg.monitoringTargets) || cfg.monitoringTargets.length === 0) {
        return 'needs-setup'
      }
    }
  }

  // Scheduled
  if (emp.cron && emp.enabled && emp.nextRunAt) return 'scheduled'

  return 'idle'
}

// ─── status badge ─────────────────────────────────────────────────────────────

function StatusDot({ status }: { status: EmployeeStatus }) {
  const base = 'h-2 w-2 rounded-full shrink-0'
  switch (status) {
    case 'working':
      return <span className={cn(base, 'bg-blue-500 animate-pulse')} />
    case 'has-report':
      return <span className={cn(base, 'bg-green-500')} />
    case 'scheduled':
      return <span className={cn(base, 'bg-amber-400')} />
    case 'needs-setup':
      return <span className={cn(base, 'bg-orange-400')} />
    default:
      return <span className={cn(base, 'bg-muted-foreground/30')} />
  }
}

function statusLabel(status: EmployeeStatus, emp: EmployeeRecord): string {
  switch (status) {
    case 'working': return '工作中'
    case 'has-report': return '有新汇报'
    case 'scheduled': {
      if (!emp.nextRunAt) return '定时驻场'
      const next = new Date(emp.nextRunAt)
      const now = new Date()
      const diffH = Math.round((next.getTime() - now.getTime()) / 3_600_000)
      if (diffH < 1) return '即将触发'
      if (diffH < 24) return `${diffH}h 后触发`
      const diffD = Math.round(diffH / 24)
      return `${diffD}天后触发`
    }
    case 'needs-setup': return '需要配置'
    default: return '空闲'
  }
}

// ─── card ─────────────────────────────────────────────────────────────────────

interface EmployeeCardProps {
  employee: EmployeeRecord
  inboxEntries: InboxEntry[]
  onClick: () => void
  onRefresh: () => Promise<void>
}

export function EmployeeCard({ employee: emp, inboxEntries, onClick, onRefresh }: EmployeeCardProps) {
  const [busy, setBusy] = useState(false)
  const status = deriveStatus(emp, inboxEntries)

  const handleTrigger = async (e: React.MouseEvent) => {
    e.stopPropagation()
    setBusy(true)
    try {
      await employeeTrigger(emp.id)
      await onRefresh()
    } catch (err) {
      console.error('[EmployeeCard] trigger error:', err)
    } finally {
      setBusy(false)
    }
  }

  const handleTogglePause = async (e: React.MouseEvent) => {
    e.stopPropagation()
    setBusy(true)
    try {
      await employeeUpdate(emp.id, { enabled: !emp.enabled })
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
        'group relative flex w-full flex-col gap-3 rounded-2xl border bg-card p-4 text-left transition-all hover:border-border/80 hover:shadow-sm',
        status === 'has-report' && 'border-green-200 bg-green-50/30',
        status === 'working' && 'border-blue-200 bg-blue-50/20',
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
        <span className={cn(
          'text-xs',
          status === 'has-report' && 'font-medium text-green-600',
          status === 'working' && 'font-medium text-blue-500',
          status === 'needs-setup' && 'font-medium text-orange-500',
          !['has-report', 'working', 'needs-setup'].includes(status) && 'text-muted-foreground',
        )}>
          {statusLabel(status, emp)}
        </span>

        <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
          {emp.cron && (
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              disabled={busy}
              onClick={handleTogglePause}
              title={emp.enabled ? '暂停' : '恢复'}
            >
              {emp.enabled
                ? <Pause className="h-3 w-3" />
                : <Play className="h-3 w-3" />
              }
            </Button>
          )}
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-xs"
            disabled={busy || status === 'working'}
            onClick={handleTrigger}
          >
            派活
          </Button>
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
