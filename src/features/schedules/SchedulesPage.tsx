import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CalendarClock, Plus } from 'lucide-react'

import { Button } from '@/components/ui/button'
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

const TEMPLATE_KEYS = ['dailyReport', 'storeInspection', 'weeklyReview'] as const

export function SchedulesPage() {
  const { t } = useTranslation()
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
  const cancelledCount = items.filter((it) => it.status === 'cancelled').length

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
    >
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        {TEMPLATES.map((tmpl) => (
          <ScheduleTemplateCard
            key={tmpl.title}
            template={tmpl}
            onPick={handleUseTemplate}
          />
        ))}
      </div>
      {displayedError ? (
        <div className="rounded-[12px] border border-destructive/30 bg-destructive/5 px-4 py-3 text-[0.8125rem] text-destructive">
          {displayedError}
        </div>
      ) : null}
      <ScheduleListCard
        header={
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="text-sm font-semibold text-foreground">
                {showCancelled ? t('schedules.cancelled') : t('schedules.taskList')}
              </div>
              <div className="text-[0.8125rem] text-muted-foreground">
                {t('schedules.itemCount', { count: visibleItems.length })}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
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
                <Button size="sm" onClick={handleCreateBlank} aria-label={t('schedules.newTaskAria')} data-aijia-agenda-new>
                  <Plus className="h-4 w-4" />
                  {t('schedules.newButton')}
                </Button>
              )}
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
          />
        ))}
      </ScheduleListCard>
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
