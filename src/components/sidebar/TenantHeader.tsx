/**
 * @designSource design.pen#6xhgh
 * @sizing width fluid, padding 8, gap 8
 */
import { ChevronsUpDown } from 'lucide-react'

interface TenantHeaderProps {
  name?: string
  logoUrl?: string
  onClick?: () => void
}

export function TenantHeader({
  name = '',
  logoUrl = '/app-icon.png',
  onClick,
}: TenantHeaderProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center justify-between gap-2 rounded-md p-2 text-left transition-colors hover:bg-sidebar-accent/50"
    >
      <div className="flex min-w-0 items-center gap-2">
        <div
          data-testid="tenant-logo"
          className="h-8 w-8 shrink-0 overflow-hidden rounded-[10px] border border-sidebar-border"
        >
          <img
            src={logoUrl}
            alt="Brand logo"
            className="h-full w-full object-cover"
          />
        </div>
        <div className="min-w-0 truncate text-sm font-semibold text-sidebar-foreground">
          {name}
        </div>
      </div>
      <ChevronsUpDown
        data-icon="chevrons-up-down"
        className="h-4 w-4 shrink-0 text-muted-foreground"
      />
    </button>
  )
}
