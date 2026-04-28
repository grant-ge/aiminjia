import { BrowserPanel } from '@/components/browser/BrowserPanel'
import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { RightPanel } from '@/components/chat/RightPanel'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { useChat } from '@/hooks/useChat'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { openGeneratedFile } from '@/lib/tauri'
import { useEffect } from 'react'

interface ChatPageProps {
  conversationId: string
}

export function ChatPage({ conversationId }: ChatPageProps) {
  const { switchConversation } = useChat()
  const conversations = useChatStore((s) => s.conversations)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const pushNotification = useNotificationStore((s) => s.push)
  const title = conversations.find((c) => c.id === conversationId)?.title ?? ''

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

  useEffect(() => {
    // Every code path that calls setRoute({ kind: 'chat', conversationId }) also
    // calls setActiveConversation(conversationId) first. So on mount,
    // activeConversationId === conversationId is always true for new conversations
    // and the sidebar's own switchConversation call already handles getMessages.
    // Only call switchConversation here when the route conversationId differs from
    // what the store considers active — a defensive guard for unexpected navigations.
    if (activeConversationId !== conversationId) {
      void switchConversation(conversationId)
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId])

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
      {title ? <ChatTopBar title={title} /> : null}
      <div className="relative flex flex-1 overflow-hidden">
        <div data-testid="chat-layout-column" className="relative flex flex-1 flex-col overflow-hidden">
          <ChatArea />
          <ChatBottomArea />
        </div>
        <RightPanel
          conversationId={conversationId}
          onOpenExternal={(target) => void handleOpenPreviewTarget(target)}
        />
        <BrowserPanel />
      </div>
    </div>
  )
}
