/**
 * InputBar — text input, upload button, file preview chips, send button.
 * Based on visual-prototype-zh.html input-bar section.
 *
 * Wired to useChat (send / stop) and useFileUpload (native file picker).
 */
import { useState, useRef, useEffect, type KeyboardEvent } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { useChat } from '@/hooks/useChat'
import { useFileUpload, type UploadedFile } from '@/hooks/useFileUpload'
import type { PendingFileInfo } from '@/hooks/useChat'

const FILE_TYPE_CONFIG: Record<string, { label: string; bg: string; color: string }> = {
  excel: { label: 'XLS', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  csv:   { label: 'CSV', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  word:  { label: 'DOC', bg: 'var(--color-filetype-blue-bg)', color: 'var(--color-semantic-blue)' },
  pdf:   { label: 'PDF', bg: 'var(--color-filetype-red-bg)', color: 'var(--color-semantic-red)' },
  json:  { label: 'JSON', bg: 'var(--color-filetype-accent-bg)', color: 'var(--color-accent)' },
}

export function InputBar() {
  const [input, setInput] = useState('')
  const [pendingFiles, setPendingFiles] = useState<UploadedFile[]>([])
  const [isSending, setIsSending] = useState(false)
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat()
  const { isUploading, selectAndUploadFile } = useFileUpload()
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const activeConversationId = useChatStore((s) => s.activeConversationId)

  // Auto-focus textarea when switching conversations or when streaming completes
  useEffect(() => {
    if (!isStreaming) {
      requestAnimationFrame(() => {
        textareaRef.current?.focus()
      })
    }
  }, [activeConversationId, isStreaming])

  // Auto-resize textarea based on content
  useEffect(() => {
    const el = textareaRef.current
    if (el) {
      el.style.height = 'auto'
      el.style.height = `${Math.min(el.scrollHeight, 160)}px`
    }
  }, [input])

  const handleSend = async () => {
    const trimmed = input.trim()
    if (!trimmed && pendingFiles.length === 0) return
    if (isStreaming || isSending) return

    setIsSending(true)
    const fileInfos: PendingFileInfo[] = pendingFiles.map((f) => ({
      id: f.id,
      fileName: f.fileName,
      fileType: f.fileType,
      fileSize: f.fileSize,
    }))

    // Use a timeout to ensure isSending is always cleared, even if the IPC
    // call hangs indefinitely (e.g. backend deadlock). The backend send_message
    // command returns almost immediately after spawning the agent loop, so 15s
    // is very generous.
    const IPC_TIMEOUT_MS = 15_000
    try {
      await Promise.race([
        sendUserMessage(trimmed || '请分析这个文件', fileInfos.length > 0 ? fileInfos : undefined),
        new Promise<void>((_, reject) =>
          setTimeout(() => reject(new Error('IPC timeout')), IPC_TIMEOUT_MS)
        ),
      ])
    } catch (err) {
      console.error('[InputBar] sendUserMessage failed or timed out:', err)
    } finally {
      setIsSending(false)
    }
    setInput('')
    setPendingFiles([])
  }

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  const handleSendButtonClick = () => {
    if (isStreaming) {
      stopCurrentStream()
    } else {
      handleSend()
    }
  }

  const handleUploadClick = async () => {
    const result = await selectAndUploadFile()
    if (result) {
      setPendingFiles((prev) => [...prev, result])
    }
  }

  const removeFile = (id: string) => {
    setPendingFiles((prev) => prev.filter((f) => f.id !== id))
  }

  const hasPendingContent = input.trim() || pendingFiles.length > 0
  const isSendDisabled = (!hasPendingContent && !isStreaming) || isSending

  return (
    <div
      className="absolute right-0 bottom-0 left-0 px-6 pt-4 pb-5"
      style={{
        background: `linear-gradient(transparent, var(--color-bg-main) 30%)`,
      }}
    >
      <div
        className="mx-auto max-w-[860px] rounded-xl"
        style={{
          background: 'var(--color-bg-input)',
          boxShadow: 'var(--shadow-input)',
        }}
      >
        {/* Pending file chips */}
        {pendingFiles.length > 0 && (
          <div className="flex flex-wrap gap-2 px-4 pt-3 pb-1">
            {pendingFiles.map((file) => {
              const config = FILE_TYPE_CONFIG[file.fileType] ?? FILE_TYPE_CONFIG.csv
              return (
                <div
                  key={file.id}
                  className="inline-flex items-center gap-2 rounded-lg py-1.5 pr-2 pl-2.5"
                  style={{
                    background: config.bg,
                  }}
                >
                  <span
                    className="text-xs font-bold"
                    style={{ color: config.color }}
                  >
                    {config.label}
                  </span>
                  <span
                    className="max-w-[180px] truncate text-xs font-medium"
                    style={{ color: 'var(--color-text-primary)' }}
                  >
                    {file.fileName}
                  </span>
                  <button
                    className="flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center rounded-full border-none transition-colors"
                    style={{
                      background: 'var(--color-primary-subtle)',
                      color: 'var(--color-text-muted)',
                    }}
                    onClick={() => removeFile(file.id)}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background = 'var(--color-primary-muted)'
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = 'var(--color-primary-subtle)'
                    }}
                  >
                    <svg className="h-2.5 w-2.5" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
                    </svg>
                  </button>
                </div>
              )
            })}
          </div>
        )}

        {/* Input row */}
        <div className="flex items-end gap-2 px-4 py-3">
          {/* Upload button */}
          <button
            className="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-lg border-none outline-none transition-all duration-150"
            style={{
              color: isUploading ? 'var(--color-text-secondary)' : 'var(--color-text-muted)',
              background: 'transparent',
            }}
            title="上传文件（Excel/Word/PDF）"
            disabled={isUploading}
            onClick={handleUploadClick}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'var(--color-bg-card-hover)'
              e.currentTarget.style.color = 'var(--color-text-secondary)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = 'transparent'
              e.currentTarget.style.color = isUploading ? 'var(--color-text-secondary)' : 'var(--color-text-muted)'
            }}
          >
            {isUploading ? (
              <svg className="h-[18px] w-[18px] animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                <circle cx="12" cy="12" r="10" strokeDasharray="50" strokeDashoffset="20" strokeLinecap="round" />
              </svg>
            ) : (
              <svg className="h-[18px] w-[18px]" viewBox="0 0 24 24" fill="currentColor">
                <path d="M16.5 6v11.5c0 2.21-1.79 4-4 4s-4-1.79-4-4V5c0-1.38 1.12-2.5 2.5-2.5s2.5 1.12 2.5 2.5v10.5c0 .55-.45 1-1 1s-1-.45-1-1V6H10v9.5c0 1.38 1.12 2.5 2.5 2.5s2.5-1.12 2.5-2.5V5c0-2.21-1.79-4-4-4S7 2.79 7 5v12.5c0 3.04 2.46 5.5 5.5 5.5s5.5-2.46 5.5-5.5V6h-1.5z" />
              </svg>
            )}
          </button>

          {/* Multi-line text input */}
          <textarea
            ref={textareaRef}
            className="flex-1 resize-none border-none bg-transparent py-[5px] text-md outline-none"
            style={{
              color: 'var(--color-text-primary)',
              fontFamily: 'var(--font-sans)',
              minHeight: '32px',
              maxHeight: '160px',
              lineHeight: '1.5',
            }}
            rows={1}
            placeholder={pendingFiles.length > 0
              ? '添加说明（可选），然后按回车发送...'
              : '随时提问，或上传文件让我分析...'}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={isStreaming}
          />

          {/* Send / Stop button */}
          <button
            className="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-lg border-none outline-none transition-colors duration-150"
            style={{
              background:
                isStreaming || hasPendingContent
                  ? 'var(--color-primary)'
                  : 'var(--color-border)',
              cursor: isSendDisabled ? 'default' : 'pointer',
            }}
            onClick={handleSendButtonClick}
            disabled={isSendDisabled}
          >
            {isStreaming ? (
              <svg className="h-3.5 w-3.5" viewBox="0 0 24 24" fill="var(--color-text-on-primary)">
                <rect x="4" y="4" width="16" height="16" rx="2" />
              </svg>
            ) : (
              <svg className="h-4 w-4" viewBox="0 0 24 24" fill="var(--color-text-on-primary)">
                <path d="M2.01 21L23 12 2.01 3 2 10l15 2-15 2z" />
              </svg>
            )}
          </button>
        </div>
      </div>
    </div>
  )
}
