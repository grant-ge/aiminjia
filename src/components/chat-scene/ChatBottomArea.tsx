/**
 * @designSource design.pen#Cbtm1 ChatBottomArea
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { SkillPopover } from '@/components/chat/SkillPopover'
import {
  RichComposer,
  pendingAttachmentsToTokens,
  useComposerAttachmentPaste,
  useComposerDropInbox,
  type RichComposerHandle,
  type RichComposerSubmitPayload,
} from '@/components/rich-composer'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { useChatAttachments } from '@/hooks/useChatAttachments'
import { useChatStore } from '@/stores/chatStore'
import { usePendingStore } from '@/stores/pendingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'
import { pendingSnapshotForSession } from '@/lib/tauri'
import { PendingChips } from '@/features/chat/PendingChips'
import { ensureExpertTeam } from '@/features/expert-teams/expertTeamRegistry'
import { getExpertTeam as findTeam } from '@/features/expert-teams/teams'
import { buildDirectorPrompt } from '@/features/expert-teams/buildDirectorPrompt'

function BottomTips() {
  const { t } = useTranslation()
  return (
    <>
      <span>{t('bottomTips.aiDisclaimer')}</span>
      <div className="flex items-center gap-3">
        <span>{t('bottomTips.enterToSend')}</span>
        <span>{t('bottomTips.shiftEnterNewline')}</span>
      </div>
    </>
  )
}

export function ChatBottomArea({
  disabled = false,
  sessionIdOverride,
  placeholderOverride,
}: {
  disabled?: boolean
  /** When the bottom area is rendered inside a channel session view (DingTalk
   * etc.), the active session id does NOT live in chatStore — pass it
   * explicitly so pending chips / snapshot can target the right queue. */
  sessionIdOverride?: string
  /** When set, overrides the default i18n placeholder. Used by expert-teams. */
  placeholderOverride?: string
}) {
  const { t } = useTranslation()
  const composerRef = useRef<RichComposerHandle>(null)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messageCount = useChatStore((s) => s.messages.length)
  const pendingSessionId = sessionIdOverride ?? activeConversationId ?? null
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const getSkillById = useSkillStore((s) => s.getById)
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')
  // Selected skill chip — UI state only (after the local IPC plumbing was
  // dropped, see commit `drop selected_skill ipc plumbing`). The id no longer
  // flows to the backend; the trigger text is prepended to the outgoing
  // markdown at submit time so the `Skill` runtime tool can discover it.
  const [selectedSkill, setSelectedSkill] = useState<{
    id: string
    label?: string
    trigger: string
  } | null>(null)

  // One-shot prefill text (e.g., from generated suggestion); consumed synchronously
  // via lazy initializer so RichComposer's useEditor receives it on its very first render.
  const [initialMarkdown] = useState<string | undefined>(() => {
    const prefill = useUiStore.getState().consumePrefillText()
    return prefill ?? undefined
  })

  useComposerDropInbox(composerRef)
  useComposerAttachmentPaste(composerRef)

  useEffect(() => {
    if (!isStreaming) {
      requestAnimationFrame(() => {
        composerRef.current?.focus()
      })
    }
  }, [activeConversationId, isStreaming])

  const handleSkillPick = useCallback((skillId: string) => {
    const skill = getSkillById(skillId)
    const trigger = skill?.triggerText || `/${skillId}`
    setSelectedSkill({
      id: skillId,
      label: skill?.displayName || skill?.id || skillId,
      trigger,
    })
    setShowSkillPopover(false)
    composerRef.current?.focus()
  }, [getSkillById])

  const handleSubmit = useCallback(async (payload: RichComposerSubmitPayload) => {
    // Note: RichComposer.trySubmit already has a `submittingRef` guard against
    // duplicate concurrent calls for the same submission. We don't need a
    // separate `isSending` gate here, and adding one breaks the PendingQueue
    // UX: when the first message is in flight (sendUserMessage returns when
    // the IPC enqueues, but isSending was tied to LLM stream completion in
    // older code), a second Enter would silently drop.
    const fileInfos: PendingFileInfo[] = payload.attachments.map((f) => ({
      id: f.id,
      fileName: f.fileName,
      filePath: f.path,
      kind: f.kind,
      fileType: f.fileType,
      fileSize: f.fileSize,
      mimeType: f.mimeType,
    }))
    // Capture and clear the skill chip BEFORE awaiting send. If the user picked
    // a different skill while the previous send is mid-flight, that selection
    // belongs to the next turn, not this one.
    const skillForThisTurn = selectedSkill
    setSelectedSkill(null)
    let markdownToSend = payload.markdown
    if (skillForThisTurn) {
      const trigger = skillForThisTurn.trigger
      const sep = trigger.endsWith(' ') ? '' : ' '
      markdownToSend = `${trigger}${sep}${markdownToSend}`
    }
    if (activeConversationId && messageCount === 0) {
      const teamId = await ensureExpertTeam(activeConversationId)
      const team = teamId ? findTeam(teamId) : undefined
      if (team) {
        markdownToSend = buildDirectorPrompt(team, markdownToSend)
      }
    }
    try {
      await sendUserMessage(
        markdownToSend,
        fileInfos.length > 0 ? fileInfos : undefined,
        skillForThisTurn,
      )
    } catch (err) {
      console.error('[ChatBottomArea] sendUserMessage failed:', err)
      throw err
    }
  }, [selectedSkill, sendUserMessage, activeConversationId, messageCount])

  const handlePickAttachments = useCallback(async () => {
    const results = await pickAttachments()
    if (results.length > 0) {
      composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(results))
    }
  }, [pickAttachments])

  // Fetch pending queue snapshot when conversation switches.
  // Backend pushes incremental updates via pending:queued/drained/removed events.
  useEffect(() => {
    if (!pendingSessionId) return
    pendingSnapshotForSession(pendingSessionId)
      .then((items) =>
        usePendingStore.getState().applySnapshot(pendingSessionId, items),
      )
      .catch((e) => {
        // eslint-disable-next-line no-console
        console.warn('[pending] snapshot fetch failed', e)
      })
  }, [pendingSessionId])

  return (
    <footer
      data-testid="chat-bottom-area"
      className="relative h-[148px] shrink-0"
    >
      <div
        className="absolute right-0 bottom-0 left-0 px-6 pt-4 pb-5"
      >
        <div className={chatWidthMode === 'full' ? 'relative w-full' : 'relative mx-auto w-full max-w-[736px]'}>
          <div className="absolute bottom-full left-1/2 z-30 mb-1 -translate-x-1/2">
            <SkillPopover
              open={showSkillPopover}
              onPick={handleSkillPick}
              onClose={() => setShowSkillPopover(false)}
            />
          </div>

          <div className="relative">
            {pendingSessionId && <PendingChips sessionId={pendingSessionId} />}
            <RichComposer
              ref={composerRef}
              placeholder={placeholderOverride ?? t('inputBar.placeholder')}
              onSubmit={handleSubmit}
              disabled={disabled}
              isStreaming={isStreaming}
              onStop={stopCurrentStream}
              clearOnSubmit
              autoFocus
              initialMarkdown={initialMarkdown}
              showProjectButton={false}
              onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
              skillCommand={
                selectedSkill
                  ? {
                      id: selectedSkill.id,
                      label: selectedSkill.label ?? selectedSkill.id,
                      command: selectedSkill.trigger.trim(),
                    }
                  : null
              }
              onClearSkillCommand={() => setSelectedSkill(null)}
              onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
              tips={<BottomTips />}
            />
          </div>
        </div>
      </div>
    </footer>
  )
}
