import { Trash2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import type { ScheduleRecord } from '@/lib/tauri'

interface ScheduleTaskRowProps {
  schedule: ScheduleRecord
  onDelete: (id: string) => void
  deleting?: boolean
}

export function ScheduleTaskRow({ schedule, onDelete, deleting }: ScheduleTaskRowProps) {
  const statusLabel = schedule.status === 'enabled' ? '已启用' : '已停用'
  const nextRun = schedule.nextRunAt ? formatNextRun(schedule.nextRunAt) : '待计算'

  return (
    <div className="grid grid-cols-[minmax(0,1.2fr)_minmax(0,0.9fr)_minmax(0,0.7fr)_auto] items-center gap-3 border-t border-border px-5 py-3 text-[13px]">
      <div className="min-w-0">
        <div className="truncate font-medium text-foreground">{schedule.title}</div>
        <div className="mt-1 truncate text-muted-foreground">{schedule.prompt}</div>
      </div>
      <div className="min-w-0 text-muted-foreground">
        <div>{schedule.humanSchedule || schedule.cron}</div>
        <div className="mt-1 text-[12px]">{schedule.cron}</div>
      </div>
      <div className="min-w-0">
        <span className="rounded-full bg-muted px-2 py-1 text-[12px] font-medium text-foreground">
          {statusLabel}
        </span>
        <div className="mt-1 truncate text-[12px] text-muted-foreground">下次：{nextRun}</div>
      </div>
      <Button
        aria-label={`删除 ${schedule.title}`}
        variant="ghost"
        size="icon"
        disabled={deleting}
        onClick={() => onDelete(schedule.id)}
      >
        <Trash2 className="h-4 w-4" />
      </Button>
    </div>
  )
}

function formatNextRun(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}
