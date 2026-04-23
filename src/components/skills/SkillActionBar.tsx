/**
 * @designSource design.pen#C4WXv heroAct
 * @sizing gap 10; outline button + primary button
 */
import { Button } from '@/components/ui/button'

interface SkillActionBarProps {
  primaryLabel: string
  secondaryLabel: string
  onPrimary: () => void
  onSecondary: () => void
  primaryDisabled?: boolean
}

export function SkillActionBar({
  primaryLabel,
  secondaryLabel,
  onPrimary,
  onSecondary,
  primaryDisabled,
}: SkillActionBarProps) {
  return (
    <div className="flex items-center gap-2.5">
      <Button variant="outline" onClick={onSecondary}>
        {secondaryLabel}
      </Button>
      <Button onClick={onPrimary} disabled={primaryDisabled}>
        {primaryLabel}
      </Button>
    </div>
  )
}
