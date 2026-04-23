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
    <section className="flex flex-col gap-3.5">
      <h2 className="text-[15px] font-semibold text-foreground">办公效率</h2>
      {categoryBar}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {children}
      </div>
    </section>
  )
}
