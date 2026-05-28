import { Pause, Pencil, Play, RotateCcw, Trash2, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { OrganizerName } from '@/components/agenda/OrganizerName'
import { describeFrequency } from '@/features/agenda/describeFrequency'
import type { AgendaItem } from '@/lib/tauri'

interface ScheduleTaskRowProps {
  item: AgendaItem
  onEdit: (item: AgendaItem) => void
  onCancel: (id: string) => void
  onRestore: (id: string) => void
  onPurge: (id: string) => void
  onRunNow: (id: string) => void
  onToggleStatus: (item: AgendaItem) => void
}

const STATUS_BADGE: Record<AgendaItem['status'], string> = {
  active: 'bg-primary/10 text-primary',
  paused: 'bg-muted text-muted-foreground',
  completed: 'bg-primary/10 text-primary',
  orphaned: 'bg-destructive/10 text-destructive',
  cancelled: 'bg-muted text-muted-foreground line-through',
}

export function ScheduleTaskRow({
  item,
  onEdit,
  onCancel,
  onRestore,
  onPurge,
  onRunNow,
  onToggleStatus,
}: ScheduleTaskRowProps) {
  const { t, i18n } = useTranslation()
  const isPaused = item.status === 'paused'
  const isCancelled = item.status === 'cancelled'
  const dimmed = isPaused || isCancelled ? 'opacity-70' : ''
  const toggleLabel = isPaused
    ? t('schedules.row.actions.resume')
    : t('schedules.row.actions.pause')
  const toggleAriaKey = isPaused
    ? 'schedules.row.actions.resumeAria'
    : 'schedules.row.actions.pauseAria'

  return (
    <div
      data-aijia-agenda-row
      data-aijia-agenda-id={item.id}
      data-aijia-agenda-title={item.title}
      data-aijia-agenda-status={item.status}
      className={`group grid grid-cols-4 items-center gap-3 border-t border-border px-5 py-3 text-[0.8125rem] hover:bg-muted/50 ${dimmed}`}
    >
      {/* Column 1: task name */}
      <div className="flex min-w-0 items-center gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate font-medium text-foreground">{item.title}</span>
            <OrganizerName employeeId={item.organizerEmployeeId} />
          </div>
          {item.workspacePath ? (
            <div
              className="truncate text-xs text-muted-foreground/70"
              title={item.workspacePath}
            >
              📁 {workspaceShortName(item.workspacePath)}
            </div>
          ) : null}
        </div>
      </div>

      {/* Column 2: frequency */}
      <div className="min-w-0 text-muted-foreground">
        <div className="truncate">
          {describeFrequency(item.rule, item.startAt, item.timezone, t, i18n.language)}
        </div>
        <div className="mt-1 truncate text-xs">
          {t('schedules.row.nextFireLabel')}
          {item.nextFireAt ? formatNextFire(item.nextFireAt, i18n.language) : '-'}
        </div>
      </div>

      {/* Column 3: status */}
      <div className="min-w-0">
        <span
          className={`rounded-full px-2 py-1 text-xs font-medium ${STATUS_BADGE[item.status]}`}
        >
          {t(`schedules.row.status.${item.status}`)}
        </span>
      </div>

      {/* Column 4: actions (hover) */}
      <div className="flex justify-end gap-1 opacity-0 group-hover:opacity-100">
        {isCancelled ? (
          <>
            <Button
              variant="ghost"
              size="icon"
              title={t('schedules.row.actions.restore')}
              aria-label={t('schedules.row.actions.restoreAria', { title: item.title })}
              onClick={() => onRestore(item.id)}
            >
              <RotateCcw className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              title={t('schedules.row.actions.purge')}
              aria-label={t('schedules.row.actions.purgeAria', { title: item.title })}
              onClick={() => onPurge(item.id)}
            >
              <Trash2 className="h-4 w-4 text-destructive" />
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="ghost"
              size="icon"
              title={t('schedules.row.actions.runNow')}
              aria-label={t('schedules.row.actions.runNowAria', { title: item.title })}
              onClick={() => onRunNow(item.id)}
            >
              <Play className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              title={toggleLabel}
              aria-label={t(toggleAriaKey, { title: item.title })}
              onClick={() => onToggleStatus(item)}
            >
              {isPaused ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
            </Button>
            <Button
              variant="ghost"
              size="icon"
              title={t('schedules.row.actions.edit')}
              aria-label={t('schedules.row.actions.editAria', { title: item.title })}
              onClick={() => onEdit(item)}
            >
              <Pencil className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              title={t('schedules.row.actions.cancel')}
              aria-label={t('schedules.row.actions.cancelAria', { title: item.title })}
              onClick={() => onCancel(item.id)}
            >
              <X className="h-4 w-4 text-destructive" />
            </Button>
          </>
        )}
      </div>
    </div>
  )
}

function formatNextFire(value: string, locale: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return new Intl.DateTimeFormat(locale, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}

function workspaceShortName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean)
  return parts[parts.length - 1] ?? path
}
