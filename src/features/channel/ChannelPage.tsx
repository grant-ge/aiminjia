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
import type { ChannelPlatformState } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { ChannelConfig } from './ChannelConfig'
import { ChannelConfigDetails } from './ChannelConfigDetails'

interface ChannelPageProps {
  sessionId?: string
}

type PlatformKey = 'dingtalk'

interface PlatformCardModel {
  key: PlatformKey
  name: string
  description: string
  icon: string
  iconClassName: string
  state: ChannelPlatformState
  statusLabel: string
  statusTone: 'success' | 'muted' | 'error' | 'pending'
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
    default:
      return { statusLabel: '未配置', statusTone: 'muted' as const }
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
    <div className={`flex h-14 w-14 shrink-0 items-center justify-center rounded-xl text-xl font-bold ${platform.iconClassName}`}>
      {platform.icon}
    </div>
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
    <div className="flex min-h-[92px] items-center justify-between rounded-xl border border-border bg-card px-6 py-4">
      <div className="flex min-w-0 items-center gap-4">
        <PlatformIcon platform={platform} />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-lg font-bold text-foreground">{platform.name}</h3>
            <StatusBadge label={platform.statusLabel} tone={platform.statusTone} />
          </div>
          <p className="mt-1 text-sm font-medium text-muted-foreground">{platform.description}</p>
        </div>
      </div>

      <div className="ml-6 flex shrink-0 items-center gap-4">
        {platform.state.configured && platform.key === 'dingtalk' && (
          <AppDropdown
            ariaLabel="更多钉钉配置"
            trigger={
              <button type="button" className="rounded-full p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
                <MoreHorizontal className="h-5 w-5" />
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
            className="rounded-full px-6"
            onClick={onRegister}
            aria-label={`配置${platform.name}`}
          >
            配置
          </Button>
        ) : (
          <Button type="button" className="rounded-full px-6" disabled>
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
        配置钉钉 IM 频道，让 AI 小家 接收并回复来自钉钉的消息。
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
}: {
  platforms: PlatformCardModel[]
  onRegisterDingtalk: () => void
  onShowDingtalkDetails: () => void
  onRemoveDingtalk: () => void
  onToggleDingtalk: (enabled: boolean) => void
}) {
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
              onRegister={platform.key === 'dingtalk' ? onRegisterDingtalk : () => {}}
              onShowDetails={platform.key === 'dingtalk' ? onShowDingtalkDetails : () => {}}
              onRemove={platform.key === 'dingtalk' ? onRemoveDingtalk : () => {}}
              onToggle={platform.key === 'dingtalk' ? onToggleDingtalk : () => {}}
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
  const title = activeConv?.displayName ?? ''
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
      <ChatTopBar title="钉钉" workspace={title || sessionId} />
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

  const platforms = useMemo<PlatformCardModel[]>(() => {
    const states: Record<PlatformKey, ChannelPlatformState> = {
      dingtalk: dingtalkState,
    }

    return [
      {
        key: 'dingtalk',
        name: '钉钉',
        description: '通过钉钉机器人接收并回复用户消息',
        icon: '钉',
        // 钉钉品牌蓝 #0b8cff：是平台 logo 识别色，不随主题切换
        iconClassName: 'bg-sky-50 text-[var(--color-semantic-blue)]',
        state: states.dingtalk,
        ...statusMeta(states.dingtalk),
      },
    ]
  }, [dingtalkState])

  return (
    <div className={sessionId ? 'h-full overflow-hidden bg-white' : 'h-full overflow-y-auto bg-white'}>
      {sessionId ? (
        <ChannelChatView sessionId={sessionId} />
      ) : (
        <ChannelOverview
          platforms={platforms}
          onRegisterDingtalk={() => setRegistrationOpen(true)}
          onShowDingtalkDetails={() => setDetailsOpen(true)}
          onRemoveDingtalk={() => void handleRemoveDingtalk()}
          onToggleDingtalk={(enabled) => void handleToggleDingtalk(enabled)}
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

      {dingtalkState.config && (
        <ChannelConfigDetails config={dingtalkState.config} open={detailsOpen} onOpenChange={setDetailsOpen} />
      )}
    </div>
  )
}
