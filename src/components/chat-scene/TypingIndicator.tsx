/**
 * @designSource design.pen#oYVXX/nVSBv/EAVW9/91cWy/gpR09
 */
import { useTranslation } from 'react-i18next'
import { Asterisk } from 'lucide-react'

export type TypingVariant = 'default' | 'analyze' | 'retrieve' | 'generate' | 'organize'

interface TypingIndicatorProps {
  variant: TypingVariant
}

export function TypingIndicator({ variant }: TypingIndicatorProps) {
  const { t } = useTranslation()
  const label = t(`typing.${variant}`)
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
