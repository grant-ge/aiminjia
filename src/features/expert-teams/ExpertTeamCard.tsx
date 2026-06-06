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
      className="flex h-full w-full flex-col gap-3 rounded-lg border border-border bg-card p-4 text-left text-card-foreground transition-colors hover:border-primary/50 hover:bg-accent/30"
    >
      <div className="flex items-center gap-2">
        <span
          className={`flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-lg ${logo.className}`}
          data-testid={`expert-team-logo-${team.id}`}
          aria-hidden
        >
          <TeamLogo className="h-4 w-4" />
        </span>
        <span className="text-base font-medium">{team.name}</span>
      </div>
      <p className="text-sm text-muted-foreground">{team.tagline}</p>

      {/* Member roster — pre-generated DiceBear "personas" avatars,
          stored under public/expert-avatars/<teamId>/. Open-table teams
          have empty experts[] (主持人按议题召集); we show a hint instead. */}
      {team.experts.length > 0 ? (
        <div className="flex flex-wrap gap-2.5" data-testid="expert-team-roster">
          {team.experts.map((expert) => {
            const avatarVisual = getExpertAvatarVisual(team.id, expert)
            return (
              <div
                key={expert.name}
                title={`${expert.name} — ${expert.persona}`}
                className="flex flex-col items-center gap-0.5"
              >
                <span className="flex h-9 w-9 items-center justify-center overflow-hidden rounded-full border border-border bg-muted/40">
                  <ExpertAvatarView visual={avatarVisual} fallback={expert.emoji} className="text-lg" />
                </span>
                <span className="max-w-[64px] truncate text-[10px] leading-tight text-muted-foreground">
                  {expert.name}
                </span>
              </div>
            )
          })}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">{t('ExpertTeams.openTableHint')}</p>
      )}

      <div className="mt-auto flex flex-wrap gap-1.5">
        {team.examples.map((ex) => (
          <span
            key={ex}
            className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground"
          >
            {ex}
          </span>
        ))}
        <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs font-medium text-primary">
          {t('ExpertTeams.viewDetail')}
        </span>
      </div>
    </button>
  )
}
