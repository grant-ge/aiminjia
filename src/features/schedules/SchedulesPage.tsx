import { useCallback, useState } from 'react'
import { CalendarClock, Trash2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { ScheduleEmptyState } from '@/components/schedules/ScheduleEmptyState'
import { ScheduleListCard } from '@/components/schedules/ScheduleListCard'
import { ScheduleTableHeader } from '@/components/schedules/ScheduleTableHeader'
import { ScheduleTemplateCard } from '@/components/schedules/ScheduleTemplateCard'
import { ConfirmDialog } from '@/components/common/ConfirmDialog'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { useAgendaItems } from '@/hooks/useAgendaItems'
import {
  type CreateAgendaItemRequest,
  deleteAgendaItem,
} from '@/lib/tauri'

const TEMPLATES: Array<{
  title: string
  desc: string
  prompt: string
}> = [
  {
    title: '日报汇总',
    desc: '每天 9 点自动汇总昨日数据生成日报。',
    prompt: '每天 9 点自动汇总昨日数据生成日报。',
  },
  {
    title: '门店巡检',
    desc: '每周一汇总各门店巡检结果并生成报表。',
    prompt: '每周一汇总各门店巡检结果并生成报表。',
  },
  {
    title: '周度复盘',
    desc: '每周五汇总周度 KPI 与团队复盘要点。',
    prompt: '每周五汇总周度 KPI 与团队复盘要点。',
  },
]

export function SchedulesPage() {
  const { items, loading, error, refresh } = useAgendaItems()
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)
  const [draftFromTemplate, setDraftFromTemplate] =
    useState<Partial<CreateAgendaItemRequest> | null>(null)
  const [editorOpen, setEditorOpen] = useState(false)
  const [pageError, setPageError] = useState<string | null>(null)

  const handleUseTemplate = useCallback(
    (template: (typeof TEMPLATES)[number]) => {
      setDraftFromTemplate({
        title: template.title,
        prompt: template.prompt,
        rule: null,
      })
      setEditorOpen(true)
    },
    [],
  )

  const handleDelete = useCallback(
    async (id: string) => {
      setPendingDeleteId(null)
      setDeletingId(id)
      setPageError(null)
      try {
        await deleteAgendaItem(id)
        await refresh()
      } catch (err) {
        setPageError(formatError(err))
      } finally {
        setDeletingId(null)
      }
    },
    [refresh],
  )

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
            title={t.title}
            desc={t.desc}
            cta={{
              label: '使用模板',
              onClick: () => handleUseTemplate(t),
            }}
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
            <div className="text-sm font-semibold text-foreground">任务列表</div>
            <div className="text-[0.8125rem] text-muted-foreground">共 {items.length} 条</div>
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
          <div
            key={item.id}
            className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,0.9fr)_minmax(0,0.7fr)_auto] items-center gap-3 border-t border-border px-5 py-3 text-[0.8125rem]"
          >
            <div className="min-w-0">
              <div className="truncate font-medium text-foreground">{item.title}</div>
              <div className="mt-1 truncate text-muted-foreground">{item.prompt}</div>
            </div>
            <div className="min-w-0 text-muted-foreground">
              <div>{item.nextFireAt ?? '待计算'}</div>
            </div>
            <div className="min-w-0">
              <span className="rounded-full bg-muted px-2 py-1 text-xs font-medium text-foreground">
                {item.status}
              </span>
            </div>
            <Button
              aria-label={`删除 ${item.title}`}
              variant="ghost"
              size="icon"
              disabled={deletingId === item.id}
              onClick={() => setPendingDeleteId(item.id)}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
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
      {editorOpen ? (
        <div data-testid="agenda-editor-placeholder" hidden>
          {JSON.stringify(draftFromTemplate)}
        </div>
      ) : null}
    </PageSectionShell>
  )
}

function formatError(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}
