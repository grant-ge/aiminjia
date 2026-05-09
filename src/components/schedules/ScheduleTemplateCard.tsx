/**
 * @designSource design.pen#YQ44C tpl1
 * @sizing r-14 border 1 padding 18 gap 10
 */
import { Button } from '@/components/ui/button'
import type { RecurrenceRule } from '@/lib/tauri'

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
  return (
    <div
      data-testid="schedule-template-card"
      className="flex w-full flex-col gap-2.5 rounded-[14px] border border-border bg-card p-[18px]"
    >
      <div className="text-[0.9375rem] font-semibold text-foreground">{template.title}</div>
      <p className="flex-1 text-[0.8125rem] text-muted-foreground">{template.desc}</p>
      <div>
        <Button variant="secondary" onClick={() => onPick(template)}>
          用此模板
        </Button>
      </div>
    </div>
  )
}
