import { useCallback, type ClipboardEvent } from 'react'

import { readClipboardFilePaths } from '@/lib/tauri'

import { useChatAttachments, type PendingAttachment } from './useChatAttachments'

function extractAbsolutePaths(text: string): string[] {
  return text
    .split(/[\n\r]+/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith('/'))
}

async function readClipboardImageBytes(file: File): Promise<Uint8Array> {
  const buffer = await file.arrayBuffer()
  return new Uint8Array(buffer)
}

export interface UseComposerPasteParams {
  onAttachmentsResolved: (attachments: PendingAttachment[]) => void
}

export function useComposerPaste({ onAttachmentsResolved }: UseComposerPasteParams) {
  const { saveClipboardImage, resolvePastedPaths } = useChatAttachments()

  const handlePaste = useCallback((event: ClipboardEvent<HTMLTextAreaElement>) => {
    const types = Array.from(event.clipboardData?.types ?? [])
    const hasFileType = types.some((t) =>
      t === 'Files' || t === 'text/uri-list' || t.startsWith('public.file-url'),
    )

    const items = Array.from(event.clipboardData?.items ?? [])
    const imageItem = items.find((item) => item.kind === 'file' && item.type.startsWith('image/'))

    // Image present: try to get a real file path first; if none, save bytes to tmpImage.
    if (imageItem) {
      const file = imageItem.getAsFile()
      if (!file) return
      event.preventDefault()
      void (async () => {
        try {
          if (hasFileType) {
            const nativePaths = await readClipboardFilePaths().catch(() => [] as string[])
            if (nativePaths.length > 0) {
              const resolved = await resolvePastedPaths(nativePaths)
              if (resolved.length > 0) onAttachmentsResolved(resolved)
              return
            }
          }
          const bytes = await readClipboardImageBytes(file)
          const pending = await saveClipboardImage(bytes, file.type || 'image/png')
          onAttachmentsResolved([pending])
        } catch (err) {
          console.error('[useComposerPaste] failed to handle image paste', err)
        }
      })()
      return
    }

    const text = event.clipboardData?.getData('text/plain') ?? ''
    const paths = extractAbsolutePaths(text)
    if (paths.length > 0) {
      event.preventDefault()
      void (async () => {
        const resolved = await resolvePastedPaths(paths)
        if (resolved.length > 0) onAttachmentsResolved(resolved)
      })()
      return
    }

    if (!hasFileType) return

    event.preventDefault()
    void (async () => {
      const nativePaths = await readClipboardFilePaths().catch(() => [] as string[])
      if (nativePaths.length === 0) return
      const resolved = await resolvePastedPaths(nativePaths)
      if (resolved.length > 0) onAttachmentsResolved(resolved)
    })()
  }, [saveClipboardImage, resolvePastedPaths, onAttachmentsResolved])

  return { handlePaste }
}

export type { PendingAttachment }
