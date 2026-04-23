/**
 * @designSource design.pen#lqhdx / L29MQ
 * @sizing header padding [6,8], gap 8
 */
import { ChevronDown } from 'lucide-react'
import type { PropsWithChildren } from 'react'

interface ProjectAccordionProps extends PropsWithChildren {
  name: string
  expanded: boolean
  onToggle: () => void
}

export function ProjectAccordion({
  name,
  expanded,
  onToggle,
  children,
}: ProjectAccordionProps) {
  return (
    <div className="flex flex-col">
      <button
        type="button"
        onClick={onToggle}
        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm font-medium text-sidebar-foreground transition-colors hover:bg-sidebar-accent/40"
      >
        <ChevronDown
          data-icon="chevron-down"
          className={
            expanded
              ? 'h-4 w-4 shrink-0 text-muted-foreground transition-transform'
              : 'h-4 w-4 shrink-0 -rotate-90 text-muted-foreground transition-transform'
          }
        />
        <span className="truncate">{name}</span>
      </button>
      {expanded ? <div className="flex flex-col gap-0.5">{children}</div> : null}
    </div>
  )
}
