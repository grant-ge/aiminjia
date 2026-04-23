/**
 * @designSource design.pen#j4hWs tableHead
 * @sizing padding [10,20] bottom-border 1
 */
interface ScheduleTableHeaderProps {
  columns: string[]
}

export function ScheduleTableHeader({ columns }: ScheduleTableHeaderProps) {
  return (
    <div
      className="grid items-center gap-3 px-5 py-2.5 text-[13px] font-medium text-muted-foreground"
      style={{ gridTemplateColumns: `repeat(${columns.length}, minmax(0, 1fr))` }}
    >
      {columns.map((c) => (
        <span key={c}>{c}</span>
      ))}
    </div>
  )
}
