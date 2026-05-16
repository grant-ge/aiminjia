import type { ExpertTeam } from '@/features/expert-teams/teams'

interface ExpertTeamWelcomeProps {
  team: ExpertTeam
}

export function ExpertTeamWelcome({ team }: ExpertTeamWelcomeProps) {
  const memberLabel =
    team.experts.length > 0
      ? team.experts.map((e) => e.name).join(' · ')
      : '主持人将按议题召集 3-5 位专家'

  return (
    <div className="mx-auto flex w-full max-w-[640px] flex-col items-center gap-5 px-6 py-10 text-center">
      <div className="text-5xl leading-none" aria-hidden>
        {team.emoji}
      </div>
      <div className="space-y-1.5">
        <h2 className="text-xl font-semibold text-foreground">{team.name}</h2>
        <p className="text-sm text-muted-foreground">{team.tagline}</p>
      </div>

      <div className="w-full rounded-lg border border-border bg-card px-4 py-3 text-left">
        <div className="text-xs text-muted-foreground">团队成员</div>
        <div className="mt-1 text-sm text-foreground">{memberLabel}</div>
      </div>

      <div className="w-full space-y-2">
        <div className="text-sm font-medium text-foreground">你可以这样开场：</div>
        <ul className="space-y-1.5 text-sm text-muted-foreground">
          {team.examples.map((example) => (
            <li
              key={example}
              className="rounded-md border border-border bg-card px-3 py-2 text-left"
            >
              「{example}」
            </li>
          ))}
        </ul>
      </div>

      <p className="text-xs text-muted-foreground">
        在下方输入你的议题，回车发送 — 主持人会拉起团队开始讨论。
      </p>
    </div>
  )
}
