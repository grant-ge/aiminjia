/**
 * 日分隔条 —— 长对话中按日分组的居中视觉锚点。
 * 形态参考 WhatsApp / iMessage：两侧细横线 + 中间小药丸。
 */
import { formatDayLabel, formatFullDateTime } from '@/lib/chatTime'

interface DayDividerProps {
  /** 该天里任意一条消息的 ISO 时间 */
  iso: string
}

export function DayDivider({ iso }: DayDividerProps) {
  const label = formatDayLabel(iso)
  if (!label) return null
  return (
    <div
      data-testid="day-divider"
      className="my-2 flex items-center gap-3 px-2 text-xs text-muted-foreground"
    >
      <span className="h-px flex-1 bg-border" aria-hidden />
      <span
        title={formatFullDateTime(iso)}
        className="rounded-full bg-muted px-2.5 py-0.5 font-medium"
      >
        {label}
      </span>
      <span className="h-px flex-1 bg-border" aria-hidden />
    </div>
  )
}
