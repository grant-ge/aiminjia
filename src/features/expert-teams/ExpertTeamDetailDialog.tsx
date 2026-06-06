import { RefreshCw, SendHorizontal, UsersRound } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { getExpertAvatarUrl } from './expertAvatar'
import { getExpertTeamLogo } from './teamLogo'
import type { ExpertTeam, ExpertTeamId } from './teams'

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

  const logo = getExpertTeamLogo(team.id)
  const TeamLogo = logo.icon
  const meta = [
    ...(team.workplaceCategoryName
      ? [{ label: t('ExpertTeams.detail.category'), value: team.workplaceCategoryName }]
      : []),
    {
      label: t('ExpertTeams.detail.members'),
      value: team.experts.length > 0
        ? t('ExpertTeams.detail.memberCount', { count: team.experts.length })
        : t('ExpertTeams.directorInvites'),
    },
    { label: t('ExpertTeams.detail.mode'), value: styleLabel(team.facilitationStyle, i18n.language) },
  ]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[86vh] max-w-3xl overflow-hidden p-0" data-aijia-expert-team-detail>
        <DialogTitle className="sr-only">{team.name}</DialogTitle>
        <DialogDescription className="sr-only">{team.tagline}</DialogDescription>
        <div className="flex max-h-[86vh] flex-col overflow-hidden">
          <div className="flex items-start gap-5 border-b border-border px-6 py-5 pr-12">
            <div className={`flex h-[88px] w-[88px] shrink-0 items-center justify-center rounded-[22px] ${logo.className}`}>
              <TeamLogo className="h-10 w-10" />
            </div>
            <div className="min-w-0 flex-1">
              <h2 className="truncate text-2xl font-bold leading-tight text-foreground">{team.name}</h2>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{team.tagline}</p>
            </div>
          </div>

          <div className="min-h-0 overflow-auto px-6 py-5">
            <div className="flex flex-wrap gap-x-12 gap-y-4">
              {meta.map((item) => (
                <div key={item.label} className="flex min-w-[120px] flex-col gap-1.5">
                  <span className="text-xs font-medium text-muted-foreground">{item.label}</span>
                  <span className="text-sm text-foreground">{item.value}</span>
                </div>
              ))}
            </div>

            <section className="mt-6 flex flex-col gap-3">
              <h3 className="text-sm font-semibold text-foreground">{t('ExpertTeams.members')}</h3>
              {team.experts.length > 0 ? (
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                  {team.experts.map((expert) => {
                    const avatarUrl = getExpertAvatarUrl(team.id, expert.avatarName ?? expert.name)
                    return (
                      <div key={expert.name} className="flex items-start gap-3 rounded-lg border border-border bg-card px-3 py-3">
                        <span className="flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-full border border-border bg-muted/40">
                          {avatarUrl ? (
                            <img src={avatarUrl} alt="" className="h-full w-full object-cover" />
                          ) : (
                            <span className="text-sm font-semibold">{Array.from(expert.name)[0]}</span>
                          )}
                        </span>
                        <span className="min-w-0">
                          <span className="block truncate text-sm font-medium text-foreground">{expert.name}</span>
                          <span className="mt-0.5 line-clamp-2 block text-xs leading-relaxed text-muted-foreground">
                            {expert.persona}
                          </span>
                        </span>
                      </div>
                    )
                  })}
                </div>
              ) : (
                <div className="flex items-center gap-3 rounded-lg border border-border bg-muted/20 px-4 py-4 text-sm text-muted-foreground">
                  <UsersRound className="h-4 w-4 text-primary" />
                  {t('ExpertTeams.directorInvites')}
                </div>
              )}
            </section>

            <section className="mt-6 flex flex-col gap-3">
              <h3 className="text-sm font-semibold text-foreground">{t('ExpertTeams.detail.examples')}</h3>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                {team.examples.map((example) => (
                  <div key={example} className="rounded-lg border border-border bg-muted/20 px-4 py-3 text-sm leading-relaxed text-foreground">
                    {example}
                  </div>
                ))}
              </div>
            </section>
          </div>
          <div className="flex shrink-0 justify-end border-t border-border bg-card px-6 py-4">
            <Button
              type="button"
              className="min-w-[128px] gap-1.5 rounded-full px-5"
              disabled={busy}
              onClick={() => onStart(team.id)}
            >
              {busy ? <RefreshCw className="h-4 w-4 animate-spin" /> : <SendHorizontal className="h-4 w-4" />}
              {busy ? t('ExpertTeams.starting') : t('ExpertTeams.summon')}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
