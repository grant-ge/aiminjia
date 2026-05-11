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
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

function BottomTips() {
  return (
    <>
      <span>内容由 AI 生成，请仔细核实回答内容</span>
      <div className="flex items-center gap-3">
        <span>Enter 发送</span>
        <span>Shift+Enter 换行</span>
      </div>
    </>
  )
}

export function ChatBottomArea({ disabled = false }: { disabled?: boolean }) {
  const { t } = useTranslation()
  const composerRef = useRef<RichComposerHandle>(null)
  const [isSending, setIsSending] = useState(false)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()
  const [showSkillPopover, setShowSkillPopover] = useState(false)
  const getSkillById = useSkillStore((s) => s.getById)
  // Selected skill chip — set by handleSkillPick, cleared after submit so the
  // skill id flows into the IPC `selectedSkillId` field. Without this the
  // backend only sees the slash-prefixed text and the prompt builder cannot
  // inject SKILL.md / mark the turn as a skill-driven flow.
  const [selectedSkill, setSelectedSkill] = useState<{ id: string; label?: string } | null>(null)

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
    const next = trigger.endsWith(' ') ? trigger : `${trigger} `
    composerRef.current?.clear()
    composerRef.current?.getEditor()?.commands.insertContent(next)
    composerRef.current?.focus()
    setShowSkillPopover(false)
    setSelectedSkill({ id: skillId, label: skill?.displayName || skill?.id || skillId })
  }, [getSkillById])

  const handleSubmit = useCallback(async (payload: RichComposerSubmitPayload) => {
    if (isSending) return
    setIsSending(true)
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
    try {
      await sendUserMessage(
        payload.markdown,
        fileInfos.length > 0 ? fileInfos : undefined,
        skillForThisTurn,
      )
    } catch (err) {
      console.error('[ChatBottomArea] sendUserMessage failed:', err)
      throw err
    } finally {
      setIsSending(false)
    }
  }, [isSending, sendUserMessage, selectedSkill])

  const handlePickAttachments = useCallback(async () => {
    const results = await pickAttachments()
    if (results.length > 0) {
      composerRef.current?.insertAttachmentTokens(pendingAttachmentsToTokens(results))
    }
  }, [pickAttachments])

  return (
    <footer
      data-testid="chat-bottom-area"
      className="relative h-[148px] shrink-0"
    >
      <div
        className="absolute right-0 bottom-0 left-0 px-6 pt-4 pb-5 [scrollbar-gutter:stable_both-edges]"
      >
        <div className="relative mx-auto w-full max-w-[736px]">
          <div className="absolute bottom-full left-1/2 z-30 mb-1 -translate-x-1/2">
            <SkillPopover
              open={showSkillPopover}
              onPick={handleSkillPick}
              onClose={() => setShowSkillPopover(false)}
            />
          </div>

          <div className="relative">
            <RichComposer
              ref={composerRef}
              placeholder={t('inputBar.placeholder')}
              onSubmit={handleSubmit}
              disabled={disabled}
              isStreaming={isStreaming}
              onStop={stopCurrentStream}
              clearOnSubmit
              autoFocus
              initialMarkdown={initialMarkdown}
              showProjectButton={false}
              onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
              onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
              tips={<BottomTips />}
            />
          </div>
        </div>
      </div>
    </footer>
  )
}
