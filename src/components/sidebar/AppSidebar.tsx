import { Search } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useUiStore } from '@/stores/uiStore'

export function AppSidebar() {
  const openSettings = useUiStore((state) => state.openSettings)

  return (
    <aside className="hidden w-64 shrink-0 border-r border-sidebar-border bg-sidebar p-4 text-sidebar-foreground md:flex md:flex-col">
      <div className="text-sm font-semibold">Skill-First</div>
      <p className="mt-2 text-xs text-muted-foreground">侧栏主导航将在下一任务完成替换。</p>
      <Button className="mt-4 justify-start" variant="secondary">
        新任务
      </Button>
      <div className="relative mt-3">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          aria-label="搜索对话"
          className="pl-9"
          placeholder="搜索对话..."
          readOnly
          value=""
        />
      </div>
      <Button className="mt-4 justify-start" variant="ghost" onClick={() => openSettings('account')}>
        设置
      </Button>
    </aside>
  )
}
