/**
 * @designSource design.pen#lqhdx / L29MQ
 * @sizing header padding [6,8], gap 8
 */
import { Folder, FolderOpen } from 'lucide-react'
import type { PropsWithChildren } from 'react'
import { Button } from '@/components/ui/button'

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
    <div className="flex w-full min-w-0 flex-col">
      <Button unstyled
        type="button"
        onClick={onToggle}
        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm font-medium text-[#636363] transition-colors hover:bg-[rgba(var(--sidebar-accent-rgb),0.40)]"
      >
        {expanded
          ? <FolderOpen className="h-4 w-4 shrink-0 text-[#636363]" />
          : <Folder className="h-4 w-4 shrink-0 text-[#636363]" />
        }
        <span className="truncate">{name}</span>
      </Button>
      {expanded ? <div className="flex w-full min-w-0 flex-col gap-0.5">{children}</div> : null}
    </div>
  )
}
