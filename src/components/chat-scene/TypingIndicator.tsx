/**
 * @designSource design.pen#oYVXX/nVSBv/EAVW9/91cWy/gpR09
 */
import { Activity, Brain, Loader2, Search, Sparkles, type LucideIcon } from 'lucide-react'

export type TypingVariant = 'default' | 'analyze' | 'retrieve' | 'generate' | 'organize'

const MAP: Record<TypingVariant, { icon: LucideIcon; label: string }> = {
  default: { icon: Loader2, label: '正在处理...' },
  analyze: { icon: Brain, label: '分析中...' },
  retrieve: { icon: Search, label: '检索中...' },
  generate: { icon: Sparkles, label: '生成中...' },
  organize: { icon: Activity, label: '整理中...' },
}

interface TypingIndicatorProps {
  variant: TypingVariant
}

export function TypingIndicator({ variant }: TypingIndicatorProps) {
  const { icon: Icon, label } = MAP[variant]
  const animate = variant === 'default' ? 'animate-spin' : 'animate-pulse'
  return (
    <div className="flex items-center gap-2 text-[13px] text-primary">
      <Icon className={`h-3.5 w-3.5 ${animate}`} />
      <span>{label}</span>
    </div>
  )
}
