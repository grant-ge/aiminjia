/**
 * @designSource design.pen#M2pKg
 * @sizing pill padding [10,16] r-999 bg secondary
 */
import { ArrowRight } from 'lucide-react'

interface HomeSkillCenterPillProps {
  onClick: () => void
  label?: string
}

export function HomeSkillCenterPill({
  onClick,
  label = '前往技能中心',
}: HomeSkillCenterPillProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex items-center gap-1.5 rounded-full bg-secondary px-4 py-2.5 text-[0.8125rem] font-medium text-muted-foreground transition-colors hover:text-foreground"
    >
      <span>{label}</span>
      <ArrowRight className="h-3.5 w-3.5" />
    </button>
  )
}
