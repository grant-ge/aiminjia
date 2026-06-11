// code/src/features/expert-teams/ExpertTeamCard.tsx
import { useTranslation } from 'react-i18next'
import type { ExpertTeam, ExpertTeamId } from './teams'
import { Button } from '@/components/ui/button'
import { ExpertTeamAvatarStack } from './ExpertTeamAvatarStack'

const EXPERT_TEAM_CHIP_CLASS =
  'max-w-full truncate rounded-[2px] bg-muted px-2 py-0.5 text-2xs text-muted-foreground'

function styleLabel(style: ExpertTeam['facilitationStyle'], language: string): string {
  const en = language.toLowerCase().startsWith('en')
  if (style === 'debate') return en ? 'Debate' : '辩论推演'
  if (style === 'open') return en ? 'Dynamic roundtable' : '动态圆桌'
  return en ? 'Round-robin discussion' : '多角色轮询'
}

interface ExpertTeamCardProps {
  team: ExpertTeam
  onStart: (id: ExpertTeamId) => void
}

export function ExpertTeamCard({ team, onStart }: ExpertTeamCardProps) {
  const { t, i18n } = useTranslation()
  const memberLabel = team.experts.length > 0
    ? t('ExpertTeams.detail.memberCount', { count: team.experts.length })
    : t('ExpertTeams.openTableHint')
  const subtitle = `${memberLabel} / ${styleLabel(team.facilitationStyle, i18n.language)}`
  const description = team.description?.trim() || team.tagline

  return (
    <Button unstyled
      type="button"
      data-aijia-expert-team-card
      data-aijia-expert-team-id={team.id}
      data-aijia-expert-team-name={team.name}
      onClick={() => onStart(team.id)}
      aria-label={t('ExpertTeams.openTeamDetail', { name: team.name })}
      className="group flex h-[154px] w-full flex-col gap-2 rounded-md border border-border/50 bg-card p-3 text-left text-card-foreground shadow-[0_1px_3px_rgba(0,0,0,0.035)] transition-all hover:border-border/70 hover:bg-muted/20"
    >
      <div className="flex min-w-0 items-start gap-3">
        <ExpertTeamAvatarStack team={team} />
        <div className="min-w-0 pt-0.5">
          <p className="truncate text-sm font-semibold leading-[22px] text-foreground">{team.name}</p>
          <p className="truncate text-xs leading-4 text-muted-foreground">{subtitle}</p>
        </div>
      </div>

      <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">{description}</p>

      <div className="mt-auto flex max-h-6 flex-wrap gap-1.5 overflow-hidden">
        {team.workplaceCategoryName && (
          <span className={EXPERT_TEAM_CHIP_CLASS}>
            {team.workplaceCategoryName}
          </span>
        )}
        {team.examples.slice(0, 3).map((ex) => (
          <span
            key={ex}
            className={EXPERT_TEAM_CHIP_CLASS}
          >
            {ex}
          </span>
        ))}
      </div>
    </Button>
  )
}
