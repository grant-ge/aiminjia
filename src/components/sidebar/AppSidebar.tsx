/**
 * @designSource design.pen#PV1ln (Sidebar) + #EbnTy (Sidebar Content)
 * @sizing width 256, padding 8, gap 16
 */
import { useState } from 'react'

import { useChat } from '@/hooks/useChat'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type Route } from '@/stores/uiStore'
import { ConfirmDialog } from '@/components/common/ConfirmDialog'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

import { ConversationTree } from './ConversationTree'
import { groupConversationsByProject } from './conversationProjects'
import { SidebarFooterSettings } from './SidebarFooterSettings'
import { SidebarNav, type SidebarNavKey } from './SidebarNav'
import { SidebarSectionTitle } from './SidebarSectionTitle'
import { TenantHeader } from './TenantHeader'

// userAgentData is the modern API but isn't in Tauri's WebView yet; userAgent
// is the only safe substring probe and is not deprecated.
const isMac =
  typeof navigator !== 'undefined' && /Mac/i.test(navigator.userAgent || '')

export function AppSidebar() {
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const tenant = useAuthStore((s) => s.tenant)
  const route = useUiStore((s) => s.route)
  const setRoute = useUiStore((s) => s.setRoute)
  const openSettings = useUiStore((s) => s.openSettings)
  const { conversations, activeConversationId, switchConversation, renameConversation, archiveConversation } = useChat()

  const [renamingId, setRenamingId] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [archivingId, setArchivingId] = useState<string | null>(null)

  const handleRenameOpen = (id: string) => {
    const conv = conversations.find((c) => c.id === id)
    setRenameValue(conv?.title ?? '')
    setRenamingId(id)
  }

  const handleRenameConfirm = async () => {
    if (!renamingId || !renameValue.trim()) return
    await renameConversation(renamingId, renameValue.trim())
    setRenamingId(null)
  }

  const handleArchiveConfirm = async () => {
    if (!archivingId) return
    await archiveConversation(archivingId)
    setArchivingId(null)
  }

  const projects = groupConversationsByProject(
    conversations,
    activeConversationId,
  )

  const activeKey: SidebarNavKey | null =
    route.kind === 'skill-center'
      ? 'skill-center'
      : route.kind === 'schedules'
        ? 'schedules'
        : route.kind === 'inbox'
          ? 'inbox'
          : route.kind === 'home'
            ? 'home'
            : null

  const tenantDisplay = tenant?.name ?? productName

  return (
    <>
    <aside className="flex h-full w-[256px] shrink-0 flex-col overflow-hidden bg-sidebar px-2 text-sidebar-foreground">
      {isMac ? (
        <div
          data-tauri-drag-region
          aria-hidden="true"
          className="h-8 w-full shrink-0"
        />
      ) : null}
      <TenantHeader name={tenantDisplay} logoUrl={logoUrl} />

      <SidebarNav
        activeKey={activeKey}
        onSelect={(key) => setRoute({ kind: key } as Route)}
      />

      <div className="flex min-h-0 flex-1 flex-col gap-2">
        <SidebarSectionTitle label="项目" />
        <div className="-mr-2 flex-1 overflow-auto">
          <ConversationTree
            projects={projects}
            onSelectConversation={(id) => void switchConversation(id)}
            onRenameConversation={handleRenameOpen}
            onArchiveConversation={setArchivingId}
          />
        </div>
      </div>

      <SidebarFooterSettings onClick={() => openSettings('account')} />
    </aside>

    {/* 重命名弹窗 */}
    <Dialog open={!!renamingId} onOpenChange={(open) => !open && setRenamingId(null)}>
      <DialogContent className="w-[400px]">
        <DialogHeader>
          <DialogTitle>重命名聊天</DialogTitle>
        </DialogHeader>
        <Input
          value={renameValue}
          onChange={(e) => setRenameValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void handleRenameConfirm()}
          autoFocus
        />
        <DialogFooter>
          <Button variant="outline" onClick={() => setRenamingId(null)}>取消</Button>
          <Button onClick={() => void handleRenameConfirm()} disabled={!renameValue.trim()}>确认</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <ConfirmDialog
      open={!!archivingId}
      title="归档此聊天？"
      description="归档后聊天将从列表中隐藏，可在设置的归档记录中查看和恢复。"
      confirmLabel="归档"
      onOpenChange={(open) => !open && setArchivingId(null)}
      onConfirm={() => void handleArchiveConfirm()}
    />
  </>
  )
}
