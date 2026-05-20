// src/components/rich-composer/types.ts

export type ComposerAttachmentTokenKind = 'file' | 'folder' | 'image'

export type ComposerAttachmentTokenFileType =
  | 'image'
  | 'excel'
  | 'word'
  | 'pdf'
  | 'json'
  | 'csv'
  | 'folder'

export interface ComposerAttachmentToken {
  id: string
  fileName: string
  path: string
  kind: ComposerAttachmentTokenKind
  fileType: ComposerAttachmentTokenFileType
  fileSize: number
  mimeType?: string
  source: 'picker' | 'paste' | 'drop' | 'clipboard-image'
}

export interface RichComposerSubmitPayload {
  markdown: string
  attachments: ComposerAttachmentToken[]
  isEmpty: boolean
}

export type ComposerMarkType = 'bold' | 'italic' | 'code' | 'strike' | 'link'

export interface ComposerMark {
  type: ComposerMarkType
  attrs?: { href?: string; [k: string]: unknown }
}

export type ComposerJsonNodeType =
  | 'doc'
  | 'paragraph'
  | 'text'
  | 'hardBreak'
  | 'blockquote'
  | 'codeBlock'
  | 'bulletList'
  | 'orderedList'
  | 'listItem'
  | 'attachmentToken'
  | 'linkChip'

export interface ComposerJsonNode {
  type: ComposerJsonNodeType
  content?: ComposerJsonNode[]
  text?: string
  marks?: ComposerMark[]
  attrs?: Record<string, unknown>
}
