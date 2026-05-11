import { useEffect } from 'react'

import { invoke } from '@tauri-apps/api/core'
import { detectAttachmentFileType, makePendingAttachment } from '@/hooks/useChatAttachments'
import type { PendingAttachment } from '@/hooks/useChatAttachments'
import { useNotificationStore } from '@/stores/notificationStore'
import { useDropInbox } from '@/stores/dropInbox'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'

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

          // 拦截 .aijia-skill 文件 → 走导入流程，不进 attachment 链路
          const skillPackages = paths.filter((p) => /\.aijia-skill$/i.test(p))
          if (skillPackages.length > 0) {
            void importSkillPackagesWithUI(skillPackages)
            // 如果全是 .aijia-skill，直接 return；否则继续处理剩下的当作普通附件
            const remaining = paths.filter((p) => !/\.aijia-skill$/i.test(p))
            if (remaining.length === 0) return
            // fallthrough: 处理其它附件路径（用 remaining 替代 paths）
            const accepted: PendingAttachment[] = []
            for (const path of remaining) {
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
            if (accepted.length > 0) useDropInbox.getState().push(accepted)
            return
          }

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

type ImportOutcome =
  | { status: 'installed'; id: string; name: string; version: string; installed_to: string }
  | { status: 'conflict'; id: string; name: string; version: string; existing_path: string }

export async function importSkillPackagesWithUI(paths: string[]): Promise<void> {
  const pushNotif = useNotificationStore.getState().push
  for (const path of paths) {
    try {
      const outcome = await invoke<ImportOutcome>('import_skill_package', {
        archivePath: path,
        force: false,
      })
      if (outcome.status === 'installed') {
        pushNotif({
          level: 'success',
          title: '技能已导入',
          message: `${outcome.name} v${outcome.version} 已安装`,
          actions: [],
          dismissible: true,
          autoHide: 4,
          context: 'toast',
        })
        continue
      }
      // conflict → 询问用户
      const ok = await requestConfirm({
        title: `覆盖已有技能「${outcome.name}」？`,
        description: `已存在 id=${outcome.id}。覆盖将替换现有版本，无法撤销。`,
        confirmLabel: '覆盖',
        variant: 'destructive',
      })
      if (!ok) {
        pushNotif({
          level: 'info',
          title: '导入已取消',
          message: `${outcome.name} 未安装（保留现有版本）`,
          actions: [],
          dismissible: true,
          autoHide: 3,
          context: 'toast',
        })
        continue
      }
      const forced = await invoke<ImportOutcome>('import_skill_package', {
        archivePath: path,
        force: true,
      })
      if (forced.status === 'installed') {
        pushNotif({
          level: 'success',
          title: '技能已覆盖',
          message: `${forced.name} v${forced.version} 已替换`,
          actions: [],
          dismissible: true,
          autoHide: 4,
          context: 'toast',
        })
      }
    } catch (err) {
      pushNotif({
        level: 'error',
        title: '导入失败',
        message: String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }
}
