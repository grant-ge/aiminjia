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
  skillEnabled?: boolean
  marketCard?: boolean
  marketInstalled?: boolean
}

export function SkillCard({ title, meta, desc, iconNode, iconBg = 'bg-[#fbeed8] text-[#d19b00]', onClick, size = 'office', actionsSlot, version, skillId, skillSource, skillEnabled, marketCard, marketInstalled }: SkillCardProps) {
  const isHot = size === 'hot'
  const height = isHot ? 'min-h-32' : 'min-h-28'
  const avatarText = Array.from(title.trim())[0]?.toUpperCase() ?? '?'

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
    ? 'cursor-pointer hover:border-primary/40 hover:bg-card/95 hover:shadow-[var(--shadow-skill-card-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring'
    : ''

  return (
    <div
      data-testid="skill-card"
      data-aijia-skill-card
      data-aijia-skill-id={skillId}
      data-aijia-skill-source={skillSource}
      data-aijia-skill-enabled={skillEnabled === undefined ? undefined : String(skillEnabled)}
      data-aijia-skill-market-card={marketCard ? 'true' : undefined}
      data-aijia-skill-installed={marketInstalled === undefined ? undefined : String(marketInstalled)}
      {...interactiveProps}
      className={`group relative flex ${height} flex-col rounded-md border border-border/65 bg-card p-3 shadow-[var(--shadow-skill-card)] transition-[border-color,background-color,box-shadow] duration-150 ${interactiveClass}`}
    >
      <div className="flex items-center gap-2.5">
        <div
          data-testid="skill-card-avatar"
          className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-md ${iconBg}`}
        >
          {iconNode ?? (
            <span
              data-testid="skill-card-fallback-avatar"
              className="text-[length:var(--text-lg)] font-semibold leading-none text-inherit"
              aria-hidden="true"
            >
              {avatarText}
            </span>
          )}
        </div>
        <div className={`flex min-w-0 flex-1 flex-col gap-0.5 ${actionsSlot ? 'pr-28' : ''}`}>
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="truncate text-sm font-semibold leading-5 text-foreground">{title}</span>
          </div>
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="truncate text-xs font-medium leading-4 text-muted-foreground">{meta}</span>
            {version ? (
              <span
                data-testid="skill-card-version"
                title={version}
                className="shrink-0 rounded-[2px] border border-border bg-muted px-1.5 py-0 font-mono text-2xs leading-4 text-muted-foreground"
              >
                {version}
              </span>
            ) : null}
          </div>
        </div>
      </div>
      <p className="mt-2 line-clamp-2 text-xs leading-5 text-muted-foreground">{desc}</p>
      {actionsSlot ? (
        <div
          className="absolute right-3 top-3"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          {actionsSlot}
        </div>
      ) : null}
    </div>
  )
}
