import { ExternalLink } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'
import type { SkillCreatedCardPayload } from './aijiaCardPayload'

interface SkillCreatedCardProps {
  payload: SkillCreatedCardPayload
}

export function SkillCreatedCard({ payload }: SkillCreatedCardProps) {
  const { t, i18n } = useTranslation()
  const skill = useSkillStore((state) => state.getById(payload.skillId))
  const reload = useSkillStore((state) => state.reload)
  const setRoute = useUiStore((state) => state.setRoute)

  useEffect(() => {
    if (!skill) void reload().catch(() => undefined)
  }, [reload, skill])

  const isEnglish = i18n.language.toLowerCase().startsWith('en')
  const title = skill
    ? isEnglish
      ? skill.displayNameEn || skill.displayName || skill.id
      : skill.displayName || skill.id
    : payload.title || payload.skillId
  const description =
    skill?.shortDescription ||
    skill?.description ||
    payload.description ||
    t('resultCards.skill.fallbackDescription')
  const triggerText = skill?.triggerText || `/${payload.skillId}`

  return (
    <div
      className="group my-3 w-full rounded-md border border-border bg-card px-3 py-2.5 text-card-foreground shadow-none"
      data-aijia-result-card="skill_created"
    >
      <div className="flex items-start gap-2.5 pr-0.5">
        <div className="min-w-0 flex-1">
          <SkillField label={t('resultCards.skill.nameLabel', '技能名称')} value={title} prominent />
          <div className="mt-1.5 grid gap-1 text-sm leading-5">
            <SkillField label={t('resultCards.skill.triggerLabel', '触发方式')} value={triggerText} mono />
            <SkillField label={t('resultCards.skill.descriptionLabel', '技能描述')} value={description} />
          </div>
        </div>
        <Button
          unstyled
          type="button"
          aria-label={t('resultCards.skill.view')}
          onClick={() => setRoute({ kind: 'skill-detail', skillId: payload.skillId })}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border-transparent bg-transparent p-0 text-muted-foreground opacity-70 transition-colors hover:bg-muted hover:text-foreground hover:opacity-100 group-hover:opacity-100"
          data-testid="skill-created-card-view"
        >
          <ExternalLink className="h-3.5 w-3.5" aria-hidden />
        </Button>
      </div>
    </div>
  )
}

function SkillField({
  label,
  value,
  prominent = false,
  mono = false,
}: {
  label: string
  value: string
  prominent?: boolean
  mono?: boolean
}) {
  return (
    <div className="flex min-w-0 items-baseline gap-2">
      <span className="shrink-0 text-xs leading-5 text-muted-foreground">{label}</span>
      <span
        className={
          prominent
            ? 'min-w-0 flex-1 truncate text-[15px] font-semibold leading-5 text-foreground'
            : mono
              ? 'min-w-0 flex-1 truncate font-mono text-sm leading-5 text-foreground/82'
              : 'min-w-0 flex-1 line-clamp-2 text-sm leading-5 text-muted-foreground'
        }
      >
        {value}
      </span>
    </div>
  )
}
