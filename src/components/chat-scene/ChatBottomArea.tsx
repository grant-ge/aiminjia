/**
 * @designSource design.pen#Cbtm1 ChatBottomArea
 */
import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { SkillPopover } from '@/components/chat/SkillPopover'
import { SlashCommandPopover } from '@/components/chat/SlashCommandPopover'
import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'
import { useChat, type PendingFileInfo } from '@/hooks/useChat'
import { useFileUpload, type UploadedFile } from '@/hooks/useFileUpload'
import { useSkillComposer } from '@/hooks/useSkillComposer'
import { useChatStore } from '@/stores/chatStore'
import { useUiStore } from '@/stores/uiStore'

const FILE_TYPE_CONFIG: Record<string, { label: string; bg: string; color: string }> = {
  excel: { label: 'XLS', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  csv: { label: 'CSV', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  word: { label: 'DOC', bg: 'var(--color-filetype-blue-bg)', color: 'var(--color-semantic-blue)' },
  pdf: { label: 'PDF', bg: 'var(--color-filetype-red-bg)', color: 'var(--color-semantic-red)' },
  json: { label: 'JSON', bg: 'var(--color-filetype-accent-bg)', color: 'var(--color-accent)' },
}

function PendingFiles({
  pendingFiles,
  onRemove,
}: {
  pendingFiles: UploadedFile[]
  onRemove: (id: string) => void
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {pendingFiles.map((file) => {
        const config = FILE_TYPE_CONFIG[file.fileType] ?? FILE_TYPE_CONFIG.csv
        return (
          <div
            key={file.id}
            className="inline-flex items-center gap-2 rounded-lg py-1.5 pr-2 pl-2.5"
            style={{ background: config.bg }}
          >
            <span className="text-xs font-bold" style={{ color: config.color }}>
              {config.label}
            </span>
            <span className="max-w-[180px] truncate text-xs font-medium" style={{ color: 'var(--color-text-primary)' }}>
              {file.fileName}
            </span>
            <button
              type="button"
              className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border-none transition-colors"
              style={{
                background: 'var(--color-primary-subtle)',
                color: 'var(--color-text-muted)',
              }}
              onClick={() => onRemove(file.id)}
            >
              <svg className="h-2.5 w-2.5" viewBox="0 0 24 24" fill="currentColor">
                <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
              </svg>
            </button>
          </div>
        )
      })}
    </div>
  )
}

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
  const [pendingFiles, setPendingFiles] = useState<UploadedFile[]>([])
  const [isSending, setIsSending] = useState(false)
  const [showAttachmentMenu, setShowAttachmentMenu] = useState(false)
  const isComposingRef = useRef(false)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const attachmentMenuRef = useRef<HTMLDivElement>(null)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const selectedSkillCommand = useChatStore((s) => activeConversationId ? s.selectedSkillCommands[activeConversationId] ?? null : null)
  const clearSelectedSkillCommand = useChatStore((s) => s.clearSelectedSkillCommand)
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isUploading, selectAndUploadFiles } = useFileUpload()
  const openSettings = useUiStore((s) => s.openSettings)
  const {
    showSkillPopover,
    setShowSkillPopover,
    slashMatch,
    slashOpen,
    handleSkillPick,
    handleSlashSelect,
    handleSlashClose,
  } = useSkillComposer({
    input,
    setInput,
    textareaRef,
  })

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

  useEffect(() => {
    if (!showAttachmentMenu) return
    const handlePointerDown = (event: MouseEvent) => {
      if (!attachmentMenuRef.current?.contains(event.target as Node)) {
        setShowAttachmentMenu(false)
      }
    }
    window.addEventListener('mousedown', handlePointerDown)
    return () => window.removeEventListener('mousedown', handlePointerDown)
  }, [showAttachmentMenu])

  const handleSend = useCallback(async (overrideText?: string) => {
    const trimmed = (overrideText ?? input).trim()
    if (!trimmed && pendingFiles.length === 0) return
    if (isStreaming || isSending) return

    setIsSending(true)
    const fileInfos: PendingFileInfo[] = pendingFiles.map((f) => ({
      id: f.id,
      fileName: f.fileName,
      fileType: f.fileType,
      fileSize: f.fileSize,
    }))

    const IPC_TIMEOUT_MS = 15_000
    try {
      const sent = await Promise.race([
        sendUserMessage(
          trimmed || t('inputBar.analyzeFile'),
          fileInfos.length > 0 ? fileInfos : undefined,
        ),
        new Promise<void>((_, reject) =>
          setTimeout(() => reject(new Error('IPC timeout')), IPC_TIMEOUT_MS),
        ),
      ])
      if (sent) {
        setInput('')
        setPendingFiles([])
        clearSelectedSkillCommand(activeConversationId)
      }
    } catch (err) {
      console.error('[ChatBottomArea] sendUserMessage failed or timed out:', err)
    } finally {
      setIsSending(false)
    }
  }, [activeConversationId, clearSelectedSkillCommand, input, isSending, isStreaming, pendingFiles, sendUserMessage, t])

  const handleKeyDown = useCallback((e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (slashOpen) return
    if (e.key === 'Enter' && !e.shiftKey && !isComposingRef.current && !e.nativeEvent.isComposing) {
      e.preventDefault()
      void handleSend()
    }
  }, [handleSend, slashOpen])

  const handleUploadFileClick = useCallback(async () => {
    setShowAttachmentMenu(false)
    const results = await selectAndUploadFiles(pendingFiles)
    if (results.length > 0) {
      setPendingFiles((prev) => [...prev, ...results])
    }
  }, [pendingFiles, selectAndUploadFiles])

  const hasPendingContent = input.trim() || pendingFiles.length > 0
  const isSendDisabled = (!hasPendingContent && !isStreaming) || isSending
  const attachmentBusy = isUploading

  return (
    <div
      className="absolute right-0 bottom-0 left-0 px-6 pt-4 pb-5"
      style={{ background: 'linear-gradient(transparent, var(--color-bg-main) 30%)' }}
    >

      <div className="relative mx-auto max-w-[1032px]">
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

        <div className="relative" ref={attachmentMenuRef}>
          {showAttachmentMenu ? (
            <div
              className="absolute bottom-[calc(100%+8px)] left-0 z-20 min-w-[220px] rounded-xl border p-2 shadow-lg"
              style={{
                borderColor: 'var(--color-border-secondary)',
                background: 'var(--color-bg-input)',
                boxShadow: 'var(--shadow-card)',
              }}
            >
              <button
                type="button"
                className="flex w-full flex-col items-start rounded-lg px-3 py-2 text-left transition-colors"
                style={{
                  background: 'transparent',
                  border: 'none',
                  color: 'var(--color-text-primary)',
                }}
                onClick={() => void handleUploadFileClick()}
              >
                <span className="text-sm font-medium">{t('inputBar.uploadFile')}</span>
                <span className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
                  继续使用复制上传模式
                </span>
              </button>
            </div>
          ) : null}

          <ChatComposerCompact
            value={input}
            onChange={setInput}
            onSubmit={(value) => void handleSend(value)}
            submitDisabled={isSendDisabled}
            placeholder={pendingFiles.length > 0 ? t('inputBar.placeholderWithFile') : t('inputBar.placeholder')}
            onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
            permissionLabel="完全访问权限"
            showProjectButton={false}
            onPermissionClick={() => openSettings('permissions')}
            isStreaming={isStreaming}
            onStop={stopCurrentStream}
            onOpenAttachment={attachmentBusy ? undefined : () => setShowAttachmentMenu((prev) => !prev)}
            pendingFilesSlot={pendingFiles.length > 0 ? (
              <PendingFiles
                pendingFiles={pendingFiles}
                onRemove={(id) => setPendingFiles((prev) => prev.filter((file) => file.id !== id))}
              />
            ) : null}
            skillCommand={selectedSkillCommand}
            onClearSkillCommand={() => clearSelectedSkillCommand(activeConversationId)}
            textareaRef={textareaRef}
            onKeyDown={handleKeyDown}
            onCompositionStart={() => { isComposingRef.current = true }}
            onCompositionEnd={() => {
              setTimeout(() => { isComposingRef.current = false }, 50)
            }}
            tips={<BottomTips />}
          />
        </div>
      </div>
    </div>
  )
}
