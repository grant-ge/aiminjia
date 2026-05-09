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

const STATUS_LABEL: Record<AgendaItem['status'], string> = {
  active: '已启用',
  paused: '已暂停',
  completed: '已完成',
  orphaned: '组织者缺失',
}

const STATUS_BADGE: Record<AgendaItem['status'], string> = {
  active: 'bg-blue-100 text-blue-700',
  paused: 'bg-muted text-muted-foreground',
  completed: 'bg-green-100 text-green-700',
  orphaned: 'bg-red-100 text-red-700',
}

export function ScheduleTaskRow({
  item,
  onEdit,
  onDelete,
  onRunNow,
  onToggleStatus,
}: ScheduleTaskRowProps) {
  const isPaused = item.status === 'paused'
  const dimmed = isPaused ? 'opacity-70' : ''

  return (
    <div
      className={`group grid grid-cols-4 items-center gap-3 border-t border-border px-5 py-3 text-[0.8125rem] hover:bg-muted/50 ${dimmed}`}
    >
      {/* 列 1：任务名称 */}
      <div className="flex min-w-0 items-center gap-2">
        <PersonaAvatar personaId={item.organizerPersonaId} size="sm" />
        <div className="min-w-0">
          <div className="truncate font-medium text-foreground">{item.title}</div>
          {item.workspacePath ? (
            <div
              className="truncate text-xs text-muted-foreground/70"
              title={item.workspacePath}
            >
              📁 {workspaceShortName(item.workspacePath)}
            </div>
          ) : null}
        </div>
      </div>

      {/* 列 2：执行频率 */}
      <div className="min-w-0 text-muted-foreground">
        <div className="truncate">
          {describeFrequency(item.rule, item.startAt, item.timezone)}
        </div>
        <div className="mt-1 truncate text-xs">
          下次：{item.nextFireAt ? formatNextFire(item.nextFireAt) : '-'}
        </div>
      </div>

      {/* 列 3：状态 */}
      <div className="min-w-0">
        <span
          className={`rounded-full px-2 py-1 text-xs font-medium ${STATUS_BADGE[item.status]}`}
        >
          {STATUS_LABEL[item.status]}
        </span>
      </div>

      {/* 列 4：操作（hover 显示） */}
      <div className="flex justify-end gap-1 opacity-0 group-hover:opacity-100">
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

function workspaceShortName(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean)
  return parts[parts.length - 1] ?? path
}
