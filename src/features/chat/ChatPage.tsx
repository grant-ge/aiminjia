import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { ExpertTeamWelcome } from '@/components/chat-scene/ExpertTeamWelcome'
import { RightPanel } from '@/components/chat/RightPanel'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { TeamChatDrawer } from '@/components/team/TeamChatDrawer'
import { TeamVisualProvider } from '@/components/team/TeamVisualContext'
import { useExpertTeamForConversation } from '@/features/expert-teams/expertTeamRegistry'
import { getExpertTeam } from '@/features/expert-teams/teams'
import { useChat } from '@/hooks/useChat'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { getConversationSource, openGeneratedFile } from '@/lib/tauri'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useEmployeeById } from '@/features/employees/useEmployeeById'

interface ChatPageProps {
  conversationId: string
}

export function ChatPage({ conversationId }: ChatPageProps) {
  const { i18n } = useTranslation()
  const { switchConversation } = useChat()
  const conversations = useChatStore((s) => s.conversations)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messageCount = useChatStore((s) => s.messages.length)
  const pushNotification = useNotificationStore((s) => s.push)
  const previewTarget = useGeneratedFilePreviewStore((s) => s.target)
  const previewOpen = previewTarget?.conversationId === conversationId
  const conv = conversations.find((c) => c.id === conversationId)
  const title = conv?.title ?? ''

  // employee_id lives in conv.json (not the index); read it lazily when this
  // conversation is an employee dispatch session.
  const [employeeId, setEmployeeId] = useState<string | null>(null)
  useEffect(() => {
    if (conv?.kind !== 'employee') { setEmployeeId(null); return }
    let cancelled = false
    getConversationSource(conversationId).then((src) => {
      if (!cancelled && src.kind === 'employee') setEmployeeId(src.employeeId)
    }).catch(() => { /* non-fatal */ })
    return () => { cancelled = true }
  }, [conversationId, conv?.kind])

  const employee = useEmployeeById(employeeId)
  const { overview: teamOverview } = useTeamOverview(activeConversationId)
  const expertTeamId = useExpertTeamForConversation(conversationId)
  const expertTeam = expertTeamId ? getExpertTeam(expertTeamId, i18n.language) : undefined
  const sourceLabel = conv?.kind === 'expertTeam'
    ? expertTeam?.name ?? conv?.sourceLabel
    : conv?.sourceLabel

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
    // Always load messages when conversationId changes — this covers:
    //   1. Full reload (persisted route, messages not yet loaded)
    //   2. Navigation from non-chat pages (expert-teams / employees) where
    //      old messages remain in chatStore from a previous conversation
    // switchConversation internally clears messages then fetches the correct
    // ones; switchVersionRef deduplicates rapid successive calls.
    void switchConversation(conversationId)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId])

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
      {title ? (
        <ChatTopBar
          title={title}
          workspace={conv?.workspaceName}
          kind={conv?.kind}
          sourceLabel={sourceLabel}
          updatedAt={conv?.updatedAt}
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
          {expertTeam && messageCount === 0 ? (
            <div className="flex-1 overflow-y-auto overscroll-contain">
              <ExpertTeamWelcome team={expertTeam} />
            </div>
          ) : (
            <ChatArea expertTeamId={expertTeamId} />
          )}
          <ChatBottomArea placeholderOverride={expertTeam?.composerPlaceholder} />
        </div>
        {activeConversationId ? (
          <TeamVisualProvider value={expertTeam ?? null}>
            <TeamChatDrawer conversationId={activeConversationId} overview={teamOverview} />
          </TeamVisualProvider>
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
