// code/src/features/expert-teams/ExpertTeamCard.tsx
import { useTranslation } from 'react-i18next'
import type { ExpertTeam, ExpertTeamId } from './teams'
import { ExpertAvatarView } from './ExpertAvatarView'
import { getExpertAvatarVisual } from './expertAvatar'
import { getExpertTeamLogo } from './teamLogo'

interface ExpertTeamCardProps {
  team: ExpertTeam
  onStart: (id: ExpertTeamId) => void
}

export function ExpertTeamCard({ team, onStart }: ExpertTeamCardProps) {
  const { t } = useTranslation()
  const logo = getExpertTeamLogo(team.id)
  const TeamLogo = logo.icon

  return (
    <button
      type="button"
      data-aijia-expert-team-card
      data-aijia-expert-team-id={team.id}
      data-aijia-expert-team-name={team.name}
      onClick={() => onStart(team.id)}
      aria-label={t('ExpertTeams.openTeamDetail', { name: team.name })}
      className="flex min-h-[212px] w-full flex-col gap-3 rounded-md border border-border bg-card p-4 text-left text-card-foreground shadow-[var(--shadow-card)] transition-all hover:border-primary/50 hover:shadow-[var(--shadow-card-hover)]"
    >
      <div className="flex items-center gap-2">
        <span
          className={`flex h-11 w-11 shrink-0 items-center justify-center rounded-md ${logo.className}`}
          data-testid={`expert-team-logo-${team.id}`}
          aria-hidden
        >
          <TeamLogo className="h-5 w-5" />
        </span>
        <span className="min-w-0 truncate text-[15px] font-semibold leading-[22px]">{team.name}</span>
      </div>
      <p className="line-clamp-2 text-[13px] leading-5 text-muted-foreground">{team.tagline}</p>

      {/* Member roster — pre-generated DiceBear "personas" avatars,
          stored under public/expert-avatars/<teamId>/. Open-table teams
          have empty experts[] (主持人按议题召集); we show a hint instead. */}
      {team.experts.length > 0 ? (
        <div className="flex flex-wrap gap-2.5" data-testid="expert-team-roster">
          {team.experts.slice(0, 4).map((expert) => {
            const avatarVisual = getExpertAvatarVisual(team.id, expert)
            return (
              <div
                key={expert.name}
                title={`${expert.name} — ${expert.persona}`}
                className="flex min-w-0 flex-col items-center gap-0.5"
              >
                <span className="flex h-9 w-9 items-center justify-center overflow-hidden rounded-md border border-border bg-muted/40">
                  <ExpertAvatarView visual={avatarVisual} fallback={expert.emoji} className="text-lg" />
                </span>
                <span className="max-w-[58px] truncate text-[10px] leading-tight text-muted-foreground">
                  {expert.name}
                </span>
              </div>
            )
          })}
          {team.experts.length > 4 ? (
            <div className="flex flex-col items-center gap-0.5">
              <span className="flex h-9 w-9 items-center justify-center rounded-md border border-border bg-muted text-xs font-medium text-muted-foreground">
                +{team.experts.length - 4}
              </span>
              <span className="text-[10px] leading-tight text-muted-foreground">{t('ExpertTeams.moreMembers')}</span>
            </div>
          ) : null}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">{t('ExpertTeams.openTableHint')}</p>
      )}

      <div className="mt-auto flex flex-wrap gap-1.5">
        {team.examples.map((ex) => (
          <span
            key={ex}
            className="max-w-full truncate rounded-md bg-muted px-2 py-0.5 text-xs text-muted-foreground"
          >
            {ex}
          </span>
        ))}
        <span className="rounded-md bg-brand-primary-subtle px-2 py-0.5 text-xs font-medium text-primary">
          {t('ExpertTeams.viewDetail')}
        </span>
      </div>
    </button>
  )
}
