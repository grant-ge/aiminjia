/**
 * @designSource design.pen#PV1ln (Sidebar) + #EbnTy (Sidebar Content)
 * @sizing width 256, padding 8, gap 16
 */
import { useChat } from '@/hooks/useChat'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type Route } from '@/stores/uiStore'

import { ConversationTree } from './ConversationTree'
import { groupConversationsByProject } from './conversationProjects'
import { SidebarFooterSettings } from './SidebarFooterSettings'
import { SidebarNav, type SidebarNavKey } from './SidebarNav'
import { SidebarSectionTitle } from './SidebarSectionTitle'
import { TenantHeader } from './TenantHeader'

export function AppSidebar() {
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const tenant = useAuthStore((s) => s.tenant)
  const route = useUiStore((s) => s.route)
  const setRoute = useUiStore((s) => s.setRoute)
  const openSettings = useUiStore((s) => s.openSettings)
  const { conversations, activeConversationId, switchConversation } = useChat()

  const projects = groupConversationsByProject(
    conversations as never,
    activeConversationId,
  )

  const activeKey: SidebarNavKey =
    route.kind === 'skill-center'
      ? 'skill-center'
      : route.kind === 'schedules'
        ? 'schedules'
        : 'home'

  const tenantDisplay = tenant?.name ?? productName

  return (
    <aside className="flex h-full w-[256px] shrink-0 flex-col gap-4 overflow-hidden border-r border-sidebar-border bg-sidebar p-2 text-sidebar-foreground">
      <TenantHeader name={tenantDisplay} logoUrl={logoUrl} />

      <SidebarNav
        activeKey={activeKey}
        onSelect={(key) => setRoute({ kind: key } as Route)}
      />

      <SidebarSectionTitle label="任务" />

      <div className="min-h-0 flex-1 overflow-auto pr-1">
        <ConversationTree
          projects={projects}
          onSelectConversation={(id) => void switchConversation(id)}
        />
      </div>

      <SidebarFooterSettings onClick={() => openSettings('account')} />
    </aside>
  )
}
