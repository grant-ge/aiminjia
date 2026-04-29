import { useCallback, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'

import { saveClipboardImageToTmp } from '@/lib/tauri'
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
      return paths.map((p) => makePendingAttachment(p))
    } finally {
      setIsPickingAttachments(false)
    }
  }, [])

  const saveClipboardImage = useCallback(async (
    bytes: Uint8Array,
    mimeType: string,
  ): Promise<PendingAttachment> => {
    const saved: SavedClipboardAttachment = await saveClipboardImageToTmp(
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
    return paths.map((path) => {
      const hasExtension = /\.[A-Za-z0-9]+$/.test(path.split('/').pop() ?? '')
      const isDirectory = !hasExtension
      const fileType: FileAttachment['fileType'] = isDirectory ? 'folder' : detectAttachmentFileType(path)
      const attachment = makePendingAttachment(path, fileType)
      return {
        ...attachment,
        kind: isDirectory ? 'folder' : fileType === 'image' ? 'image' : 'file',
        fileType,
        fileSize: 0,
        source: 'paste' as const,
      }
    })
  }, [])

  return {
    isPickingAttachments,
    pickAttachments,
    resolvePastedPaths,
    saveClipboardImage,
  } as const
}
