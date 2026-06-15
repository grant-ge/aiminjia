/**
 * @designSource design.pen#Ifs8C emptyArea
 * @sizing h 280 center; gap 14
 */
import type { ReactNode } from 'react'
import { Button } from '@/components/ui/button'


interface ScheduleEmptyStateProps {
  icon?: ReactNode
  title: string
  desc?: string
  cta?: { label: string; onClick: () => void }
}

export function ScheduleEmptyState({ icon, title, desc, cta }: ScheduleEmptyStateProps) {
  return (
    <div className="flex flex-col items-center gap-3">
      {icon}
      <div className="text-sm font-semibold text-foreground">{title}</div>
      {desc ? <div className="text-sm text-muted-foreground">{desc}</div> : null}
      {cta ? <Button onClick={cta.onClick}>{cta.label}</Button> : null}
    </div>
  )
}
