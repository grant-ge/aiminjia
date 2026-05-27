/**
 * @designSource design.pen#PV1ln (Sidebar) + #EbnTy (Sidebar Content)
 * @sizing width 256, padding 8, gap 16
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckSquare, GraduationCap, MessageSquare, Users } from 'lucide-react'

import { useChat } from '@/hooks/useChat'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type Route, type SidebarBodyTab, useActiveConversationId, useActiveChannelSessionId } from '@/stores/uiStore'
import { useChannelStore } from '@/stores/channelStore'
import { hasExpertTeam } from '@/features/expert-teams/expertTeamRegistry'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'

import { ConversationRow } from './ConversationRow'
import { ConversationTree } from './ConversationTree'
import { groupConversationsByProject } from './conversationProjects'
import { SidebarFooterSettings } from './SidebarFooterSettings'
import { SidebarNav, type SidebarNavKey } from './SidebarNav'
import { TenantHeader } from './TenantHeader'

const CHANNEL_PLATFORM_NAME: Record<string, string> = {
  dingtalk: '钉钉',
  feishu: '飞书',
  wecom: '企业微信',
  wechat: '个人微信',
  telegram: 'Telegram',
  whatsapp: 'WhatsApp',
}

/** 频道会话侧边栏显示名。
 *  规则按平台分流（2026-05-21 调整）：
 *  - WhatsApp：私聊里 displayName = sender push_name 真名稳定可用，直接展示；
 *    群聊或拿不到名时 fallback 成 "WhatsApp 私聊"/"WhatsApp 群聊"。
 *  - 其他平台（钉钉/飞书/企微/个微/Telegram）：入站消息要么没有真名（飞书/
 *    企微/个微只有 user_id），要么有真名但视觉上和 WhatsApp 处理统一更整齐 ——
 *    一律显示 "<平台>私聊" / "<平台>群聊"，避免有的会话叫张三、有的叫
 *    `ou_45...` 这种参差感。
 */
function channelConversationLabel(conversation: {
  platform: string
  conversationType: 'group' | 'private'
  displayName?: string | null
}): string {
  const platform = CHANNEL_PLATFORM_NAME[conversation.platform] ?? conversation.platform
  const kind = conversation.conversationType === 'group' ? '群聊' : '私聊'
  if (conversation.platform === 'whatsapp') {
    const trimmed = conversation.displayName?.trim()
    if (trimmed) return trimmed
  }
  return `${platform}${kind}`
}

export function AppSidebar() {
  const { t } = useTranslation()
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const route = useUiStore((s) => s.route)
  const setRoute = useUiStore((s) => s.setRoute)
  const openSettings = useUiStore((s) => s.openSettings)
  const sidebarTab = useUiStore((s) => s.sidebarTab)
  const setSidebarTab = useUiStore((s) => s.setSidebarTab)
  const {
    conversations,
    switchConversation,
    renameConversation,
    archiveConversation,
    setConversationPinned,
  } = useChat()
  const activeConversationId = useActiveConversationId()
  const channelActiveSessionId = useActiveChannelSessionId()
  const channelConversations = useChannelStore((s) => s.conversations)
  const dingtalkState = useChannelStore((s) => s.platforms.dingtalk)
  const feishuState = useChannelStore((s) => s.platforms.feishu)
  const wecomState = useChannelStore((s) => s.platforms.wecom)
  const wechatState = useChannelStore((s) => s.platforms.wechat)
  const telegramState = useChannelStore((s) => s.platforms.telegram)
  const whatsappState = useChannelStore((s) => s.platforms.whatsapp)

  const [renamingId, setRenamingId] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')

  const switchTab = (next: SidebarBodyTab) => {
    setSidebarTab(next)
  }

  // isActiveRobot=undefined (old data) treated as active for backward compatibility
  const dingtalkConversations = channelConversations.filter((c) => c.platform === 'dingtalk')
  const feishuConversations = channelConversations.filter((c) => c.platform === 'feishu')
  const wecomConversations = channelConversations.filter((c) => c.platform === 'wecom')
  const wechatConversations = channelConversations.filter((c) => c.platform === 'wechat')
  const telegramConversations = channelConversations.filter((c) => c.platform === 'telegram')
  const whatsappConversations = channelConversations.filter((c) => c.platform === 'whatsapp')
  const activeConversations = dingtalkConversations.filter((c) => c.isActiveRobot !== false)
  const activeFeishuConversations = feishuConversations.filter((c) => c.isActiveRobot !== false)
  const activeWecomConversations = wecomConversations.filter((c) => c.isActiveRobot !== false)
  const activeWechatConversations = wechatConversations.filter((c) => c.isActiveRobot !== false)
  const activeTelegramConversations = telegramConversations.filter((c) => c.isActiveRobot !== false)
  const activeWhatsappConversations = whatsappConversations.filter((c) => c.isActiveRobot !== false)



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

  const handleArchive = async (id: string) => {
    await archiveConversation(id)
  }

  // IM 频道（钉钉私聊/群）的 session id 复用 conv_store 的 conversation id，
  // 所以会同时出现在 useChat().conversations 里。useChat 已用 kind !== 'im'
  // 过了一遍，这里再用 channelSessionIdSet 做兜底以防 backend kind 标记漏标。
  const channelSessionIdSet = new Set(channelConversations.map((c) => c.sessionId))
  const nonChannelConversations = conversations.filter((c) => !channelSessionIdSet.has(c.id))
  // 员工 / 专家团 tab 走独立列表渲染；项目 tab 走白名单：只展示 `kind=user`
  // 或 `kind` 未标记的旧会话（视作 user 兼容）。员工 / 专家团 / IM 类的会话
  // 都被显式排除，避免未来加 kind 时项目 tab 误带入新类型。
  const expertTeamConversations = nonChannelConversations.filter((c) => hasExpertTeam(c.id))
  const employeeConversations = nonChannelConversations.filter((c) => c.kind === 'employee')
  const projectConversations = nonChannelConversations.filter(
    (c) => (c.kind ?? 'user') === 'user' && !hasExpertTeam(c.id),
  )

  // Global pinned section: every pinned in-app conversation regardless of
  // tab kind. Order = useChat ordering (already pinned-first from the Rust
  // sort), so users see consistent ordering whichever tab is active. IM
  // channel sessions are not yet pin-aware (no isPinned column on
  // ChannelConversation) so they're excluded.
  const globalPinned = nonChannelConversations.filter((c) => c.isPinned)

  // 各 tab 内已经经过全局置顶过滤，避免 pinned 会话同时出现在置顶区和 tab 内。
  const projects = groupConversationsByProject(
    projectConversations.filter((c) => !c.isPinned),
    activeConversationId,
  )

  /**
   * Render a flat tab body (employee / expert-team).  The global pinned
   * section above already surfaces pinned items, so within the tab we just
   * list non-pinned conversations to avoid duplicates.
   */
  const renderFlatTab = (items: typeof conversations) => {
    if (items.length === 0) {
      return (
        <div className="px-2 py-4 text-sm text-muted-foreground">{t('sidebar.noHistory')}</div>
      )
    }
    const visible = items.filter((c) => !c.isPinned)
    if (visible.length === 0) {
      return (
        <div className="px-2 py-4 text-sm text-muted-foreground">{t('sidebar.allPinnedHint')}</div>
      )
    }
    return (
      <div className="flex flex-col gap-0.5">
        {visible.map((conversation) => (
          <ConversationRow
            key={conversation.id}
            id={conversation.id}
            title={conversation.title}
            active={activeConversationId === conversation.id}
            indent={false}
            pinned={conversation.isPinned ?? false}
            onClick={() => void switchConversation(conversation.id)}
            onRename={() => handleRenameOpen(conversation.id)}
            onArchive={() => void handleArchive(conversation.id)}
            onTogglePin={() =>
              void setConversationPinned(conversation.id, !(conversation.isPinned ?? false))
            }
          />
        ))}
      </div>
    )
  }

  const activeKey: SidebarNavKey | null =
    route.kind === 'channel'
      ? route.sessionId ? null : 'channel'
      : route.kind === 'employees' ? 'employees'
      : route.kind === 'skill-center' || route.kind === 'skill-detail' ? 'skill-center'
      : route.kind === 'schedules' ? 'schedules'
      : route.kind === 'expert-teams' ? 'expert-teams'
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
        {globalPinned.length > 0 ? (
          <div className="flex flex-col mb-1">
            <div className="px-2 py-1 text-xs font-medium text-muted-foreground">
              {t('sidebar.pinnedSection')}
            </div>
            <div className="flex flex-col gap-0.5">
              {globalPinned.map((conversation) => (
                <ConversationRow
                  key={conversation.id}
                  id={conversation.id}
                  title={conversation.title}
                  active={activeConversationId === conversation.id}
                  indent={false}
                  pinned
                  onClick={() => void switchConversation(conversation.id)}
                  onRename={() => handleRenameOpen(conversation.id)}
                  onArchive={() => void handleArchive(conversation.id)}
                  onTogglePin={() => void setConversationPinned(conversation.id, false)}
                />
              ))}
            </div>
          </div>
        ) : null}
        {(() => {
          const TABS = [
            { key: 'project' as SidebarBodyTab, Icon: CheckSquare, labelKey: 'sidebar.project' },
            { key: 'employee' as SidebarBodyTab, Icon: Users, labelKey: 'sidebar.employeeTab' },
            { key: 'expert-team' as SidebarBodyTab, Icon: GraduationCap, labelKey: 'sidebar.expertTeamTab' },
            { key: 'channel' as SidebarBodyTab, Icon: MessageSquare, labelKey: 'sidebar.channel' },
          ]
          const activeIndex = TABS.findIndex((tab) => tab.key === sidebarTab)
          return (
            <div className="relative grid grid-cols-4 rounded-md bg-sidebar-accent py-0.5 px-1 text-xs font-medium text-muted-foreground">
              {/* Sliding indicator — left/width account for px-1 (4px) horizontal padding */}
              <div
                className="absolute rounded-[5px] bg-background shadow-sm"
                style={{
                  top: '2px',
                  bottom: '2px',
                  left: '4px',
                  width: 'calc(25% - 2px)',
                  transform: `translateX(${activeIndex * 100}%)`,
                  transition: 'transform 200ms ease-in-out',
                }}
              />
              {TABS.map(({ key, Icon, labelKey }) => (
                <TooltipProvider key={key} delayDuration={400}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        aria-label={t(labelKey)}
                        onClick={() => switchTab(key)}
                        className={`relative z-10 flex items-center justify-center py-1.5 transition-colors duration-200 ${
                          sidebarTab === key ? 'text-foreground' : ''
                        }`}
                      >
                        <Icon className="h-3.5 w-3.5 shrink-0" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="bottom">{t(labelKey)}</TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              ))}
            </div>
          )
        })()}

        {sidebarTab === 'project' ? (
          <div className="-mr-2 flex-1 overflow-auto">
            <ConversationTree
              projects={projects}
              onSelectConversation={(id) => void switchConversation(id)}
              onRenameConversation={handleRenameOpen}
              onArchiveConversation={(id) => void handleArchive(id)}
              onTogglePinConversation={(id, next) => void setConversationPinned(id, next)}
            />
          </div>
        ) : sidebarTab === 'employee' ? (
          <div className="-mr-2 flex-1 overflow-auto py-1">
            {renderFlatTab(employeeConversations)}
          </div>
        ) : sidebarTab === 'expert-team' ? (
          <div className="-mr-2 flex-1 overflow-auto py-1">
            {renderFlatTab(expertTeamConversations)}
          </div>
        ) : (
          <div className="-mr-2 flex-1 overflow-auto pr-2">
            <div className="mt-2 flex flex-col gap-3">
              <div>
                <div className="mb-2 flex items-center gap-2 px-2 text-sm font-semibold text-sidebar-foreground">
                  <img src="/logos/dingtalk.png" alt="" className="h-5 w-5 rounded" draggable={false} />
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
                      {!dingtalkState?.configured
                        ? '未配置，点击右侧设置'
                        : !dingtalkState.enabled
                          ? '未连接，点击右侧启用'
                          : dingtalkState.connection === 'connected'
                            ? '暂无会话，等待消息…'
                            : dingtalkState.connection === 'connecting'
                              ? '连接中…'
                              : '未连接，点击右侧重试'}
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
                        <span className="truncate">{channelConversationLabel(conversation)}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>
              </div>

              <div>
                <div className="mb-2 flex items-center gap-2 px-2 text-sm font-semibold text-sidebar-foreground">
                  <img src="/logos/feishu.png" alt="" className="h-5 w-5 rounded" draggable={false} />
                  飞书
                  <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                    {!feishuState?.configured
                      ? '未配置'
                      : !feishuState.enabled
                        ? '未连接'
                        : feishuState.connection === 'connected'
                          ? '已连接'
                          : feishuState.connection === 'connecting'
                            ? '连接中'
                            : feishuState.connection === 'reconnecting'
                              ? '重连中'
                              : feishuState.connection === 'configError'
                                ? '配置有误'
                                : '未连接'}
                  </span>
                </div>
                <div className="pl-6">
                  {activeFeishuConversations.length === 0 ? (
                    <button
                      type="button"
                      onClick={openChannelOverview}
                      className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
                    >
                      {!feishuState?.configured
                        ? '未配置，点击右侧设置'
                        : !feishuState.enabled
                          ? '未连接，点击右侧启用'
                          : feishuState.connection === 'connected'
                            ? '暂无会话，等待消息…'
                            : feishuState.connection === 'connecting'
                              ? '连接中…'
                              : '未连接，点击右侧重试'}
                    </button>
                  ) : (
                    activeFeishuConversations.map((conversation) => (
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
                        <span className="truncate">{channelConversationLabel(conversation)}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>
              </div>

              <div>
                <div className="mb-2 flex items-center gap-2 px-2 text-sm font-semibold text-sidebar-foreground">
                  <img src="/logos/wecom.png" alt="" className="h-5 w-5 rounded" draggable={false} />
                  企业微信
                  <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                    {!wecomState?.configured
                      ? '未配置'
                      : !wecomState.enabled
                        ? '未连接'
                        : wecomState.connection === 'connected'
                          ? '已连接'
                          : wecomState.connection === 'connecting'
                            ? '连接中'
                            : wecomState.connection === 'reconnecting'
                              ? '重连中'
                              : wecomState.connection === 'configError'
                                ? '配置有误'
                                : '未连接'}
                  </span>
                </div>
                <div className="pl-6">
                  {activeWecomConversations.length === 0 ? (
                    <button
                      type="button"
                      onClick={openChannelOverview}
                      className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
                    >
                      {!wecomState?.configured
                        ? '未配置，点击右侧设置'
                        : !wecomState.enabled
                          ? '未连接，点击右侧启用'
                          : wecomState.connection === 'connected'
                            ? '暂无会话，等待消息…'
                            : wecomState.connection === 'connecting'
                              ? '连接中…'
                              : '未连接，点击右侧重试'}
                    </button>
                  ) : (
                    activeWecomConversations.map((conversation) => (
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
                        <span className="truncate">{channelConversationLabel(conversation)}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>
              </div>

              <div>
                <div className="mb-2 flex items-center gap-2 px-2 text-sm font-semibold text-sidebar-foreground">
                  <img src="/logos/wechat.png" alt="" className="h-5 w-5 rounded" draggable={false} />
                  个人微信
                  <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                    {!wechatState?.configured
                      ? '未配置'
                      : !wechatState.enabled
                        ? '未连接'
                        : wechatState.connection === 'connected'
                          ? '已连接'
                          : wechatState.connection === 'connecting'
                            ? '连接中'
                            : wechatState.connection === 'reconnecting'
                              ? '重连中'
                              : wechatState.connection === 'configError'
                                ? '配置有误'
                                : wechatState.connection === 'needsReauth'
                                  ? '需重新扫码'
                                  : '未连接'}
                  </span>
                </div>
                <div className="pl-6">
                  {activeWechatConversations.length === 0 ? (
                    <button
                      type="button"
                      onClick={openChannelOverview}
                      className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
                    >
                      {!wechatState?.configured
                        ? '未配置，点击右侧设置'
                        : !wechatState.enabled
                          ? '未连接，点击右侧启用'
                          : wechatState.connection === 'connected'
                            ? '暂无会话，等待消息…'
                            : wechatState.connection === 'connecting'
                              ? '连接中…'
                              : wechatState.connection === 'needsReauth'
                                ? '会话已过期，请重新扫码'
                                : '未连接，点击右侧重试'}
                    </button>
                  ) : (
                    activeWechatConversations.map((conversation) => (
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
                        <span className="truncate">{channelConversationLabel(conversation)}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>
              </div>

              <div>
                <div className="mb-2 flex items-center gap-2 px-2 text-sm font-semibold text-sidebar-foreground">
                  <img src="/logos/telegram.png" alt="" className="h-5 w-5 rounded" draggable={false} />
                  Telegram
                  <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                    {!telegramState?.configured
                      ? '未配置'
                      : !telegramState.enabled
                        ? '未连接'
                        : telegramState.connection === 'connected'
                          ? '已连接'
                          : telegramState.connection === 'connecting'
                            ? '连接中'
                            : telegramState.connection === 'reconnecting'
                              ? '重连中'
                              : telegramState.connection === 'configError'
                                ? '配置有误'
                                : telegramState.connection === 'needsReauth'
                                  ? 'Token 失效'
                                  : '未连接'}
                  </span>
                </div>
                <div className="pl-6">
                  {activeTelegramConversations.length === 0 ? (
                    <button
                      type="button"
                      onClick={openChannelOverview}
                      className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
                    >
                      {!telegramState?.configured
                        ? '未配置，点击右侧设置'
                        : !telegramState.enabled
                          ? '未连接，点击右侧启用'
                          : telegramState.connection === 'connected'
                            ? '暂无会话，等待消息…'
                            : telegramState.connection === 'connecting'
                              ? '连接中…'
                              : telegramState.connection === 'needsReauth'
                                ? 'Token 失效，请重新配置'
                                : '未连接，点击右侧重试'}
                    </button>
                  ) : (
                    activeTelegramConversations.map((conversation) => (
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
                        <span className="truncate">{channelConversationLabel(conversation)}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>
              </div>

              <div>
                <div className="mb-2 flex items-center gap-2 px-2 text-sm font-semibold text-sidebar-foreground">
                  <img src="/logos/whatsapp.png" alt="" className="h-5 w-5 rounded" draggable={false} />
                  WhatsApp
                  <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                    {!whatsappState?.configured
                      ? '未配置'
                      : !whatsappState.enabled
                        ? '未连接'
                        : whatsappState.connection === 'connected'
                          ? '已连接'
                          : whatsappState.connection === 'connecting'
                            ? '连接中'
                            : whatsappState.connection === 'reconnecting'
                              ? '重连中'
                              : whatsappState.connection === 'configError'
                                ? '配置有误'
                                : whatsappState.connection === 'needsReauth'
                                  ? '会话失效'
                                  : '未连接'}
                  </span>
                </div>
                <div className="pl-6">
                  {activeWhatsappConversations.length === 0 ? (
                    <button
                      type="button"
                      onClick={openChannelOverview}
                      className="w-full rounded-lg px-3 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground"
                    >
                      {!whatsappState?.configured
                        ? '未配置，点击右侧设置'
                        : !whatsappState.enabled
                          ? '未连接，点击右侧启用'
                          : whatsappState.connection === 'connected'
                            ? '暂无会话，等待消息…'
                            : whatsappState.connection === 'connecting'
                              ? '连接中…'
                              : whatsappState.connection === 'needsReauth'
                                ? '会话失效，请重新扫码'
                                : '未连接，点击右侧重试'}
                    </button>
                  ) : (
                    activeWhatsappConversations.map((conversation) => (
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
                        <span className="truncate">{channelConversationLabel(conversation)}</span>
                        {conversation.unreadCount > 0 && (
                          <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                            {conversation.unreadCount}
                          </span>
                        )}
                      </button>
                    ))
                  )}
                </div>
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
  </>
  )
}
