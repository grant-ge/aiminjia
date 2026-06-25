import { CalendarClock, Folder, Pause, Pencil, Play, RotateCcw, Trash2, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { OrganizerName } from '@/components/agenda/OrganizerName'
import { describeFrequency } from '@/features/agenda/describeFrequency'
import type { AgendaItem } from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { SCHEDULE_TABLE_GRID_COLUMNS } from '@/components/schedules/ScheduleTableHeader'

interface ScheduleTaskRowProps {
  item: AgendaItem
  onEdit: (item: AgendaItem) => void
  onCancel: (id: string) => void
  onRestore: (id: string) => void
  onPurge: (id: string) => void
  onRunNow: (id: string) => void
  onToggleStatus: (item: AgendaItem) => void
  onOpenDetail?: (item: AgendaItem) => void
}

const STATUS_BADGE: Record<AgendaItem['status'], string> = {
  active: 'bg-primary/10 text-primary shadow-[inset_0_0_0_1px_rgba(var(--primary-rgb),0.10)]',
  paused: 'bg-muted text-foreground shadow-[inset_0_0_0_1px_rgba(0,0,0,0.04)]',
  completed: 'bg-emerald-50 text-emerald-700 shadow-[inset_0_0_0_1px_rgba(16,185,129,0.12)]',
  orphaned: 'bg-destructive/10 text-destructive shadow-[inset_0_0_0_1px_rgba(220,38,38,0.10)]',
  cancelled: 'bg-muted text-foreground line-through shadow-[inset_0_0_0_1px_rgba(0,0,0,0.04)]',
}

const ACTION_BUTTON_CLASS =
  'h-7 w-7 rounded border border-transparent text-muted-foreground transition-[border-color,background-color,color] hover:border-foreground/15 hover:bg-foreground hover:text-background'

export function ScheduleTaskRow({
  item,
  onEdit,
  onCancel,
  onRestore,
  onPurge,
  onRunNow,
  onToggleStatus,
  onOpenDetail,
}: ScheduleTaskRowProps) {
  const { t, i18n } = useTranslation()
  const isPaused = item.status === 'paused'
  const isCancelled = item.status === 'cancelled'
  const dimmed = isCancelled ? 'opacity-85' : ''
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
      className={`grid ${SCHEDULE_TABLE_GRID_COLUMNS} items-center gap-3 border-t border-border/55 bg-card px-4 py-2.5 text-[0.8125rem] transition-colors hover:bg-muted/20 ${dimmed}`}
    >
      {/* Column 1: task name */}
      <div className="flex min-w-0 items-center gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            {onOpenDetail ? (
              <Button
                unstyled
                type="button"
                className="min-w-0 truncate text-left font-semibold text-foreground underline-offset-4 hover:underline"
                onClick={() => onOpenDetail(item)}
              >
                {item.title}
              </Button>
            ) : (
              <span className="truncate font-semibold text-foreground">{item.title}</span>
            )}
            <OrganizerName employeeId={item.organizerEmployeeId} />
          </div>
          {item.workspacePath ? (
            <div
              className="mt-0.5 flex min-w-0 items-center gap-1 text-xs text-muted-foreground"
              title={item.workspacePath}
            >
              <Folder className="h-3 w-3 shrink-0" aria-hidden="true" />
              <span className="truncate">{workspaceShortName(item.workspacePath)}</span>
            </div>
          ) : null}
        </div>
      </div>

      {/* Column 2: frequency */}
      <div className="min-w-0 text-foreground">
        <div className="truncate font-medium">
          {describeFrequency(item.rule, item.startAt, item.timezone, t, i18n.language)}
        </div>
        <div className="mt-0.5 flex min-w-0 items-center gap-1 truncate text-xs text-muted-foreground">
          <CalendarClock className="h-3 w-3 shrink-0" aria-hidden="true" />
          <span>{t('schedules.row.nextFireLabel')}</span>
          {item.nextFireAt ? formatNextFire(item.nextFireAt, i18n.language) : '-'}
        </div>
      </div>

      {/* Column 3: status */}
      <div className="min-w-0">
        <span
          className={`inline-flex items-center rounded-md px-2 py-1 text-xs font-medium ${STATUS_BADGE[item.status]}`}
        >
          {t(`schedules.row.status.${item.status}`)}
        </span>
      </div>

      {/* Column 4: actions */}
      <div className="flex min-w-max justify-end gap-1">
        {isCancelled ? (
          <>
            <Button
              variant="ghost"
              size="icon"
              className={ACTION_BUTTON_CLASS}
              title={t('schedules.row.actions.restore')}
              aria-label={t('schedules.row.actions.restoreAria', { title: item.title })}
              icon={<RotateCcw className="h-4 w-4" />}
              onClick={() => onRestore(item.id)}
            />
            <Button
              variant="ghost"
              size="icon"
              className={ACTION_BUTTON_CLASS}
              title={t('schedules.row.actions.purge')}
              aria-label={t('schedules.row.actions.purgeAria', { title: item.title })}
              icon={<Trash2 className="h-4 w-4" />}
              onClick={() => onPurge(item.id)}
            />
          </>
        ) : (
          <>
            {isPaused ? null : (
              <Button
                variant="ghost"
                size="icon"
                className={ACTION_BUTTON_CLASS}
                title={t('schedules.row.actions.runNow')}
                aria-label={t('schedules.row.actions.runNowAria', { title: item.title })}
                icon={<Play className="h-4 w-4" />}
                onClick={() => onRunNow(item.id)}
              />
            )}
            <Button
              variant="ghost"
              size="icon"
              className={ACTION_BUTTON_CLASS}
              title={toggleLabel}
              aria-label={t(toggleAriaKey, { title: item.title })}
              icon={isPaused ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
              onClick={() => onToggleStatus(item)}
            />
            <Button
              variant="ghost"
              size="icon"
              className={ACTION_BUTTON_CLASS}
              title={t('schedules.row.actions.edit')}
              aria-label={t('schedules.row.actions.editAria', { title: item.title })}
              icon={<Pencil className="h-4 w-4" />}
              onClick={() => onEdit(item)}
            />
            <Button
              variant="ghost"
              size="icon"
              className={ACTION_BUTTON_CLASS}
              title={t('schedules.row.actions.cancel')}
              aria-label={t('schedules.row.actions.cancelAria', { title: item.title })}
              icon={<X className="h-4 w-4" />}
              onClick={() => onCancel(item.id)}
            />
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
