/**
 * @designSource design.pen#jhWGa listCard
 * @sizing r-14 border 1 bg card
 */
import type { ReactNode } from 'react'

interface ScheduleListCardProps {
  header: ReactNode
  table: ReactNode
  empty?: ReactNode
}

export function ScheduleListCard({ header, table, empty }: ScheduleListCardProps) {
  return (
    <div className="flex w-full flex-col rounded-[14px] border border-border bg-card">
      <div className="px-5 py-4">{header}</div>
      <div className="border-t border-border">{table}</div>
      {empty ? (
        <div className="flex h-[280px] items-center justify-center">{empty}</div>
      ) : null}
    </div>
  )
}
