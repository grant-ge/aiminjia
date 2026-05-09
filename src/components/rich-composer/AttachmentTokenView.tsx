import { NodeViewWrapper } from '@tiptap/react'
import { Image as ImageIcon, Folder, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { ComposerAttachmentToken } from './types'

const FILE_TYPE_LABEL: Partial<Record<ComposerAttachmentToken['fileType'], string>> = {
  excel: 'XLS',
  csv: 'CSV',
  word: 'DOC',
  pdf: 'PDF',
  json: 'JSON',
  image: 'IMG',
}

interface AttachmentTokenViewProps {
  node: { attrs: ComposerAttachmentToken }
  deleteNode: () => void
}

export function AttachmentTokenView({ node, deleteNode }: AttachmentTokenViewProps) {
  const attrs = node.attrs
  return (
    <NodeViewWrapper
      as="span"
      data-attachment-chip
      contentEditable={false}
      className={cn(
        'inline-flex max-w-[180px] items-center gap-1 rounded-md border border-border bg-muted px-1.5 py-0.5 align-middle text-xs leading-none text-foreground',
      )}
    >
      {attrs.kind === 'image' ? (
        <ImageIcon aria-label="image attachment" className="h-3.5 w-3.5 shrink-0" />
      ) : attrs.kind === 'folder' ? (
        <Folder aria-label="folder attachment" className="h-3.5 w-3.5 shrink-0" />
      ) : (
        <span className="shrink-0 rounded bg-background px-1 text-[10px] font-bold text-muted-foreground">
          {FILE_TYPE_LABEL[attrs.fileType] ?? 'FILE'}
        </span>
      )}
      <span className="truncate">{attrs.fileName}</span>
      <button
        type="button"
        aria-label="remove attachment"
        // mousedown inside contentEditable=false re-positions the editor selection before
        // click fires; preventing default keeps the cursor where the user already had it.
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          e.preventDefault()
          e.stopPropagation()
          deleteNode()
        }}
        className="ml-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded hover:bg-background"
      >
        <X className="h-3 w-3" />
      </button>
    </NodeViewWrapper>
  )
}
