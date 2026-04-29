import { useCallback, type ClipboardEvent } from 'react'

import { readClipboardFilePaths } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'

import { useChatAttachments, type PendingAttachment } from './useChatAttachments'

const MAX_PASTED_PATHS = 50

// macOS WebKit will hang the main thread if a React onPaste handler reads
// `clipboardData.types` / `.items` in the bubble phase when the clipboard
// contains Finder file references (e.g. an alias to "Macintosh HD"). The
// access in capture phase is safe though, so a single document-level capture
// listener snapshots the types into module state and the React handler reads
// the snapshot instead of touching clipboardData at all.
let lastPasteHasFile = false

function snapshotPaste(e: globalThis.ClipboardEvent) {
  try {
    const types = Array.from(e.clipboardData?.types ?? [])
    lastPasteHasFile = types.some(
      (t) => t === 'Files' || t === 'text/uri-list' || t.startsWith('public.file-url'),
    )
  } catch {
    lastPasteHasFile = false
  }
}

let snapshotterInstalled = false
function ensureSnapshotterInstalled() {
  if (snapshotterInstalled || typeof document === 'undefined') return
  snapshotterInstalled = true
  document.addEventListener('paste', snapshotPaste, true)
}

function pushToast(level: 'info', title: string, message: string) {
  useNotificationStore.getState().push({
    level,
    title,
    message,
    actions: [],
    dismissible: true,
    autoHide: 4,
    context: 'toast',
  })
}

export interface UseComposerPasteParams {
  onAttachmentsResolved: (attachments: PendingAttachment[]) => void
}

export function useComposerPaste({ onAttachmentsResolved }: UseComposerPasteParams) {
  ensureSnapshotterInstalled()
  const { resolvePastedPaths } = useChatAttachments()

  const handlePaste = useCallback((event: ClipboardEvent<HTMLTextAreaElement>) => {
    if (!lastPasteHasFile) return // Plain text paste — let the textarea handle it.

    event.preventDefault()

    void (async () => {
      const nativePaths = await readClipboardFilePaths().catch(() => [] as string[])
      if (nativePaths.length === 0) {
        pushToast('info', '无法粘贴', '剪贴板中的文件类型暂不支持作为附件粘贴，请改用左下角"+"按钮选择文件。')
        return
      }
      const capped = nativePaths.slice(0, MAX_PASTED_PATHS)
      const resolved = await resolvePastedPaths(capped)
      if (resolved.length === 0) {
        pushToast('info', '无法粘贴', '选中的项目（如磁盘根目录、系统目录或别名）不支持作为附件粘贴。')
        return
      }
      if (resolved.length < capped.length) {
        pushToast(
          'info',
          '部分项目已忽略',
          `已忽略 ${capped.length - resolved.length} 个不支持的项目（如磁盘根目录、系统目录或别名）。`,
        )
      }
      onAttachmentsResolved(resolved)
    })()
  }, [resolvePastedPaths, onAttachmentsResolved])

  return { handlePaste }
}

export type { PendingAttachment }
