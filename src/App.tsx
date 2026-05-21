import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { AuthGate } from '@/components/auth/AuthGate'
import { ConfirmDialogHost } from '@/components/common/ConfirmDialogHost'
import { ToastContainer } from '@/components/common/ToastContainer'
import { UpdaterPanel } from '@/components/common/UpdaterPanel'
import { PermissionAskDialog } from '@/components/common/PermissionAskDialog'
import type { PermissionAskDecision } from '@/components/common/PermissionAskDialog'
import { AskUserQuestionDialog } from '@/components/interactions/AskUserQuestionDialog'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { TitleBar } from '@/components/layout/TitleBar'
import { AppSidebar } from '@/components/sidebar/AppSidebar'
import { ChatPage } from '@/features/chat/ChatPage'
import { ChannelPage } from '@/features/channel/ChannelPage'
import { EmployeesPage } from '@/features/home/EmployeesPage'
import { HomePage } from '@/features/home/HomePage'
import { InboxPage } from '@/features/inbox/InboxPage'
import { ExpertTeamsPage } from '@/features/expert-teams/ExpertTeamsPage'
import { SchedulesPage } from '@/features/schedules/SchedulesPage'
import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { SkillDetailPage } from '@/features/skill-detail/SkillDetailPage'
import { useStreaming } from '@/hooks/useStreaming'
import { useUpdater } from '@/hooks/useUpdater'
import { useDragDropListener } from '@/hooks/useDragDropListener'
import { usePendingEventListener } from '@/hooks/usePendingEventListener'
import {
  approvePermissionRequest,
  cancelPermissionRequest,
  denyPermissionRequest,
  getConversations,
  getPluginInfo,
  getSettings,
  onAuthExpired,
  onConversationCreated,
  onConversationTitleUpdated,
} from '@/lib/tauri'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { usePluginStore } from '@/stores/pluginStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { useSkillStore } from '@/stores/skillStore'
import { useStreamingStore } from '@/stores/streamingStore'
import { useInteractionStore } from '@/stores/interactionStore'
import { useUiStore } from '@/stores/uiStore'
import { hydrateHomeStore } from '@/stores/homeStore'
import { initChannelListeners } from '@/stores/channelStore'
import { applyFontScale, loadPersistedFontScale } from '@/styles/fontScale'

applyFontScale(loadPersistedFontScale())

function RouteSwitch() {
  const route = useUiStore((state) => state.route)

  switch (route.kind) {
    case 'home':
      return <HomePage />
    case 'employees':
      return <EmployeesPage />
    case 'skill-center':
      return <SkillCenterPage />
    case 'skill-detail':
      return <SkillDetailPage skillId={route.skillId} />
    case 'schedules':
      return <SchedulesPage />
    case 'inbox':
      return <InboxPage />
    case 'expert-teams':
      return <ExpertTeamsPage />
    case 'chat':
      return <ChatPage conversationId={route.conversationId} />
    case 'channel':
      return <ChannelPage sessionId={route.sessionId} />
  }
}

function AppShell() {
  const pendingAsks = useStreamingStore((s) => s.pendingAsks)
  const removePendingAsk = useStreamingStore((s) => s.removePendingAsk)
  const pendingInteractions = useInteractionStore((s) => s.pendingInteractions)
  const removeInteraction = useInteractionStore((s) => s.removeInteraction)
  const activeAsk = pendingAsks.size > 0 ? (pendingAsks.values().next().value ?? null) : null
  const activeInteraction = pendingInteractions[0] ?? null
  useUpdater()

  const handleAllowAsk = async ({ remember, destination }: PermissionAskDecision) => {
    if (!activeAsk) return
    const toolCallId = activeAsk.toolCallId
    removePendingAsk(toolCallId)
    try {
      await approvePermissionRequest(toolCallId, null, remember, destination)
    } catch (err) {
      console.error('[permission:ask] approve failed', err)
    }
  }

  const handleDenyAsk = async ({ remember, destination }: PermissionAskDecision) => {
    if (!activeAsk) return
    const toolCallId = activeAsk.toolCallId
    removePendingAsk(toolCallId)
    try {
      await denyPermissionRequest(toolCallId, undefined, remember, destination)
    } catch (err) {
      console.error('[permission:ask] deny failed', err)
    }
  }

  const handleCancelAsk = async () => {
    if (!activeAsk) return
    const toolCallId = activeAsk.toolCallId
    removePendingAsk(toolCallId)
    try {
      await cancelPermissionRequest(toolCallId)
    } catch (err) {
      console.error('[permission:ask] cancel failed', err)
    }
  }

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <AppSidebar />
        <main className="min-w-0 flex-1 overflow-hidden border-l border-border">
          <RouteSwitch />
        </main>
      </div>
      <SettingsModal />
      <ConfirmDialogHost />
      <ToastContainer />
      <PermissionAskDialog
        open={activeAsk !== null}
        ask={activeAsk}
        onAllow={handleAllowAsk}
        onDeny={handleDenyAsk}
        onCancel={handleCancelAsk}
      />
      {activeInteraction ? (
        <AskUserQuestionDialog
          interactionId={activeInteraction.interactionId}
          questions={activeInteraction.payload.questions}
          onClose={() => removeInteraction(activeInteraction.interactionId)}
        />
      ) : null}
      <UpdaterPanel />
    </div>
  )
}

function App() {
  useStreaming()
  useDragDropListener()
  usePendingEventListener()
  const { t } = useTranslation()

  useEffect(() => {
    getPluginInfo()
      .then(({ tools, skills }) => {
        usePluginStore.getState().setAll(tools, skills)
        useSkillStore.setState({ skills, isLoading: false })
      })
      .catch((err) => console.error('Failed to load plugin info:', err))
  }, [])

  useEffect(() => {
    getSettings()
      .then((settings) => {
        useSettingsStore.getState().setSettings(settings)
      })
      .catch((err) => console.error('Failed to load settings:', err))
  }, [])

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const settings = await getSettings()
        if (!cancelled) hydrateHomeStore(settings)
      } catch (err) {
        console.warn('[App] hydrate homeStore failed:', err)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    const unlisten = onAuthExpired(({ message }) => {
      console.warn('[auth:expired]', message)
      useAuthStore.getState().clearAndRedirect(useUiStore.getState().route)
      useBrandingStore.getState().reset()
      useNotificationStore.getState().push({
        level: 'warning',
        title: t('auth.expired'),
        message: t('auth.expiredDesc'),
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    })
    return () => {
      unlisten.then((fn) => fn())
    }
  }, [t])

  useEffect(() => {
    const unlisten = onConversationTitleUpdated(({ conversationId, title }) => {
      const store = useChatStore.getState()
      store.setConversations(
        store.conversations.map((conversation) =>
          conversation.id === conversationId ? { ...conversation, title } : conversation,
        ),
      )
    })
    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  // 监听后端创建新 conversation：每次 agenda / employee / schedule_runner 或用户自己
  // 建新对话时，sidebar 的 chatStore 需要 reload 才能看到。此处只刷列表，
  // 不改 activeConversationId / 路由 —— 用户可能正在别的对话里操作。
  useEffect(() => {
    const unlisten = onConversationCreated(async () => {
      try {
        const raw = await getConversations()
        const convs = raw.map((c) => ({
          id: (c.id as string) ?? '',
          title: (c.title as string) ?? '新对话',
          createdAt: (c.createdAt as string) ?? new Date().toISOString(),
          updatedAt: (c.updatedAt as string) ?? new Date().toISOString(),
          isArchived: (c.isArchived as boolean) ?? false,
          kind: (c.kind as import('@/types/message').Conversation['kind']) ?? undefined,
          workspaceName: (c.workspaceName as string | undefined) ?? undefined,
        }))
        useChatStore.getState().setConversations(convs)
      } catch (err) {
        console.error('[App] reload conversations after conversation:created failed:', err)
      }
    })
    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  useEffect(() => {
    void initChannelListeners()
  }, [])

  return (
    <AuthGate>
      <AppShell />
    </AuthGate>
  )
}

export default App
