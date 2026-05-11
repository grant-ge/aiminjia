/**
 * @designSource design.pen#CoiX7 ofcSec
 * @sizing title 15/600; outer gap 14
 */
import type { PropsWithChildren, ReactNode } from 'react'

interface SkillOfficeSectionProps extends PropsWithChildren {
  categoryBar: ReactNode
}

export function SkillOfficeSection({ categoryBar, children }: SkillOfficeSectionProps) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-md font-semibold text-foreground">全部技能</h2>
      {categoryBar}
      <div className="grid grid-cols-1 gap-2.5 md:grid-cols-2 xl:grid-cols-3">
        {children}
      </div>
    </section>
  )
}
