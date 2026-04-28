/**
 * @designSource design.pen#47U5w (nv1/nv2/nv3)
 * @sizing each row padding [6,8], gap 2
 */
import { Blocks, Clock3, SquarePen, type LucideIcon } from 'lucide-react'

export type SidebarNavKey = 'home' | 'skill-center' | 'schedules'

interface SidebarNavProps {
  activeKey?: SidebarNavKey | null
  onSelect?: (key: SidebarNavKey) => void
}

const NAV: Array<{ key: SidebarNavKey; label: string; icon: LucideIcon }> = [
  { key: 'home', label: '新任务', icon: SquarePen },
  { key: 'skill-center', label: '技能中心', icon: Blocks },
  { key: 'schedules', label: '定时任务', icon: Clock3 },
]

export function SidebarNav({ activeKey = null, onSelect = () => {} }: SidebarNavProps) {
  return (
    <nav className="flex flex-col gap-0.5 mt-3 mb-4">
      {NAV.map(({ key, label, icon: Icon }) => {
        const active = key === activeKey
        return (
          <button
            key={key}
            type="button"
            onClick={() => onSelect(key)}
            className={
              active
                ? 'flex w-full items-center gap-2 rounded-md bg-sidebar-accent px-2 py-1.5 text-left text-sm text-sidebar-foreground'
                : 'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent/40'
            }
          >
            <Icon className="h-4 w-4 shrink-0" />
            <span className="truncate">{label}</span>
          </button>
        )
      })}
    </nav>
  )
}
