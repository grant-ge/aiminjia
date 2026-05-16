import type { KeyboardEvent, ReactNode } from 'react'

interface SkillCardProps {
  title: string
  meta: string
  desc: string
  iconNode: ReactNode
  iconBg?: string
  onClick?: () => void
  size?: 'hot' | 'office'
  actionsSlot?: ReactNode
  /**
   * Optional version chip shown next to the title (e.g. "v1.2"). Rendered
   * only when non-empty. Source: `SkillInfo.version`, pulled from the
   * SKILL.md frontmatter `version:` field by the backend.
   */
  version?: string | null
}

export function SkillCard({ title, meta, desc, iconNode, iconBg = 'bg-brand-primary-subtle', onClick, size = 'office', actionsSlot, version }: SkillCardProps) {
  const isHot = size === 'hot'
  const height = isHot ? 'h-[140px]' : 'h-[120px]'
  const iconSize = isHot ? 'h-9 w-9' : 'h-[34px] w-[34px]'

  const interactiveProps = onClick
    ? {
        role: 'button',
        tabIndex: 0,
        onClick,
        onKeyDown: (e: KeyboardEvent<HTMLDivElement>) => {
          if (e.key === 'Enter' || e.key === ' ') onClick()
        },
      }
    : {}
  const interactiveClass = onClick
    ? 'cursor-pointer hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring'
    : ''

  return (
    <div
      data-testid="skill-card"
      {...interactiveProps}
      className={`group relative flex ${height} flex-col rounded-lg border border-border bg-card p-4 transition-all duration-150 ${interactiveClass}`}
    >
      <div className="flex items-center gap-2.5">
        <div className={`flex ${iconSize} shrink-0 items-center justify-center rounded-md ${iconBg}`}>
          {iconNode}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="truncate text-sm font-semibold text-foreground">{title}</span>
            {version ? (
              <span
                data-testid="skill-card-version"
                title={version}
                className="shrink-0 rounded-full border border-border bg-muted px-1.5 py-0 font-mono text-[10px] leading-relaxed text-muted-foreground"
              >
                v{version}
              </span>
            ) : null}
          </div>
          <span className="text-xs font-medium text-brand-secondary">{meta}</span>
        </div>
      </div>
      <p className="mt-2.5 line-clamp-2 text-xs text-muted-foreground">{desc}</p>
      {actionsSlot ? (
        <div
          className="absolute right-4 top-4"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          {actionsSlot}
        </div>
      ) : null}
    </div>
  )
}
