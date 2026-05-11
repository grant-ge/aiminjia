/**
 * @designSource design.pen#47U5w (nv1/nv2/nv3)
 * @sizing each row padding [6,8], gap 2
 */
import { Blocks, Clock3, Inbox, MessageSquare, SquarePen, Users, type LucideIcon } from 'lucide-react'
import { useGlobalUnreadCount } from '@/features/employees/useInbox'

export type SidebarNavKey = 'home' | 'employees' | 'skill-center' | 'schedules' | 'inbox' | 'channel'

interface SidebarNavProps {
  activeKey?: SidebarNavKey | null
  onSelect?: (key: SidebarNavKey) => void
}

const NAV: Array<{ key: SidebarNavKey; label: string; icon: LucideIcon; badge?: boolean }> = [
  { key: 'home', label: '新任务', icon: SquarePen },
  { key: 'employees', label: '数字员工', icon: Users },
  { key: 'inbox', label: '汇报中心', icon: Inbox, badge: true },
  { key: 'skill-center', label: '技能中心', icon: Blocks },
  { key: 'schedules', label: '定时任务', icon: Clock3 },
  { key: 'channel', label: 'IM 频道', icon: MessageSquare },
]

export function SidebarNav({ activeKey = null, onSelect = () => {} }: SidebarNavProps) {
  const unreadCount = useGlobalUnreadCount()

  return (
    <nav className="flex flex-col gap-0.5 mt-3 mb-4">
      {NAV.map(({ key, label, icon: Icon, badge }) => {
        const active = key === activeKey
        const count = badge ? unreadCount : 0
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
            <span className="flex-1 truncate">{label}</span>
            {count > 0 && (
              <span className="ml-auto rounded-full bg-primary px-1.5 py-0.5 text-xs font-semibold leading-none text-primary-foreground">
                {count > 99 ? '99+' : count}
              </span>
            )}
          </button>
        )
      })}
    </nav>
  )
}
