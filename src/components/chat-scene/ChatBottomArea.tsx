/**
 * @designSource design.pen#Cbtm1 ChatBottomArea
 */
import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { SkillPopover } from '@/components/chat/SkillPopover'
import { SlashCommandPopover } from '@/components/chat/SlashCommandPopover'
import { PendingAttachmentChips } from '@/components/chat/PendingAttachmentChips'
import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { useChatAttachments, type PendingAttachment } from '@/hooks/useChatAttachments'
import { useComposerPaste } from '@/hooks/useComposerPaste'
import { useSkillComposer } from '@/hooks/useSkillComposer'
import { useChatStore } from '@/stores/chatStore'
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

export function ChatBottomArea() {
  const { t } = useTranslation()
  const [input, setInput] = useState('')
  const [pendingFiles, setPendingFiles] = useState<PendingAttachment[]>([])
  const [isSending, setIsSending] = useState(false)
  const isComposingRef = useRef(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isPickingAttachments, pickAttachments } = useChatAttachments()
  // TODO: openSettings 待权限按钮功能上线后恢复使用
  // const openSettings = useUiStore((s) => s.openSettings)
  const {
    showSkillPopover,
    setShowSkillPopover,
    slashMatch,
    slashOpen,
    handleSkillPick,
    handleInputChange,
    handleSlashSelect,
    handleSlashClose,
  } = useSkillComposer({
    input,
    setInput,
    textareaRef,
    conversationId: activeConversationId,
  })

  useEffect(() => {
    const prefill = useUiStore.getState().consumePrefillText()
    if (prefill) {
      setInput(prefill)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (!isStreaming) {
      requestAnimationFrame(() => {
        textareaRef.current?.focus()
      })
    }
  }, [activeConversationId, isStreaming])

  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`
  }, [input])


  const handleSend = useCallback(async (overrideText?: string) => {
    const trimmed = (overrideText ?? input).trim()
    if (!trimmed && pendingFiles.length === 0) return
    if (isStreaming || isSending) return

    console.debug('[skill-command][composer-submit]', {
      traceId: activeConversationId,
      conversationId: activeConversationId,
      hasOverrideText: overrideText !== undefined,
      textLength: trimmed.length,
      pendingFileCount: pendingFiles.length,
    })

    setIsSending(true)
    setInput('')
      const fileInfos: PendingFileInfo[] = pendingFiles.map((f) => ({
        id: f.id,
        fileName: f.fileName,
        filePath: f.path,
        kind: f.kind,
        fileType: f.fileType,
        fileSize: f.fileSize,
        mimeType: f.mimeType,
      }))

    try {
      const sent = await sendUserMessage(
        trimmed || t('inputBar.analyzeFile'),
        fileInfos.length > 0 ? fileInfos : undefined,
      )
      if (sent) {
        setPendingFiles([])
      }
    } catch (err) {
      console.error('[ChatBottomArea] sendUserMessage failed:', err)
    } finally {
      setIsSending(false)
    }
  }, [activeConversationId, input, isSending, isStreaming, pendingFiles, sendUserMessage, t])

  const handleKeyDown = useCallback((e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (slashOpen) return
    if (e.key === 'Enter' && !e.shiftKey && !isComposingRef.current && !e.nativeEvent.isComposing) {
      e.preventDefault()
      void handleSend()
    }
  }, [handleSend, slashOpen])

  const handlePickAttachments = useCallback(async () => {
    const results = await pickAttachments()
    if (results.length > 0) {
      setPendingFiles((prev) => {
        const seen = new Set(prev.map((file) => file.id))
        const deduped = results.filter((file) => !seen.has(file.id))
        return deduped.length > 0 ? [...prev, ...deduped] : prev
      })
    }
  }, [pickAttachments])

  const appendPendingFiles = useCallback((resolved: PendingAttachment[]) => {
    setPendingFiles((prev) => {
      const seen = new Set(prev.map((file) => file.id))
      const next = resolved.filter((file) => !seen.has(file.id))
      return next.length > 0 ? [...prev, ...next] : prev
    })
  }, [])
  const { handlePaste } = useComposerPaste({ onAttachmentsResolved: appendPendingFiles })

  const hasPendingContent = input.trim() || pendingFiles.length > 0
  const isSendDisabled = (!hasPendingContent && !isStreaming) || isSending
  const attachmentBusy = isPickingAttachments

  return (
    <footer
      data-testid="chat-bottom-area"
      className="relative h-[148px] shrink-0"
      style={{ background: 'var(--color-bg-main)' }}
    >
      <div
        className="absolute right-0 bottom-0 left-0 px-6 pt-4 pb-5 [scrollbar-gutter:stable_both-edges]"
        style={{ background: 'linear-gradient(transparent, var(--color-bg-main) 30%)' }}
      >
        <div className="relative mx-auto w-full max-w-[736px]">
          <div className="absolute bottom-full left-10 z-30 mb-3">
            <SkillPopover
              open={showSkillPopover}
              onPick={handleSkillPick}
              onClose={() => setShowSkillPopover(false)}
            />
          </div>

          {slashOpen && slashMatch ? (
            <SlashCommandPopover
              filterText={slashMatch.filter}
              onSelect={handleSlashSelect}
              onClose={handleSlashClose}
            />
          ) : null}

          <div className="relative">
            <ChatComposerCompact
              value={input}
              onChange={handleInputChange}
              onSubmit={(value) => void handleSend(value)}
              submitDisabled={isSendDisabled}
              placeholder={pendingFiles.length > 0 ? t('inputBar.placeholderWithFile') : t('inputBar.placeholder')}
              onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
              showProjectButton={false}
              isStreaming={isStreaming}
              onStop={stopCurrentStream}
              onOpenAttachment={attachmentBusy ? undefined : () => void handlePickAttachments()}
              pendingFilesSlot={pendingFiles.length > 0 ? (
                <PendingAttachmentChips
                  pendingFiles={pendingFiles}
                  onRemove={(id: string) => setPendingFiles((prev) => prev.filter((file) => file.id !== id))}
                />
              ) : null}
              textareaRef={textareaRef}
              onKeyDown={handleKeyDown}
              onCompositionStart={() => { isComposingRef.current = true }}
              onCompositionEnd={() => {
                setTimeout(() => { isComposingRef.current = false }, 50)
              }}
              onPaste={handlePaste}
              tips={<BottomTips />}
            />
          </div>
        </div>
      </div>
    </footer>
  )
}
