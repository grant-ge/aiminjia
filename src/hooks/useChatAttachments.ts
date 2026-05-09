import { useCallback, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'

import { saveClipboardImageToWorkspaceStaging } from '@/lib/tauri'
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
  // Split on both POSIX and Windows separators so `C:\Users\…\file.xlsx`
  // surfaces as `file.xlsx` instead of the whole path.
  const fileName = filePath.split(/[\\/]/).pop() ?? filePath
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

/**
 * Reject pathological paths that should never be attached:
 * - empty string
 * - root-only paths and well-known system roots on macOS/Linux ("/", "/Volumes",
 *   "/System", "/private", "/var", "/etc", "/dev", "/usr", "/bin", "/sbin",
 *   "/Library"); these tend to be alias-resolution artifacts (e.g. macOS
 *   "Macintosh HD" → "/") that would attach the entire disk.
 * - Windows volume roots ("C:\", "D:\") and Recycle Bin / system folders.
 *
 * The path may use either `/` (POSIX) or `\` (Windows) separators.
 */
function isAcceptablePastedPath(path: string): boolean {
  if (!path) return false
  const trimmed = path.trim()
  if (!trimmed) return false

  // Detect Windows path: drive letter + `:\` or `:/`.
  const isWindowsPath = /^[A-Za-z]:[\\/]/.test(trimmed)

  if (isWindowsPath) {
    // Reject bare volume root ("C:\" or "D:/").
    const stripped = trimmed.replace(/[\\/]+$/, '')
    if (/^[A-Za-z]:$/.test(stripped)) return false
    // Reject trash / system folders that would attach huge / dangerous trees.
    const lower = stripped.toLowerCase()
    const FORBIDDEN_PREFIXES = ['c:\\windows', 'c:\\$recycle.bin', 'c:\\program files']
    if (FORBIDDEN_PREFIXES.some((p) => lower.startsWith(p))) return false
    return true
  }

  // POSIX path
  if (!trimmed.startsWith('/')) return false
  const normalized = trimmed.replace(/\/+$/, '')
  if (normalized === '') return false
  const FORBIDDEN = new Set([
    '', '/Volumes', '/System', '/private', '/var', '/etc', '/dev', '/cores',
    '/usr', '/bin', '/sbin', '/Library',
  ])
  if (FORBIDDEN.has(normalized)) return false
  return true
}

export function useChatAttachments() {
  const [isPickingAttachments, setIsPickingAttachments] = useState(false)

  const pickAttachments = useCallback(async (): Promise<PendingAttachment[]> => {
    setIsPickingAttachments(true)
    try {
      const selected = await open({
        multiple: true,
        directory: false,
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
    const saved: SavedClipboardAttachment = await saveClipboardImageToWorkspaceStaging(
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
    return paths
      .filter((path) => isAcceptablePastedPath(path))
      .map((path) => {
        const basename = path.split(/[\\/]/).pop() ?? ''
        const hasExtension = /\.[A-Za-z0-9]+$/.test(basename)
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
