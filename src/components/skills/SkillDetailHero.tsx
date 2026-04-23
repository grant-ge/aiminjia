/**
 * @designSource design.pen#UDRR3 hero
 * @sizing heroIc 88×88 r-22 brand-primary-subtle; gap 20
 */
import type { ReactNode } from 'react'

interface SkillDetailHeroProps {
  iconNode: ReactNode
  title: string
  subtitle: string
  actionBar: ReactNode
}

export function SkillDetailHero({ iconNode, title, subtitle, actionBar }: SkillDetailHeroProps) {
  return (
    <div className="flex w-full items-start gap-5">
      <div
        data-testid="hero-ic"
        className="flex h-[88px] w-[88px] shrink-0 items-center justify-center rounded-[22px] bg-brand-primary-subtle"
      >
        {iconNode}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="text-[28px] font-bold leading-tight text-foreground">{title}</div>
        <div className="text-sm text-muted-foreground">{subtitle}</div>
      </div>
      <div className="shrink-0">{actionBar}</div>
    </div>
  )
}
