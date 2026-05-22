import { useEffect, useMemo, useState } from 'react'
import { MoreHorizontal } from 'lucide-react'
import { AppDropdown } from '@/components/common/AppDropdown'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { initChannelListeners, useChannelStore } from '@/stores/channelStore'
import { useChatStore } from '@/stores/chatStore'
import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { RightPanel } from '@/components/chat/RightPanel'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { TeamChatDrawer } from '@/components/team/TeamChatDrawer'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Switch } from '@/components/common/Switch'
import { getMessages, getTasks, openGeneratedFile } from '@/lib/tauri'
import type { ChannelPlatform, ChannelPlatformState } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { ChannelConfig } from './ChannelConfig'
import { ChannelConfigDetails } from './ChannelConfigDetails'
import { FeishuChannelConfig } from './FeishuChannelConfig'
import { TelegramChannelConfig } from './TelegramChannelConfig'
import { WecomChannelConfig } from './WecomChannelConfig'
import { WechatChannelConfig } from './WechatChannelConfig'
import { WhatsappChannelConfig } from './WhatsappChannelConfig'

interface ChannelPageProps {
  sessionId?: string
}

type PlatformKey = ChannelPlatform

const PLATFORM_DISPLAY_NAME: Record<PlatformKey, string> = {
  dingtalk: '钉钉',
  feishu: '飞书',
  wecom: '企业微信',
  wechat: '个人微信',
  telegram: 'Telegram',
  whatsapp: 'WhatsApp',
}

// eslint-disable-next-line react-refresh/only-export-components
export const PLATFORM_LOGO_SRC: Record<PlatformKey, string> = {
  dingtalk: '/logos/dingtalk.png',
  feishu: '/logos/feishu.png',
  wecom: '/logos/wecom.png',
  wechat: '/logos/wechat.png',
  telegram: '/logos/telegram.png',
  whatsapp: '/logos/whatsapp.png',
}

interface PlatformCardModel {
  key: PlatformKey
  name: string
  description: string
  logoSrc: string
  state: ChannelPlatformState
  statusLabel: string
  statusTone: 'success' | 'muted' | 'error' | 'pending'
  networkHint: string | null
}

function statusMeta(state: ChannelPlatformState) {
  if (state.capability === 'comingSoon') return { statusLabel: '未配置', statusTone: 'muted' as const }
  if (!state.configured) return { statusLabel: '未配置', statusTone: 'muted' as const }
  if (!state.enabled) return { statusLabel: '已配置 / 未连接', statusTone: 'muted' as const }
  switch (state.connection) {
    case 'connected':
      return { statusLabel: '已连接', statusTone: 'success' as const }
    case 'connecting':
      return { statusLabel: '连接中', statusTone: 'pending' as const }
    case 'reconnecting':
      return { statusLabel: '重连中', statusTone: 'pending' as const }
    case 'configError':
      return { statusLabel: '配置有误', statusTone: 'error' as const }
    case 'needsReauth':
      return { statusLabel: '会话失效', statusTone: 'error' as const }
    case 'disconnected':
      return { statusLabel: '未连接', statusTone: 'muted' as const }
    default:
      return { statusLabel: '未连接', statusTone: 'muted' as const }
  }
}

/**
 * 已配置 + 已启用但连不上时给一句具体的、用户可操作的提示，覆盖默认
 * "通过 XX 机器人接收并回复用户消息" 描述。
 *
 * 现在只对 Telegram / WhatsApp 触发：这两个走海外服务器，国内通常需要代理。
 * 其它渠道（钉钉/飞书/企微/微信）走国内服务器，"重连中"通常是临时问题，
 * 不给一刀切的代理提示避免误导。
 *
 * 重要：connection=disconnected 不一定是网络问题。WhatsApp 主端踢掉 web
 * session 时后端可能仍停留在 disconnected（等待 connector 翻成 needsReauth），
 * 此时再说"网络/代理不可用"是误报。所以这里的提示只描述现象 + 列出几种
 * 可能的原因，不主观断言。
 */
function networkHint(state: ChannelPlatformState): string | null {
  if (!state.configured || !state.enabled) return null
  if (state.platform !== 'telegram' && state.platform !== 'whatsapp') return null
  const platformName = state.platform === 'telegram' ? 'Telegram' : 'WhatsApp'
  switch (state.connection) {
    case 'connecting':
      return `正在连接 ${platformName} 服务器…`
    case 'reconnecting':
      return `${platformName} 连接已断开，正在重连…`
    case 'disconnected':
      return state.platform === 'whatsapp'
        ? '未连接到 WhatsApp。可能原因：网络/代理不可用、手机端 WhatsApp 离线，或主端已踢掉本设备会话（需重新扫码）'
        : '未连接到 Telegram 服务器，请检查网络 / 代理是否可用'
    case 'needsReauth':
      return state.platform === 'whatsapp'
        ? '与 WhatsApp 主端会话失效（常见于手机端退出登录或长时间离线），需要重新扫码配对'
        : 'Telegram 会话失效，请重新登录'
    default:
      return null
  }
}

function StatusBadge({ label, tone }: { label?: string; tone?: PlatformCardModel['statusTone'] }) {
  if (!label) return null
  const className =
    tone === 'success'
      ? 'bg-emerald-50 text-emerald-600'
      : tone === 'error'
        ? 'bg-red-50 text-red-500'
        : tone === 'pending'
          ? 'bg-amber-50 text-amber-600'
          : 'bg-muted text-muted-foreground'
  return <span className={`rounded-md px-2 py-1 text-xs font-bold ${className}`}>{label}</span>
}

function PlatformIcon({ platform }: { platform: PlatformCardModel }) {
  return (
    <img
      src={platform.logoSrc}
      alt=""
      className="h-10 w-10 shrink-0 rounded-lg shadow-[var(--shadow-md)]"
      draggable={false}
    />
  )
}

function PlatformCard({
  platform,
  onRegister,
  onShowDetails,
  onRemove,
  onToggle,
}: {
  platform: PlatformCardModel
  onRegister: () => void
  onShowDetails: () => void
  onRemove: () => void
  onToggle: (enabled: boolean) => void
}) {
  return (
    <div className="flex min-h-[64px] items-center justify-between rounded-lg border border-border bg-card px-4 py-3">
      <div className="flex min-w-0 items-center gap-3">
        <PlatformIcon platform={platform} />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-foreground">{platform.name}</h3>
            <StatusBadge label={platform.statusLabel} tone={platform.statusTone} />
          </div>
          {platform.networkHint ? (
            <p className="mt-0.5 text-xs font-medium text-destructive">{platform.networkHint}</p>
          ) : (
            <p className="mt-0.5 text-xs font-medium text-muted-foreground">{platform.description}</p>
          )}
        </div>
      </div>

      <div className="ml-4 flex shrink-0 items-center gap-3">
        {platform.state.configured && (
          <AppDropdown
            ariaLabel={`更多${platform.name}配置`}
            trigger={
              <button type="button" className="rounded-full p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
                <MoreHorizontal className="h-4 w-4" />
              </button>
            }
            items={[
              { id: 'configure', label: '配置', onSelect: onShowDetails },
              { id: 'remove', label: '移除', className: 'text-destructive', onSelect: onRemove },
            ]}
          />
        )}
        {platform.state.configured ? (
          <Switch
            checked={platform.state.enabled}
            aria-label={platform.state.enabled ? `${platform.name}频道已启用` : `${platform.name}频道已停用`}
            onCheckedChange={onToggle}
          />
        ) : platform.state.capability === 'available' ? (
          <Button
            type="button"
            size="sm"
            className="rounded-full px-4"
            onClick={onRegister}
            aria-label={`配置${platform.name}`}
          >
            配置
          </Button>
        ) : (
          <Button type="button" size="sm" className="rounded-full px-4" disabled>
            配置
          </Button>
        )}
      </div>
    </div>
  )
}

function ChannelHero() {
  return (
    <div className="flex flex-col items-center text-center">
      <h1 className="text-4xl font-extrabold tracking-tight text-foreground">IM 频道</h1>
      <p className="mt-5 max-w-2xl text-lg font-semibold leading-8 text-muted-foreground">
        配置 IM 频道，让 AI 小家 接收并回复来自各平台的消息。
        <br />频道配置信息仅存储在本地，不会上传到云端。
      </p>
    </div>
  )
}

function ChannelOverview({
  platforms,
  onRegisterDingtalk,
  onShowDingtalkDetails,
  onRemoveDingtalk,
  onToggleDingtalk,
  onRegisterFeishu,
  onShowFeishuDetails,
  onRemoveFeishu,
  onToggleFeishu,
  onRegisterWecom,
  onShowWecomDetails,
  onRemoveWecom,
  onToggleWecom,
  onRegisterWechat,
  onShowWechatDetails,
  onRemoveWechat,
  onToggleWechat,
  onRegisterTelegram,
  onShowTelegramDetails,
  onRemoveTelegram,
  onToggleTelegram,
  onRegisterWhatsapp,
  onShowWhatsappDetails,
  onRemoveWhatsapp,
  onToggleWhatsapp,
}: {
  platforms: PlatformCardModel[]
  onRegisterDingtalk: () => void
  onShowDingtalkDetails: () => void
  onRemoveDingtalk: () => void
  onToggleDingtalk: (enabled: boolean) => void
  onRegisterFeishu: () => void
  onShowFeishuDetails: () => void
  onRemoveFeishu: () => void
  onToggleFeishu: (enabled: boolean) => void
  onRegisterWecom: () => void
  onShowWecomDetails: () => void
  onRemoveWecom: () => void
  onToggleWecom: (enabled: boolean) => void
  onRegisterWechat: () => void
  onShowWechatDetails: () => void
  onRemoveWechat: () => void
  onToggleWechat: (enabled: boolean) => void
  onRegisterTelegram: () => void
  /** 已配对后 kebab "配置" 入口 —— 复用注册对话框,组件内部按 alreadyConfigured 渲染管理界面。 */
  onShowTelegramDetails: () => void
  onRemoveTelegram: () => void
  onToggleTelegram: (enabled: boolean) => void
  onRegisterWhatsapp: () => void
  /** 同上,WhatsApp 二次编辑入口,对话框按 connected 显示"允许列表"管理。 */
  onShowWhatsappDetails: () => void
  onRemoveWhatsapp: () => void
  onToggleWhatsapp: (enabled: boolean) => void
}) {
  const noop = () => {}
  const noopToggle = () => {}
  return (
    <div className="flex min-h-full flex-col">
      <div data-tauri-drag-region className="h-10 shrink-0" />
      <div className="mx-auto flex w-full max-w-4xl flex-1 flex-col justify-center gap-8 px-6 pb-24 pt-6">
        <ChannelHero />

        <div className="flex flex-col gap-4">
          {platforms.map((platform) => (
            <PlatformCard
              key={platform.key}
              platform={platform}
              onRegister={
                platform.key === 'dingtalk'
                  ? onRegisterDingtalk
                  : platform.key === 'feishu'
                    ? onRegisterFeishu
                    : platform.key === 'wecom'
                      ? onRegisterWecom
                      : platform.key === 'wechat'
                        ? onRegisterWechat
                        : platform.key === 'telegram'
                          ? onRegisterTelegram
                          : platform.key === 'whatsapp'
                            ? onRegisterWhatsapp
                            : noop
              }
              onShowDetails={
                platform.key === 'dingtalk'
                  ? onShowDingtalkDetails
                  : platform.key === 'feishu'
                    ? onShowFeishuDetails
                    : platform.key === 'wecom'
                      ? onShowWecomDetails
                      : platform.key === 'wechat'
                        ? onShowWechatDetails
                        : platform.key === 'telegram'
                          ? onShowTelegramDetails
                          : platform.key === 'whatsapp'
                            ? onShowWhatsappDetails
                            : noop
              }
              onRemove={
                platform.key === 'dingtalk'
                  ? onRemoveDingtalk
                  : platform.key === 'feishu'
                    ? onRemoveFeishu
                    : platform.key === 'wecom'
                      ? onRemoveWecom
                      : platform.key === 'wechat'
                        ? onRemoveWechat
                        : platform.key === 'telegram'
                          ? onRemoveTelegram
                          : platform.key === 'whatsapp'
                            ? onRemoveWhatsapp
                            : noop
              }
              onToggle={
                platform.key === 'dingtalk'
                  ? onToggleDingtalk
                  : platform.key === 'feishu'
                    ? onToggleFeishu
                    : platform.key === 'wecom'
                      ? onToggleWecom
                      : platform.key === 'wechat'
                        ? onToggleWechat
                        : platform.key === 'telegram'
                          ? onToggleTelegram
                          : platform.key === 'whatsapp'
                            ? onToggleWhatsapp
                            : noopToggle
              }
            />
          ))}
        </div>
      </div>
    </div>
  )
}

function ChannelChatView({ sessionId }: { sessionId: string }) {
  const conversations = useChannelStore((s) => s.conversations)
  const pushNotification = useNotificationStore((s) => s.push)
  const activeConv = conversations.find((c) => c.sessionId === sessionId)
  // 顶栏副标题：直接展示后端填的 displayName（= sender push_name / nick / userid）。
  // 飞书 / 个人微信目前后端拿不到真实用户名（飞书是 "飞书用户 ou_xxx" 占位，
  // 微信是裸 wxid_xxx 或 openid@im.wechat），展示无价值，整段 workspace 隐藏；
  // 等后端补上真实用户名时把 platform 从 HIDE_WORKSPACE_PLATFORMS 删掉即可。
  const HIDE_WORKSPACE_PLATFORMS = new Set(['feishu', 'wechat'])
  const title = activeConv
    ? activeConv.displayName?.trim() ||
      (activeConv.conversationType === 'group' ? '群聊' : '私聊')
    : ''
  const workspaceLabel =
    activeConv && HIDE_WORKSPACE_PLATFORMS.has(activeConv.platform)
      ? undefined
      : title || sessionId
  const platformTitle = activeConv
    ? (PLATFORM_DISPLAY_NAME[activeConv.platform] ?? activeConv.platform)
    : ''
  const isInactiveSession = !!activeConv && !activeConv.isActiveRobot
  const { overview: teamOverview } = useTeamOverview(sessionId)

  const handleOpenPreviewTarget = async (target: PreviewTarget) => {
    try {
      await openGeneratedFile(target.fileId, target.conversationId)
    } catch (err) {
      pushNotification({
        level: 'error',
        title: '无法打开文件',
        message: err instanceof Error ? err.message : '打开生成文件失败。',
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-background">
      <ChatTopBar title={platformTitle} workspace={workspaceLabel} />
      <div className="relative flex min-h-0 flex-1 overflow-hidden">
        <div data-testid="channel-chat-layout-column" className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
          <ChatArea />
          {isInactiveSession && (
            <div className="px-6 pb-2">
              <div className="rounded-md bg-muted px-3 py-2 text-sm text-muted-foreground">
                该会话来自已下线的机器人，无法发送新消息
              </div>
            </div>
          )}
          <ChatBottomArea disabled={isInactiveSession} sessionIdOverride={sessionId} />
        </div>
        <TeamChatDrawer conversationId={sessionId} overview={teamOverview} />
        <RightPanel conversationId={sessionId} onOpenExternal={(target) => void handleOpenPreviewTarget(target)} />
      </div>
    </div>
  )
}

export function ChannelPage({ sessionId }: ChannelPageProps) {
  const platformsByKey = useChannelStore((s) => s.platforms)
  const loadConversations = useChannelStore((s) => s.loadConversations)
  const setEnabled = useChannelStore((s) => s.setEnabled)
  const removePlatform = useChannelStore((s) => s.removePlatform)
  const [registrationOpen, setRegistrationOpen] = useState(false)
  const [detailsOpen, setDetailsOpen] = useState(false)
  const [feishuRegistrationOpen, setFeishuRegistrationOpen] = useState(false)
  const [feishuDetailsOpen, setFeishuDetailsOpen] = useState(false)
  const [wecomRegistrationOpen, setWecomRegistrationOpen] = useState(false)
  const [wecomDetailsOpen, setWecomDetailsOpen] = useState(false)
  const [wechatRegistrationOpen, setWechatRegistrationOpen] = useState(false)
  const [wechatDetailsOpen, setWechatDetailsOpen] = useState(false)
  const [telegramRegistrationOpen, setTelegramRegistrationOpen] = useState(false)
  const [whatsappRegistrationOpen, setWhatsappRegistrationOpen] = useState(false)

  useEffect(() => {
    void initChannelListeners()
    void loadConversations()
  }, [loadConversations])

  useEffect(() => {
    const store = useChatStore.getState()
    const activeId = sessionId ?? null

    if (!activeId) {
      if (store.activeConversationId !== null) {
        store.setMessages([])
      }
      return
    }

    // Selecting a session = the user has seen the new messages → reset the
    // unread badge. `incrementUnread` fires on every `channel:message` event
    // when this session isn't the active one (see channelStore.ts:171),
    // so without this counter clear the badge would grow forever.
    useChannelStore.getState().clearUnread(activeId)

    let cancelled = false
    store.setMessages([])

    void Promise.all([
      getMessages(activeId),
      getTasks(activeId).catch(() => []),
    ])
      .then(([messages, tasks]) => {
        if (cancelled) return
        const latest = useChatStore.getState()
        latest.setMessages(messages)
        for (const task of tasks) {
          latest.upsertConversationTaskState(activeId, task)
        }
      })
      .catch((err) => {
        if (!cancelled) console.error('[ChannelPage] load channel session failed', err)
      })

    return () => {
      cancelled = true
    }
  }, [sessionId])

  const dingtalkState = platformsByKey.dingtalk ?? {
    platform: 'dingtalk',
    capability: 'available',
    configured: false,
    enabled: false,
    connection: 'unconfigured',
    config: null,
    lastConnectedAt: null,
    lastError: null,
  } satisfies ChannelPlatformState

  const feishuState = platformsByKey.feishu ?? {
    platform: 'feishu',
    capability: 'available',
    configured: false,
    enabled: false,
    connection: 'unconfigured',
    config: null,
    lastConnectedAt: null,
    lastError: null,
  } satisfies ChannelPlatformState

  const wecomState = platformsByKey.wecom ?? {
    platform: 'wecom',
    capability: 'available',
    configured: false,
    enabled: false,
    connection: 'unconfigured',
    config: null,
    lastConnectedAt: null,
    lastError: null,
  } satisfies ChannelPlatformState

  const wechatState = platformsByKey.wechat ?? {
    platform: 'wechat',
    // MVP: report as available so the user can click the button to drive a
    // real scan flow. Backend persistence still lands in Phase 5 PR3, so the
    // card never reaches "configured" state in this MVP cut.
    capability: 'available',
    configured: false,
    enabled: false,
    connection: 'unconfigured',
    config: null,
    lastConnectedAt: null,
    lastError: null,
  } satisfies ChannelPlatformState

  const telegramState = platformsByKey.telegram ?? {
    platform: 'telegram',
    capability: 'available',
    configured: false,
    enabled: false,
    connection: 'unconfigured',
    config: null,
    lastConnectedAt: null,
    lastError: null,
  } satisfies ChannelPlatformState

  const whatsappState = platformsByKey.whatsapp ?? {
    platform: 'whatsapp',
    capability: 'comingSoon',
    configured: false,
    enabled: false,
    connection: 'unconfigured',
    config: null,
    lastConnectedAt: null,
    lastError: null,
  } satisfies ChannelPlatformState

  const handleRemoveDingtalk = async () => {
    const confirmed = await requestConfirm({
      title: '移除钉钉频道？',
      description: '这会断开钉钉频道，并删除本地保存的 AppKey 和 AppSecret。已有聊天历史会保留。之后需要重新扫码才能再次配置。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    await removePlatform('dingtalk')
  }

  const handleToggleDingtalk = async (enabled: boolean) => {
    await setEnabled('dingtalk', enabled)
  }

  const handleRemoveFeishu = async () => {
    const confirmed = await requestConfirm({
      title: '移除飞书频道？',
      description: '这会断开飞书频道，并删除本地保存的 AppID 和 AppSecret。已有聊天历史会保留。之后需要重新扫码才能再次配置。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    await removePlatform('feishu')
  }

  const handleToggleFeishu = async (enabled: boolean) => {
    await setEnabled('feishu', enabled)
  }

  const handleRemoveWecom = async () => {
    const confirmed = await requestConfirm({
      title: '移除企业微信频道？',
      description: '这会断开企业微信频道，并删除本地保存的 Bot ID 和 Secret。已有聊天历史会保留。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    await removePlatform('wecom')
  }

  const handleToggleWecom = async (enabled: boolean) => {
    await setEnabled('wecom', enabled)
  }

  const handleRemoveWechat = async () => {
    const confirmed = await requestConfirm({
      title: '移除个人微信频道？',
      description: '这会断开个人微信频道并删除本地保存的登录凭证。已有聊天历史会保留。之后需要重新扫码才能再次配置。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    await removePlatform('wechat')
  }

  const handleToggleWechat = async (enabled: boolean) => {
    await setEnabled('wechat', enabled)
  }

  const handleRemoveTelegram = async () => {
    const confirmed = await requestConfirm({
      title: '移除 Telegram 频道？',
      description: '会删除本地保存的 bot token 和已配对用户列表。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    await removePlatform('telegram')
  }

  const handleToggleTelegram = async (enabled: boolean) => {
    await setEnabled('telegram', enabled)
  }

  const handleRemoveWhatsapp = async () => {
    const confirmed = await requestConfirm({
      title: '移除 WhatsApp 频道？',
      description: '会删除本地保存的 WhatsApp 会话和允许列表。已有聊天历史保留。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    await removePlatform('whatsapp')
  }

  const handleToggleWhatsapp = async (enabled: boolean) => {
    await setEnabled('whatsapp', enabled)
  }

  const platforms = useMemo<PlatformCardModel[]>(() => {
    const states: Record<PlatformKey, ChannelPlatformState> = {
      dingtalk: dingtalkState,
      feishu: feishuState,
      wecom: wecomState,
      wechat: wechatState,
      telegram: telegramState,
      whatsapp: whatsappState,
    }

    return [
      {
        key: 'dingtalk',
        name: PLATFORM_DISPLAY_NAME.dingtalk,
        description: '通过钉钉机器人接收并回复用户消息',
        logoSrc: PLATFORM_LOGO_SRC.dingtalk,
        state: states.dingtalk,
        networkHint: networkHint(states.dingtalk),
        ...statusMeta(states.dingtalk),
      },
      {
        key: 'feishu',
        name: PLATFORM_DISPLAY_NAME.feishu,
        description: '通过飞书机器人接收并回复用户消息',
        logoSrc: PLATFORM_LOGO_SRC.feishu,
        state: states.feishu,
        networkHint: networkHint(states.feishu),
        ...statusMeta(states.feishu),
      },
      {
        key: 'wecom',
        name: PLATFORM_DISPLAY_NAME.wecom,
        description: '通过企业微信机器人接收并回复用户消息',
        logoSrc: PLATFORM_LOGO_SRC.wecom,
        state: states.wecom,
        networkHint: networkHint(states.wecom),
        ...statusMeta(states.wecom),
      },
      {
        key: 'wechat',
        name: PLATFORM_DISPLAY_NAME.wechat,
        description: '通过个人微信账号接收并回复用户消息',
        logoSrc: PLATFORM_LOGO_SRC.wechat,
        state: states.wechat,
        networkHint: networkHint(states.wechat),
        ...statusMeta(states.wechat),
      },
      {
        key: 'telegram',
        name: PLATFORM_DISPLAY_NAME.telegram,
        description: '通过 Telegram 机器人接收并回复用户消息',
        logoSrc: PLATFORM_LOGO_SRC.telegram,
        state: states.telegram,
        networkHint: networkHint(states.telegram),
        ...statusMeta(states.telegram),
      },
      {
        key: 'whatsapp',
        name: PLATFORM_DISPLAY_NAME.whatsapp,
        description: '通过 WhatsApp 机器人接收并回复用户消息',
        logoSrc: PLATFORM_LOGO_SRC.whatsapp,
        state: states.whatsapp,
        networkHint: networkHint(states.whatsapp),
        ...statusMeta(states.whatsapp),
      },
    ]
  }, [dingtalkState, feishuState, wecomState, wechatState, telegramState, whatsappState])

  return (
    <div className={sessionId ? 'h-full overflow-hidden bg-background' : 'h-full overflow-y-auto bg-background'}>
      {sessionId ? (
        <ChannelChatView sessionId={sessionId} />
      ) : (
        <ChannelOverview
          platforms={platforms}
          onRegisterDingtalk={() => setRegistrationOpen(true)}
          onShowDingtalkDetails={() => setDetailsOpen(true)}
          onRemoveDingtalk={() => void handleRemoveDingtalk()}
          onToggleDingtalk={(enabled) => void handleToggleDingtalk(enabled)}
          onRegisterFeishu={() => setFeishuRegistrationOpen(true)}
          onShowFeishuDetails={() => setFeishuDetailsOpen(true)}
          onRemoveFeishu={() => void handleRemoveFeishu()}
          onToggleFeishu={(enabled) => void handleToggleFeishu(enabled)}
          onRegisterWecom={() => setWecomRegistrationOpen(true)}
          onShowWecomDetails={() => setWecomDetailsOpen(true)}
          onRemoveWecom={() => void handleRemoveWecom()}
          onToggleWecom={(enabled) => void handleToggleWecom(enabled)}
          onRegisterWechat={() => setWechatRegistrationOpen(true)}
          onShowWechatDetails={() => setWechatDetailsOpen(true)}
          onRemoveWechat={() => void handleRemoveWechat()}
          onToggleWechat={(enabled) => void handleToggleWechat(enabled)}
          onRegisterTelegram={() => setTelegramRegistrationOpen(true)}
          // 已配置后的 kebab "配置" 入口复用同一个对话框 ——
          // TelegramChannelConfig 检测到 alreadyConfigured 会跳过 token 步骤,
          // 直接进入"扫码配对 / 已连接用户管理 / 移除整个频道"管理界面。
          onShowTelegramDetails={() => setTelegramRegistrationOpen(true)}
          onRemoveTelegram={() => void handleRemoveTelegram()}
          onToggleTelegram={(enabled) => void handleToggleTelegram(enabled)}
          onRegisterWhatsapp={() => setWhatsappRegistrationOpen(true)}
          // 同 telegram —— WhatsappChannelConfig 收到 connected=true 时显示
          // "允许的发送人(E.164)"管理区域,允许编辑 allow_from 列表。
          onShowWhatsappDetails={() => setWhatsappRegistrationOpen(true)}
          onRemoveWhatsapp={() => void handleRemoveWhatsapp()}
          onToggleWhatsapp={(enabled) => void handleToggleWhatsapp(enabled)}
        />
      )}

      <Dialog open={registrationOpen} onOpenChange={setRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>配置钉钉</DialogTitle>
            <DialogDescription>在钉钉中扫码完成应用注册，也可以手动填写应用凭证。</DialogDescription>
          </DialogHeader>
          <ChannelConfig
            onSaved={() => {
              void loadConversations()
            }}
            onClose={() => setRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog open={feishuRegistrationOpen} onOpenChange={setFeishuRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>配置飞书</DialogTitle>
            <DialogDescription>在飞书中扫码完成应用注册。</DialogDescription>
          </DialogHeader>
          <FeishuChannelConfig
            onSaved={() => {
              void loadConversations()
            }}
            onClose={() => setFeishuRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog open={wecomRegistrationOpen} onOpenChange={setWecomRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>配置企业微信</DialogTitle>
            <DialogDescription>填写企业微信智能机器人凭证完成频道配置。</DialogDescription>
          </DialogHeader>
          <WecomChannelConfig
            onSaved={() => {
              void loadConversations()
            }}
            onClose={() => setWecomRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog open={wechatRegistrationOpen} onOpenChange={setWechatRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>添加个人微信账号</DialogTitle>
            <DialogDescription>使用微信扫码登录 iLink 个人微信账号。</DialogDescription>
          </DialogHeader>
          <WechatChannelConfig
            onSaved={() => {
              void loadConversations()
            }}
            onClose={() => setWechatRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog open={telegramRegistrationOpen} onOpenChange={setTelegramRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>配置 Telegram</DialogTitle>
            <DialogDescription>输入 Bot Token 并扫码配对。</DialogDescription>
          </DialogHeader>
          <TelegramChannelConfig
            onSaved={() => {
              void loadConversations()
            }}
            onClose={() => setTelegramRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog open={whatsappRegistrationOpen} onOpenChange={setWhatsappRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>添加 WhatsApp 账号</DialogTitle>
            <DialogDescription>使用手机 WhatsApp 扫码登录。</DialogDescription>
          </DialogHeader>
          <WhatsappChannelConfig
            onSaved={() => {
              void loadConversations()
            }}
            onClose={() => setWhatsappRegistrationOpen(false)}
            connected={
              whatsappState.connection === 'connected' ||
              whatsappState.connection === 'reconnecting'
            }
          />
        </DialogContent>
      </Dialog>

      {dingtalkState.config && (
        <ChannelConfigDetails config={dingtalkState.config} open={detailsOpen} onOpenChange={setDetailsOpen} />
      )}
      {feishuState.config && (
        <ChannelConfigDetails config={feishuState.config} open={feishuDetailsOpen} onOpenChange={setFeishuDetailsOpen} />
      )}
      {wecomState.config && (
        <ChannelConfigDetails config={wecomState.config} open={wecomDetailsOpen} onOpenChange={setWecomDetailsOpen} />
      )}
      {wechatState.config && (
        <ChannelConfigDetails config={wechatState.config} open={wechatDetailsOpen} onOpenChange={setWechatDetailsOpen} />
      )}
    </div>
  )
}
