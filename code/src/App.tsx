import { useEffect, useState } from 'react'
import { Sidebar } from '@/components/layout/Sidebar'
import { TopBar } from '@/components/layout/TopBar'
import { ChatArea } from '@/components/layout/ChatArea'
import { InputBar } from '@/components/layout/InputBar'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { ToastContainer } from '@/components/common/ToastContainer'
import { useStreaming } from '@/hooks/useStreaming'
import { useChat } from '@/hooks/useChat'
import { onConversationTitleUpdated, onAuthExpired, getCloudAuth, getCloudModels } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useAuthStore } from '@/stores/authStore'
import { useNotificationStore } from '@/stores/notificationStore'

function App() {
  useStreaming()

  const { loadConversations } = useChat()

  useEffect(() => {
    loadConversations()
  }, [loadConversations])

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
          } catch (err) {
            console.error('Failed to fetch cloud models on restore:', err)
          }
        }
      })
      .catch((err) => console.error('Failed to restore cloud auth:', err))
  }, [])

  // Listen for auth:expired events from backend
  useEffect(() => {
    const unlisten = onAuthExpired(({ message }) => {
      console.warn('[auth:expired]', message)
      useAuthStore.getState().clearAuth()
      useNotificationStore.getState().push({
        level: 'warning',
        title: '登录已过期',
        message: '请重新登录以使用云端服务',
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
