/**
 * @designSource design.pen#1JNrw bubble/adaptive-max-80
 * @sizing r-16 padding [8,12] bg primary fg primary-foreground; align right; max-w 80%
 */
import { openLocalFile } from '@/lib/tauri'
import { Blocks } from 'lucide-react'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { isPreviewableFileType } from '@/components/chat/generatedFileActions'
import type { FileAttachment, SkillCommandBreadcrumb } from '@/types/message'

interface UserMessageBubbleProps {
  text: string
  commandText?: string
  skillCommand?: SkillCommandBreadcrumb
  files?: FileAttachment[]
  conversationId?: string
}

function AttachmentIcon({ kind }: { kind: FileAttachment['kind'] }) {
  if (kind === 'folder') {
    return (
      <svg className="h-3.5 w-3.5 shrink-0" viewBox="0 0 24 24" fill="currentColor">
        <path d="M10 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z" />
      </svg>
    )
  }
  if (kind === 'image') {
    return (
      <svg className="h-3.5 w-3.5 shrink-0" viewBox="0 0 24 24" fill="currentColor">
        <path d="M21 19V5c0-1.1-.9-2-2-2H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2zM8.9 13.98l2.1 2.53 3.1-3.99c.2-.26.6-.26.8.01l3.51 4.68a.5.5 0 01-.4.8H6.02a.5.5 0 01-.39-.81L8.12 13.98c.19-.26.58-.26.78 0z" />
      </svg>
    )
  }
  return (
    <svg className="h-3.5 w-3.5 shrink-0" viewBox="0 0 24 24" fill="currentColor">
      <path d="M14 2H6c-1.1 0-1.99.9-1.99 2L4 20c0 1.1.89 2 1.99 2H18c1.1 0 2-.9 2-2V8l-6-6zm4 18H6V4h7v5h5v11z" />
    </svg>
  )
}

export function UserMessageBubble({ text, commandText, skillCommand, files, conversationId }: UserMessageBubbleProps) {
  const command = skillCommand?.command ?? commandText?.split(/\s+/)[0]
  const tokenLabel = skillCommand?.label ?? skillCommand?.id ?? command?.replace(/^\//, '')
  const hasFiles = (files?.length ?? 0) > 0
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)

  const handleAttachmentClick = (file: FileAttachment) => {
    const localPath = file.filePath
    if (!localPath) return
    if (file.kind === 'folder') {
      void openLocalFile(localPath)
      return
    }
    if (isPreviewableFileType(file.fileType, file.fileName) && conversationId) {
      openPreview({
        fileId: file.id,
        conversationId,
        fileName: file.fileName,
        fileType: file.fileType,
        localPath,
      })
      return
    }
    void openLocalFile(localPath)
  }

  return (
    <div className="flex w-full flex-col items-end gap-1.5">
      {hasFiles ? (
        <div className="flex max-w-[80%] flex-wrap justify-end gap-1.5">
          {files!.map((file) => (
            <button
              key={file.id}
              type="button"
              onClick={() => handleAttachmentClick(file)}
              className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 transition-opacity hover:opacity-80"
              style={{ background: 'var(--color-bg-subtle)', color: 'var(--color-text-secondary)' }}
              title={file.fileName}
            >
              <AttachmentIcon kind={file.kind ?? (file.fileType === 'folder' ? 'folder' : file.fileType === 'image' ? 'image' : 'file')} />
              <span className="max-w-[64px] truncate text-xs" style={{ color: 'var(--color-text-primary)' }}>
                {file.fileName}
              </span>
            </button>
          ))}
        </div>
      ) : null}
      {text || tokenLabel ? (
        <div
          data-testid="user-bubble"
          className="max-w-[80%] rounded-2xl bg-primary px-3 py-2 text-sm leading-relaxed text-primary-foreground"
        >
          {tokenLabel ? (
            <span
              data-testid="user-skill-token"
              className="mr-2 inline-flex translate-y-[1px] items-center gap-1.5 rounded-lg bg-white/24 px-2 py-1 text-xs font-semibold leading-none text-white shadow-[inset_0_0_0_1px_rgba(255,255,255,0.24)]"
              title={command}
            >
              <Blocks aria-hidden="true" className="shrink-0" style={{ width: '0.75rem', height: '0.75rem', transform: 'translateY(1px)' }} />
              <span>{tokenLabel}</span>
            </span>
          ) : null}
          <span className="whitespace-pre-wrap break-words">{text}</span>
        </div>
      ) : null}
    </div>
  )
}
