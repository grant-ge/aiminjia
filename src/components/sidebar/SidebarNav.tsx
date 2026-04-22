import { Puzzle, Sparkles, Timer } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useUiStore, type Route } from '@/stores/uiStore'

const NAV_ITEMS: Array<{ kind: Route['kind']; label: string; icon: typeof Sparkles }> = [
  { kind: 'home', label: '新任务', icon: Sparkles },
  { kind: 'skill-center', label: '技能中心', icon: Puzzle },
  { kind: 'schedules', label: '定时任务', icon: Timer },
]

export function SidebarNav() {
  const route = useUiStore((state) => state.route)
  const setRoute = useUiStore((state) => state.setRoute)

  return (
    <div className="space-y-1 px-3 py-3">
      {NAV_ITEMS.map(({ kind, label, icon: Icon }) => (
        <Button
          key={kind}
          className="w-full justify-start"
          variant={route.kind === kind ? 'secondary' : 'ghost'}
          onClick={() => setRoute({ kind } as Route)}
        >
          <Icon className="size-4" />
          {label}
        </Button>
      ))}
    </div>
  )
}
