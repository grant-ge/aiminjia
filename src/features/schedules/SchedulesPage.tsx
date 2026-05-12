import { useCallback, useEffect, useState } from 'react'
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
  employeeList,
  restoreAgendaItem,
  runAgendaItemNow,
  updateAgendaItem,
} from '@/lib/tauri'

const TEMPLATES: ScheduleTemplate[] = [
  {
    title: '日报汇总',
    desc: '每天 9 点自动汇总昨日数据生成日报。',
    prompt: '每天 9 点自动汇总昨日数据生成日报。',
    rule: null,
  },
  {
    title: '门店巡检',
    desc: '每周一汇总各门店巡检结果并生成报表。',
    prompt: '每周一汇总各门店巡检结果并生成报表。',
    rule: null,
  },
  {
    title: '周度复盘',
    desc: '每周五汇总周度 KPI 与团队复盘要点。',
    prompt: '每周五汇总周度 KPI 与团队复盘要点。',
    rule: null,
  },
]

export function SchedulesPage() {
  const { items, loading, error, refresh } = useAgendaItems()
  const [pendingCancelId, setPendingCancelId] = useState<string | null>(null)
  const [pendingPurgeId, setPendingPurgeId] = useState<string | null>(null)
  const [showCancelled, setShowCancelled] = useState(false)
  const [draftFromTemplate, setDraftFromTemplate] =
    useState<Partial<CreateAgendaItemRequest> | null>(null)
  const [editing, setEditing] = useState<AgendaItem | null>(null)
  const [editorOpen, setEditorOpen] = useState(false)
  const [detail, setDetail] = useState<AgendaItem | null>(null)
  const [defaultEmployeeId, setDefaultEmployeeId] = useState<string>('')
  const [pageError, setPageError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void employeeList()
      .then((list) => {
        if (cancelled) return
        const first = list.find((e) => e.lifecycle === 'active')
        setDefaultEmployeeId(first?.id ?? '')
      })
      .catch(() => {
        if (!cancelled) setDefaultEmployeeId('')
      })
    return () => {
      cancelled = true
    }
  }, [])

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
    ? '正在加载定时任务'
    : showCancelled
      ? '没有已取消的任务'
      : '还没有定时任务'
  const emptyDesc = loading
    ? '请稍候，正在读取本地任务配置。'
    : showCancelled
      ? '取消的任务会出现在这里，可以恢复或永久删除。'
      : '选择上方模板或在对话中创建你的第一个定时任务。'

  const displayedError = pageError ?? error

  return (
    <PageSectionShell
      topBar={<PageTopBar variant="title" title="定时任务" />}
      padding="px-7 pt-6 pb-8"
      gap="gap-6"
    >
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        {TEMPLATES.map((t) => (
          <ScheduleTemplateCard
            key={t.title}
            template={t}
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
                {showCancelled ? '已取消' : '任务列表'}
              </div>
              <div className="text-[0.8125rem] text-muted-foreground">
                共 {visibleItems.length} 条
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant={showCancelled ? 'default' : 'outline'}
                onClick={() => setShowCancelled((v) => !v)}
                aria-label={showCancelled ? '返回任务列表' : '查看已取消'}
              >
                {showCancelled ? '返回' : `已取消 ${cancelledCount > 0 ? `(${cancelledCount})` : ''}`}
              </Button>
              {showCancelled ? null : (
                <Button size="sm" onClick={handleCreateBlank} aria-label="新建日程">
                  <Plus className="h-4 w-4" />
                  新建
                </Button>
              )}
            </div>
          </div>
        }
        table={<ScheduleTableHeader columns={['任务名称', '执行频率', '状态', '操作']} />}
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
        title="取消此定时任务？"
        description="取消后该任务将停止触发，但可以从「已取消」中恢复。"
        confirmLabel="确认取消"
        variant="destructive"
        onOpenChange={(open) => !open && setPendingCancelId(null)}
        onConfirm={() => pendingCancelId && void handleCancel(pendingCancelId)}
      />
      <ConfirmDialog
        open={!!pendingPurgeId}
        title="永久删除此任务？"
        description="此操作会从磁盘抹除任务及其执行历史，无法恢复。"
        confirmLabel="确认永久删除"
        variant="destructive"
        onOpenChange={(open) => !open && setPendingPurgeId(null)}
        onConfirm={() => pendingPurgeId && void handlePurge(pendingPurgeId)}
      />
      <AgendaItemEditor
        open={editorOpen}
        initial={editing}
        initialDraft={draftFromTemplate}
        organizerEmployeeId={editing?.organizerEmployeeId ?? defaultEmployeeId}
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
