/**
 * @designSource design.pen#ZQLFS trySec
 * @sizing title 15/600; grid gap 16
 */
import type { PropsWithChildren } from 'react'

export function SkillTryGrid({ children }: PropsWithChildren) {
  return (
    <section className="flex w-full flex-col gap-3.5">
      <div className="text-[0.9375rem] font-semibold text-foreground">试试让 AI 小家这样做</div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">{children}</div>
    </section>
  )
}
