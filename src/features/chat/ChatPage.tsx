import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { ExpertTeamWelcome } from '@/components/chat-scene/ExpertTeamWelcome'
import { RightPanel } from '@/components/chat/RightPanel'
import { savePreviewTargetToDisk } from '@/components/chat/fileDownload'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { TeamChatDrawer } from '@/components/team/TeamChatDrawer'
import { TeamVisualProvider } from '@/components/team/TeamVisualContext'
import {
  ConversationExportDialog,
} from '@/features/chat/ConversationExportDialog'
import { useExpertTeamForConversation } from '@/features/expert-teams/expertTeamRegistry'
import { ExpertTeamAvatarStack } from '@/features/expert-teams/ExpertTeamAvatarStack'
import { getExpertTeam } from '@/features/expert-teams/teams'
import { useChat } from '@/hooks/useChat'
import { useConversationExport } from '@/hooks/useConversationExport'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import {
  getConversationSource,
  type ConversationSourceDto,
  openGeneratedFile,
} from '@/lib/tauri'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useEmployeeById } from '@/features/employees/useEmployeeById'
import { getLocalEmployeeAvatarUrl } from '@/features/employees/employeeVisual'
import { localizeEmployeeDisplay } from '@/features/employees/templates'
import { localizedSkillName } from '@/lib/skillLocalization'
import { useSkillStore } from '@/stores/skillStore'

interface ChatPageProps {
  conversationId: string
}

export function ChatPage({ conversationId }: ChatPageProps) {
  const { i18n, t } = useTranslation()
  const { switchConversation } = useChat()
  const conversations = useChatStore((s) => s.conversations)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messageCount = useChatStore((s) => s.messages.length)
  const pushNotification = useNotificationStore((s) => s.push)
  const previewTarget = useGeneratedFilePreviewStore((s) => s.target)
  const previewOpen = previewTarget?.conversationId === conversationId
  const conv = conversations.find((c) => c.id === conversationId)
  const title = conv?.title ?? ''
  const conversationExport = useConversationExport(conversationId)

  const [employeeId, setEmployeeId] = useState<string | null>(null)
  const [conversationSource, setConversationSource] = useState<ConversationSourceDto | null>(null)
  useEffect(() => {
    let cancelled = false
    setConversationSource(null)
    setEmployeeId(null)
    getConversationSource(conversationId).then((src) => {
      if (cancelled) return
      setConversationSource(src)
      setEmployeeId(src.kind === 'employee' ? src.employeeId : null)
    }).catch(() => {
      if (cancelled) return
      setConversationSource(null)
      setEmployeeId(null)
    })
    return () => { cancelled = true }
  }, [conversationId])

  const employee = useEmployeeById(employeeId)
  const employeeDisplay = employee
    ? localizeEmployeeDisplay(
        employee.templateId,
        { name: employee.name, role: employee.role, description: employee.description },
        i18n.language,
      )
    : null
  const defaultSkill = useSkillStore((s) =>
    employee?.defaultSkillId ? s.getById(employee.defaultSkillId) : null,
  )
  const defaultSkillLabel = employee?.defaultSkillId
    ? localizedSkillName(defaultSkill, employee.defaultSkillId, i18n.language)
    : null
  const { overview: teamOverview } = useTeamOverview(activeConversationId)
  const expertTeamId = useExpertTeamForConversation(conversationId)
  const expertTeam = expertTeamId ? getExpertTeam(expertTeamId, i18n.language) : undefined
  const sourceLabel = conv?.kind === 'expertTeam'
    ? expertTeam?.name ?? conv?.sourceLabel
    : conv?.sourceLabel
  const isEmployeeConversation = conversationSource?.kind === 'employee' || conv?.kind === 'employee'
  const isExpertTeamConversation = Boolean(expertTeam) || conv?.kind === 'expertTeam'
  const headerKind = isEmployeeConversation ? 'employee' : isExpertTeamConversation ? 'expertTeam' : conv?.kind
  const headerTitle = title || employeeDisplay?.name || expertTeam?.name || ''
  const shouldRenderHeader = Boolean(headerTitle || employeeDisplay || expertTeam)

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

  const handleDownloadPreviewTarget = async (target: PreviewTarget) => {
    try {
      const savedPath = await savePreviewTargetToDisk(target)
      if (!savedPath) return
      pushNotification({
        level: 'success',
        title: t('messageList.fileDownloaded', '已下载文件'),
        message: savedPath,
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      pushNotification({
        level: 'error',
        title: t('messageList.cannotDownload', '无法下载文件'),
        message: err instanceof Error ? err.message : '下载生成文件失败。',
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
      {shouldRenderHeader ? (
        <ChatTopBar
          title={headerTitle}
          workspace={conv?.workspaceName}
          kind={headerKind}
          sourceLabel={sourceLabel}
          updatedAt={conv?.updatedAt}
          employee={
            employee && employeeDisplay
              ? {
                  avatar: employee.avatar,
                  avatarUrl: getLocalEmployeeAvatarUrl(employeeDisplay.name),
                  name: employeeDisplay.name,
                  role: employeeDisplay.role,
                  defaultSkillLabel,
                }
              : undefined
          }
          expertTeam={expertTeam
            ? {
                avatar: <ExpertTeamAvatarStack team={expertTeam} size="xs" />,
                name: expertTeam.name,
                tagline: expertTeam.tagline,
              }
            : undefined}
          onShare={conversationExport.openExportDialog}
          shareLabel="导出对话"
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
            onDownload={(target) => void handleDownloadPreviewTarget(target)}
          />
        ) : null}
      </div>
      <ConversationExportDialog
        {...conversationExport.dialogProps}
      />
    </div>
  )
}
