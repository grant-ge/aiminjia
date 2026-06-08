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
    <section className="flex min-w-0 flex-col gap-3">
      <h2 className="text-lg font-semibold leading-6 text-foreground">全部技能</h2>
      {categoryBar}
      <div className="grid min-w-0 grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        {children}
      </div>
    </section>
  )
}
