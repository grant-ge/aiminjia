import { useEffect, useState } from 'react'
import { readFile } from '@tauri-apps/plugin-fs'

const cache = new Map<string, string>()

const MAX_BYTES = 8 * 1024 * 1024 // 8 MB upper bound for inline thumbnail

function mimeForExtension(path: string): string {
  const ext = path.toLowerCase().split('.').pop() ?? ''
  switch (ext) {
    case 'png':
      return 'image/png'
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg'
    case 'gif':
      return 'image/gif'
    case 'webp':
      return 'image/webp'
    case 'bmp':
      return 'image/bmp'
    case 'svg':
      return 'image/svg+xml'
    default:
      return 'image/png'
  }
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

/**
 * Loads a local image file and returns a `data:` URL safe to use in <img src>.
 * Result is cached per-path, so re-renders and re-mounts are O(1).
 *
 * Returns:
 *   url   — set once the image is loaded; otherwise null while loading or on error
 *   error — true if the file could not be read or exceeded MAX_BYTES
 */
export function useLocalImageDataUrl(filePath: string | undefined, mimeHint?: string) {
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
        const bytes = await readFile(filePath)
        if (cancelled) return
        if (bytes.length > MAX_BYTES) {
          setError(true)
          return
        }
        const mime = mimeHint || mimeForExtension(filePath)
        const dataUrl = `data:${mime};base64,${bytesToBase64(bytes)}`
        cache.set(filePath, dataUrl)
        setUrl(dataUrl)
      } catch {
        if (!cancelled) setError(true)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [filePath, mimeHint])

  return { url, error }
}
