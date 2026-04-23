/**
 * @designSource design.pen#jTgSA
 * @sizing padding [6,8], gap 8
 */
import { Settings } from 'lucide-react'

interface SidebarFooterSettingsProps {
  onClick: () => void
}

export function SidebarFooterSettings({ onClick }: SidebarFooterSettingsProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/40"
    >
      <Settings className="h-4 w-4 shrink-0 text-muted-foreground" />
      <span>设置</span>
    </button>
  )
}
