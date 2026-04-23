import { BrowserPanel } from '@/components/browser/BrowserPanel'
import { ChatArea } from '@/components/layout/ChatArea'
import { InputBar } from '@/components/layout/InputBar'
import { TitleBar } from '@/components/layout/TitleBar'
import { ExportMenu } from '@/components/rich-content/ExportMenu'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { useChat } from '@/hooks/useChat'
import { useChatStore } from '@/stores/chatStore'
import { useEffect } from 'react'

interface ChatPageProps {
  conversationId: string
}

export function ChatPage({ conversationId }: ChatPageProps) {
  const { switchConversation } = useChat()
  const conversations = useChatStore((s) => s.conversations)
  const streamStates = useChatStore((s) => s.streamStates)
  const isStreaming = streamStates[conversationId]?.isStreaming ?? false

  const title =
    conversations.find((c) => c.id === conversationId)?.title ?? ''

  useEffect(() => {
    void switchConversation(conversationId)
  }, [conversationId, switchConversation])

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
      <TitleBar />
      {title ? (
        <ChatTopBar
          title={title}
          trailing={
            !isStreaming ? (
              <ExportMenu conversationId={conversationId} />
            ) : undefined
          }
        />
      ) : null}
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
