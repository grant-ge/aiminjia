/**
 * @designSource design.pen#0EZDr / HsGnf / GknhC
 * @sizing padding [6,8,6,30] (indent 30 under ProjectAccordion), fontSize 13
 */
import { Loader2 } from 'lucide-react'

interface ConversationRowProps {
  title: string
  active?: boolean
  loading?: boolean
  onClick: () => void
}

export function ConversationRow({
  title,
  active = false,
  loading = false,
  onClick,
}: ConversationRowProps) {
  const base =
    'flex w-full items-center gap-2 rounded-md py-1.5 pl-[30px] pr-2 text-left text-[13px]'
  const cls = active
    ? `${base} bg-sidebar-accent text-sidebar-foreground`
    : `${base} text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/40`

  return (
    <button type="button" onClick={onClick} className={cls}>
      {loading ? (
        <Loader2
          data-icon="loader"
          className="h-3.5 w-3.5 shrink-0 animate-spin text-sidebar-foreground"
        />
      ) : null}
      <span className="truncate">{title}</span>
    </button>
  )
}
