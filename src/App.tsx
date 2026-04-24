import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { AuthGate } from '@/components/auth/AuthGate'
import { ToastContainer } from '@/components/common/ToastContainer'
import { PermissionAskDialog } from '@/components/common/PermissionAskDialog'
import type { PermissionAskDecision } from '@/components/common/PermissionAskDialog'
import { AskUserQuestionDialog } from '@/components/interactions/AskUserQuestionDialog'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { AppSidebar } from '@/components/sidebar/AppSidebar'
import { ChatPage } from '@/features/chat/ChatPage'
import { HomePage } from '@/features/home/HomePage'
import { SchedulesPage } from '@/features/schedules/SchedulesPage'
import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { SkillDetailPage } from '@/features/skill-detail/SkillDetailPage'
import { useStreaming } from '@/hooks/useStreaming'
import { useUpdater } from '@/hooks/useUpdater'
import {
  approvePermissionRequest,
  cancelPermissionRequest,
  denyPermissionRequest,
  getPluginInfo,
  onAuthExpired,
  onBrowserClosed,
  onBrowserNavigating,
  onBrowserPageReady,
  onConversationTitleUpdated,
} from '@/lib/tauri'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useBrowserStore } from '@/stores/browserStore'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { usePluginStore } from '@/stores/pluginStore'
import { useStreamingStore } from '@/stores/streamingStore'
import { useInteractionStore } from '@/stores/interactionStore'
import { useUiStore } from '@/stores/uiStore'

function RouteSwitch() {
  const route = useUiStore((state) => state.route)

  switch (route.kind) {
    case 'home':
      return <HomePage />
    case 'skill-center':
      return <SkillCenterPage />
    case 'skill-detail':
      return <SkillDetailPage skillId={route.skillId} />
    case 'schedules':
      return <SchedulesPage />
    case 'chat':
      return <ChatPage conversationId={route.conversationId} />
  }
}

function AppShell() {
  const pendingAsks = useStreamingStore((s) => s.pendingAsks)
  const removePendingAsk = useStreamingStore((s) => s.removePendingAsk)
  const pendingInteractions = useInteractionStore((s) => s.pendingInteractions)
  const removeInteraction = useInteractionStore((s) => s.removeInteraction)
  const activeAsk = pendingAsks.size > 0 ? (pendingAsks.values().next().value ?? null) : null
  const activeInteraction = pendingInteractions[0] ?? null

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
    <div className="flex h-screen w-screen bg-sidebar text-foreground">
      <AppSidebar />
      <main className="min-w-0 flex-1 overflow-hidden rounded-bl-xl rounded-tl-xl">
        <RouteSwitch />
      </main>
      <SettingsModal />
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
    </div>
  )
}

function App() {
  useStreaming()
  useUpdater()
  const { t } = useTranslation()

  useEffect(() => {
    getPluginInfo()
      .then(({ tools, skills }) => {
        usePluginStore.getState().setAll(tools, skills)
      })
      .catch((err) => console.error('Failed to load plugin info:', err))
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

  useEffect(() => {
    const unlistenNavigating = onBrowserNavigating(({ appId, url }) => {
      useBrowserStore.getState().setNavigating(appId ?? 0, url)
    })
    const unlistenReady = onBrowserPageReady(({ appId, url, title }) => {
      useBrowserStore.getState().setPageReady(appId ?? 0, url, title)
    })
    const unlistenClosed = onBrowserClosed(({ appId }) => {
      useBrowserStore.getState().setClosed(appId ?? 0)
    })
    return () => {
      unlistenNavigating.then((fn) => fn())
      unlistenReady.then((fn) => fn())
      unlistenClosed.then((fn) => fn())
    }
  }, [])

  return (
    <AuthGate>
      <AppShell />
    </AuthGate>
  )
}

export default App
