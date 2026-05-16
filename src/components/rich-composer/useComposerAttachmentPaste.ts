import { useEffect } from 'react'
import type { RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import { readClipboardFilePaths } from '@/lib/tauri'
import { useChatAttachments } from '@/hooks/useChatAttachments'
import { useNotificationStore } from '@/stores/notificationStore'
import { pendingAttachmentsToTokens } from './pendingAttachmentToToken'
import type { RichComposerHandle } from './RichComposer'

const MAX_PASTED_PATHS = 50

// macOS WebKit hangs the main thread if a React onPaste handler reads
// clipboardData.types/items in the bubble phase when the clipboard contains
// Finder file references. Capture-phase access is safe; one document-level
// capture listener snapshots whether the paste contains files AND extracts
// any image blob into module state. The main paste handler reads only the
// snapshot.
let lastPasteHasFile = false
let lastPasteImageFile: File | null = null

function snapshotPaste(e: globalThis.ClipboardEvent) {
  lastPasteHasFile = false
  lastPasteImageFile = null
  try {
    const types = Array.from(e.clipboardData?.types ?? [])
    const hasFileRef = types.some(
      (t) => t === 'Files' || t === 'text/uri-list' || t.startsWith('public.file-url'),
    )
    const items = e.clipboardData?.items
    if (items) {
      for (let i = 0; i < items.length; i++) {
        const item = items[i]
        if (item.kind === 'file' && item.type.startsWith('image/')) {
          lastPasteImageFile = item.getAsFile()
          break
        }
      }
    }
    // Trigger our intercept path when there's either a file ref OR a clipboard
    // image blob (e.g., Cmd+Shift+4 screenshot has only `Files` + image/png item;
    // Preview "copy image" may set only image/png without a `Files` type).
    lastPasteHasFile = hasFileRef || lastPasteImageFile !== null
  } catch {
    lastPasteHasFile = false
    lastPasteImageFile = null
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

/** @internal test-only — sets the snapshot state normally managed by document capture listener. */
export function __test_setLastPasteHasFile(value: boolean) {
  lastPasteHasFile = value
}

/**
 * Attach paste handling to a RichComposer's editor DOM. When the clipboard
 * contains file references (Finder/Explorer drags, file URIs) or image blobs,
 * resolves them to attachments and inserts as attachment tokens. Plain text
 * and HTML paste fall through to Tiptap's default paste handler.
 *
 * Trade-off: when the clipboard contains BOTH file refs AND text content,
 * we currently fully preventDefault and discard the text portion. Mixed paste
 * is rare in practice (Finder copy is files-only), so this simplification is
 * acceptable for the first cut.
 */
export function useComposerAttachmentPaste(
  composerRef: RefObject<RichComposerHandle | null>,
): void {
  ensureSnapshotterInstalled()
  const { t } = useTranslation()
  const { resolvePastedPaths, saveClipboardImage } = useChatAttachments()

  useEffect(() => {
    let attached = false
    let cleanupFn: (() => void) | null = null

    const tryAttach = () => {
      if (attached) return
      const handle = composerRef.current
      const editor = handle?.getEditor()
      if (!editor) return
      attached = true
      const dom = editor.view.dom

      const onPaste = (event: ClipboardEvent) => {
        if (!lastPasteHasFile) return // Plain text/HTML — let Tiptap handle it.
        event.preventDefault()

        // Read the image blob from the snapshot captured at capture phase.
        // Reading clipboardData.items here can hang in some macOS WebKit cases.
        const imageFile = lastPasteImageFile

        void (async () => {
          const nativePaths = await readClipboardFilePaths().catch(() => [] as string[])
          if (nativePaths.length > 0) {
            const capped = nativePaths.slice(0, MAX_PASTED_PATHS)
            const resolved = await resolvePastedPaths(capped)
            if (resolved.length === 0) {
              pushToast(
                'info',
                t('composer.paste.cannotPaste'),
                t('composer.paste.selectedItemsUnsupported'),
              )
              return
            }
            if (resolved.length < capped.length) {
              pushToast(
                'info',
                t('composer.paste.someItemsIgnored'),
                t('composer.paste.ignoredCount', { count: capped.length - resolved.length }),
              )
            }
            handle?.insertAttachmentTokens(pendingAttachmentsToTokens(resolved))
            return
          }

          if (imageFile) {
            try {
              const buffer = await imageFile.arrayBuffer()
              const attachment = await saveClipboardImage(new Uint8Array(buffer), imageFile.type)
              handle?.insertAttachmentTokens(pendingAttachmentsToTokens([attachment]))
            } catch {
              pushToast('info', t('composer.paste.cannotPaste'), t('composer.paste.clipboardImageFailed'))
            }
            return
          }

          pushToast(
            'info',
            t('composer.paste.cannotPaste'),
            t('composer.paste.fileTypeUnsupported'),
          )
        })()
      }

      dom.addEventListener('paste', onPaste)
      cleanupFn = () => {
        dom.removeEventListener('paste', onPaste)
      }
    }

    // useEditor initializes asynchronously; the editor ref may be null on the
    // first effect run. Try immediately, then poll briefly until the editor is
    // ready. Once attached, the interval is cleared.
    tryAttach()
    const intervalId = attached ? null : window.setInterval(() => {
      tryAttach()
      if (attached && intervalId !== null) window.clearInterval(intervalId)
    }, 50)

    return () => {
      if (intervalId !== null) window.clearInterval(intervalId)
      cleanupFn?.()
    }
  }, [composerRef, resolvePastedPaths, saveClipboardImage, t])
}
