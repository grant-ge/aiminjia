/**
 * @designSource design.pen#24cM4
 * @sizing padding 8, fontSize 12
 */
interface SidebarSectionTitleProps {
  label: string
}

export function SidebarSectionTitle({ label }: SidebarSectionTitleProps) {
  return (
    <div className="px-2 py-2 text-xs font-semibold tracking-wide text-muted-foreground">
      {label}
    </div>
  )
}
