// code/src/features/expert-teams/ExpertTeamCard.tsx
import type { ExpertTeam, ExpertTeamId } from './teams'
import { getExpertAvatarUrl } from './expertAvatar'

interface ExpertTeamCardProps {
  team: ExpertTeam
  onStart: (id: ExpertTeamId) => void
}

export function ExpertTeamCard({ team, onStart }: ExpertTeamCardProps) {
  return (
    <button
      type="button"
      onClick={() => onStart(team.id)}
      aria-label={`启动 ${team.name}`}
      className="flex h-full w-full flex-col gap-3 rounded-lg border border-border bg-card p-4 text-left text-card-foreground transition-colors hover:border-primary/50 hover:bg-accent/30"
    >
      <div className="flex items-center gap-2">
        <span className="text-2xl leading-none" aria-hidden>
          {team.emoji}
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
            const avatarUrl = getExpertAvatarUrl(team.id, expert.name)
            return (
              <div
                key={expert.name}
                title={`${expert.name} — ${expert.persona}`}
                className="flex flex-col items-center gap-0.5"
              >
                <span className="flex h-9 w-9 items-center justify-center overflow-hidden rounded-full border border-border bg-muted/40">
                  {avatarUrl ? (
                    <img
                      src={avatarUrl}
                      alt=""
                      className="h-full w-full object-cover"
                    />
                  ) : (
                    <span aria-hidden className="text-lg">
                      {expert.emoji}
                    </span>
                  )}
                </span>
                <span className="max-w-[64px] truncate text-[10px] leading-tight text-muted-foreground">
                  {expert.name}
                </span>
              </div>
            )
          })}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">主持人按议题召集</p>
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
      </div>
    </button>
  )
}
