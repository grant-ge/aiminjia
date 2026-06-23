import type { PreviewTarget } from './generatedFileActions'
import { saveGeneratedFileAs, saveLocalFileAs } from '@/lib/tauri'

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'svg'])

function normalizeExtension(value?: string): string | null {
  const ext = value?.trim().toLowerCase().replace(/^\./, '')
  if (!ext || !/^[a-z0-9]+$/.test(ext)) return null
  if (ext === 'image') return null
  return ext
}

function fileNameExtension(fileName: string): string | null {
  const lastDot = fileName.lastIndexOf('.')
  if (lastDot <= 0 || lastDot === fileName.length - 1) return null
  return normalizeExtension(fileName.slice(lastDot + 1))
}

export function suggestedDownloadFileName(target: Pick<PreviewTarget, 'fileName' | 'fileType'>): string {
  const baseName = target.fileName.split(/[\\/]/).pop()?.trim() || 'download'
  if (fileNameExtension(baseName)) return baseName

  const ext = normalizeExtension(target.fileType)
  if (!ext) return baseName
  return `${baseName}.${ext === 'jpeg' ? 'jpg' : ext}`
}

function saveDialogFilters(fileName: string) {
  const ext = fileNameExtension(fileName)
  if (!ext) return undefined
  return [
    {
      name: IMAGE_EXTENSIONS.has(ext) ? 'Images' : ext.toUpperCase(),
      extensions: [ext],
    },
  ]
}

export async function savePreviewTargetToDisk(target: PreviewTarget): Promise<string | null> {
  const fileName = suggestedDownloadFileName(target)
  const { save } = await import('@tauri-apps/plugin-dialog')
  const destinationPath = await save({
    defaultPath: fileName,
    filters: saveDialogFilters(fileName),
  })
  if (!destinationPath) return null

  if (target.localPath) {
    return saveLocalFileAs(target.localPath, destinationPath)
  }

  if (!target.fileId || !target.conversationId) {
    throw new Error('生成文件缺少所属对话，无法下载。')
  }

  return saveGeneratedFileAs(target.fileId, target.conversationId, destinationPath)
}
