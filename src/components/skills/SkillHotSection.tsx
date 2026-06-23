/**
 * @designSource design.pen#znwZc hotSec
 * @sizing title 15/600; grid gap 16
 */
import { Inbox } from 'lucide-react'
import { Children, type PropsWithChildren } from 'react'

export function SkillHotSection({ children }: PropsWithChildren) {
  const hasChildren = Children.count(children) > 0
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-md font-semibold text-foreground">热门推荐</h2>
      {hasChildren ? (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">{children}</div>
      ) : (
        <div className="flex flex-col items-center justify-center gap-2 rounded-md border border-dashed border-border bg-card/40 px-6 py-10 text-center">
          <Inbox className="h-5 w-5 text-muted-foreground" />
          <div className="text-sm font-medium text-foreground">暂无热门技能</div>
        </div>
      )}
    </section>
  )
}
