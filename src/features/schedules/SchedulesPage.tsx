import { CalendarClock } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { ScheduleEmptyState } from '@/components/schedules/ScheduleEmptyState'
import { ScheduleListCard } from '@/components/schedules/ScheduleListCard'
import { ScheduleTableHeader } from '@/components/schedules/ScheduleTableHeader'
import { ScheduleTemplateCard } from '@/components/schedules/ScheduleTemplateCard'

const TEMPLATES = [
  { title: '日报汇总', desc: '每天 9 点自动汇总昨日数据生成日报。' },
  { title: '门店巡检', desc: '每周一汇总各门店巡检结果并生成报表。' },
  { title: '周度复盘', desc: '每周五汇总周度 KPI 与团队复盘要点。' },
]

export function SchedulesPage() {
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
            cta={{ label: '使用模板', onClick: () => {} }}
          />
        ))}
      </div>
      <ScheduleListCard
        header={
          <div className="flex items-center justify-between">
            <div className="text-sm font-semibold text-foreground">任务列表</div>
            <div className="text-[13px] text-muted-foreground">共 0 条</div>
          </div>
        }
        table={<ScheduleTableHeader columns={['任务名称', '执行频率', '状态']} />}
        empty={
          <ScheduleEmptyState
            icon={<CalendarClock className="h-8 w-8 text-muted-foreground" />}
            title="还没有定时任务"
            desc="选择上方模板或在对话中创建你的第一个定时任务。"
          />
        }
      />
    </PageSectionShell>
  )
}
