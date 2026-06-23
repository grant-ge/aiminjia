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
  layout?: 'card' | 'row'
  /**
   * Optional version chip shown next to the title (e.g. "1.2"). Rendered
   * only when non-empty. Source: `SkillInfo.version`, pulled from the
   * SKILL.md frontmatter `version:` field by the backend. Canonical form
   * is MAJOR.MINOR with no "v" prefix (mirrors lotus DB storage).
   */
  version?: string | null
  sourceLabel?: string | null
  /** Skill id (e.g. `demo-skill`)—used for e2e selectors. */
  skillId?: string
  /** Skill source (`builtin` / `user`)—used for e2e selectors. */
  skillSource?: string
  skillEnabled?: boolean
  marketCard?: boolean
  marketInstalled?: boolean
  marketPackageId?: string
}

export function SkillCard({ title, meta, desc, iconNode, iconBg = 'bg-[rgba(var(--primary-rgb),0.10)] text-primary', onClick, size = 'office', actionsSlot, layout = 'card', version, sourceLabel, skillId, skillSource, skillEnabled, marketCard, marketInstalled, marketPackageId }: SkillCardProps) {
  const isHot = size === 'hot'
  const height = isHot ? 'min-h-32' : 'min-h-28'
  const isRow = layout === 'row'
  const avatarText = Array.from(title.trim())[0]?.toUpperCase() ?? '?'
  const cardHeaderPadding = !isRow && actionsSlot ? 'pr-12' : 'pr-0'
  const chipClass = 'shrink-0 rounded-[2px] border border-border bg-card px-1.5 py-0 text-2xs font-medium leading-4 text-muted-foreground'

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
      data-aijia-skill-market-package-id={marketPackageId}
      data-aijia-skill-description={desc}
      {...interactiveProps}
      data-aijia-skill-card-layout={layout}
      className={`group relative flex rounded-md border border-border/65 bg-card shadow-[var(--shadow-skill-card)] transition-[border-color,background-color,box-shadow] duration-150 ${isRow ? 'min-h-20 flex-row items-center gap-3 px-3 py-3' : `${height} flex-col p-3`} ${interactiveClass}`}
    >
      <div className={isRow ? 'flex min-w-0 flex-1 items-center gap-2.5' : 'flex items-center gap-2.5'}>
        <div
          data-testid="skill-card-avatar"
          className={`flex ${isRow ? 'h-9 w-9' : 'h-8 w-8'} shrink-0 items-center justify-center rounded-md ${iconBg}`}
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
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div data-testid="skill-card-title-row" className="flex min-w-0 items-center gap-1.5">
            <span data-testid="skill-card-title-main" className={`flex min-w-0 max-w-full flex-1 items-center gap-1.5 ${isRow ? '' : cardHeaderPadding}`}>
              <span data-testid="skill-card-title" className="min-w-0 truncate text-sm font-semibold leading-5 text-foreground">{title}</span>
              {isRow && version ? (
                <span
                  data-testid="skill-card-version"
                  title={version}
                  className={chipClass}
                >
                  {version}
                </span>
              ) : null}
              {sourceLabel ? (
                <span
                  data-testid="skill-card-source"
                  className={chipClass}
                >
                  {sourceLabel}
                </span>
              ) : null}
            </span>
          </div>
          {isRow ? (
            <p className="min-w-0 truncate text-xs leading-5 text-muted-foreground">{desc || meta}</p>
          ) : (
            <div data-testid="skill-card-meta-row" className={`flex min-w-0 items-center gap-1.5 ${cardHeaderPadding}`}>
              <span className="truncate text-xs font-medium leading-4 text-muted-foreground">{meta}</span>
              {version ? (
                <span
                  data-testid="skill-card-version"
                  title={version}
                  className={chipClass}
                >
                  {version}
                </span>
              ) : null}
            </div>
          )}
        </div>
      </div>
      {isRow ? null : <p className="mt-2 line-clamp-2 text-xs leading-5 text-muted-foreground">{desc || meta}</p>}
      {actionsSlot ? (
        <div
          data-testid="skill-card-actions"
          className={isRow ? 'shrink-0' : 'absolute right-3 top-3'}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          {actionsSlot}
        </div>
      ) : null}
    </div>
  )
}
