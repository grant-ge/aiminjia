/**
 * @designSource design.pen#47U5w (nv1/nv2/nv3)
 * @sizing each row padding [6,8], gap 2
 */
import { useTranslation } from 'react-i18next'
import { Blocks, CheckSquare, Clock3, GraduationCap, MessageSquare, Users, type LucideIcon } from 'lucide-react'

export type SidebarNavKey = 'home' | 'employees' | 'skill-center' | 'schedules' | 'expert-teams' | 'channel'

interface SidebarNavProps {
  activeKey?: SidebarNavKey | null
  onSelect?: (key: SidebarNavKey) => void
}

const NAV: Array<{ key: SidebarNavKey; i18nKey: string; icon: LucideIcon }> = [
  { key: 'home', i18nKey: 'nav.home', icon: CheckSquare },
  { key: 'employees', i18nKey: 'nav.employees', icon: Users },
  { key: 'expert-teams', i18nKey: 'nav.expertTeams', icon: GraduationCap },
  { key: 'skill-center', i18nKey: 'nav.skillCenter', icon: Blocks },
  { key: 'schedules', i18nKey: 'nav.schedules', icon: Clock3 },
  { key: 'channel', i18nKey: 'nav.channel', icon: MessageSquare },
]

export function SidebarNav({ activeKey = null, onSelect = () => {} }: SidebarNavProps) {
  const { t } = useTranslation()
  return (
    <nav className="mb-2 flex flex-col gap-0.5">
      {NAV.map(({ key, i18nKey, icon: Icon }) => {
        const active = key === activeKey
        return (
          <button
            key={key}
            type="button"
            data-aijia-nav={key}
            onClick={() => onSelect(key)}
            className={
              active
                ? 'flex h-9 w-full items-center gap-2 rounded-md bg-sidebar-accent px-2.5 text-left text-sm font-semibold text-sidebar-foreground'
                : 'flex h-9 w-full items-center gap-2 rounded-md px-2.5 text-left text-sm font-medium text-sidebar-foreground/75 transition-colors hover:bg-sidebar-accent/60 hover:text-sidebar-foreground'
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
