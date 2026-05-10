/**
 * @designSource design.pen#YQ44C tpl1
 * @sizing r-14 border 1 padding 18 gap 10
 */
import { Button } from '@/components/ui/button'

interface ScheduleTemplateCardProps {
  title: string
  desc: string
  cta: { label: string; onClick: () => void }
}

export function ScheduleTemplateCard({ title, desc, cta }: ScheduleTemplateCardProps) {
  return (
    <div
      data-testid="schedule-template-card"
      className="flex w-full flex-col gap-2.5 rounded-lg border border-border bg-card p-5"
    >
      <div className="text-md font-semibold text-foreground">{title}</div>
      <p className="flex-1 text-sm text-muted-foreground">{desc}</p>
      <div>
        <Button variant="secondary" onClick={cta.onClick}>
          {cta.label}
        </Button>
      </div>
    </div>
  )
}
