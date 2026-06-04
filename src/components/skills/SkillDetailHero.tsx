/**
 * @designSource design.pen#UDRR3 hero
 * @sizing gap 20
 */
import type { ReactNode } from 'react'

interface SkillDetailHeroProps {
  title: string
  subtitle: string
  actionBar: ReactNode
}

export function SkillDetailHero({ title, subtitle, actionBar }: SkillDetailHeroProps) {
  return (
    <div className="flex w-full items-start gap-5">
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="text-[1.75rem] font-bold leading-tight text-foreground">{title}</div>
        <div className="text-sm text-muted-foreground">{subtitle}</div>
      </div>
      <div className="shrink-0">{actionBar}</div>
    </div>
  )
}
