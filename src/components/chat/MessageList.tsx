/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { useEffect, useMemo, useRef } from 'react'

import { AiBubble } from '@/components/chat/AiBubble'
import { StreamingBubble } from '@/components/chat/StreamingBubble'
import { ChatRow } from '@/components/chat-scene/ChatRow'
import { GeneratedFileCard } from '@/components/chat-scene/GeneratedFileCard'
import { PeerMessageBanner } from '@/components/chat-scene/PeerMessageBanner'
import { parseDispatchHeader } from '@/components/chat-scene/parseDispatchHeader'
import { SuggestChipGroup } from '@/components/chat-scene/SuggestChipGroup'
import { ToolGroupCard } from '@/components/chat-scene/ToolGroupCard'
import { UserMessageBubble } from '@/components/chat-scene/UserMessageBubble'
import { TeamProgressBlock } from '@/components/team/TeamProgressBlock'
import { toPreviewTarget } from '@/components/chat/generatedFileActions'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useChatStore } from '@/stores/chatStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useChat } from '@/hooks/useChat'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { useTurnRenderModel, type RenderGeneratedFile } from '@/hooks/useTurnRenderModel'
import { openGeneratedFile, revealFileInFolder } from '@/lib/tauri'
import { useConversationTeamState, useTeamStore } from '@/stores/teamStore'

type FileActionKind = 'preview' | 'open' | 'reveal'

const FILE_ACTION_ERROR_TITLES: Record<FileActionKind, string> = {
  preview: '无法预览文件',
  open: '无法打开文件',
  reveal: '无法定位文件',
}

export function MessageList() {
  const turns = useTurnRenderModel()
  useChat()
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => {
    const activeId = s.activeConversationId
    return activeId ? (s.streamStates[activeId]?.streamingContent ?? '') : ''
  })
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  const clearIfConversationChanged = useGeneratedFilePreviewStore((s) => s.clearIfConversationChanged)
  const pushNotification = useNotificationStore((s) => s.push)

  // Sender identity for the chat row headers (avatar + name).
  // AI side follows the tenant brand (logoUrl + productName), so a custom
  // tenant logo / name automatically propagates into every chat. User side
  // falls back to the colored-initial ChatAvatar when no profile image is
  // configured (none of the current users have one).
  const assistantName = useBrandingStore((s) => s.productName)
  const assistantLogo = useBrandingStore((s) => s.logoUrl)
  const userName = useAuthStore((s) => s.user?.name ?? s.user?.username ?? '我')
  // User profile photos are not yet stored anywhere — colored-initial
  // avatar from `ChatAvatar` is the default. When a profile-image field
  // lands on the auth user, plug it in here.
  const userAvatarUrl: string | null = null

  // Team chat drawer wiring.
  const { overview } = useTeamOverview(activeConversationId)
  const teamState = useConversationTeamState(activeConversationId)
  const openDrawer = useTeamStore((s) => s.openDrawer)
  const autoOpenedForConvRef = useRef<string | null>(null)

  // Auto-open the drawer the first time a team appears in the active
  // conversation, but only if the user hasn't manually closed it yet.
  // Re-armed when the active conversation changes.
  useEffect(() => {
    if (!activeConversationId) return
    if (!overview || overview.teams.length === 0) return
    if (autoOpenedForConvRef.current === activeConversationId) return
    if (teamState.userClosedDrawer) {
      autoOpenedForConvRef.current = activeConversationId
      return
    }
    // Only auto-open while streaming — on conversation reload we leave it closed.
    if (!isStreaming) {
      autoOpenedForConvRef.current = activeConversationId
      return
    }
    openDrawer(activeConversationId)
    autoOpenedForConvRef.current = activeConversationId
  }, [activeConversationId, overview, teamState.userClosedDrawer, isStreaming, openDrawer])

  // Reset the auto-open guard when switching conversations.
  useEffect(() => {
    autoOpenedForConvRef.current = null
  }, [activeConversationId])

  // Walk turns in order; assign each TeamCreate marker to the next unused team session.
  // Team sessions on disk are ordered by createdAt; turns are ordered by message
  // chronology — so an ordinal pairing is correct (and is the same logic the
  // backend uses when grouping events into sessions).
  const teamSessionForTurnIdx = useMemo(() => {
    const result: Array<NonNullable<typeof overview>['teams'][number] | null> = []
    if (!overview || overview.teams.length === 0) {
      return turns.map(() => null)
    }
    let teamCursor = 0
    for (const t of turns) {
      if (t.teamMarker?.kind === 'create' && teamCursor < overview.teams.length) {
        result.push(overview.teams[teamCursor])
        teamCursor += 1
      } else {
        result.push(null)
      }
    }
    return result
  }, [turns, overview])

  useEffect(() => {
    if (activeConversationId) clearIfConversationChanged(activeConversationId)
  }, [activeConversationId, clearIfConversationChanged])

  const notifyFileError = (kind: FileActionKind, message: string) => {
    pushNotification({
      level: 'error',
      title: FILE_ACTION_ERROR_TITLES[kind],
      message,
      actions: [],
      dismissible: true,
      context: 'toast',
    })
  }

  const handlePreview = (file: RenderGeneratedFile) => {
    if (!file.conversationId) {
      notifyFileError('preview', '生成文件缺少所属对话，无法预览。')
      return
    }
    openPreview(toPreviewTarget(file, file.conversationId))
  }

  const handleOpenExternal = async (file: RenderGeneratedFile) => {
    if (!file.conversationId) {
      notifyFileError('open', '生成文件缺少所属对话，无法打开。')
      return
    }
    try {
      await openGeneratedFile(file.id, file.conversationId)
    } catch (err) {
      notifyFileError('open', err instanceof Error ? err.message : '打开生成文件失败。')
    }
  }

  const handleReveal = async (file: RenderGeneratedFile) => {
    if (!file.conversationId) {
      notifyFileError('reveal', '生成文件缺少所属对话，无法定位。')
      return
    }
    try {
      await revealFileInFolder(file.id, file.conversationId)
    } catch (err) {
      notifyFileError('reveal', err instanceof Error ? err.message : '定位生成文件失败。')
    }
  }

  const handleOpenTeamDrawer = () => {
    if (activeConversationId) openDrawer(activeConversationId)
  }

  return (
    <div className="flex flex-col gap-5 px-2 py-3">
      {turns.map((t, i) => {
        const teamSession = teamSessionForTurnIdx[i]
        // Dispatch-prompt user turns render as a centered system banner
        // (handled inside UserMessageBubble). For those, skip the chat-row
        // avatar wrapper — the banner already announces the dispatch.
        const isDispatchTurn = !!(t.userMessage && parseDispatchHeader(t.userMessage.text))
        return (
          <div key={i} className="flex flex-col gap-4">
            {t.peerBanners.length > 0 ? (
              <PeerMessageBanner banners={t.peerBanners} />
            ) : null}
            {t.userMessage ? (
              isDispatchTurn ? (
                <UserMessageBubble
                  text={t.userMessage.text}
                  commandText={t.userMessage.commandText}
                  skillCommand={t.userMessage.skillCommand}
                  files={t.userMessage.files}
                  conversationId={activeConversationId ?? undefined}
                />
              ) : (
                <ChatRow role="user" name={userName} avatarUrl={userAvatarUrl}>
                  <UserMessageBubble
                    text={t.userMessage.text}
                    commandText={t.userMessage.commandText}
                    skillCommand={t.userMessage.skillCommand}
                    files={t.userMessage.files}
                    conversationId={activeConversationId ?? undefined}
                  />
                </ChatRow>
              )
            ) : null}
            {t.toolGroup ? (
              <ToolGroupCard
                status={t.toolGroup.status}
                steps={t.toolGroup.steps}
              />
            ) : null}
            {teamSession ? (
              <TeamProgressBlock session={teamSession} onOpen={handleOpenTeamDrawer} />
            ) : null}
            {t.aiSegments.map((s) => (
              <ChatRow
                key={s.id}
                role="assistant"
                name={assistantName}
                avatarUrl={assistantLogo}
              >
                <AiBubble message={s.message} />
              </ChatRow>
            ))}
            {t.generatedFiles.map((f) => (
              <GeneratedFileCard
                key={f.id}
                title={f.title}
                sub={f.sub}
                appName={f.primaryAction === 'preview' ? '预览' : f.appName}
                primaryAction={f.primaryAction}
                canPreview={f.canPreview}
                canOpenExternal={f.canOpenExternal}
                canReveal={f.canReveal}
                onPreview={() => handlePreview(f)}
                onOpenExternal={() => void handleOpenExternal(f)}
                onReveal={() => void handleReveal(f)}
              />
            ))}
            {t.suggestions.length > 0 ? (
              <SuggestChipGroup
                items={t.suggestions.map((s) => ({ label: s, onClick: () => {} }))}
              />
            ) : null}
          </div>
        )
      })}
      {isStreaming ? (
        <ChatRow role="assistant" name={assistantName} avatarUrl={assistantLogo}>
          <StreamingBubble content={streamingContent} />
        </ChatRow>
      ) : null}
    </div>
  )
}
