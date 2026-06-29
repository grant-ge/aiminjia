import type { GeneratedFile } from '@/types/message'

export const ARTIFACT_ALT = 'artifact'

const ARTIFACT_MARK_RE = /!\[artifact\]\((.+?)\)/g

const ARTIFACT_EXT_TO_TYPE: Record<string, string> = {
  md: 'markdown',
  markdown: 'markdown',
  html: 'html',
  json: 'json',
  csv: 'csv',
  txt: 'txt',
  text: 'txt',
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  bmp: 'image',
  svg: 'image',
  xlsx: 'excel',
  xls: 'excel',
  docx: 'word',
  doc: 'word',
  pptx: 'ppt',
  ppt: 'ppt',
  pdf: 'pdf',
}

export interface ArtifactTarget {
  path: string
  fileName: string
  fileType?: string
}

export function decodeMarkdownUrlValue(value: string): string {
  try {
    return decodeURI(value)
  } catch {
    return value
  }
}

export function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

export function normalizeComparablePath(value: string): string {
  return decodeMarkdownUrlValue(value).replace(/\\/g, '/').replace(/^\.\//, '').replace(/\/+/g, '/')
}

export function isExternalHref(href: string): boolean {
  return /^(https?:|mailto:|ircs?:|xmpp:)/i.test(href)
}

export function isAbsoluteLocalPath(value: string): boolean {
  return value.startsWith('/') || /^[A-Za-z]:[\\/]/.test(value)
}

export function inferArtifactFileType(fileName: string): string | undefined {
  const ext = fileName.includes('.') ? fileName.split('.').pop()?.toLowerCase() : undefined
  return ext ? ARTIFACT_EXT_TO_TYPE[ext] : undefined
}

export function collectArtifactMarkdownPaths(text: string): string[] {
  const paths: string[] = []
  const protectedRanges = collectMarkdownCodeRanges(text)
  for (const match of text.matchAll(ARTIFACT_MARK_RE)) {
    const index = match.index ?? 0
    if (isIndexInRanges(index, protectedRanges)) continue
    paths.push(match[1].trim())
  }
  return paths
}

export function findGeneratedFileForArtifactPath(
  path: string | undefined,
  generatedFiles: GeneratedFile[] | undefined,
): GeneratedFile | null {
  if (!path || !generatedFiles?.length) return null
  const decoded = normalizeComparablePath(path.trim())
  if (!decoded || isExternalHref(decoded) || decoded.startsWith('file://')) return null
  const sourceName = basename(decoded)
  const isGeneratedReference = decoded.startsWith('generated/') || decoded.includes('/generated/')

  for (const file of generatedFiles) {
    const filePath = file.filePath?.trim()
    if (!filePath) continue
    const comparableFilePath = normalizeComparablePath(filePath)
    const fileName = file.fileName || basename(comparableFilePath)
    if (isAbsoluteLocalPath(decoded) && comparableFilePath === decoded) return file
    if (comparableFilePath.endsWith(`/${decoded}`)) return file
    if (isGeneratedReference && sourceName === fileName) return file
  }
  return null
}

function collectMarkdownCodeRanges(text: string): Array<[number, number]> {
  return mergeRanges([
    ...collectFencedCodeRanges(text),
    ...collectInlineCodeRanges(text),
  ])
}

function collectFencedCodeRanges(text: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = []
  const lineRe = /^(?:[ \t]*)(`{3,}|~{3,}).*$/gm
  let open: { start: number; marker: '`' | '~'; length: number } | null = null

  for (const match of text.matchAll(lineRe)) {
    const fence = match[1]
    const marker = fence[0] as '`' | '~'
    const length = fence.length
    const start = match.index ?? 0
    const lineEnd = start + match[0].length
    if (!open) {
      open = { start, marker, length }
      continue
    }
    if (open.marker === marker && length >= open.length) {
      ranges.push([open.start, lineEnd])
      open = null
    }
  }

  if (open) ranges.push([open.start, text.length])
  return ranges
}

function collectInlineCodeRanges(text: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = []
  const re = /`+/g
  let open: { start: number; ticks: string } | null = null

  for (const match of text.matchAll(re)) {
    const ticks = match[0]
    const index = match.index ?? 0
    if (!open) {
      open = { start: index, ticks }
      continue
    }
    if (open.ticks === ticks) {
      ranges.push([open.start, index + ticks.length])
      open = null
    }
  }

  return ranges
}

function mergeRanges(ranges: Array<[number, number]>): Array<[number, number]> {
  const sorted = ranges
    .filter(([start, end]) => end > start)
    .sort((a, b) => a[0] - b[0])
  const merged: Array<[number, number]> = []
  for (const range of sorted) {
    const previous = merged[merged.length - 1]
    if (previous && range[0] <= previous[1]) {
      previous[1] = Math.max(previous[1], range[1])
    } else {
      merged.push([...range])
    }
  }
  return merged
}

function isIndexInRanges(index: number, ranges: Array<[number, number]>): boolean {
  return ranges.some(([start, end]) => index >= start && index < end)
}
