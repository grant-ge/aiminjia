import type { ReactNode } from 'react'

interface SkillCardProps {
  title: string
  meta: string
  desc: string
  iconNode: ReactNode
  onClick: () => void
  size?: 'hot' | 'office'
}

export function SkillCard({ title, meta, desc, iconNode, onClick, size = 'office' }: SkillCardProps) {
  const isHot = size === 'hot'
  const height = isHot ? 'h-[140px]' : 'h-[120px]'
  const iconSize = isHot ? 'h-9 w-9' : 'h-[34px] w-[34px]'

  return (
    <div
      data-testid="skill-card"
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && onClick()}
      className={`flex ${height} cursor-pointer flex-col rounded-[14px] border border-border bg-card p-4 transition-all duration-150 hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring`}
    >
      <div className="flex items-center gap-2.5">
        <div className={`flex ${iconSize} shrink-0 items-center justify-center rounded-[10px] bg-brand-primary-subtle`}>
          {iconNode}
        </div>
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-sm font-semibold text-foreground">{title}</span>
          <span className="text-[12px] font-medium text-brand-secondary">{meta}</span>
        </div>
      </div>
      <p className="mt-2.5 line-clamp-2 text-[12px] text-muted-foreground">{desc}</p>
    </div>
  )
}
