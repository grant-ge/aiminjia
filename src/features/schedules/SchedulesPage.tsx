import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  CalendarClock,
  Plus,
} from 'lucide-react'

import { ScheduleEmptyState } from '@/components/schedules/ScheduleEmptyState'
import { ScheduleListCard } from '@/components/schedules/ScheduleListCard'
import { ScheduleTableHeader } from '@/components/schedules/ScheduleTableHeader'
import { ScheduleTaskRow } from '@/components/schedules/ScheduleTaskRow'
import { ScheduleTemplateCard, type ScheduleTemplate } from '@/components/schedules/ScheduleTemplateCard'
import { ConfirmDialog } from '@/components/common/ConfirmDialog'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { useAgendaItems } from '@/hooks/useAgendaItems'
import { AgendaItemEditor } from '@/features/agenda/AgendaItemEditor'
import { AgendaItemDetail } from '@/features/agenda/AgendaItemDetail'
import {
  type AgendaItem,
  type CreateAgendaItemRequest,
  cancelAgendaItem,
  deleteAgendaItem,
  restoreAgendaItem,
  runAgendaItemNow,
  updateAgendaItem,
} from '@/lib/tauri'
import { Button } from '@/components/ui/button'

const TEMPLATE_KEYS = ['dailyReport', 'storeInspection', 'weeklyReview'] as const

export function SchedulesPage() {
  const { t, i18n } = useTranslation()
  const TEMPLATES: ScheduleTemplate[] = useMemo(
    () =>
      TEMPLATE_KEYS.map((k) => ({
        title: t(`schedules.templates.${k}.title`),
        desc: t(`schedules.templates.${k}.desc`),
        prompt: t(`schedules.templates.${k}.desc`),
        rule: null,
      })),
    [t],
  )
  const { items, loading, error, refresh } = useAgendaItems()
  const [pendingCancelId, setPendingCancelId] = useState<string | null>(null)
  const [pendingPurgeId, setPendingPurgeId] = useState<string | null>(null)
  const [showCancelled, setShowCancelled] = useState(false)
  const [draftFromTemplate, setDraftFromTemplate] =
    useState<Partial<CreateAgendaItemRequest> | null>(null)
  const [editing, setEditing] = useState<AgendaItem | null>(null)
  const [editorOpen, setEditorOpen] = useState(false)
  const [detail, setDetail] = useState<AgendaItem | null>(null)
  const [pageError, setPageError] = useState<string | null>(null)

  const handleUseTemplate = useCallback((template: ScheduleTemplate) => {
    setDraftFromTemplate({
      title: template.title,
      prompt: template.prompt,
      rule: template.rule ?? null,
    })
    setEditing(null)
    setEditorOpen(true)
  }, [])

  const handleCancel = useCallback(
    async (id: string) => {
      setPendingCancelId(null)
      setPageError(null)
      try {
        await cancelAgendaItem(id)
        await refresh()
      } catch (err) {
        setPageError(formatError(err))
      }
    },
    [refresh],
  )

  const handleRestore = useCallback(
    async (id: string) => {
      setPageError(null)
      try {
        await restoreAgendaItem(id)
        await refresh()
      } catch (err) {
        setPageError(formatError(err))
      }
    },
    [refresh],
  )

  const handlePurge = useCallback(
    async (id: string) => {
      setPendingPurgeId(null)
      setPageError(null)
      try {
        await deleteAgendaItem(id)
        await refresh()
      } catch (err) {
        setPageError(formatError(err))
      }
    },
    [refresh],
  )

  const handleRunNow = useCallback(
    async (id: string) => {
      setPageError(null)
      try {
        await runAgendaItemNow(id)
        await refresh()
      } catch (err) {
        setPageError(formatError(err))
      }
    },
    [refresh],
  )

  const handleToggleStatus = useCallback(
    async (item: AgendaItem) => {
      setPageError(null)
      try {
        await updateAgendaItem(item.id, {
          status: item.status === 'active' ? 'paused' : 'active',
        })
        await refresh()
      } catch (err) {
        setPageError(formatError(err))
      }
    },
    [refresh],
  )

  const handleEdit = useCallback((item: AgendaItem) => {
    setEditing(item)
    setDraftFromTemplate(null)
    setEditorOpen(true)
  }, [])

  const handleCreateBlank = useCallback(() => {
    setEditing(null)
    setDraftFromTemplate(null)
    setEditorOpen(true)
  }, [])

  const closeEditor = useCallback(() => {
    setEditorOpen(false)
    setEditing(null)
    setDraftFromTemplate(null)
  }, [])

  const onEditorSaved = useCallback(() => {
    void refresh()
    closeEditor()
  }, [refresh, closeEditor])

  const visibleItems = showCancelled
    ? items.filter((it) => it.status === 'cancelled')
    : items.filter((it) => it.status !== 'cancelled')
  const activeItems = items.filter((it) => it.status === 'active')
  const pausedItems = items.filter((it) => it.status === 'paused')
  const orphanedItems = items.filter((it) => it.status === 'orphaned')
  const cancelledCount = items.filter((it) => it.status === 'cancelled').length
  const next24hCount = items.filter((it) => {
    if (!it.nextFireAt || it.status !== 'active') return false
    const time = new Date(it.nextFireAt).getTime()
    if (Number.isNaN(time)) return false
    const now = Date.now()
    return time >= now && time <= now + 24 * 60 * 60 * 1000
  }).length
  const nextItem = activeItems
    .filter((it) => it.nextFireAt)
    .sort(
      (a, b) =>
        new Date(a.nextFireAt ?? '').getTime() -
        new Date(b.nextFireAt ?? '').getTime(),
    )[0]

  const emptyTitle = loading
    ? t('schedules.empty.loading')
    : showCancelled
      ? t('schedules.empty.noCancelled')
      : t('schedules.empty.noTasks')
  const emptyDesc = loading
    ? t('schedules.empty.loadingDesc')
    : showCancelled
      ? t('schedules.empty.noCancelledDesc')
      : t('schedules.empty.noTasksDesc')

  const displayedError = pageError ?? error

  return (
    <PageSectionShell
      topBar={<PageTopBar variant="title" title={t('schedules.pageTitle')} />}
      maxWidthClass="max-w-[1360px]"
      padding="px-6 pt-4 pb-6"
      gap="gap-3"
    >
      <div className="rounded-md border border-sidebar-border bg-sidebar px-4 py-3 text-foreground shadow-[var(--shadow-schedule-panel)]">
        <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
          <div className="min-w-0">
            <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
              {t('schedules.console.kicker')}
            </div>
            <h1 className="mt-0.5 text-lg font-semibold leading-6 text-foreground">
              {t('schedules.console.title', '定时任务运行台')}
            </h1>
            <p className="mt-0.5 max-w-2xl text-xs leading-5 text-muted-foreground">
              {t('schedules.console.desc')}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button
              size="md"
              variant={showCancelled ? 'default' : 'outline'}
              onClick={() => setShowCancelled((v) => !v)}
              aria-label={showCancelled ? t('schedules.backToListAria') : t('schedules.viewCancelled')}
            >
              {showCancelled
                ? t('schedules.backToList')
                : cancelledCount > 0
                  ? t('schedules.cancelledWithCount', { count: cancelledCount })
                  : t('schedules.cancelled')}
            </Button>
            {showCancelled ? null : (
              <Button
                size="md"
                icon={<Plus className="h-4 w-4" />}
                onClick={handleCreateBlank}
                aria-label={t('schedules.newTaskAria')}
                data-aijia-agenda-new
              >
                {t('schedules.newButton')}
              </Button>
            )}
          </div>
        </div>

        <div className="mt-3 grid grid-cols-2 gap-2 lg:grid-cols-4">
          <ScheduleStat
            label={t('schedules.console.active', '运行中')}
            value={activeItems.length}
          />
          <ScheduleStat
            label={t('schedules.console.paused', '已暂停')}
            value={pausedItems.length}
          />
          <ScheduleStat
            label={t('schedules.console.next24h', '未来 24h')}
            value={next24hCount}
          />
          <ScheduleStat
            label={t('schedules.console.needsCare', '需处理')}
            value={orphanedItems.length}
          />
        </div>
      </div>

      {displayedError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-4 py-3 text-[0.8125rem] text-destructive">
          {displayedError}
        </div>
      ) : null}
      <div className="grid min-h-0 grid-cols-1 gap-3 xl:grid-cols-[minmax(0,1fr)_300px]">
        <ScheduleListCard
          header={
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-3">
                  <div className="text-sm font-semibold text-foreground">
                    {showCancelled ? t('schedules.cancelled') : t('schedules.taskList')}
                  </div>
                  <div className="rounded-md bg-muted px-2 py-0.5 text-xs font-medium text-muted-foreground">
                    {t('schedules.itemCount', { count: visibleItems.length })}
                  </div>
                </div>
                <p className="mt-0.5 text-xs text-muted-foreground">
                  {showCancelled
                    ? t('schedules.console.cancelledHint', '管理已停止触发的任务。')
                    : t('schedules.console.listHint', '按下次触发时间和运行状态检查任务。')}
                </p>
              </div>
            </div>
          }
          table={<ScheduleTableHeader columns={[t('schedules.columns.name'), t('schedules.columns.frequency'), t('schedules.columns.status'), t('schedules.columns.actions')]} />}
          empty={
            visibleItems.length === 0 ? (
              <ScheduleEmptyState
                icon={<CalendarClock className="h-8 w-8 text-muted-foreground" />}
                title={emptyTitle}
                desc={emptyDesc}
              />
            ) : null
          }
        >
          {visibleItems.map((item) => (
            <ScheduleTaskRow
              key={item.id}
              item={item}
              onEdit={handleEdit}
              onCancel={(id) => setPendingCancelId(id)}
              onRestore={handleRestore}
              onPurge={(id) => setPendingPurgeId(id)}
              onRunNow={handleRunNow}
              onToggleStatus={handleToggleStatus}
              onOpenDetail={setDetail}
            />
          ))}
        </ScheduleListCard>

        <aside className="flex flex-col gap-2">
          <div className="rounded-md border border-border/70 bg-card p-3 shadow-[var(--shadow-schedule-panel)]">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-sm font-semibold text-foreground">
                  {t('schedules.templatesTitle', '常用模板')}
                </div>
                <div className="mt-0.5 text-xs text-muted-foreground">
                  {t('schedules.templatesDesc', '适合周期性例行工作的起点。')}
                </div>
              </div>
            </div>
            <div className="mt-2 grid gap-1.5">
              {TEMPLATES.map((tmpl) => (
                <ScheduleTemplateCard
                  key={tmpl.title}
                  template={tmpl}
                  onPick={handleUseTemplate}
                />
              ))}
            </div>
          </div>

          <div className="rounded-md border border-border/70 bg-card p-3 shadow-[var(--shadow-schedule-panel)]">
            <div className="text-sm font-semibold text-foreground">
              {t('schedules.console.nextUp', '下一次执行')}
            </div>
            {nextItem ? (
              <div className="mt-2 rounded-md bg-muted/35 px-3 py-2">
                <div className="truncate text-sm font-semibold text-foreground">
                  {t('schedules.console.nextUpTask', '即将触发：{{title}}', { title: nextItem.title })}
                </div>
                <div className="mt-0.5 text-xs text-muted-foreground">
                  {formatDateTime(nextItem.nextFireAt, i18n.language)}
                </div>
              </div>
            ) : (
              <div className="mt-2 rounded-md border border-dashed border-border/70 px-3 py-4 text-center text-xs text-muted-foreground">
                {t('schedules.console.noNext', '暂无即将触发的任务')}
              </div>
            )}
          </div>

        </aside>
      </div>
      <ConfirmDialog
        open={!!pendingCancelId}
        title={t('schedules.cancel.title')}
        description={t('schedules.cancel.description')}
        confirmLabel={t('schedules.cancel.confirm')}
        variant="destructive"
        onOpenChange={(open) => !open && setPendingCancelId(null)}
        onConfirm={() => pendingCancelId && void handleCancel(pendingCancelId)}
      />
      <ConfirmDialog
        open={!!pendingPurgeId}
        title={t('schedules.delete.title')}
        description={t('schedules.delete.description')}
        confirmLabel={t('schedules.delete.confirm')}
        variant="destructive"
        onOpenChange={(open) => !open && setPendingPurgeId(null)}
        onConfirm={() => pendingPurgeId && void handlePurge(pendingPurgeId)}
      />
      <AgendaItemEditor
        open={editorOpen}
        initial={editing}
        initialDraft={draftFromTemplate}
        organizerEmployeeId={editing?.organizerEmployeeId}
        onClose={closeEditor}
        onSaved={onEditorSaved}
      />
      <AgendaItemDetail
        open={detail !== null}
        item={detail}
        onClose={() => setDetail(null)}
        onChanged={() => void refresh()}
      />
    </PageSectionShell>
  )
}

function formatError(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}

function ScheduleStat({
  label,
  value,
}: {
  label: string
  value: number
}) {
  return (
    <div className="rounded-md border border-sidebar-border bg-card/70 px-3 py-2">
      <div className="flex items-center gap-2">
        <span className="text-xs font-medium text-muted-foreground">{label}</span>
      </div>
      <div className="mt-1 text-xl font-semibold leading-none text-foreground">{value}</div>
    </div>
  )
}

function formatDateTime(value: string | null | undefined, locale: string) {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat(locale, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}
