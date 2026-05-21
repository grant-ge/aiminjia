import { useCallback, useState } from 'react'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { buildDirectorPrompt } from '@/features/expert-teams/buildDirectorPrompt'
import type { ExpertTeam } from '@/features/expert-teams/teams'
import { getExpertAvatarUrl } from '@/features/expert-teams/expertAvatar'
import { getExpertTeamLogo } from '@/features/expert-teams/teamLogo'
import { useSettingsStore } from '@/stores/settingsStore'

interface ExpertTeamWelcomeProps {
  team: ExpertTeam
}

export function ExpertTeamWelcome({ team }: ExpertTeamWelcomeProps) {
  const { sendUserMessage } = useChat()
  const [picking, setPicking] = useState<string | null>(null)
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')
  const logo = getExpertTeamLogo(team.id)
  const TeamLogo = logo.icon

  const welcomeWidthClass = chatWidthMode === 'centered' ? 'mx-auto max-w-[640px]' : 'w-full'

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
    <div
      data-testid="expert-team-welcome-shell"
      className={`flex ${welcomeWidthClass} flex-col items-center gap-5 px-6 py-10 text-center`}
    >
      <div
        data-testid="expert-team-welcome-logo"
        className={`flex h-16 w-16 items-center justify-center rounded-2xl ${logo.className}`}
        aria-hidden
      >
        <TeamLogo className="h-8 w-8" />
      </div>
      <div className="space-y-1.5">
        <h2 className="text-xl font-semibold text-foreground">{team.name}</h2>
        <p className="text-sm text-muted-foreground">{team.tagline}</p>
      </div>

      <div className="w-full rounded-lg border border-border bg-card px-4 py-3 text-left">
        <div className="text-xs text-muted-foreground">团队成员</div>
        {team.experts.length > 0 ? (
          <div className="mt-2 flex flex-wrap gap-2">
            {team.experts.map((expert) => {
              const avatarUrl = getExpertAvatarUrl(team.id, expert.name)
              return (
                <span
                  key={expert.name}
                  className="inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-2 py-1 text-sm text-foreground"
                  title={expert.persona}
                >
                  <span className="flex h-5 w-5 items-center justify-center overflow-hidden rounded-full bg-muted text-xs">
                    {avatarUrl ? <img src={avatarUrl} alt="" className="h-full w-full object-cover" /> : expert.emoji}
                  </span>
                  {expert.name}
                </span>
              )
            })}
          </div>
        ) : (
          <div className="mt-1 text-sm text-foreground">主持人将按议题召集 3-5 位专家</div>
        )}
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
