import { Button } from '@/components/ui/button'
/**
 * @designSource design.pen#C4WXv heroAct
 * @sizing gap 10; outline button + primary button
 */

interface SkillActionBarProps {
  primaryLabel: string
  onPrimary: () => void
  primaryDisabled?: boolean
  primaryAction?: string
  secondaryLabel?: string
  onSecondary?: () => void
  secondaryAction?: string
}

export function SkillActionBar({
  primaryLabel,
  onPrimary,
  primaryDisabled,
  primaryAction = 'primary',
  secondaryLabel,
  onSecondary,
  secondaryAction,
}: SkillActionBarProps) {
  return (
    <div className="flex items-center gap-2.5">
      {secondaryLabel && onSecondary ? (
        <Button
          variant="outline"
          data-aijia-skill-detail-action={secondaryAction}
          onClick={onSecondary}
        >
          {secondaryLabel}
        </Button>
      ) : null}
      <Button
        data-aijia-skill-detail-action={primaryAction}
        onClick={onPrimary}
        disabled={primaryDisabled}
      >
        {primaryLabel}
      </Button>
    </div>
  )
}
