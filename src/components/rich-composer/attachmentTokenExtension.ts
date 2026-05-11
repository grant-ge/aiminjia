import { Node, mergeAttributes } from '@tiptap/core'
import { ReactNodeViewRenderer } from '@tiptap/react'
import type { ReactNodeViewProps } from '@tiptap/react'
import type { ComponentType } from 'react'
import type { ComposerAttachmentToken } from './types'
import { AttachmentTokenView } from './AttachmentTokenView'

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    attachmentToken: {
      insertAttachmentTokens: (tokens: ComposerAttachmentToken[]) => ReturnType
    }
  }
}

const DATA_ATTR = 'data-rich-composer-attachment-token'
const CARET_BOUNDARY = '\u200B'

function readNumber(value: unknown): number | null {
  if (typeof value === 'number') return Number.isFinite(value) ? value : null
  if (typeof value === 'string') {
    const n = Number(value)
    return Number.isFinite(n) ? n : null
  }
  return null
}

function readKind(value: unknown): ComposerAttachmentToken['kind'] | null {
  return value === 'file' || value === 'folder' || value === 'image' ? value : null
}

function readSource(value: unknown): ComposerAttachmentToken['source'] | null {
  return value === 'picker' ||
    value === 'paste' ||
    value === 'drop' ||
    value === 'clipboard-image'
    ? value
    : null
}

const FILE_TYPES: ReadonlyArray<ComposerAttachmentToken['fileType']> = [
  'image',
  'excel',
  'word',
  'pdf',
  'json',
  'csv',
  'folder',
]

function readFileType(value: unknown): ComposerAttachmentToken['fileType'] | null {
  return typeof value === 'string' && (FILE_TYPES as ReadonlyArray<string>).includes(value)
    ? (value as ComposerAttachmentToken['fileType'])
    : null
}

type AttachmentNodeAttrs = {
  id: string | null
  fileName: string | null
  path: string | null
  kind: ComposerAttachmentToken['kind'] | null
  fileType: ComposerAttachmentToken['fileType'] | null
  fileSize: number | null
  mimeType: string | null
  source: ComposerAttachmentToken['source'] | null
}

export const AttachmentTokenExtension = Node.create({
  name: 'attachmentToken',
  group: 'inline',
  inline: true,
  atom: true,
  // selectable=false: ArrowLeft/ArrowRight 跨过 chip 时直接落到文本光标，
  // 不再创建 NodeSelection（避免单按一次方向键就选中整个 chip）。
  selectable: false,
  draggable: true,

  addAttributes() {
    return {
      id: { default: null },
      fileName: { default: null },
      path: { default: null },
      kind: { default: null },
      fileType: { default: null },
      fileSize: { default: null },
      mimeType: { default: null },
      source: { default: null },
    }
  },

  parseHTML() {
    return [
      {
        tag: `span[${DATA_ATTR}]`,
        getAttrs: (el) => {
          if (!(el instanceof HTMLElement)) return false
          const id = el.getAttribute('data-id')
          const fileName = el.getAttribute('data-file-name')
          const path = el.getAttribute('data-path')
          const kind = readKind(el.getAttribute('data-kind'))
          const fileType = readFileType(el.getAttribute('data-file-type'))
          const fileSize = readNumber(el.getAttribute('data-file-size'))
          const source = readSource(el.getAttribute('data-source'))
          if (!id || !fileName || !path || !kind || !fileType || fileSize === null || !source) {
            return false
          }
          const mimeType = el.getAttribute('data-mime-type')
          return {
            id,
            fileName,
            path,
            kind,
            fileType,
            fileSize,
            source,
            mimeType: mimeType ?? null,
          }
        },
      },
    ]
  },

  renderHTML({ HTMLAttributes, node }) {
    const attrs = node.attrs as AttachmentNodeAttrs
    // If any required attr is missing the node cannot round-trip — render an empty
    // span so parseHTML rejects it on re-parse, matching the same contract.
    if (
      !attrs.id ||
      !attrs.fileName ||
      !attrs.path ||
      !attrs.kind ||
      !attrs.fileType ||
      attrs.fileSize === null ||
      !attrs.source
    ) {
      return ['span', mergeAttributes(HTMLAttributes, { [DATA_ATTR]: '' })]
    }
    const dataset: Record<string, string> = {
      [DATA_ATTR]: '',
      'data-id': attrs.id,
      'data-file-name': attrs.fileName,
      'data-path': attrs.path,
      'data-kind': attrs.kind,
      'data-file-type': attrs.fileType,
      'data-file-size': String(attrs.fileSize),
      'data-source': attrs.source,
    }
    if (attrs.mimeType) dataset['data-mime-type'] = attrs.mimeType
    return ['span', mergeAttributes(HTMLAttributes, dataset)]
  },

  addNodeView() {
    // AttachmentTokenView only declares { node, deleteNode } — the extra
    // ReactNodeViewProps fields are safely ignored by React at runtime. We cast via
    // unknown to satisfy TypeScript without modifying the view component.
    // The view component must use NodeViewWrapper as its root element (TipTap 3.x
    // requires this; otherwise the renderer throws and the insert is rolled back).
    return ReactNodeViewRenderer(
      AttachmentTokenView as unknown as ComponentType<ReactNodeViewProps>,
    )
  },

  addCommands() {
    return {
      insertAttachmentTokens:
        (tokens: ComposerAttachmentToken[]) =>
        ({ chain, state }) => {
          if (!tokens.length) return false
          let c = chain()
          if (state.selection.$from.parentOffset === 0) {
            c = c.insertContent({ type: 'text', text: CARET_BOUNDARY })
          }
          tokens.forEach((token, idx) => {
            c = c.insertContent({ type: 'attachmentToken', attrs: token })
            if (idx < tokens.length - 1) {
              c = c.insertContent({ type: 'text', text: ' ' })
            }
          })
          return c.run()
        },
    }
  },
})
