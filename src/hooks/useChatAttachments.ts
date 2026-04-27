import { useCallback, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { stat } from '@tauri-apps/plugin-fs'

import { saveClipboardImageAttachment } from '@/lib/tauri'
import type { FileAttachment } from '@/types/message'

export interface PendingAttachment {
  id: string
  fileName: string
  path: string
  kind: 'file' | 'folder' | 'image'
  fileType: FileAttachment['fileType']
  fileSize: number
  mimeType?: string
  source: 'picker' | 'paste' | 'drop' | 'clipboard-image'
}

export interface SavedClipboardAttachment {
  fileName: string
  path: string
  fileSize: number
  mimeType: string
}

const ALLOWED_EXTENSIONS = [
  'xlsx', 'xls', 'csv', 'pdf', 'docx', 'doc', 'json',
  'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg',
]

export function detectAttachmentFileType(path: string): FileAttachment['fileType'] {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  switch (ext) {
    case 'png':
    case 'jpg':
    case 'jpeg':
    case 'gif':
    case 'webp':
    case 'bmp':
    case 'svg':
      return 'image'
    case 'xlsx':
    case 'xls':
      return 'excel'
    case 'docx':
    case 'doc':
      return 'word'
    case 'pdf':
      return 'pdf'
    case 'json':
      return 'json'
    default:
      return 'csv'
  }
}

export function makePendingAttachment(filePath: string, fileType?: FileAttachment['fileType']): PendingAttachment {
  const fileName = filePath.split('/').pop() ?? filePath
  return {
    id: filePath,
    fileName,
    path: filePath,
    kind: fileType === 'folder' ? 'folder' : fileType === 'image' ? 'image' : 'file',
    fileSize: 0,
    fileType: fileType ?? detectAttachmentFileType(filePath),
    mimeType: undefined,
    source: 'picker',
  }
}

export function useChatAttachments() {
  const [isPickingAttachments, setIsPickingAttachments] = useState(false)

  const pickAttachments = useCallback(async (): Promise<PendingAttachment[]> => {
    setIsPickingAttachments(true)
    try {
      const selected = await open({
        multiple: true,
        directory: false,
        filters: [{
          name: 'Supported Files',
          extensions: ALLOWED_EXTENSIONS,
        }],
      })

      if (!selected) return []
      const paths = Array.isArray(selected) ? selected : [selected]
      return paths.map(makePendingAttachment)
    } finally {
      setIsPickingAttachments(false)
    }
  }, [])

  const saveClipboardImage = useCallback(async (
    conversationId: string,
    bytes: Uint8Array,
    mimeType: string,
  ): Promise<PendingAttachment> => {
    const saved: SavedClipboardAttachment = await saveClipboardImageAttachment(
      conversationId,
      Array.from(bytes),
      mimeType,
    )
    return {
      id: saved.path,
      fileName: saved.fileName,
      path: saved.path,
      kind: 'image',
      fileType: 'image',
      fileSize: saved.fileSize,
      mimeType: saved.mimeType,
      source: 'clipboard-image',
    }
  }, [])

  const resolvePastedPaths = useCallback(async (paths: string[]): Promise<PendingAttachment[]> => {
    const resolved = await Promise.all(paths.map(async (path) => {
      try {
        const info = await stat(path)
        const attachment = makePendingAttachment(path, info.isDirectory ? 'folder' : detectAttachmentFileType(path))
        return {
          ...attachment,
          kind: info.isDirectory ? 'folder' : attachment.fileType === 'image' ? 'image' : 'file',
          fileType: info.isDirectory ? 'folder' : attachment.fileType,
          fileSize: info.isDirectory ? 0 : info.size,
          source: 'paste' as const,
        }
      } catch (error) {
        console.warn('[useChatAttachments] resolvePastedPaths skipped path:', path, error)
        return null
      }
    }))

    return resolved.filter((attachment): attachment is PendingAttachment => attachment !== null)
  }, [])

  return {
    isPickingAttachments,
    pickAttachments,
    resolvePastedPaths,
    saveClipboardImage,
  } as const
}
