import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { ExpertTeamBanner } from '@/components/chat-scene/ExpertTeamBanner'
import { RightPanel } from '@/components/chat/RightPanel'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { TeamChatDrawer } from '@/components/team/TeamChatDrawer'
import { useExpertTeamForConversation } from '@/features/expert-teams/expertTeamRegistry'
import { getExpertTeam } from '@/features/expert-teams/teams'
import { useChat } from '@/hooks/useChat'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { openGeneratedFile } from '@/lib/tauri'
import { useEffect } from 'react'
import { useEmployeeById } from '@/features/employees/useEmployeeById'

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
  const conv = conversations.find((c) => c.id === conversationId)
  const title = conv?.title ?? ''
  const employee = useEmployeeById(conv?.employeeId ?? null)
  const { overview: teamOverview } = useTeamOverview(activeConversationId)
  const expertTeamId = useExpertTeamForConversation(conversationId)
  const expertTeam = expertTeamId ? getExpertTeam(expertTeamId) : undefined

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
      {title ? (
        <ChatTopBar
          title={title}
          employee={
            employee
              ? {
                  avatar: employee.avatar,
                  name: employee.name,
                  role: employee.role,
                }
              : undefined
          }
        />
      ) : null}
      <div className="relative flex flex-1 overflow-hidden">
        <div data-testid="chat-layout-column" className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
          {expertTeamId ? (
            <ExpertTeamBanner conversationId={conversationId} teamId={expertTeamId} />
          ) : null}
          <ChatArea />
          <ChatBottomArea placeholderOverride={expertTeam?.composerPlaceholder} />
        </div>
        {activeConversationId ? (
          <TeamChatDrawer conversationId={activeConversationId} overview={teamOverview} />
        ) : null}
        {previewOpen ? (
          <RightPanel
            conversationId={conversationId}
            onOpenExternal={(target) => void handleOpenPreviewTarget(target)}
          />
        ) : null}
      </div>
    </div>
  )
}
