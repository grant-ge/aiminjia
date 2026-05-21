/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'

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
import { TeamVisualProvider } from '@/components/team/TeamVisualContext'
import { toPreviewTarget } from '@/components/chat/generatedFileActions'
import type { ExpertTeamId } from '@/features/expert-teams/teams'
import { getExpertTeam } from '@/features/expert-teams/teams'
import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useChannelStore } from '@/stores/channelStore'
import { useChatStore } from '@/stores/chatStore'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useChat } from '@/hooks/useChat'
import { useTeamOverview } from '@/hooks/useTeamOverview'
import { useTurnRenderModel, type RenderGeneratedFile } from '@/hooks/useTurnRenderModel'
import { openGeneratedFile, revealFileInFolder } from '@/lib/tauri'
import { useConversationTeamState, useTeamStore } from '@/stores/teamStore'

type FileActionKind = 'preview' | 'open' | 'reveal'

// Display name for IM platforms when the inbound conversation's sender is
// rendered as the user-side identity. Keep in sync with AppSidebar's
// CHANNEL_PLATFORM_NAME — WhatsApp intentionally not in this map because it
// uses the contact's real push_name rather than the platform brand.
const CHANNEL_PLATFORM_DISPLAY: Record<string, string> = {
  dingtalk: '钉钉',
  feishu: '飞书',
  wecom: '企业微信',
  wechat: '个人微信',
  telegram: 'Telegram',
}

interface MessageListProps {
  expertTeamId?: ExpertTeamId
}

export function MessageList({ expertTeamId }: MessageListProps = {}) {
  const { t } = useTranslation()
  const expertTeam = expertTeamId ? getExpertTeam(expertTeamId) : undefined

  const FILE_ACTION_ERROR_TITLES: Record<FileActionKind, string> = {
    preview: t('messageList.cannotPreview'),
    open: t('messageList.cannotOpen'),
    reveal: t('messageList.cannotReveal'),
  }

  const turns = useTurnRenderModel()
  useChat()
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const isStreaming = useChatStore((s) => s.isStreaming)
  // Show the streaming bubble whenever this conversation is "busy" — that
  // window is wider than `isStreaming` alone (covers sentinel resume + the
  // moment between `addBusyConversation` and the first stream event), which
  // closes the visual gap users see today (spec §6.4).
  const showStreamingBubble = useChatStore((s) => {
    const id = s.activeConversationId
    if (!id) return false
    return s.busyConversations.has(id) || (s.streamStates[id]?.isStreaming ?? false)
  })
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
  const authUserName = useAuthStore((s) => s.user?.name ?? s.user?.username ?? '我')
  // In channel chats (WhatsApp/Telegram/dingtalk/...), the "user" role
  // bubbles come from the **external contact**, not the local AIjia operator.
  // Identity rules (2026-05-21):
  //   - WhatsApp: displayName (push_name) is reliable → use it for name +
  //     initial-style avatar so each contact looks distinct.
  //   - Other IM platforms (dingtalk/feishu/wecom/wechat/telegram): inbound
  //     messages don't carry a stable real name (feishu/wecom/wechat only
  //     give user_id). To stay visually consistent across IM tabs, render the
  //     platform display name + platform logo as the "from" side identity.
  //   - In-app (no channel binding): local auth user + neutral silhouette.
  const channelConversation = useChannelStore((s) => {
    if (!activeConversationId) return null
    return s.conversations.find((conv) => conv.sessionId === activeConversationId) ?? null
  })
  const { userName, userAvatarUrl, userAvatarVariant } = (() => {
    if (!channelConversation) {
      return {
        userName: authUserName,
        userAvatarUrl: null as string | null,
        userAvatarVariant: 'neutral' as 'initial' | 'neutral',
      }
    }
    if (channelConversation.platform === 'whatsapp') {
      const trimmed = channelConversation.displayName?.trim()
      return {
        userName: trimmed && trimmed.length > 0 ? trimmed : 'WhatsApp 私聊',
        userAvatarUrl: null as string | null,
        userAvatarVariant: 'initial' as 'initial' | 'neutral',
      }
    }
    return {
      userName: CHANNEL_PLATFORM_DISPLAY[channelConversation.platform] ?? channelConversation.platform,
      userAvatarUrl: `/logos/${channelConversation.platform}.png`,
      userAvatarVariant: 'initial' as 'initial' | 'neutral',
    }
  })()

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

  const handleOpenTeamDrawer = (teamId: string) => {
    if (activeConversationId) openDrawer(activeConversationId, teamId)
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
                <ChatRow role="user" name={userName} avatarUrl={userAvatarUrl} avatarVariant={userAvatarVariant}>
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
                durationMs={t.toolGroup.durationMs}
              />
            ) : null}
            {teamSession ? (
              <TeamVisualProvider value={expertTeam ?? null}>
                <TeamProgressBlock session={teamSession} onOpen={handleOpenTeamDrawer} />
              </TeamVisualProvider>
            ) : null}
            {/*
             * One ChatRow groups what the assistant actually produced:
             * text segments → generated-file cards → follow-up suggestion
             * chips. Tool execution trace is rendered above as a sibling
             * (no avatar) because it's process metadata, not output.
             *
             * Skipped when the turn produced nothing on the assistant side
             * (e.g. a bare peer banner / task-notification / tools-only
             * mid-stream turn — the StreamingBubble below owns that case).
             */}
            {t.aiSegments.length > 0 ||
            t.generatedFiles.length > 0 ||
            t.suggestions.length > 0 ? (
              <ChatRow
                role="assistant"
                name={assistantName}
                avatarUrl={assistantLogo}
              >
                {t.aiSegments.map((s) => (
                  <AiBubble key={s.id} message={s.message} />
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
              </ChatRow>
            ) : null}
          </div>
        )
      })}
      {showStreamingBubble ? (
        <ChatRow role="assistant" name={assistantName} avatarUrl={assistantLogo}>
          <StreamingBubble content={streamingContent} />
        </ChatRow>
      ) : null}
    </div>
  )
}
