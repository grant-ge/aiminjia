// code/src/features/expert-teams/ExpertTeamCard.tsx
import type { ExpertTeam, ExpertTeamId } from './teams'

interface ExpertTeamCardProps {
  team: ExpertTeam
  onStart: (id: ExpertTeamId) => void
}

export function ExpertTeamCard({ team, onStart }: ExpertTeamCardProps) {
  const expertCountLabel =
    team.experts.length > 0 ? `${team.experts.length} 位专家` : '主持人按议题召集'
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
      <p className="text-xs text-muted-foreground">{expertCountLabel}</p>
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
