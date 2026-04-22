import { Settings } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useChat } from '@/hooks/useChat'
import { useUiStore } from '@/stores/uiStore'

import { ConversationTree } from './ConversationTree'
import { SidebarNav } from './SidebarNav'
import { TenantHeader } from './TenantHeader'

export function AppSidebar() {
  const openSettings = useUiStore((state) => state.openSettings)
  const { createNewConversation } = useChat()

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground">
      <TenantHeader />
      <SidebarNav />
      <div className="px-3">
        <Button className="w-full justify-start" variant="outline" onClick={() => void createNewConversation()}>
          + 新对话
        </Button>
      </div>
      <div className="px-3 pt-3">
        <Input aria-label="搜索对话" placeholder="搜索对话..." readOnly value="" />
      </div>
      <div className="px-4 pb-2 pt-3 text-xs font-medium text-muted-foreground">任务</div>
      <ScrollArea className="min-h-0 flex-1 px-3">
        <ConversationTree />
      </ScrollArea>
      <div className="p-3">
        <Button className="w-full justify-start" variant="ghost" onClick={() => openSettings('account')}>
          <Settings className="size-4" />
          设置
        </Button>
      </div>
    </aside>
  )
}
