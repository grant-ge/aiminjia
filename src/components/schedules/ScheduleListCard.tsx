/**
 * @designSource design.pen#jhWGa listCard
 * @sizing r-14 border 1 bg card
 */
import type { ReactNode } from 'react'

interface ScheduleListCardProps {
  header: ReactNode
  table: ReactNode
  empty?: ReactNode
  children?: ReactNode
}

export function ScheduleListCard({ header, table, empty, children }: ScheduleListCardProps) {
  return (
    <div className="flex w-full flex-col overflow-hidden rounded-md border border-border/70 bg-card shadow-[var(--shadow-schedule-panel)]">
      <div className="border-b border-border/60 px-4 py-2.5">{header}</div>
      <div className="bg-muted/20">{table}</div>
      {children}
      {empty ? (
        <div className="flex h-[240px] items-center justify-center border-t border-border/60">{empty}</div>
      ) : null}
    </div>
  )
}
