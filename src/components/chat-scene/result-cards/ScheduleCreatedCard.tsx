import { Pencil } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { AgendaItemEditor } from '@/features/agenda/AgendaItemEditor'
import { describeFrequency } from '@/features/agenda/describeFrequency'
import { Button } from '@/components/ui/button'
import { type AgendaItem, getAgendaItem } from '@/lib/tauri'
import type { ScheduleCreatedCardPayload } from './aijiaCardPayload'

interface ScheduleCreatedCardProps {
  payload: ScheduleCreatedCardPayload
}

export function ScheduleCreatedCard({ payload }: ScheduleCreatedCardProps) {
  const { t, i18n } = useTranslation()
  const [item, setItem] = useState<AgendaItem | null>(null)
  const [loadFailed, setLoadFailed] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)

  const loadItem = useCallback(async () => {
    try {
      const next = await getAgendaItem(payload.scheduleId)
      setItem(next)
      setLoadFailed(false)
    } catch {
      setLoadFailed(true)
    }
  }, [payload.scheduleId])

  useEffect(() => {
    void loadItem()
  }, [loadItem])

  const title = item?.title || payload.title || payload.scheduleId
  const prompt = item?.prompt || payload.prompt || t('resultCards.schedule.fallbackPrompt')
  const nextFire = item?.nextFireAt || payload.nextFireAt || null
  const scheduleLabel = item
    ? describeFrequency(item.rule, item.startAt, item.timezone, t, i18n.language)
    : payload.frequencyLabel || t('schedules.editor.freqOptions.oneShot')
  const nextFireLabel = nextFire ? formatCompactDateTime(nextFire, i18n.language) : '-'

  return (
    <div
      className="group my-3 w-full rounded-md border border-border bg-card px-3 py-2.5 text-card-foreground shadow-none"
      data-aijia-result-card="schedule_created"
    >
      <div className="flex items-start gap-2.5 pr-0.5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[15px] font-semibold leading-5 text-foreground">{title}</div>
          <div className="mt-1 line-clamp-2 text-sm leading-5 text-muted-foreground">
            {prompt}
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs leading-5 text-muted-foreground">
            <ScheduleMeta label={t('resultCards.schedule.frequency')} value={scheduleLabel} />
            <span className="text-muted-foreground/45" aria-hidden>
              /
            </span>
            <ScheduleMeta label={t('resultCards.schedule.nextFire')} value={nextFireLabel} />
          </div>
          {loadFailed ? (
            <div className="mt-2 text-xs text-muted-foreground">
              {t('resultCards.schedule.unavailable')}
            </div>
          ) : null}
        </div>
        <Button
          unstyled
          type="button"
          aria-label={t('resultCards.schedule.edit')}
          onClick={() => setEditorOpen(true)}
          disabled={!item}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border-transparent bg-transparent p-0 text-muted-foreground opacity-70 transition-colors hover:bg-muted hover:text-foreground hover:opacity-100 disabled:pointer-events-none disabled:opacity-45 group-hover:opacity-100"
          data-testid="schedule-created-card-edit"
        >
          <Pencil className="h-3.5 w-3.5" aria-hidden />
        </Button>
      </div>
      <AgendaItemEditor
        open={editorOpen}
        initial={item}
        onClose={() => setEditorOpen(false)}
        onSaved={() => {
          void loadItem()
        }}
      />
    </div>
  )
}

function ScheduleMeta({ label, value }: { label: string; value: string }) {
  return (
    <span className="inline-flex max-w-full items-baseline gap-1.5">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="min-w-0 truncate font-medium tabular-nums text-foreground/88">{value}</span>
    </span>
  )
}

function formatCompactDateTime(value: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(new Date(value))
}
