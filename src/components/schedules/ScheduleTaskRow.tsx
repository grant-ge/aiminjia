import { Pause, Pencil, Play, Trash2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { PersonaAvatar } from '@/components/agenda/PersonaAvatar'
import { describeFrequency } from '@/features/agenda/describeFrequency'
import type { AgendaItem } from '@/lib/tauri'

interface ScheduleTaskRowProps {
  item: AgendaItem
  onEdit: (item: AgendaItem) => void
  onDelete: (id: string) => void
  onRunNow: (id: string) => void
  onToggleStatus: (item: AgendaItem) => void
}

const STATUS_CLASS: Record<AgendaItem['status'], string> = {
  active: 'border-l-2 border-blue-500',
  paused: 'opacity-70',
  completed: '',
  orphaned: 'border-l-2 border-red-500',
}

export function ScheduleTaskRow({
  item,
  onEdit,
  onDelete,
  onRunNow,
  onToggleStatus,
}: ScheduleTaskRowProps) {
  const statusClass = STATUS_CLASS[item.status]
  const isPaused = item.status === 'paused'

  return (
    <div
      className={`group flex items-center gap-3 border-t border-border px-4 py-3 hover:bg-muted/50 ${statusClass}`}
    >
      <PersonaAvatar personaId={item.organizerPersonaId} size="sm" />
      <div className="flex-1 min-w-0">
        <div className="truncate font-medium text-foreground">{item.title}</div>
        <div className="truncate text-xs text-muted-foreground">
          {describeFrequency(item.rule, item.startAt, item.timezone)}
        </div>
      </div>
      <div className="text-xs text-muted-foreground whitespace-nowrap">
        {item.nextFireAt ? formatNextFire(item.nextFireAt) : '-'}
      </div>
      <div className="opacity-0 group-hover:opacity-100 flex gap-1">
        <Button
          variant="ghost"
          size="icon"
          title="立即运行"
          aria-label={`立即运行 ${item.title}`}
          onClick={() => onRunNow(item.id)}
        >
          <Play className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          title={isPaused ? '启用' : '暂停'}
          aria-label={`${isPaused ? '启用' : '暂停'} ${item.title}`}
          onClick={() => onToggleStatus(item)}
        >
          {isPaused ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          title="编辑"
          aria-label={`编辑 ${item.title}`}
          onClick={() => onEdit(item)}
        >
          <Pencil className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          title="删除"
          aria-label={`删除 ${item.title}`}
          onClick={() => onDelete(item.id)}
        >
          <Trash2 className="h-4 w-4 text-destructive" />
        </Button>
      </div>
    </div>
  )
}

function formatNextFire(value: string) {
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
