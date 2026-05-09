import { useCallback, useEffect, useState } from 'react'
import { CalendarClock, Plus } from 'lucide-react'
import { invoke } from '@tauri-apps/api/core'

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
  deleteAgendaItem,
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
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const [draftFromTemplate, setDraftFromTemplate] =
    useState<Partial<CreateAgendaItemRequest> | null>(null)
  const [editing, setEditing] = useState<AgendaItem | null>(null)
  const [editorOpen, setEditorOpen] = useState(false)
  const [detail, setDetail] = useState<AgendaItem | null>(null)
  const [activePersonaId, setActivePersonaId] = useState('default')
  const [pageError, setPageError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void invoke<{ id?: string } | null>('get_active_persona')
      .then((p) => {
        if (!cancelled && p?.id) setActivePersonaId(p.id)
      })
      .catch(() => {
        // 没有 persona 也不阻塞页面，组织者用 'default' 兜底
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

  const handleDelete = useCallback(
    async (id: string) => {
      setPendingDeleteId(null)
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

  const emptyTitle = loading ? '正在加载定时任务' : '还没有定时任务'
  const emptyDesc = loading
    ? '请稍候，正在读取本地任务配置。'
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
              <div className="text-sm font-semibold text-foreground">任务列表</div>
              <div className="text-[0.8125rem] text-muted-foreground">共 {items.length} 条</div>
            </div>
            <Button size="sm" onClick={handleCreateBlank} aria-label="新建日程">
              <Plus className="h-4 w-4" />
              新建
            </Button>
          </div>
        }
        table={<ScheduleTableHeader columns={['任务名称', '执行频率', '状态', '操作']} />}
        empty={
          items.length === 0 ? (
            <ScheduleEmptyState
              icon={<CalendarClock className="h-8 w-8 text-muted-foreground" />}
              title={emptyTitle}
              desc={emptyDesc}
            />
          ) : null
        }
      >
        {items.map((item) => (
          <ScheduleTaskRow
            key={item.id}
            item={item}
            onEdit={handleEdit}
            onDelete={(id) => setPendingDeleteId(id)}
            onRunNow={handleRunNow}
            onToggleStatus={handleToggleStatus}
          />
        ))}
      </ScheduleListCard>
      <ConfirmDialog
        open={!!pendingDeleteId}
        title="删除此定时任务？"
        description="删除后该定时任务将停止执行，且无法从任务列表中恢复。"
        confirmLabel="确认删除"
        variant="destructive"
        onOpenChange={(open) => !open && setPendingDeleteId(null)}
        onConfirm={() => pendingDeleteId && void handleDelete(pendingDeleteId)}
      />
      <AgendaItemEditor
        open={editorOpen}
        initial={editing}
        initialDraft={draftFromTemplate}
        organizerPersonaId={editing?.organizerPersonaId ?? activePersonaId}
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
