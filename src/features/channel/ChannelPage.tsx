import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
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
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
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
  statusKey: string
  statusTone: 'success' | 'muted' | 'error' | 'pending'
}

function statusMeta(state: ChannelPlatformState) {
  if (state.capability === 'comingSoon') return { statusKey: 'channel.status.unconfigured', statusTone: 'muted' as const }
  if (!state.configured) return { statusKey: 'channel.status.unconfigured', statusTone: 'muted' as const }
  if (!state.enabled) return { statusKey: 'channel.status.configuredOffline', statusTone: 'muted' as const }
  switch (state.connection) {
    case 'connected':
      return { statusKey: 'channel.status.connected', statusTone: 'success' as const }
    case 'connecting':
      return { statusKey: 'channel.status.connecting', statusTone: 'pending' as const }
    case 'reconnecting':
      return { statusKey: 'channel.status.reconnecting', statusTone: 'pending' as const }
    case 'configError':
      return { statusKey: 'channel.status.configError', statusTone: 'error' as const }
    default:
      return { statusKey: 'channel.status.unconfigured', statusTone: 'muted' as const }
  }
}

function StatusBadge({ tKey, tone }: { tKey?: string; tone?: PlatformCardModel['statusTone'] }) {
  const { t } = useTranslation()
  if (!tKey) return null
  const className =
    tone === 'success'
      ? 'bg-primary/10 text-primary'
      : tone === 'error'
        ? 'bg-destructive/10 text-destructive'
        : tone === 'pending'
          ? 'bg-primary/8 text-primary/80'
          : 'bg-muted text-muted-foreground'
  return <span className={`rounded-md px-2 py-1 text-xs font-bold ${className}`}>{t(tKey)}</span>
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
  const { t } = useTranslation()
  return (
    <div className="flex min-h-[92px] items-center justify-between rounded-xl border border-border bg-card px-6 py-4">
      <div className="flex min-w-0 items-center gap-4">
        <PlatformIcon platform={platform} />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-lg font-bold text-foreground">{platform.name}</h3>
            <StatusBadge tKey={platform.statusKey} tone={platform.statusTone} />
          </div>
          <p className="mt-1 text-sm font-medium text-muted-foreground">{platform.description}</p>
        </div>
      </div>

      <div className="ml-6 flex shrink-0 items-center gap-4">
        {platform.state.configured && platform.key === 'dingtalk' && (
          <AppDropdown
            ariaLabel={t('channel.actions.moreDingtalkConfig')}
            trigger={
              <button type="button" className="rounded-full p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
                <MoreHorizontal className="h-5 w-5" />
              </button>
            }
            items={[
              { id: 'configure', label: t('channel.actions.configure'), onSelect: onShowDetails },
              { id: 'remove', label: t('channel.actions.remove'), className: 'text-destructive', onSelect: onRemove },
            ]}
          />
        )}
        {platform.state.configured ? (
          <Switch
            checked={platform.state.enabled}
            aria-label={t(platform.state.enabled ? 'channel.actions.enabledAria' : 'channel.actions.disabledAria', { name: platform.name })}
            onCheckedChange={onToggle}
          />
        ) : platform.state.capability === 'available' ? (
          <Button
            type="button"
            className="rounded-full px-6"
            onClick={onRegister}
            aria-label={t('channel.actions.configureWith', { name: platform.name })}
          >
            {t('channel.actions.configure')}
          </Button>
        ) : (
          <Button type="button" className="rounded-full px-6" disabled>
            {t('channel.actions.configure')}
          </Button>
        )}
      </div>
    </div>
  )
}

function ChannelHero() {
  const { t } = useTranslation()
  return (
    <div className="flex flex-col items-center text-center">
      <h1 className="text-2xl font-bold tracking-tight text-foreground">{t('channel.heroTitle')}</h1>
      <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
        {t('channel.heroDesc')}
        <br />{t('channel.heroPrivacy')}
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
  const { t } = useTranslation()
  return (
    <PageSectionShell
      topBar={<PageTopBar variant="title" title={t('channel.title')} />}
      maxWidthClass="max-w-4xl"
    >
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
    </PageSectionShell>
  )
}

function ChannelChatView({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation()
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
        title: t('channel.errors.openFileTitle'),
        message: err instanceof Error ? err.message : t('channel.errors.openFileMessage'),
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-background">
      <ChatTopBar title={t('channel.dingtalk.topbarTitle')} workspace={title || sessionId} />
      <div className="relative flex min-h-0 flex-1 overflow-hidden">
        <div data-testid="channel-chat-layout-column" className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
          <ChatArea />
          {isInactiveSession && (
            <div className="px-6 pb-2">
              <div className="rounded-md bg-muted px-3 py-2 text-sm text-muted-foreground">
                {t('channel.dingtalk.inactive')}
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
  const { t } = useTranslation()
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
      title: t('channel.remove.title'),
      description: t('channel.remove.description'),
      confirmLabel: t('channel.remove.confirm'),
      cancelLabel: t('channel.remove.cancel'),
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
        name: t('channel.dingtalk.name'),
        description: t('channel.dingtalk.description'),
        icon: t('channel.dingtalk.icon'),
        // 钉钉品牌蓝 #0b8cff：是平台 logo 识别色，不随主题切换
        iconClassName: 'bg-sky-50 text-[var(--color-semantic-blue)]',
        state: states.dingtalk,
        ...statusMeta(states.dingtalk),
      },
    ]
  }, [dingtalkState, t])

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
        />
      )}

      <Dialog open={registrationOpen} onOpenChange={setRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t('channel.dialog.title')}</DialogTitle>
            <DialogDescription>{t('channel.dialog.description')}</DialogDescription>
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
