import { Button } from '@/components/ui/button'
/**
 * @designSource design.pen#C4WXv heroAct
 * @sizing gap 10; outline button + primary button
 */

interface SkillActionBarProps {
  primaryLabel: string
  onPrimary: () => void
  primaryDisabled?: boolean
  secondaryLabel?: string
  onSecondary?: () => void
}

export function SkillActionBar({
  primaryLabel,
  onPrimary,
  primaryDisabled,
  secondaryLabel,
  onSecondary,
}: SkillActionBarProps) {
  return (
    <div className="flex items-center gap-2.5">
      {secondaryLabel && onSecondary ? (
        <Button variant="outline" onClick={onSecondary}>
          {secondaryLabel}
        </Button>
      ) : null}
      <Button onClick={onPrimary} disabled={primaryDisabled}>
        {primaryLabel}
      </Button>
    </div>
  )
}
