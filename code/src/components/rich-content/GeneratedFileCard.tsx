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
      className="my-2 flex flex-col rounded-lg border"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: file.isDegraded ? 'var(--color-semantic-orange)' : 'var(--color-border)',
        opacity: file.isLatest ? 1 : 0.6,
      }}
    >
      <div className="flex items-center gap-3.5 p-3.5">
        {/* Type icon with degradation badge */}
        <div className="relative shrink-0">
          <div
            className="flex h-10 w-10 items-center justify-center rounded-lg text-xs font-bold"
            style={{ background: icon.bg, color: icon.color }}
          >
            {icon.label}
          </div>
          {file.isDegraded && (
            <div
              className="absolute -right-0.5 -top-0.5 h-2.5 w-2.5 rounded-full border-2"
              style={{
                background: 'var(--color-semantic-orange)',
                borderColor: 'var(--color-bg-card)',
              }}
            />
          )}
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
                className="shrink-0 rounded-full px-1.5 py-0.5 text-xs font-medium"
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

      {/* Degradation notice */}
      {file.isDegraded && file.degradationNotice && (
        <div
          className="border-t px-3.5 py-2 text-xs"
          style={{
            borderColor: 'var(--color-semantic-orange-border)',
            background: 'var(--color-semantic-orange-bg-light)',
            color: 'var(--color-semantic-orange)',
          }}
        >
          {file.degradationNotice}
          {file.requestedFormat && (
            <span style={{ color: 'var(--color-text-muted)' }}>
              {' '}(requested: {file.requestedFormat.toUpperCase()})
            </span>
          )}
        </div>
      )}
    </div>
  )
}
