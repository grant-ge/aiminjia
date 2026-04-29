import type { PendingAttachment } from '@/hooks/useChatAttachments'

function AttachmentIcon({ kind }: { kind: PendingAttachment['kind'] }) {
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

export function PendingAttachmentChips({
  pendingFiles,
  onRemove,
}: {
  pendingFiles: PendingAttachment[]
  onRemove: (id: string) => void
}) {
  return (
    <div className="relative -top-2 max-h-[80px] overflow-y-auto flex flex-wrap gap-1.5">
      {pendingFiles.map((file) => (
        <div
          key={file.id}
          className="inline-flex items-center gap-1.5 rounded-md px-2 py-1"
          style={{ background: 'var(--color-bg-subtle)', color: 'var(--color-text-secondary)' }}
        >
          <AttachmentIcon kind={file.kind} />
          <span className="max-w-[64px] truncate text-xs" style={{ color: 'var(--color-text-primary)' }}>
            {file.fileName}
          </span>
          <button
            type="button"
            className="flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-full border-none bg-transparent transition-colors hover:opacity-70"
            style={{ color: 'var(--color-text-muted)' }}
            onClick={() => onRemove(file.id)}
            aria-label="移除附件"
          >
            <svg className="h-2.5 w-2.5" viewBox="0 0 24 24" fill="currentColor">
              <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
            </svg>
          </button>
        </div>
      ))}
    </div>
  )
}
