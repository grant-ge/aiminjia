/**
 * @designSource design.pen#oYVXX/nVSBv/EAVW9/91cWy/gpR09
 */
import { Asterisk } from 'lucide-react'

export type TypingVariant = 'default' | 'analyze' | 'retrieve' | 'generate' | 'organize'

const LABELS: Record<TypingVariant, string> = {
  default: '思考中…',
  analyze: '分析中…',
  retrieve: '检索中…',
  generate: '生成中…',
  organize: '整理中…',
}

interface TypingIndicatorProps {
  variant: TypingVariant
}

export function TypingIndicator({ variant }: TypingIndicatorProps) {
  const label = LABELS[variant]
  return (
    <div className="flex items-center gap-1 text-primary">
      <Asterisk
        className="h-[18px] w-[18px] shrink-0"
        strokeWidth={1.75}
        style={{ animation: 'typingIndicatorBreath 1.2s ease-in-out infinite' }}
      />
      <span className="text-sm leading-none">{label}</span>
    </div>
  )
}
