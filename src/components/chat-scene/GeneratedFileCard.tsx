/**
 * @designSource design.pen#v46uG
 * @sizing r-14 border 1 bg card padding [14,16] gap 16; fileIcon 44×52 r-6 bg muted
 */
import type { ReactNode } from 'react'
import { ChevronDown } from 'lucide-react'

interface GeneratedFileCardProps {
  title: string
  sub: string
  appName: string
  fileIcon?: ReactNode
  appIcon?: ReactNode
  onOpen: () => void
}

export function GeneratedFileCard({
  title,
  sub,
  appName,
  fileIcon,
  appIcon,
  onOpen,
}: GeneratedFileCardProps) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-[14px] border border-border bg-card p-4">
      <div className="flex min-w-0 items-center gap-3">
        <div className="flex h-[52px] w-11 items-center justify-center rounded-md border border-border bg-muted">
          {fileIcon}
        </div>
        <div className="flex min-w-0 flex-col gap-1">
          <div className="truncate text-[15px] font-semibold text-foreground">{title}</div>
          <div className="truncate text-[13px] text-muted-foreground">{sub}</div>
        </div>
      </div>
      <button
        type="button"
        onClick={onOpen}
        aria-label={`${appName} open`}
        className="flex shrink-0 items-center gap-2 rounded-full border border-border bg-background py-1.5 pl-3 pr-1.5 text-[13px] text-foreground transition-colors hover:bg-muted"
      >
        {appIcon}
        <span>{appName}</span>
        <span className="mx-1 h-4 w-px bg-border" />
        <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
      </button>
    </div>
  )
}
