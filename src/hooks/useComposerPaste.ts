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
    const items = Array.from(event.clipboardData?.items ?? [])
    const imageItem = items.find((item) => item.kind === 'file' && item.type.startsWith('image/'))
    if (imageItem) {
      const file = imageItem.getAsFile()
      if (file) {
        event.preventDefault()
        void (async () => {
          const bytes = await readClipboardImageBytes(file)
          const pending = await saveClipboardImage(bytes, file.type || 'image/png')
          onAttachmentsResolved([pending])
        })()
      }
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
