import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { AuthGate } from '@/components/auth/AuthGate'
import { ConfirmDialogHost } from '@/components/common/ConfirmDialogHost'
import { ToastContainer } from '@/components/common/ToastContainer'
import { UpdaterPanel } from '@/components/common/UpdaterPanel'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { SidebarCollapseFrame } from '@/components/layout/SidebarCollapseFrame'
import { TitleBar } from '@/components/layout/TitleBar'
import { NetworkStatusIndicator } from '@/components/shell/NetworkStatusIndicator'
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
import { useNetworkStatus } from '@/hooks/useNetworkStatus'
import { useStreaming } from '@/hooks/useStreaming'
import { useUpdater } from '@/hooks/useUpdater'
import { useDragDropListener } from '@/hooks/useDragDropListener'
import { usePendingEventListener } from '@/hooks/usePendingEventListener'
import { useAppNavigationMenu } from '@/hooks/useAppNavigationMenu'
import {
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
  useUpdater()
  const sidebarHidden = useUiStore((state) => state.sidebarHidden)

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground">
      <TitleBar />
      <NetworkStatusIndicator />
      <div className="flex min-h-0 flex-1 bg-sidebar">
        <SidebarCollapseFrame hidden={sidebarHidden}>
          <AppSidebar />
        </SidebarCollapseFrame>
        <main
          className={`min-w-0 flex-1 overflow-hidden border-t border-border bg-background ${
            sidebarHidden ? '' : 'rounded-l-lg border-l'
          }`}
        >
          <RouteSwitch />
        </main>
      </div>
      <SettingsModal />
      <ConfirmDialogHost />
      <ToastContainer />
      <UpdaterPanel />
    </div>
  )
}

function App() {
  useStreaming()
  useNetworkStatus()
  useDragDropListener()
  usePendingEventListener()
  useAppNavigationMenu()
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
        const convs = raw
          .map((c) => ({
            id: (c.id as string) ?? '',
            title: (c.title as string) ?? '新对话',
            createdAt: (c.createdAt as string) ?? new Date().toISOString(),
            updatedAt: (c.updatedAt as string) ?? new Date().toISOString(),
            isArchived: (c.isArchived as boolean) ?? false,
            kind: (c.kind as import('@/types/message').Conversation['kind']) ?? undefined,
            workspaceName: (c.workspaceName as string | undefined) ?? undefined,
          }))
          // Sidebar / project list only shows app-side conversations;
          // IM-origin chats are surfaced through the channel page.
          .filter((c) => c.kind !== 'im')
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
