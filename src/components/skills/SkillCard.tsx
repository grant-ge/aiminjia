/**
 * @designSource design.pen technical card derivative (Card / Card Action)
 * @sizing r-8 border 1 padding 16
 */
import type { ReactNode } from 'react'

import { Button } from '@/components/ui/button'

interface SkillCardProps {
  title: string
  desc: string
  iconNode: ReactNode
  onUse: () => void
  onOpen: () => void
}

export function SkillCard({ title, desc, iconNode, onUse, onOpen }: SkillCardProps) {
  return (
    <div
      data-testid="skill-card"
      className="flex h-full flex-col rounded-lg border border-border bg-card p-4 shadow-sm transition-colors hover:border-primary/40"
    >
      <div className="mb-3 flex items-center gap-2">
        <div className="flex h-8 w-8 items-center justify-center rounded-md bg-brand-primary-subtle">
          {iconNode}
        </div>
        <div className="text-sm font-semibold text-foreground">{title}</div>
      </div>
      <p className="flex-1 text-[13px] text-muted-foreground">{desc}</p>
      <div className="mt-4 flex items-center gap-2">
        <Button variant="secondary" className="flex-1" onClick={onOpen}>
          详情
        </Button>
        <Button className="flex-1" onClick={onUse}>
          使用
        </Button>
      </div>
    </div>
  )
}
