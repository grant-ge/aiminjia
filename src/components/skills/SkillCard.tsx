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
   * Optional version chip shown next to the title (e.g. "1.2"). Rendered
   * only when non-empty. Source: `SkillInfo.version`, pulled from the
   * SKILL.md frontmatter `version:` field by the backend. Canonical form
   * is MAJOR.MINOR with no "v" prefix (mirrors lotus DB storage).
   */
  version?: string | null
  /** Skill id (e.g. `demo-skill`)—used for e2e selectors. */
  skillId?: string
  /** Skill source (`builtin` / `user`)—used for e2e selectors. */
  skillSource?: string
}

export function SkillCard({ title, meta, desc, iconNode, iconBg = 'bg-brand-primary-subtle', onClick, size = 'office', actionsSlot, version, skillId, skillSource }: SkillCardProps) {
  const isHot = size === 'hot'
  const height = isHot ? 'min-h-[156px]' : 'min-h-[140px]'
  const iconSize = isHot ? 'h-11 w-11' : 'h-10 w-10'

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
    ? 'cursor-pointer hover:border-primary/50 hover:shadow-[var(--shadow-card-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring'
    : ''

  return (
    <div
      data-testid="skill-card"
      data-aijia-skill-card
      data-aijia-skill-id={skillId}
      data-aijia-skill-source={skillSource}
      {...interactiveProps}
      className={`group relative flex ${height} flex-col rounded-md border border-border bg-card p-4 shadow-[var(--shadow-card)] transition-all duration-150 ${interactiveClass}`}
    >
      <div className="flex items-center gap-2.5">
        <div className={`flex ${iconSize} shrink-0 items-center justify-center rounded-md ${iconBg}`}>
          {iconNode}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="truncate text-[15px] font-semibold leading-[22px] text-foreground">{title}</span>
            {version ? (
              <span
                data-testid="skill-card-version"
                title={version}
                className="shrink-0 rounded-full border border-border bg-muted px-1.5 py-0 font-mono text-[10px] leading-relaxed text-muted-foreground"
              >
                {version}
              </span>
            ) : null}
          </div>
          <span className="text-xs font-medium text-muted-foreground">{meta}</span>
        </div>
      </div>
      <p className="mt-3 line-clamp-2 text-[13px] leading-5 text-muted-foreground">{desc}</p>
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
