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
    <div className="flex w-full flex-col rounded-lg border border-border bg-card">
      <div className="px-5 py-4">{header}</div>
      <div className="border-t border-border">{table}</div>
      {children}
      {empty ? (
        <div className="flex h-[280px] items-center justify-center">{empty}</div>
      ) : null}
    </div>
  )
}
