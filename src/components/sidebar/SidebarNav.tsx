/**
 * @designSource design.pen#47U5w (nv1/nv2/nv3)
 * @sizing each row padding [6,8], gap 2
 */
import { useTranslation } from 'react-i18next'
import { Blocks, Clock3, GraduationCap, MessageSquare, SquarePen, Users, type LucideIcon } from 'lucide-react'

export type SidebarNavKey = 'home' | 'employees' | 'skill-center' | 'schedules' | 'expert-teams' | 'channel'

interface SidebarNavProps {
  activeKey?: SidebarNavKey | null
  onSelect?: (key: SidebarNavKey) => void
}

const NAV: Array<{ key: SidebarNavKey; i18nKey: string; icon: LucideIcon }> = [
  { key: 'home', i18nKey: 'nav.home', icon: SquarePen },
  { key: 'employees', i18nKey: 'nav.employees', icon: Users },
  { key: 'expert-teams', i18nKey: 'nav.expertTeams', icon: GraduationCap },
  { key: 'skill-center', i18nKey: 'nav.skillCenter', icon: Blocks },
  { key: 'schedules', i18nKey: 'nav.schedules', icon: Clock3 },
  { key: 'channel', i18nKey: 'nav.channel', icon: MessageSquare },
]

export function SidebarNav({ activeKey = null, onSelect = () => {} }: SidebarNavProps) {
  const { t } = useTranslation()
  return (
    <nav className="flex flex-col gap-0.5 mt-3 mb-4">
      {NAV.map(({ key, i18nKey, icon: Icon }) => {
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
            <span className="flex-1 truncate">{t(i18nKey)}</span>
          </button>
        )
      })}
    </nav>
  )
}
