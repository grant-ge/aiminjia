import { useCallback, useEffect, useState } from 'react'
import { CalendarClock } from 'lucide-react'

import { ScheduleEmptyState } from '@/components/schedules/ScheduleEmptyState'
import { ScheduleListCard } from '@/components/schedules/ScheduleListCard'
import { ScheduleTableHeader } from '@/components/schedules/ScheduleTableHeader'
import { ScheduleTaskRow } from '@/components/schedules/ScheduleTaskRow'
import { ScheduleTemplateCard } from '@/components/schedules/ScheduleTemplateCard'
import { ConfirmDialog } from '@/components/common/ConfirmDialog'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { createSchedule, deleteSchedule, listSchedules, type ScheduleRecord } from '@/lib/tauri'

const TEMPLATES = [
  {
    title: '日报汇总',
    desc: '每天 9 点自动汇总昨日数据生成日报。',
    cron: '0 9 * * *',
    prompt: '每天 9 点自动汇总昨日数据生成日报。',
  },
  {
    title: '门店巡检',
    desc: '每周一汇总各门店巡检结果并生成报表。',
    cron: '0 9 * * 1',
    prompt: '每周一汇总各门店巡检结果并生成报表。',
  },
  {
    title: '周度复盘',
    desc: '每周五汇总周度 KPI 与团队复盘要点。',
    cron: '0 17 * * 5',
    prompt: '每周五汇总周度 KPI 与团队复盘要点。',
  },
]

export function SchedulesPage() {
  const [schedules, setSchedules] = useState<ScheduleRecord[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busyTemplate, setBusyTemplate] = useState<string | null>(null)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setError(null)
    const items = await listSchedules()
    setSchedules(items)
  }, [])

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    refresh()
      .catch((err) => {
        if (!cancelled) setError(formatError(err))
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [refresh])

  async function handleUseTemplate(template: (typeof TEMPLATES)[number]) {
    setBusyTemplate(template.title)
    setError(null)
    try {
      await createSchedule({
        title: template.title,
        prompt: template.prompt,
        cron: template.cron,
        timezone: 'Asia/Shanghai',
        enabled: true,
      })
      await refresh()
    } catch (err) {
      setError(formatError(err))
    } finally {
      setBusyTemplate(null)
    }
  }

  async function handleDelete(id: string) {
    setPendingDeleteId(null)
    setDeletingId(id)
    setError(null)
    try {
      await deleteSchedule(id)
      await refresh()
    } catch (err) {
      setError(formatError(err))
    } finally {
      setDeletingId(null)
    }
  }

  const emptyTitle = loading ? '正在加载定时任务' : '还没有定时任务'
  const emptyDesc = loading
    ? '请稍候，正在读取本地任务配置。'
    : '选择上方模板或在对话中创建你的第一个定时任务。'

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
              label: busyTemplate === t.title ? '创建中...' : '使用模板',
              onClick: () => handleUseTemplate(t),
            }}
          />
        ))}
      </div>
      {error ? (
        <div className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive">
          {error}
        </div>
      ) : null}
      <ScheduleListCard
        header={
          <div className="flex items-center justify-between">
            <div className="text-sm font-semibold text-foreground">任务列表</div>
            <div className="text-sm text-muted-foreground">共 {schedules.length} 条</div>
          </div>
        }
        table={<ScheduleTableHeader columns={['任务名称', '执行频率', '状态', '操作']} />}
        empty={
          schedules.length === 0 ? (
            <ScheduleEmptyState
              icon={<CalendarClock className="h-8 w-8 text-muted-foreground" />}
              title={emptyTitle}
              desc={emptyDesc}
            />
          ) : null
        }
      >
        {schedules.map((schedule) => (
          <ScheduleTaskRow
            key={schedule.id}
            schedule={schedule}
            deleting={deletingId === schedule.id}
            onDelete={setPendingDeleteId}
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
    </PageSectionShell>
  )
}

function formatError(err: unknown) {
  return err instanceof Error ? err.message : String(err)
}
