import type { FileAction } from '@/types/message'

export type GeneratedFilePrimaryAction = 'preview' | 'open'

export interface PreviewTarget {
  fileId: string
  conversationId: string
  fileName: string
  fileType?: string
}

interface GeneratedFileActionSource {
  id?: string
  title?: string
  fileName?: string
  fileType?: string
}

const PREVIEWABLE_FILE_TYPES = new Set(['md', 'markdown', 'html', 'txt', 'text', 'json', 'csv'])

function normalizeFileType(fileType?: string): string | undefined {
  const normalized = fileType?.trim().toLowerCase()
  return normalized || undefined
}

function getFileExtension(fileName?: string): string | undefined {
  const name = fileName?.trim()
  if (!name) return undefined
  const extension = name.split('.').pop()
  return extension && extension !== name ? extension.toLowerCase() : undefined
}

export function isPreviewableFileType(fileType?: string, fileName?: string): boolean {
  const normalizedType = normalizeFileType(fileType) ?? getFileExtension(fileName)
  return normalizedType ? PREVIEWABLE_FILE_TYPES.has(normalizedType) : false
}

export function getGeneratedFilePrimaryAction(file: Pick<GeneratedFileActionSource, 'fileType' | 'title' | 'fileName'>): GeneratedFilePrimaryAction {
  return isPreviewableFileType(file.fileType, file.title ?? file.fileName) ? 'preview' : 'open'
}

export function isFileActionEnabled(actions: FileAction[] | undefined, actionType: FileAction['type']): boolean {
  if (!actions?.length) return true
  return actions.find((action) => action.type === actionType)?.enabled ?? false
}

export function toPreviewTarget(file: GeneratedFileActionSource & { id: string }, conversationId: string): PreviewTarget {
  return {
    fileId: file.id,
    conversationId,
    fileName: file.title ?? file.fileName ?? '未命名文件',
    fileType: file.fileType,
  }
}
