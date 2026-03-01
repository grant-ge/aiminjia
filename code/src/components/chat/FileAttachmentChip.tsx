/**
 * FileAttachmentChip — inline file display in user messages.
 * Based on visual-prototype-zh.html .file-attach styles.
 */
import type { FileAttachment } from '@/types/message'

const FILE_TYPE_CONFIG: Record<
  FileAttachment['fileType'],
  { label: string; bg: string; color: string }
> = {
  excel: {
    label: 'XLS',
    bg: 'var(--color-filetype-green-bg)',
    color: 'var(--color-semantic-green)',
  },
  csv: {
    label: 'CSV',
    bg: 'var(--color-filetype-green-bg)',
    color: 'var(--color-semantic-green)',
  },
  word: {
    label: 'DOC',
    bg: 'var(--color-filetype-blue-bg)',
    color: 'var(--color-semantic-blue)',
  },
  pdf: {
    label: 'PDF',
    bg: 'var(--color-filetype-red-bg)',
    color: 'var(--color-semantic-red)',
  },
  json: {
    label: 'JSON',
    bg: 'var(--color-filetype-accent-bg)',
    color: 'var(--color-accent)',
  },
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

interface FileAttachmentChipProps {
  file: FileAttachment
}

export function FileAttachmentChip({ file }: FileAttachmentChipProps) {
  const config = FILE_TYPE_CONFIG[file.fileType] ?? FILE_TYPE_CONFIG.csv
  const statusText =
    file.status === 'error'
      ? file.errorMessage ?? 'Error'
      : file.status === 'uploading'
        ? 'Uploading...'
        : file.status === 'parsing'
          ? 'Parsing...'
          : ''

  return (
    <div
      className="mb-1.5 inline-flex max-w-[360px] items-center gap-2.5 rounded-lg border px-3.5 py-2.5"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
      }}
    >
      {/* File type icon */}
      <div
        className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-xs font-bold"
        style={{ background: config.bg, color: config.color }}
      >
        {config.label}
      </div>

      {/* File info */}
      <div className="min-w-0 flex-1">
        <div
          className="truncate text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {file.fileName}
        </div>
        <div
          className="text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {formatFileSize(file.fileSize)}
          {statusText && ` · ${statusText}`}
        </div>
      </div>
    </div>
  )
}
