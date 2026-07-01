import { NodeViewWrapper } from '@tiptap/react'
import {
  File,
  FileJson,
  FileSpreadsheet,
  FileText,
  Folder,
  Image as ImageIcon,
  X,
} from 'lucide-react'
import type { ComposerAttachmentToken } from './types'
import { Tag } from '@/components/common/Tag'
import { Button } from '@/components/ui/button'

const FILE_TYPE_ICON = {
  excel: FileSpreadsheet,
  csv: FileSpreadsheet,
  word: FileText,
  pdf: FileText,
  json: FileJson,
  image: ImageIcon,
  folder: Folder,
}

interface AttachmentTokenViewProps {
  node: { attrs: ComposerAttachmentToken }
  deleteNode: () => void
}

export function AttachmentTokenView({ node, deleteNode }: AttachmentTokenViewProps) {
  const attrs = node.attrs
  const AttachmentIcon =
    attrs.kind === 'image'
      ? ImageIcon
      : attrs.kind === 'folder'
        ? Folder
        : (FILE_TYPE_ICON[attrs.fileType] ?? File)

  return (
    <NodeViewWrapper
      as="span"
      contentEditable={false}
    >
      <Tag
        data-attachment-chip
        size="sm"
        className="max-w-[180px] text-muted-foreground"
        icon={
          <AttachmentIcon
            aria-label={`${attrs.fileType} attachment`}
            aria-hidden={false}
            className="text-muted-foreground"
          />
        }
      >
        <span className="truncate">{attrs.fileName}</span>
        <Button unstyled
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
        </Button>
      </Tag>
    </NodeViewWrapper>
  )
}
