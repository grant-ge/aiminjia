/**
 * @designSource design.pen#YQ44C tpl1
 * @sizing r-14 border 1 padding 18 gap 10
 */
import { useTranslation } from 'react-i18next'

import type { RecurrenceRule } from '@/lib/tauri'
import { Button } from '@/components/ui/button'

export interface ScheduleTemplate {
  title: string
  desc: string
  prompt: string
  rule?: RecurrenceRule | null
}

interface ScheduleTemplateCardProps {
  template: ScheduleTemplate
  onPick: (template: ScheduleTemplate) => void
}

export function ScheduleTemplateCard({ template, onPick }: ScheduleTemplateCardProps) {
  const { t } = useTranslation()
  return (
    <div
      data-testid="schedule-template-card"
      className="group flex w-full items-start justify-between gap-3 rounded-md border border-border/70 bg-background/65 px-3 py-2.5 transition-[border-color,background-color] hover:border-foreground/20 hover:bg-background"
    >
      <div className="min-w-0">
        <div className="text-sm font-semibold leading-5 text-foreground">{template.title}</div>
        <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">{template.desc}</p>
      </div>
      <div className="shrink-0 pt-0.5">
        <Button variant="outline" onClick={() => onPick(template)}>
          {t('schedules.template.useThis')}
        </Button>
      </div>
    </div>
  )
}
