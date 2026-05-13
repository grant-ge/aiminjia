import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { RightPanel } from '@/components/chat/RightPanel'
import { TeamDrawer } from '@/components/chat/TeamDrawer'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { useChat } from '@/hooks/useChat'
import { useTeamView } from '@/hooks/useTeamView'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { openGeneratedFile } from '@/lib/tauri'
import { useEffect } from 'react'

interface ChatPageProps {
  conversationId: string
}

export function ChatPage({ conversationId }: ChatPageProps) {
  const { switchConversation } = useChat()
  const conversations = useChatStore((s) => s.conversations)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messageCount = useChatStore((s) => s.messages.length)
  const pushNotification = useNotificationStore((s) => s.push)
  const previewTarget = useGeneratedFilePreviewStore((s) => s.target)
  const previewOpen = previewTarget?.conversationId === conversationId
  const title = conversations.find((c) => c.id === conversationId)?.title ?? ''
  // 当前对话的群聊视图——若没调过 TeamCreate 则 view.roster.team_name == null。
  // 抽屉、TeamCard、对话标题角标都从这里派生。
  const { view: teamView } = useTeamView(conversationId)

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
    // On a full reload, activeConversationId is derived synchronously from the
    // persisted route before messages are loaded, so matching ids alone do not
    // prove the message cache is hydrated.
    if (activeConversationId !== conversationId || messageCount === 0) {
      void switchConversation(conversationId)
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId, activeConversationId, messageCount])

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
      {title ? <ChatTopBar title={title} /> : null}
      <div className="relative flex flex-1 overflow-hidden">
        <div data-testid="chat-layout-column" className="relative flex flex-1 flex-col overflow-hidden">
          <ChatArea />
          <ChatBottomArea />
        </div>
        {previewOpen ? (
          <RightPanel
            conversationId={conversationId}
            onOpenExternal={(target) => void handleOpenPreviewTarget(target)}
          />
        ) : null}
        {teamView ? <TeamDrawer view={teamView} /> : null}
      </div>
    </div>
  )
}
