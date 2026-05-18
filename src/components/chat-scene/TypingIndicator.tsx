/**
 * @designSource design.pen#oYVXX/nVSBv/EAVW9/91cWy/gpR09
 */
import { useTranslation } from 'react-i18next'
import { Asterisk } from 'lucide-react'

export type TypingVariant = 'default' | 'analyze' | 'retrieve' | 'generate' | 'organize'

interface TypingIndicatorProps {
  variant: TypingVariant
  /** When provided, overrides the variant-derived label.  Used by
   *  StreamingBubble to render the turn-stage label (e.g. "等待模型响应 · 已 5s")
   *  while keeping the breathing-asterisk visual identical. */
  label?: string
}

export function TypingIndicator({ variant, label }: TypingIndicatorProps) {
  const { t } = useTranslation()
  const text = label ?? t(`typing.${variant}`)
  return (
    <div className="flex items-center gap-1 text-primary">
      <Asterisk
        className="h-[18px] w-[18px] shrink-0"
        strokeWidth={1.75}
        style={{ animation: 'typingIndicatorBreath 1.2s ease-in-out infinite' }}
      />
      <span className="text-sm leading-none">{text}</span>
    </div>
  )
}
