/**
 * @designSource design.pen#j4hWs tableHead
 * @sizing padding [10,20] bottom-border 1
 */
interface ScheduleTableHeaderProps {
  columns: string[]
}

export const SCHEDULE_TABLE_GRID_COLUMNS =
  'grid-cols-[minmax(20rem,1fr)_minmax(13rem,0.75fr)_7rem_9rem]'

export function ScheduleTableHeader({ columns }: ScheduleTableHeaderProps) {
  return (
    <div
      className={`grid ${SCHEDULE_TABLE_GRID_COLUMNS} items-center gap-3 px-5 py-2.5 text-sm font-medium text-muted-foreground`}
    >
      {columns.map((c, index) => (
        <span key={c} className={index === columns.length - 1 ? 'justify-self-end' : undefined}>
          {c}
        </span>
      ))}
    </div>
  )
}
