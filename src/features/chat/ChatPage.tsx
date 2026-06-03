import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { ExpertTeamWelcome } from '@/components/chat-scene/ExpertTeamWelcome'
import { RightPanel } from '@/components/chat/RightPanel'
import type { PreviewTarget } from '@/components/chat/generatedFileActions'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatTopBar } from '@/components/shell/ChatTopBar'
import { TeamChatDrawer } from '@/components/team/TeamChatDrawer'
import { TeamVisualProvider } from '@/components/team/TeamVisualContext'
import {
  ConversationExportDialog,
  type ConversationExportStatus,
} from '@/features/chat/ConversationExportDialog'
import { useExpertTeamForConversation } from '@/features/expert-teams/expertTeamRegistry'
import { getExpertTeam } from '@/features/expert-teams/teams'
import { useChat } from '@/hooks/useChat'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import {
  exportConversation,
  getConversationSource,
  openGeneratedFile,
  revealExportInFolder,
  type ExportConversationResult,
} from '@/lib/tauri'
import { useEffect, useRef, useState } from 'react'
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
  const [exportDialogOpen, setExportDialogOpen] = useState(false)
  const [exportStatus, setExportStatus] = useState<ConversationExportStatus>('idle')
  const [exportProgressStep, setExportProgressStep] = useState(0)
  const [exportResult, setExportResult] = useState<ExportConversationResult | null>(null)
  const [exportError, setExportError] = useState<string | null>(null)
  const currentConversationIdRef = useRef(conversationId)
  const exportRequestSeqRef = useRef(0)

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

  const handleOpenExportDialog = () => {
    if (exportStatus === 'exporting') return
    setExportDialogOpen(true)
    setExportStatus('idle')
    setExportProgressStep(0)
    setExportResult(null)
    setExportError(null)
  }

  const handleExportConversation = async () => {
    if (exportStatus === 'exporting') return
    const requestSeq = exportRequestSeqRef.current + 1
    exportRequestSeqRef.current = requestSeq
    const requestConversationId = conversationId
    setExportDialogOpen(true)
    setExportStatus('exporting')
    setExportProgressStep(0)
    setExportResult(null)
    setExportError(null)

    try {
      const result = await exportConversation(requestConversationId)
      if (
        exportRequestSeqRef.current !== requestSeq ||
        currentConversationIdRef.current !== requestConversationId
      ) {
        return
      }
      setExportProgressStep(2)
      setExportResult(result)
      setExportStatus('success')
    } catch (err) {
      if (
        exportRequestSeqRef.current !== requestSeq ||
        currentConversationIdRef.current !== requestConversationId
      ) {
        return
      }
      const message = err instanceof Error ? err.message : '导出失败。'
      setExportError(message)
      setExportStatus('error')
      pushNotification({
        level: 'error',
        title: '导出失败',
        message,
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  const handleRevealExport = async () => {
    if (!exportResult) return
    try {
      await revealExportInFolder(exportResult.zipPath)
    } catch (err) {
      pushNotification({
        level: 'error',
        title: '无法打开文件夹',
        message: err instanceof Error ? err.message : '打开导出文件夹失败。',
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  useEffect(() => {
    if (exportStatus !== 'exporting') return undefined
    const timers = [
      window.setTimeout(() => setExportProgressStep(1), 300),
      window.setTimeout(() => setExportProgressStep(2), 900),
    ]
    return () => timers.forEach(window.clearTimeout)
  }, [exportStatus])

  useEffect(() => {
    currentConversationIdRef.current = conversationId
    exportRequestSeqRef.current += 1
    setExportDialogOpen(false)
    setExportStatus('idle')
    setExportProgressStep(0)
    setExportResult(null)
    setExportError(null)
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
          onShare={handleOpenExportDialog}
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
          />
        ) : null}
      </div>
      <ConversationExportDialog
        open={exportDialogOpen}
        status={exportStatus}
        progressStep={exportProgressStep}
        result={exportResult}
        error={exportError}
        onOpenChange={setExportDialogOpen}
        onStart={() => void handleExportConversation()}
        onReveal={() => void handleRevealExport()}
      />
    </div>
  )
}
