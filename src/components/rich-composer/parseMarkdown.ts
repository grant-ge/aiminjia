import type { ComposerJsonNode, ComposerAttachmentToken } from './types'

const ATTACHMENT_RE = /(!?)\[(?:附件: )?([^\]]+)\]\(file:\/\/([^)]+)\)/g

const EXT_TO_FILE_TYPE: Record<string, ComposerAttachmentToken['fileType']> = {
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  svg: 'image',
  pdf: 'pdf',
  xlsx: 'excel',
  xls: 'excel',
  doc: 'word',
  docx: 'word',
  json: 'json',
  csv: 'csv',
}

function inferFileType(fileName: string): ComposerAttachmentToken['fileType'] {
  const ext = fileName.toLowerCase().split('.').pop() ?? ''
  return EXT_TO_FILE_TYPE[ext] ?? 'pdf'
}

let counter = 0
function genId(): string {
  counter += 1
  return `prefill-${Date.now().toString(36)}-${counter}`
}

function buildAttachmentTokenAttrs(
  isImage: boolean,
  fileName: string,
  path: string,
): ComposerAttachmentToken {
  if (isImage) {
    return {
      id: genId(),
      fileName,
      path,
      kind: 'image',
      fileType: 'image',
      fileSize: 0,
      source: 'paste',
    }
  }
  return {
    id: genId(),
    fileName,
    path,
    kind: 'file',
    fileType: inferFileType(fileName),
    fileSize: 0,
    source: 'paste',
  }
}

function parseInline(line: string): ComposerJsonNode[] {
  const out: ComposerJsonNode[] = []
  let lastIndex = 0
  ATTACHMENT_RE.lastIndex = 0
  let match: RegExpExecArray | null
  while ((match = ATTACHMENT_RE.exec(line)) !== null) {
    const [whole, bang, fileName, rawPath] = match
    if (match.index > lastIndex) {
      out.push({ type: 'text', text: line.slice(lastIndex, match.index) })
    }
    out.push({
      type: 'attachmentToken',
      attrs: { ...buildAttachmentTokenAttrs(bang === '!', fileName, rawPath) },
    })
    lastIndex = match.index + whole.length
  }
  if (lastIndex < line.length) {
    out.push({ type: 'text', text: line.slice(lastIndex) })
  }
  return out
}

function parseLineWithBreaks(line: string): ComposerJsonNode[] {
  // markdown soft break: line ending with two spaces + \n → hardBreak.
  // We also fall back to single \n → hardBreak for robustness (composer prefill
  // is intentionally permissive — see plan).
  const segments = line.split(/  ?\n|\n/)
  const out: ComposerJsonNode[] = []
  segments.forEach((seg, idx) => {
    out.push(...parseInline(seg))
    if (idx < segments.length - 1) out.push({ type: 'hardBreak' })
  })
  return out
}

export function parseMarkdownToComposerJson(markdown: string): ComposerJsonNode {
  if (markdown.length === 0) {
    return { type: 'doc', content: [{ type: 'paragraph' }] }
  }
  const paragraphs = markdown.split(/\n\n+/)
  const content: ComposerJsonNode[] = paragraphs.map((para) => {
    const inline = parseLineWithBreaks(para)
    return inline.length > 0
      ? { type: 'paragraph', content: inline }
      : { type: 'paragraph' }
  })
  return { type: 'doc', content }
}
