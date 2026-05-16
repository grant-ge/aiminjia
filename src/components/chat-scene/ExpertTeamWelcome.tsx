import { useCallback, useState } from 'react'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { buildDirectorPrompt } from '@/features/expert-teams/buildDirectorPrompt'
import type { ExpertTeam } from '@/features/expert-teams/teams'

interface ExpertTeamWelcomeProps {
  team: ExpertTeam
}

export function ExpertTeamWelcome({ team }: ExpertTeamWelcomeProps) {
  const { sendUserMessage } = useChat()
  const [picking, setPicking] = useState<string | null>(null)

  const memberLabel =
    team.experts.length > 0
      ? team.experts.map((e) => e.name).join(' · ')
      : '主持人将按议题召集 3-5 位专家'

  const handlePick = useCallback(async (example: string) => {
    if (picking) return
    setPicking(example)
    try {
      const prompt = buildDirectorPrompt(team, example)
      const files: PendingFileInfo[] | undefined = undefined
      await sendUserMessage(prompt, files, null)
    } catch (err) {
      console.error('[ExpertTeamWelcome] sendUserMessage failed:', err)
    } finally {
      setPicking(null)
    }
  }, [picking, sendUserMessage, team])

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
        <div className="text-sm font-medium text-foreground">点击一个议题直接开始：</div>
        <ul className="space-y-1.5">
          {team.examples.map((example) => {
            const isPicking = picking === example
            return (
              <li key={example}>
                <button
                  type="button"
                  disabled={picking !== null}
                  onClick={() => void handlePick(example)}
                  className="w-full rounded-md border border-border bg-card px-3 py-2 text-left text-sm text-foreground transition-colors hover:border-primary/50 hover:bg-accent/30 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {isPicking ? `正在启动：${example}…` : `「${example}」`}
                </button>
              </li>
            )
          })}
        </ul>
      </div>

      <p className="text-xs text-muted-foreground">
        或在下方输入你自己的议题，回车发送。
      </p>
    </div>
  )
}
