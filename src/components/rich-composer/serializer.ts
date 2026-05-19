// src/components/rich-composer/serializer.ts
import type {
  ComposerAttachmentToken,
  ComposerJsonNode,
  ComposerSkillToken,
  ComposerMark,
  RichComposerSubmitPayload,
} from './types'

export function serializeComposerDoc(doc: ComposerJsonNode): RichComposerSubmitPayload {
  const attachments: ComposerAttachmentToken[] = []
  const skills: ComposerSkillToken[] = []
  const markdown = renderBlocks(doc.content ?? [], attachments, skills)
  const isEmpty = markdown.trim().length === 0 && attachments.length === 0 && skills.length === 0
  return { markdown, attachments, skills, isEmpty }
}

function renderBlocks(
  nodes: ComposerJsonNode[],
  attachments: ComposerAttachmentToken[],
  skills: ComposerSkillToken[],
): string {
  const parts: string[] = []
  for (const node of nodes) {
    parts.push(renderBlock(node, attachments, skills))
  }
  return parts.filter((s) => s.length > 0).join('\n\n')
}

function renderBlock(
  node: ComposerJsonNode,
  attachments: ComposerAttachmentToken[],
  skills: ComposerSkillToken[],
): string {
  switch (node.type) {
    case 'paragraph':
      return renderInline(node.content ?? [], attachments, skills)
    case 'blockquote':
      return renderBlockquote(node, attachments, skills)
    case 'codeBlock':
      return renderCodeBlock(node)
    case 'bulletList':
      return renderList(node, attachments, skills, false)
    case 'orderedList':
      return renderList(node, attachments, skills, true)
    default:
      return ''
  }
}

function renderBlockquote(
  node: ComposerJsonNode,
  attachments: ComposerAttachmentToken[],
  skills: ComposerSkillToken[],
): string {
  const inner = renderBlocks(node.content ?? [], attachments, skills)
  return inner
    .split('\n')
    .map((line) => (line.length === 0 ? '>' : '> ' + line))
    .join('\n')
}

function renderCodeBlock(node: ComposerJsonNode): string {
  const language = typeof node.attrs?.language === 'string' ? node.attrs.language : ''
  const text = (node.content ?? [])
    .filter((c) => c.type === 'text')
    .map((c) => c.text ?? '')
    .join('')
  return '```' + language + '\n' + text + '\n```'
}

function renderList(
  node: ComposerJsonNode,
  attachments: ComposerAttachmentToken[],
  skills: ComposerSkillToken[],
  ordered: boolean,
): string {
  const items = node.content ?? []
  return items
    .map((item, idx) => {
      const marker = ordered ? `${idx + 1}. ` : '- '
      const itemText = renderBlocks(item.content ?? [], attachments, skills)
      return itemText
        .split('\n')
        .map((line, lineIdx) => indentListLine(line, lineIdx, marker))
        .join('\n')
    })
    .join('\n')
}

function indentListLine(line: string, lineIdx: number, marker: string): string {
  if (lineIdx === 0) return marker + line
  if (line.length === 0) return ''
  return '    ' + line
}

const MARK_ORDER: Array<'code' | 'bold' | 'italic' | 'strike' | 'link'> = [
  'code',
  'bold',
  'italic',
  'strike',
  'link',
]

function renderInline(
  nodes: ComposerJsonNode[],
  attachments: ComposerAttachmentToken[],
  skills: ComposerSkillToken[],
): string {
  const parts: string[] = []
  for (const node of nodes) {
    if (node.type === 'text') {
      parts.push(renderText(node))
    } else if (node.type === 'hardBreak') {
      parts.push('  \n')
    } else if (node.type === 'attachmentToken') {
      parts.push(renderAttachmentToken(node, attachments))
    } else if (node.type === 'skillToken') {
      collectSkillToken(node, skills)
    }
  }
  return parts.join('')
}

function renderText(node: ComposerJsonNode): string {
  const raw = (node.text ?? '').replace(/\u200B/g, '')
  const marks = node.marks ?? []
  const hasCode = marks.some((m) => m.type === 'code')
  // text inside `code` mark must not be markdown-escaped (it's verbatim)
  let result = hasCode ? raw : escapeMarkdownText(raw)
  for (const markType of MARK_ORDER) {
    const mark = marks.find((m) => m.type === markType)
    if (!mark) continue
    result = wrapMark(result, markType, mark)
  }
  return result
}

function wrapMark(
  text: string,
  type: 'code' | 'bold' | 'italic' | 'strike' | 'link',
  mark: ComposerMark,
): string {
  switch (type) {
    case 'code':
      return '`' + text + '`'
    case 'bold':
      return '**' + text + '**'
    case 'italic':
      return '*' + text + '*'
    case 'strike':
      return '~~' + text + '~~'
    case 'link': {
      const href = typeof mark.attrs?.href === 'string' ? mark.attrs.href : ''
      return '[' + text + '](' + escapeUrl(href) + ')'
    }
  }
}

function escapeMarkdownText(text: string): string {
  return text.replace(/([\\*_`~[\]<>])/g, '\\$1')
}

function escapeUrl(url: string): string {
  // [ ] are valid in file paths but break the markdown link syntax `[text](url)`.
  return url.replace(/([\\()[\]])/g, '\\$1')
}

function renderAttachmentToken(
  node: ComposerJsonNode,
  attachments: ComposerAttachmentToken[],
): string {
  const token = readAttachmentTokenAttrs(node)
  if (!token) return ''
  if (!attachments.some((existing) => existing.id === token.id)) {
    attachments.push(token)
  }
  const safeName = escapeMarkdownLinkText(token.fileName)
  // Wrap URL in <...> so spaces/CJK/special chars don't break CommonMark
  // link parsing. Inside <...>, only `<` `>` and `\` need escaping.
  const safePath = escapeAngleBracketedUrl(toFileUrl(token.path))
  if (token.kind === 'image') {
    return `![${safeName}](<${safePath}>)`
  }
  return `[附件: ${safeName}](<${safePath}>)`
}

function escapeAngleBracketedUrl(url: string): string {
  return url.replace(/([\\<>])/g, '\\$1')
}

function readAttachmentTokenAttrs(node: ComposerJsonNode): ComposerAttachmentToken | null {
  const attrs = node.attrs ?? {}
  const id = typeof attrs.id === 'string' ? attrs.id : null
  const fileName = typeof attrs.fileName === 'string' ? attrs.fileName : null
  const path = typeof attrs.path === 'string' ? attrs.path : null
  const kind =
    attrs.kind === 'image' || attrs.kind === 'folder' || attrs.kind === 'file'
      ? attrs.kind
      : null
  const fileType =
    typeof attrs.fileType === 'string'
      ? (attrs.fileType as ComposerAttachmentToken['fileType'])
      : null
  const fileSize = typeof attrs.fileSize === 'number' ? attrs.fileSize : null
  const source =
    attrs.source === 'picker' ||
    attrs.source === 'paste' ||
    attrs.source === 'drop' ||
    attrs.source === 'clipboard-image'
      ? attrs.source
      : null
  if (!id || !fileName || !path || !kind || !fileType || fileSize === null || !source) {
    return null
  }
  const mimeType = typeof attrs.mimeType === 'string' ? attrs.mimeType : undefined
  return { id, fileName, path, kind, fileType, fileSize, source, mimeType }
}



function collectSkillToken(node: ComposerJsonNode, skills: ComposerSkillToken[]): void {
  const token = readSkillTokenAttrs(node)
  if (!token) return
  if (!skills.some((existing) => existing.id === token.id)) {
    skills.push(token)
  }
}

function readSkillTokenAttrs(node: ComposerJsonNode): ComposerSkillToken | null {
  const attrs = node.attrs ?? {}
  const id = typeof attrs.id === 'string' ? attrs.id : null
  const label = typeof attrs.label === 'string' ? attrs.label : null
  const command = typeof attrs.command === 'string' ? attrs.command : null
  if (!id || !label || !command) return null
  return { id, label, command }
}

function escapeMarkdownLinkText(text: string): string {
  return text.replace(/([\\[\]])/g, '\\$1')
}

function toFileUrl(path: string): string {
  // Unix abs path: '/abs/x' → 'file:///abs/x' (the '//' prefix + leading '/' yields three slashes).
  // Windows abs path: 'C:\foo\bar' → 'file:///C:/foo/bar' (third slash before drive letter, backslashes → forward slashes).
  if (/^[A-Za-z]:[\\/]/.test(path)) {
    return 'file:///' + path.replace(/\\/g, '/')
  }
  return 'file://' + path
}
