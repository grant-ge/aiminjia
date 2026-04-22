import { BrowserPanel } from '@/components/browser/BrowserPanel'
import { ChatArea } from '@/components/layout/ChatArea'
import { InputBar } from '@/components/layout/InputBar'
import { TopBar } from '@/components/layout/TopBar'
import { TitleBar } from '@/components/layout/TitleBar'
import { useChat } from '@/hooks/useChat'
import { useEffect } from 'react'

interface ChatPageProps {
  conversationId: string
}

export function ChatPage({ conversationId }: ChatPageProps) {
  const { switchConversation } = useChat()

  useEffect(() => {
    void switchConversation(conversationId)
  }, [conversationId, switchConversation])

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
      <TitleBar />
      <TopBar />
      <div className="relative flex flex-1 overflow-hidden">
        <div className="flex flex-1 flex-col overflow-hidden">
          <ChatArea />
          <InputBar />
        </div>
        <BrowserPanel />
      </div>
    </div>
  )
}
