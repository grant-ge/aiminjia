import { useEffect, useState } from 'react'
import { Sidebar } from '@/components/layout/Sidebar'
import { TopBar } from '@/components/layout/TopBar'
import { ChatArea } from '@/components/layout/ChatArea'
import { InputBar } from '@/components/layout/InputBar'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { ToastContainer } from '@/components/common/ToastContainer'
import { useStreaming } from '@/hooks/useStreaming'
import { useChat } from '@/hooks/useChat'
import { onConversationTitleUpdated } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'

function App() {
  useStreaming()

  const { loadConversations } = useChat()

  useEffect(() => {
    loadConversations()
  }, [loadConversations])

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
