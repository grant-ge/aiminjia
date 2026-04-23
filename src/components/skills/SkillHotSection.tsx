/**
 * @designSource design.pen#znwZc hotSec
 * @sizing title 15/600; grid gap 16
 */
import type { PropsWithChildren } from 'react'

export function SkillHotSection({ children }: PropsWithChildren) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-[15px] font-semibold text-foreground">热门推荐</h2>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {children}
      </div>
    </section>
  )
}
