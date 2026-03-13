import { useEffect, useState } from 'react'
import { Sidebar } from '@/components/layout/Sidebar'
import { TopBar } from '@/components/layout/TopBar'
import { ChatArea } from '@/components/layout/ChatArea'
import { InputBar } from '@/components/layout/InputBar'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { ToastContainer } from '@/components/common/ToastContainer'
import { useStreaming } from '@/hooks/useStreaming'
import { useUpdater } from '@/hooks/useUpdater'
import { useChat } from '@/hooks/useChat'
import { onConversationTitleUpdated, onAuthExpired, getCloudAuth, getCloudModels, getSettings, updateSettings, getPluginInfo } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useAuthStore } from '@/stores/authStore'
import { usePluginStore } from '@/stores/pluginStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { useNotificationStore } from '@/stores/notificationStore'

function App() {
  useStreaming()
  useUpdater()

  const { loadConversations } = useChat()

  useEffect(() => {
    loadConversations()
  }, [loadConversations])

  // Load plugin info (tools + skills) on startup
  useEffect(() => {
    getPluginInfo()
      .then(({ tools, skills }) => {
        usePluginStore.getState().setAll(tools, skills)
      })
      .catch((err) => console.error('Failed to load plugin info:', err))
  }, [])

  // Restore cloud auth state on startup
  useEffect(() => {
    getCloudAuth()
      .then(async (info) => {
        if (info.loggedIn) {
          useAuthStore.getState().setAuth(info)
          // Fetch cloud models (get_auth_info returns empty models)
          try {
            const models = await getCloudModels()
            useAuthStore.getState().setCloudModels(models)
            // Restore selectedCloudModel and useCloud from persisted settings
            const saved = await getSettings()
            useSettingsStore.getState().setSettings({ useCloud: saved.useCloud ?? false })
            if (saved.cloudModel && models.find((m) => m.id === saved.cloudModel)) {
              useAuthStore.getState().setSelectedCloudModel(saved.cloudModel)
            } else if (models.length > 0) {
              useAuthStore.getState().setSelectedCloudModel(models[0].id)
            }
          } catch (err) {
            console.error('Failed to fetch cloud models on restore:', err)
          }
        } else {
          // Not logged in — ensure useCloud is false
          const saved = await getSettings()
          if (saved.useCloud) {
            await updateSettings({ ...saved, useCloud: false }).catch(() => {})
          }
          useSettingsStore.getState().setSettings({ useCloud: false })
        }
      })
      .catch((err) => console.error('Failed to restore cloud auth:', err))
  }, [])

  // Listen for auth:expired events from backend
  useEffect(() => {
    const unlisten = onAuthExpired(({ message }) => {
      console.warn('[auth:expired]', message)
      useAuthStore.getState().clearAuth()
      // Keep useCloud unchanged — user must explicitly switch
      useNotificationStore.getState().push({
        level: 'warning',
        title: '登录已过期',
        message: '云端服务暂不可用。你可以重新登录或在设置中切换到本地模式。',
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    })
    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  // Listen for conversation title updates from backend
  useEffect(() => {
    const unlisten = onConversationTitleUpdated(({ conversationId, title }) => {
      const store = useChatStore.getState()
      store.setConversations(
        store.conversations.map((c) =>
          c.id === conversationId ? { ...c, title } : c,
        ),
      )
    })
    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  const [settingsOpen, setSettingsOpen] = useState(false)

  return (
    <>
      <Sidebar onOpenSettings={() => setSettingsOpen(true)} />
      <main className="flex flex-1 flex-col overflow-hidden">
        <TopBar />
        <div className="relative flex flex-1 flex-col overflow-hidden">
          <ChatArea />
          <InputBar />
        </div>
      </main>
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <ToastContainer />
    </>
  )
}

export default App
