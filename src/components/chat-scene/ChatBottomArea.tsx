/**
 * @designSource design.pen#Cbtm1 ChatBottomArea
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
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
import { localizeSkill, localizedSkillName } from '@/lib/skillLocalization'
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
  const { t, i18n } = useTranslation()
  const composerRef = useRef<RichComposerHandle>(null)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messageCount = useChatStore((s) => s.messages.length)
  const pendingSessionId = sessionIdOverride ?? activeConversationId ?? null
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const skills = useSkillStore((s) => s.skills)
  const getSkillById = useSkillStore((s) => s.getById)
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')
  // Snapshot of the installed skills as composer-friendly tokens.  The list
  // drives both the slash-command input rule inside the editor and the chip
  // rendered for any inline skill token already in the document.
  const skillTokens = useMemo(
    () =>
      skills.map((skill) => ({
        id: skill.id,
        label: localizeSkill(skill, i18n.language).name,
        command: skill.triggerText || `/${skill.id}`,
      })),
    [skills, i18n.language],
  )

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
    composerRef.current?.insertSkillToken({
      id: skillId,
      label: localizedSkillName(skill, skillId, i18n.language),
      command: skill?.triggerText || `/${skillId}`,
    })
    composerRef.current?.focus()
    setShowSkillPopover(false)
  }, [getSkillById, i18n.language])

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
    // The inline skill chip is the source of truth — it travels with the
    // doc, gets cleared automatically on submit, and is collected by the
    // serializer into payload.skills.  Only the first skill in a turn drives
    // the runtime; additional chips are dropped to avoid ambiguous routing.
    const skillForThisTurn = payload.skills[0] ?? null
    let markdownToSend = payload.markdown
    if (activeConversationId && messageCount === 0) {
      const teamId = await ensureExpertTeam(activeConversationId)
      const team = teamId ? findTeam(teamId, i18n.language) : undefined
      if (team) {
        markdownToSend = buildDirectorPrompt(team, markdownToSend, i18n.language)
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
  }, [sendUserMessage, activeConversationId, messageCount, i18n.language])

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
      className="relative shrink-0"
    >
      <div
        className="px-6 pt-4 pb-5"
      >
        <div
          data-testid="chat-composer-width-shell"
          className={chatWidthMode === 'full' ? 'relative w-full' : 'relative mx-auto w-full max-w-[736px]'}
        >
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
              skillTokens={skillTokens}
              onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
              tips={<BottomTips />}
              containerClassName="shadow-[var(--shadow-md)]"
              limitEditorHeight
            />
          </div>
        </div>
      </div>
    </footer>
  )
}
