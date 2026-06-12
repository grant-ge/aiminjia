import { UsersRound } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { ExpertAvatarView } from './ExpertAvatarView'
import { ExpertTeamAvatarStack } from './ExpertTeamAvatarStack'
import { getExpertAvatarVisual } from './expertAvatar'
import type { ExpertTeam, ExpertTeamId } from './teams'
import { Button } from '@/components/ui/button'

interface ExpertTeamDetailDialogProps {
  team: ExpertTeam | null
  open: boolean
  busy: boolean
  onOpenChange: (open: boolean) => void
  onStart: (id: ExpertTeamId) => void
}

function styleLabel(style: ExpertTeam['facilitationStyle'], language: string): string {
  const en = language.toLowerCase().startsWith('en')
  if (style === 'debate') return en ? 'Debate' : '辩论推演'
  if (style === 'open') return en ? 'Dynamic roundtable' : '动态圆桌'
  return en ? 'Round-robin discussion' : '多角色轮询'
}

export function ExpertTeamDetailDialog({
  team,
  open,
  busy,
  onOpenChange,
  onStart,
}: ExpertTeamDetailDialogProps) {
  const { t, i18n } = useTranslation()
  if (!team) return null

  const memberLabel = team.experts.length > 0
    ? t('ExpertTeams.detail.memberCount', { count: team.experts.length })
    : t('ExpertTeams.directorInvites')
  const styleText = styleLabel(team.facilitationStyle, i18n.language)
  const description = team.description?.trim() || team.tagline
  const summaryChips = [
    team.workplaceCategoryName,
    styleText,
    memberLabel,
  ].filter((item): item is string => Boolean(item))
  const meta = [
    ...(team.workplaceCategoryName
      ? [{ label: t('ExpertTeams.detail.category'), value: team.workplaceCategoryName }]
      : []),
    {
      label: t('ExpertTeams.detail.members'),
      value: memberLabel,
    },
    { label: t('ExpertTeams.detail.mode'), value: styleText },
  ]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(86vh,calc(100vh-32px))] w-[calc(100vw-32px)] max-w-[680px] gap-0 overflow-hidden rounded-md border-border/70 p-0 shadow-[0_18px_52px_rgba(15,23,42,0.18)]" data-aijia-expert-team-detail>
        <DialogTitle className="sr-only">{team.name}</DialogTitle>
        <DialogDescription className="sr-only">{description}</DialogDescription>
        <div className="flex max-h-[min(86vh,calc(100vh-32px))] flex-col overflow-hidden">
          <div className="flex items-start gap-4 border-b border-border/70 bg-card px-5 py-5 pr-14" data-aijia-expert-team-detail-chrome>
            <ExpertTeamAvatarStack team={team} size="lg" />
            <div className="min-w-0 flex-1">
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <h2 className="truncate text-[20px] font-bold leading-6 text-foreground">{team.name}</h2>
                <span className="rounded-[2px] bg-brand-primary-subtle px-2 py-0.5 text-xs font-medium leading-4 text-primary">
                  {styleText}
                </span>
              </div>
              <p className="mt-2 line-clamp-2 text-xs leading-5 text-muted-foreground">{description}</p>
              <div className="mt-2 flex max-h-6 flex-wrap gap-1.5 overflow-hidden">
                {summaryChips.slice(0, 3).map((chip) => (
                  <span
                    key={chip}
                    className="max-w-full truncate rounded-[2px] bg-muted px-2 py-0.5 text-2xs text-muted-foreground"
                  >
                    {chip}
                  </span>
                ))}
              </div>
            </div>
          </div>

          <div className="min-h-0 overflow-auto px-5 py-4">
            <section>
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('ExpertTeams.detail.intro')}</h3>
              <p className="mt-2 text-xs leading-5 text-foreground">{description}</p>
            </section>

            <section className="mt-4">
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('ExpertTeams.detail.overview')}</h3>
              <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
                {meta.map((item) => (
                  <div key={item.label} className="rounded-md border border-border/70 bg-muted/20 px-2.5 py-2">
                    <span className="block text-xs leading-4 text-muted-foreground">{item.label}</span>
                    <span className="mt-1 block truncate text-xs font-semibold leading-5 text-foreground">{item.value}</span>
                  </div>
                ))}
              </div>
            </section>

            <section className="mt-4">
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('ExpertTeams.members')}</h3>
              {team.experts.length > 0 ? (
                <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-2">
                  {team.experts.map((expert) => {
                    const avatarVisual = getExpertAvatarVisual(team.id, expert)
                    return (
                      <div key={expert.name} className="flex items-start gap-3 rounded-md border border-border/70 bg-card px-2.5 py-2.5 shadow-[0_1px_2px_rgba(0,0,0,0.03)]">
                        <span className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-md bg-muted/40">
                          <ExpertAvatarView
                            visual={avatarVisual}
                            fallback={expert.emoji || Array.from(expert.name)[0]}
                            className="text-base leading-none"
                          />
                        </span>
                        <span className="min-w-0">
                          <span className="block truncate text-xs font-semibold leading-5 text-foreground">{expert.name}</span>
                          <span className="line-clamp-2 block text-xs leading-5 text-muted-foreground">
                            {expert.persona}
                          </span>
                        </span>
                      </div>
                    )
                  })}
                </div>
              ) : (
                <div className="mt-2 rounded-md border border-border/70 bg-muted/20 px-3 py-2.5 text-xs leading-5 text-muted-foreground">
                  {t('ExpertTeams.directorInvites')}
                </div>
              )}
            </section>

            <section className="mt-4">
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('ExpertTeams.detail.examples')}</h3>
              <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-2">
                {team.examples.map((example) => (
                  <div key={example} className="rounded-md border border-border/70 bg-card px-3 py-2.5 text-xs leading-5 text-foreground shadow-[0_1px_2px_rgba(0,0,0,0.03)]">
                    {example}
                  </div>
                ))}
              </div>
            </section>
          </div>
          <div className="flex shrink-0 justify-end gap-2 border-t border-border/70 bg-card px-5 py-3">
            <Button
              type="button"
              loading={busy}
              icon={<UsersRound />}
              onClick={() => onStart(team.id)}
              data-aijia-expert-team-action="start"
              data-aijia-expert-team-id={team.id}
              data-aijia-expert-team-name={team.name}
            >
              {busy ? t('ExpertTeams.starting') : t('ExpertTeams.summon')}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
