/**
 * GeneratedFileCard — file card with action buttons.
 * Based on visual-prototype-zh.html .rcard + file management patterns.
 */
import type { GeneratedFile } from '@/types/message'
import { Button } from '@/components/common/Button'

interface GeneratedFileCardProps {
  file: GeneratedFile
  onAction?: (fileId: string, action: string) => void
}

const FILE_TYPE_ICON: Record<string, { label: string; bg: string; color: string }> = {
  excel: { label: 'XLS', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  xlsx: { label: 'XLS', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  xls: { label: 'XLS', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  html: { label: 'HTML', bg: 'var(--color-filetype-blue-bg)', color: 'var(--color-semantic-blue)' },
  pdf: { label: 'PDF', bg: 'var(--color-filetype-red-bg)', color: 'var(--color-semantic-red)' },
  csv: { label: 'CSV', bg: 'var(--color-filetype-green-bg)', color: 'var(--color-semantic-green)' },
  json: { label: 'JSON', bg: 'var(--color-filetype-gray-bg)', color: 'var(--color-text-muted)' },
  png: { label: 'PNG', bg: 'var(--color-filetype-purple-bg)', color: 'var(--color-semantic-purple)' },
  py: { label: 'PY', bg: 'var(--color-filetype-orange-bg)', color: 'var(--color-semantic-orange)' },
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function GeneratedFileCard({ file, onAction }: GeneratedFileCardProps) {
  const icon = FILE_TYPE_ICON[file.fileType] ?? FILE_TYPE_ICON.json

  return (
    <div
      className="my-2 flex items-center gap-3.5 rounded-lg border p-3.5"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
        opacity: file.isLatest ? 1 : 0.6,
      }}
    >
      {/* Type icon */}
      <div
        className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-xs font-bold"
        style={{ background: icon.bg, color: icon.color }}
      >
        {icon.label}
      </div>

      {/* Info */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span
            className="truncate text-base font-semibold"
            style={{ color: 'var(--color-text-primary)' }}
          >
            {file.fileName}
          </span>
          {!file.isLatest && (
            <span
              className="shrink-0 rounded-md px-1.5 py-0.5 text-xs font-medium"
              style={{
                background: 'var(--color-bg-neutral)',
                color: 'var(--color-text-muted)',
              }}
            >
              v{file.version}
            </span>
          )}
        </div>
        <div
          className="mt-0.5 text-sm"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {formatFileSize(file.fileSize)}
          {file.description && ` — ${file.description}`}
        </div>
      </div>

      {/* Actions — built-in Open + Open Folder, plus any extra from LLM */}
      <div className="flex shrink-0 items-center gap-1.5">
        <Button
          variant="ghost"
          className="!px-2 !py-1 !text-xs"
          onClick={() => onAction?.(file.id, 'open')}
        >
          Open
        </Button>
        <Button
          variant="ghost"
          className="!px-2 !py-1 !text-xs"
          onClick={() => onAction?.(file.id, 'reveal')}
        >
          Open Folder
        </Button>
        {(file.actions ?? [])
          .filter((a) => a.enabled && a.type !== 'open' && a.type !== 'reveal')
          .map((a) => (
            <Button
              key={a.type}
              variant="ghost"
              className="!px-2 !py-1 !text-xs"
              onClick={() => onAction?.(file.id, a.type)}
            >
              {a.label}
            </Button>
          ))}
      </div>
    </div>
  )
}
