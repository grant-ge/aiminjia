import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { buildDirectorPrompt } from '@/features/expert-teams/buildDirectorPrompt'
import type { ExpertTeam } from '@/features/expert-teams/teams'
import { ExpertAvatarView } from '@/features/expert-teams/ExpertAvatarView'
import { getExpertAvatarVisual } from '@/features/expert-teams/expertAvatar'
import { ExpertTeamAvatarStack } from '@/features/expert-teams/ExpertTeamAvatarStack'
import { useSettingsStore } from '@/stores/settingsStore'
import { Button } from '@/components/ui/button'

interface ExpertTeamWelcomeProps {
  team: ExpertTeam
}

export function ExpertTeamWelcome({ team }: ExpertTeamWelcomeProps) {
  const { t, i18n } = useTranslation()
  const { sendUserMessage } = useChat()
  const [picking, setPicking] = useState<string | null>(null)
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')

  const welcomeWidthClass = chatWidthMode === 'centered'
    ? 'mx-auto max-w-[680px]'
    : 'mx-auto w-full max-w-[820px]'
  const memberCountLabel = team.experts.length > 0
    ? t('ExpertTeams.memberCount', { count: team.experts.length, defaultValue: `${team.experts.length} 位专家` })
    : t('ExpertTeams.dynamicExperts', '动态召集专家')
  const facilitationLabel = team.facilitationStyle === 'debate'
    ? t('ExpertTeams.debateMode', '辩论推演')
    : team.facilitationStyle === 'open'
      ? t('ExpertTeams.openMode', '动态圆桌')
      : t('ExpertTeams.roundsMode', '多角色轮询')
  const teamSummary = team.description || team.workplaceCategoryDescription || team.tagline

  const handlePick = useCallback(async (example: string) => {
    if (picking) return
    setPicking(example)
    try {
      const prompt = buildDirectorPrompt(team, example, i18n.language)
      const files: PendingFileInfo[] | undefined = undefined
      await sendUserMessage(prompt, files, null)
    } catch (err) {
      console.error('[ExpertTeamWelcome] sendUserMessage failed:', err)
    } finally {
      setPicking(null)
    }
  }, [i18n.language, picking, sendUserMessage, team])

  return (
    <div
      data-testid="expert-team-welcome-shell"
      className={`flex ${welcomeWidthClass} flex-col gap-6 px-6 py-8 text-left`}
    >
      <div className="flex items-start gap-4">
        <ExpertTeamAvatarStack team={team} size="lg" />
        <div className="min-w-0 flex-1 pt-1">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <h2 className="truncate text-xl font-semibold leading-7 text-foreground">{team.name}</h2>
            <span className="rounded-[2px] bg-muted px-2 py-0.5 text-2xs font-medium text-muted-foreground">
              {memberCountLabel}
            </span>
            <span className="rounded-[2px] bg-muted px-2 py-0.5 text-2xs font-medium text-muted-foreground">
              {facilitationLabel}
            </span>
          </div>
          <p className="mt-1 line-clamp-2 text-sm leading-6 text-muted-foreground">{teamSummary}</p>
        </div>
      </div>

      <section className="w-full border-t border-[rgba(var(--border-rgb),0.70)] pt-4">
        <div className="flex items-center justify-between gap-3">
          <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('ExpertTeams.members')}</h3>
          <span className="truncate text-2xs text-[rgba(var(--muted-foreground-rgb),0.80)]">{team.tagline}</span>
        </div>
        {team.experts.length > 0 ? (
          <div className="mt-3 grid gap-2 sm:grid-cols-2">
            {team.experts.map((expert) => {
              const avatarVisual = getExpertAvatarVisual(team.id, expert)
              return (
                <div
                  key={expert.name}
                  className="flex min-w-0 items-center gap-2 rounded-md border border-[rgba(var(--border-rgb),0.70)] bg-card px-2.5 py-2"
                  title={expert.persona}
                >
                  <span className="flex h-7 w-7 shrink-0 items-center justify-center overflow-hidden rounded-full border border-[rgba(var(--border-rgb),0.70)] bg-muted text-xs">
                    <ExpertAvatarView visual={avatarVisual} fallback={expert.emoji} />
                  </span>
                  <span className="min-w-0">
                    <span className="block truncate text-xs font-semibold leading-4 text-foreground">{expert.name}</span>
                    <span className="block truncate text-2xs leading-4 text-muted-foreground">{expert.persona}</span>
                  </span>
                </div>
              )
            })}
          </div>
        ) : (
          <div className="mt-3 rounded-md border border-dashed border-[rgba(var(--border-rgb),0.80)] px-3 py-3 text-sm leading-6 text-muted-foreground">
            {t('ExpertTeams.directorInvites')}，{t('ExpertTeams.customTopicHint')}
          </div>
        )}
      </section>

      <section className="w-full border-t border-[rgba(var(--border-rgb),0.70)] pt-4">
        <div className="mb-3 flex items-center justify-between gap-3">
          <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('ExpertTeams.pickTopic')}</h3>
          <span className="truncate text-2xs text-[rgba(var(--muted-foreground-rgb),0.80)]">{t('ExpertTeams.customTopicHint')}</span>
        </div>
        <ul className="grid gap-2 sm:grid-cols-2">
          {team.examples.map((example) => {
            const isPicking = picking === example
            return (
              <li key={example}>
                <Button unstyled
                  type="button"
                  disabled={picking !== null}
                  onClick={() => void handlePick(example)}
                  className="flex min-h-11 w-full items-center rounded-md border border-[rgba(var(--border-rgb),0.70)] bg-card px-3 py-2 text-left text-sm leading-5 text-foreground transition-colors hover:border-[rgba(var(--primary-rgb),0.50)] hover:bg-[rgba(var(--accent-rgb),0.30)] disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {isPicking
                    ? t('ExpertTeams.startingTopic', { topic: example })
                    : t('ExpertTeams.topicQuote', { topic: example })}
                </Button>
              </li>
            )
          })}
        </ul>
      </section>
    </div>
  )
}
