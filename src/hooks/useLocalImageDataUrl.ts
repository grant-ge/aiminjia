import { useEffect, useState } from 'react'
import { readLocalImageAsDataUrl } from '@/lib/tauri'

const cache = new Map<string, string>()

/**
 * Loads a local image file and returns a `data:` URL safe to use in <img src>.
 * Result is cached per-path, so re-renders and re-mounts are O(1).
 *
 * Reads through the Rust `read_local_image_as_data_url` command, which limits
 * the path to the AIjia home tree (`~/.renlijia/`). This avoids relying on
 * Tauri's `fs:default` scope, which doesn't grant read access to that tree
 * and would silently fail for pasted/staged images.
 *
 * Returns:
 *   url   — set once the image is loaded; otherwise null while loading or on error
 *   error — true if the file could not be read or exceeded the inline limit
 */
export function useLocalImageDataUrl(filePath: string | undefined, _mimeHint?: string) {
  const [url, setUrl] = useState<string | null>(filePath ? cache.get(filePath) ?? null : null)
  const [error, setError] = useState(false)

  useEffect(() => {
    if (!filePath) {
      setUrl(null)
      setError(false)
      return
    }
    const cached = cache.get(filePath)
    if (cached) {
      setUrl(cached)
      setError(false)
      return
    }
    let cancelled = false
    setError(false)
    void (async () => {
      try {
        const dataUrl = await readLocalImageAsDataUrl(filePath)
        if (cancelled) return
        cache.set(filePath, dataUrl)
        setUrl(dataUrl)
      } catch {
        if (!cancelled) setError(true)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [filePath])

  return { url, error }
}
