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
      className="flex w-full flex-col gap-2.5 rounded-md border border-border bg-card p-4 shadow-[var(--shadow-card)] transition-shadow hover:shadow-[var(--shadow-card-hover)]"
    >
      <div className="text-[15px] font-semibold leading-[22px] text-foreground">{template.title}</div>
      <p className="flex-1 text-[13px] leading-5 text-muted-foreground">{template.desc}</p>
      <div>
        <Button variant="secondary" onClick={() => onPick(template)}>
          {t('schedules.template.useThis')}
        </Button>
      </div>
    </div>
  )
}
