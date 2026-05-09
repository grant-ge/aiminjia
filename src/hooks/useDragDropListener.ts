import { useEffect } from 'react'

import { detectAttachmentFileType, makePendingAttachment } from '@/hooks/useChatAttachments'
import type { PendingAttachment } from '@/hooks/useChatAttachments'
import { useNotificationStore } from '@/stores/notificationStore'
import { useDropInbox } from '@/stores/dropInbox'

/**
 * App-level subscription to the Tauri webview's native drag-drop event.
 *
 * Why it lives outside the composer: Tauri's HTML5 drop is intercepted at the
 * webview layer, so React `onDrop` handlers never fire — the only reliable way
 * to receive native paths (with `C:\…` on Windows, `/Users/…` on macOS) is via
 * the webview-level callback. We dispatch into `useDropInbox` so whichever
 * composer is mounted (Home or Chat) drains it.
 *
 * Behavior on drop:
 * - Validate each path through the same gates as paste (Windows volume roots,
 *   macOS `/`, system folders all rejected).
 * - Emit a single toast with the count.
 * - Push resolved `PendingAttachment[]` into the drop inbox.
 *
 * The actual file copy into `workspace/uploads/` happens later when the user
 * sends the message — same path as the picker / paste flow — so dropping a
 * file is a zero-extra-IPC operation here.
 */
export function useDragDropListener() {
  useEffect(() => {
    let cancelled = false
    let unlistenFn: (() => void) | null = null

    void (async () => {
      const { getCurrentWebview } = await import('@tauri-apps/api/webview')
      try {
        const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
          if (cancelled) return
          if (event.payload.type !== 'drop') return
          const paths = event.payload.paths
          if (!paths || paths.length === 0) return

          const accepted: PendingAttachment[] = []
          for (const path of paths) {
            if (!isAcceptableDropPath(path)) continue
            const basename = path.split(/[\\/]/).pop() ?? ''
            const hasExtension = /\.[A-Za-z0-9]+$/.test(basename)
            const fileType = hasExtension
              ? detectAttachmentFileType(path)
              : 'folder'
            const attachment = makePendingAttachment(path, fileType)
            accepted.push({
              ...attachment,
              kind: !hasExtension ? 'folder' : fileType === 'image' ? 'image' : 'file',
              source: 'drop',
            })
          }

          if (accepted.length === 0) {
            useNotificationStore.getState().push({
              level: 'info',
              title: '已忽略拖入项',
              message: '系统目录或卷根路径不能作为附件，请选择具体文件。',
              actions: [],
              dismissible: true,
              autoHide: 4,
              context: 'toast',
            })
            return
          }

          useDropInbox.getState().push(accepted)
          useNotificationStore.getState().push({
            level: 'info',
            title: accepted.length === 1 ? '已加入对话' : `已加入 ${accepted.length} 个文件`,
            message: accepted.map((a) => a.fileName).slice(0, 3).join('、')
              + (accepted.length > 3 ? ` 等 ${accepted.length} 项` : ''),
            actions: [],
            dismissible: true,
            autoHide: 3,
            context: 'toast',
          })
        })
        if (cancelled) {
          unlisten()
        } else {
          unlistenFn = unlisten
        }
      } catch (err) {
        console.warn('[drag-drop] failed to subscribe:', err)
      }
    })()

    return () => {
      cancelled = true
      unlistenFn?.()
    }
  }, [])
}

function isAcceptableDropPath(path: string): boolean {
  if (!path) return false
  const trimmed = path.trim()
  if (!trimmed) return false

  const isWindowsPath = /^[A-Za-z]:[\\/]/.test(trimmed)

  if (isWindowsPath) {
    const stripped = trimmed.replace(/[\\/]+$/, '')
    if (/^[A-Za-z]:$/.test(stripped)) return false
    const lower = stripped.toLowerCase()
    if (
      lower.startsWith('c:\\windows')
      || lower.startsWith('c:/windows')
      || lower.startsWith('c:\\$recycle.bin')
      || lower.startsWith('c:/$recycle.bin')
      || lower.startsWith('c:\\program files')
      || lower.startsWith('c:/program files')
    ) {
      return false
    }
    return true
  }

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
