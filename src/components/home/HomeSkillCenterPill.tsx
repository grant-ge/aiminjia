/**
 * @designSource design.pen#M2pKg
 * @sizing pill padding [10,16] r-999 bg secondary
 */
import { ArrowRight } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface HomeSkillCenterPillProps {
  onClick: () => void
  label?: string
}

export function HomeSkillCenterPill({
  onClick,
  label = '前往技能中心',
}: HomeSkillCenterPillProps) {
  return (
    <Button unstyled
      type="button"
      onClick={onClick}
      className="flex items-center gap-1.5 rounded-md bg-secondary px-4 py-2.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground"
    >
      <span>{label}</span>
      <ArrowRight className="h-3.5 w-3.5" />
    </Button>
  )
}
