/**
 * @designSource design.pen#PV1ln (Sidebar) + #EbnTy (Sidebar Content)
 * @sizing width 256, padding 8, gap 16
 */
import { useState } from 'react'
import { CheckSquare, ChevronRight, MessageSquare } from 'lucide-react'

import { useChat } from '@/hooks/useChat'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type Route, useActiveConversationId, useActiveChannelSessionId } from '@/stores/uiStore'
import { useChannelStore } from '@/stores/channelStore'
import { ConfirmDialog } from '@/components/common/ConfirmDialog'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

import { ConversationTree } from './ConversationTree'
import { groupConversationsByProject } from './conversationProjects'
import { SidebarFooterSettings } from './SidebarFooterSettings'
import { SidebarNav, type SidebarNavKey } from './SidebarNav'
import { TenantHeader } from './TenantHeader'

const SIDEBAR_TAB_STORAGE_KEY = 'aijia-sidebar-tab'

function loadPersistedSidebarTab(): 'project' | 'channel' {
  if (typeof localStorage === 'undefined') return 'project'
  try {
    const raw = localStorage.getItem(SIDEBAR_TAB_STORAGE_KEY)
    return raw === 'channel' ? 'channel' : 'project'
  } catch {
    return 'project'
  }
}

function persistSidebarTab(tab: 'project' | 'channel') {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(SIDEBAR_TAB_STORAGE_KEY, tab)
  } catch {
    // Ignore storage failures; tab still works in memory.
  }
}

export function AppSidebar() {
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const route = useUiStore((s) => s.route)
  const setRoute = useUiStore((s) => s.setRoute)
  const openSettings = useUiStore((s) => s.openSettings)
  const { conversations, switchConversation, renameConversation, archiveConversation } = useChat()
  const activeConversationId = useActiveConversationId()
  const channelActiveSessionId = useActiveChannelSessionId()
  const channelConversations = useChannelStore((s) => s.conversations)
  const dingtalkState = useChannelStore((s) => s.platforms.dingtalk)

  const [renamingId, setRenamingId] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [archivingId, setArchivingId] = useState<string | null>(null)

  const [sidebarTab, setSidebarTab] = useState<'project' | 'channel'>(loadPersistedSidebarTab)
  const [legacyExpanded, setLegacyExpanded] = useState(false)

  const switchTab = (next: 'project' | 'channel') => {
    setSidebarTab(next)
    persistSidebarTab(next)
  }

  // isActiveRobot=undefined (old data) treated as active for backward compatibility
  const activeConversations = channelConversations.filter((c) => c.isActiveRobot !== false)
  const legacyConversations = channelConversations.filter((c) => c.isActiveRobot === false)
  const legacyByRobot = legacyConversations.reduce<Record<string, typeof channelConversations>>(
    (acc, c) => {
      ;(acc[c.robotCode] ??= []).push(c)
      return acc
    },
    {},
  )



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
    route.kind === 'channel'
      ? route.sessionId ? null : 'channel'
      : route.kind === 'employees' ? 'employees'
      : route.kind === 'skill-center' || route.kind === 'skill-detail' ? 'skill-center'
      : route.kind === 'schedules' ? 'schedules'
      : route.kind === 'inbox' ? 'inbox'
      : route.kind === 'home' ? 'home'
      : null

  const tenantDisplay = productName
  const dingtalkChannelLabel = !dingtalkState?.configured
    ? '未配置'
    : !dingtalkState.enabled
    ? '未连接'
    : dingtalkState.connection === 'connected'
    ? '已连接'
    : dingtalkState.connection === 'connecting'
    ? '连接中'
    : dingtalkState.connection === 'reconnecting'
    ? '重连中'
    : dingtalkState.connection === 'configError'
    ? '配置有误'
    : '未连接'

  const openChannelOverview = () => {
    setRoute({ kind: 'channel' })
  }

  const selectChannelSession = (sessionId: string) => {
    setRoute({ kind: 'channel', sessionId })
  }

  return (
    <>
    <aside className="flex h-full w-[256px] shrink-0 flex-col overflow-hidden bg-sidebar px-2 pt-2 text-sidebar-foreground">
      <TenantHeader name={tenantDisplay} logoUrl={logoUrl} />

      <SidebarNav
        activeKey={activeKey}
        onSelect={(key) => setRoute({ kind: key } as Route)}
      />

      <div className="flex min-h-0 flex-1 flex-col gap-2">
        <div className="grid grid-cols-2 rounded-lg bg-sidebar-accent p-0.5 text-xs font-medium text-muted-foreground">
          <button
            type="button"
            onClick={() => switchTab('project')}
            className={
              sidebarTab === 'project'
                ? 'flex items-center justify-center gap-1.5 rounded-md bg-background px-2 py-1.5 text-foreground shadow-sm'
                : 'flex items-center justify-center gap-1.5 rounded-md px-2 py-1.5'
            }
          >
            <CheckSquare className="h-3.5 w-3.5" />
            项目
          </button>
          <button
            type="button"
            onClick={() => switchTab('channel')}
            className={
              sidebarTab === 'channel'
                ? 'flex items-center justify-center gap-1.5 rounded-md bg-background px-2 py-1.5 text-foreground shadow-sm'
                : 'flex items-center justify-center gap-1.5 rounded-md px-2 py-1.5'
            }
          >
            <MessageSquare className="h-3.5 w-3.5" />
            频道
          </button>
        </div>

        {sidebarTab === 'project' ? (
          <>
            <div className="-mr-2 flex-1 overflow-auto">
              <ConversationTree
                projects={projects}
                onSelectConversation={(id) => void switchConversation(id)}
                onRenameConversation={handleRenameOpen}
                onArchiveConversation={setArchivingId}
              />
            </div>
          </>
        ) : (
          <div className="-mr-2 flex-1 overflow-auto pr-2">
            <div className="mt-2 flex flex-col gap-3">
              <div>
                <div className="mb-2 flex items-center gap-2 px-2 text-sm font-semibold text-sidebar-foreground">
                  {/* 钉钉品牌色 #0b8cff，跨主题保持品牌识别 */}
                  <span className="flex h-5 w-5 items-center justify-center rounded-full bg-sky-50 text-xs text-[var(--color-semantic-blue)]">钉</span>
                  钉钉
                  <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                    {dingtalkChannelLabel}
                  </span>
                </div>
                <div className="pl-6">
                  {activeConversations.length === 0 ? (
                    <button
                      type="button"
                      onClick={openChannelOverview}
                      className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
                    >
                      未配置，点击右侧设置
                    </button>
                  ) : (
                    activeConversations.map((conversation) => (
                      <button
                        key={conversation.sessionId}
                        type="button"
                        onClick={() => selectChannelSession(conversation.sessionId)}
                        className={
                          channelActiveSessionId === conversation.sessionId
                            ? 'flex w-full items-center justify-between rounded-lg bg-sidebar-accent px-3 py-2 text-left text-sm font-semibold text-sidebar-foreground'
                            : 'flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm font-medium text-sidebar-foreground/70 hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'
                        }
                      >
                        <span className="truncate">{conversation.displayName}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>

                {legacyConversations.length > 0 && (
                  <div className="mt-2 pl-6">
                    <button
                      type="button"
                      onClick={() => setLegacyExpanded((v) => !v)}
                      className="flex w-full items-center gap-1 rounded-lg px-3 py-1.5 text-left text-xs font-medium text-muted-foreground hover:bg-sidebar-accent/50"
                    >
                      <ChevronRight
                        className={`h-3 w-3 transition-transform ${legacyExpanded ? 'rotate-90' : ''}`}
                      />
                      历史会话 ({legacyConversations.length})
                    </button>
                    {legacyExpanded && (
                      <div className="mt-1 flex flex-col gap-1 pl-4">
                        {Object.entries(legacyByRobot).map(([robotCode, list]) => (
                          <div key={robotCode}>
                            <div className="px-3 py-1 text-xs font-medium text-muted-foreground/70">
                              {robotCode.length > 14 ? `${robotCode.slice(0, 14)}…` : robotCode}
                            </div>
                            {list.map((conversation) => (
                              <button
                                key={conversation.sessionId}
                                type="button"
                                onClick={() => selectChannelSession(conversation.sessionId)}
                                className="flex w-full items-center justify-between rounded-lg px-3 py-1.5 text-left text-sm text-muted-foreground hover:bg-sidebar-accent/50"
                              >
                                <span className="truncate">{conversation.displayName}</span>
                              </button>
                            ))}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          </div>
        )}
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
